//! tmux sessions, the saved session definition, restore, and the canvas layout file.

use super::*;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session: String,
    pub tmux_available: bool,
    pub tmux_version: String,
    pub config_path: Option<String>,
    pub home: String,
    pub default_folder: Option<String>,
    pub roles: Vec<String>,
    pub cli_path: Option<String>,
    pub cli_note: String,
    /// `true` when `hubcodes` resolves on the login shell PATH (what agents started elsewhere see).
    pub cli_on_path: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LayoutDto {
    pub nodes: HashMap<String, (f64, f64)>,
    #[serde(default)]
    pub sizes: HashMap<String, (f64, f64)>,
    pub viewport: Option<(f64, f64, f64)>,
    /// Bands: titled boxes on the canvas that group cards by position.
    #[serde(default)]
    pub groups: HashMap<String, GroupDto>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
}

#[tauri::command]
pub fn session_info(state: State<'_, Arc<AppState>>) -> SessionInfo {
    let version = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let reg = state.reg();
    SessionInfo {
        session: state.tmux.session.clone(),
        tmux_available: tmux::available(),
        tmux_version: version,
        config_path: reg.source.as_ref().map(|p| p.display().to_string()),
        home: dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default(),
        default_folder: reg.desktop.default_folder.clone(),
        roles: reg.desktop.roles.clone(),
        cli_path: state.cli.as_ref().map(|p| p.display().to_string()),
        cli_note: state.cli_note.clone(),
        cli_on_path: std::env::var_os("PATH").map(|paths| std::env::split_paths(&paths).any(|d| d.join("hubcodes").is_file())).unwrap_or(false),
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub name: String,
    pub running: bool,
    pub agents: usize,
    pub saved: bool,
    pub current: bool,
}

/// tmux sessions with hubcodes agents plus saved session definitions.
#[tauri::command]
pub fn sessions(state: State<'_, Arc<AppState>>) -> Vec<SessionEntry> {
    let mut names: Vec<String> = std::process::Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    let state_dir = hubcodes_core::db::state_dir();
    let mut saved = std::collections::HashSet::new();
    if let Ok(rd) = std::fs::read_dir(&state_dir) {
        for e in rd.flatten() {
            if let Some(n) = e.file_name().to_str().and_then(|f| f.strip_suffix("-def.json")) {
                saved.insert(n.to_string());
                if !names.contains(&n.to_string()) {
                    names.push(n.to_string());
                }
            }
        }
    }
    if !names.contains(&state.tmux.session) {
        names.push(state.tmux.session.clone());
    }
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let t = Tmux::named(&name);
            let running = t.has_session();
            let agents = if running { t.panes().map(|p| p.iter().filter(|p| p.provider.is_some() && p.provider.as_deref() != Some("proxy")).count()).unwrap_or(0) } else { 0 };
            SessionEntry { current: name == state.tmux.session, saved: saved.contains(&name), name, running, agents }
        })
        .filter(|s| s.current || s.saved || s.agents > 0)
        .collect()
}

/// Remember the session for the next launch and restart into it.
#[tauri::command]
pub fn switch_session(app: AppHandle, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.contains(['/', ':', '.', ' ']) {
        return Err("a session name is one word without / : . or spaces".into());
    }
    let p = hubcodes_core::db::state_dir().join("desktop-session");
    std::fs::create_dir_all(p.parent().unwrap()).map_err(err)?;
    std::fs::write(&p, &name).map_err(err)?;
    log_line(&format!("switching to session {name}; restarting"));
    app.restart();
}

/// The saved session definition, if any (for the restore offer).
#[tauri::command]
pub fn session_def(state: State<'_, Arc<AppState>>) -> Option<SessionDef> {
    sessiondef::load(&state.tmux.session)
}

/// "Start fresh": forget the saved agents; the canvas starts empty.
#[tauri::command]
pub fn discard_saved_session(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    sessiondef::discard(&state.tmux.session).map_err(err)
}

#[tauri::command]
pub async fn restore_session(state: State<'_, Arc<AppState>>, resume: bool) -> Result<sessiondef::Restored, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || restore_session_now(&st, resume)).await.map_err(err)?
}

fn restore_session_now(state: &Arc<AppState>, resume: bool) -> Result<sessiondef::Restored, String> {
    state.ensure_session().map_err(err)?;
    let def = sessiondef::load(&state.tmux.session).ok_or("nothing saved for this session")?;
    let reg = state.reg().clone();
    let exe = state.cli.as_ref().map(|c| c.display().to_string()).unwrap_or_else(|| "hubcodes".into());
    let path_prefix = state.cli.as_ref().and_then(|c| c.parent()).map(|d| d.display().to_string());
    Ok(sessiondef::restore(&state.tmux, &reg, &def, &exe, path_prefix.as_deref(), resume))
}

/// Restart the named agents with the current brief (links, roles, folders kept; conversation continued where possible).
#[tauri::command]
pub async fn restart_agents(state: State<'_, Arc<AppState>>, names: Vec<String>, resume: bool) -> Result<sessiondef::Restored, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || restart_agents_now(&st, names, resume)).await.map_err(err)?
}

fn restart_agents_now(state: &Arc<AppState>, names: Vec<String>, resume: bool) -> Result<sessiondef::Restored, String> {
    let reg = state.reg().clone();
    let exe = state.cli.as_ref().map(|c| c.display().to_string()).unwrap_or_else(|| "hubcodes".into());
    let path_prefix = state.cli.as_ref().and_then(|c| c.parent()).map(|d| d.display().to_string());
    sessiondef::restart(&state.tmux, &reg, &names, &exe, path_prefix.as_deref(), resume).map_err(err)
}

/// Run an agent on another provider or model; its name, role, folder and links stay.
#[tauri::command]
pub async fn switch_agent(state: State<'_, Arc<AppState>>, name: String, provider: String, model: Option<String>) -> Result<sessiondef::Switched, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || switch_agent_now(&st, name, provider, model)).await.map_err(err)?
}

fn switch_agent_now(state: &Arc<AppState>, name: String, provider: String, model: Option<String>) -> Result<sessiondef::Switched, String> {
    let reg = state.reg().clone();
    let exe = state.cli.as_ref().map(|c| c.display().to_string()).unwrap_or_else(|| "hubcodes".into());
    let path_prefix = state.cli.as_ref().and_then(|c| c.parent()).map(|d| d.display().to_string());
    sessiondef::switch(&state.tmux, &reg, &name, &provider, model.as_deref(), &exe, path_prefix.as_deref()).map_err(err)
}

pub(crate) fn layout_path(app: &AppHandle, session: &str) -> Result<std::path::PathBuf> {
    let dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let safe: String = session
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    Ok(dir.join(format!("layout-{safe}.json")))
}

#[tauri::command]
pub fn load_layout(app: AppHandle, state: State<'_, Arc<AppState>>) -> LayoutDto {
    layout_path(&app, &state.tmux.session)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_layout(app: AppHandle, state: State<'_, Arc<AppState>>, layout: LayoutDto) -> Result<(), String> {
    let p = layout_path(&app, &state.tmux.session).map_err(err)?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&layout).map_err(err)?).map_err(err)?;
    std::fs::rename(&tmp, &p).map_err(err)
}
