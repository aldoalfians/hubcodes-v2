//! Team templates, saved prompts and role briefs.

use super::*;
use crate::agents::expand_prompt;

/// Templates: the repo's own first (when `cwd` is in a repo with .hubcodes/team.toml), then global.
#[tauri::command]
pub fn templates(state: State<'_, Arc<AppState>>, cwd: Option<String>) -> Vec<Template> {
    let reg = state.reg();
    match cwd.filter(|c| !c.trim().is_empty()).map(|c| expand_home(c.trim())).or_else(|| reg.desktop.default_folder.clone().map(|d| expand_home(&d))) {
        Some(c) => reg.for_dir(Path::new(&c)).templates,
        None => reg.templates.clone(),
    }
}

#[tauri::command]
pub fn team_start(state: State<'_, Arc<AppState>>, name: String, cwd: Option<String>, models: Option<std::collections::BTreeMap<String, String>>) -> Result<team::Started, String> {
    state.ensure_session().map_err(err)?;
    let base = state.reg().clone();
    let cwd = cwd.filter(|c| !c.trim().is_empty()).map(|c| expand_home(c.trim())).or_else(|| base.desktop.default_folder.clone().map(|d| expand_home(&d)));
    let reg = match &cwd {
        Some(c) => base.for_dir(Path::new(c)),
        None => base,
    };
    let tpl = reg.templates.iter().find(|t| t.name == name).ok_or_else(|| format!("no template {name}"))?;
    let exe = state.cli.as_ref().map(|c| c.display().to_string()).unwrap_or_else(|| "hubcodes".into());
    let path_prefix = state.cli.as_ref().and_then(|c| c.parent()).map(|d| d.display().to_string());
    Ok(team::start_with(&state.tmux, &reg, tpl, cwd.as_deref(), &exe, path_prefix.as_deref(), &models.unwrap_or_default()))
}

/// Save the running agents and links as a template in hubcodes.toml.
#[tauri::command]
pub fn team_save(state: State<'_, Arc<AppState>>, name: String) -> Result<Template, String> {
    let tpl = team::capture(&state.tmux, name.trim()).map_err(err)?;
    let mut next = state.reg().clone();
    next.templates.retain(|t| t.name != tpl.name);
    next.templates.insert(0, tpl.clone());
    let path = next.save().map_err(|e| format!("{e:#}"))?;
    next.source = Some(path);
    *state.reg.write().unwrap_or_else(|e| e.into_inner()) = next;
    Ok(tpl)
}

#[tauri::command]
pub fn prompt_list(state: State<'_, Arc<AppState>>) -> Vec<Prompt> {
    state.reg().prompts.clone()
}

/// Expanded text of `/name …` for an agent, so the composer can show what will be sent.
#[tauri::command]
pub fn prompt_preview(state: State<'_, Arc<AppState>>, id: String, text: String) -> String {
    expand_prompt(&state, &id, &text)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RoleBrief {
    /// The generated brief with the role file layered on.
    pub text: String,
    /// The role file that was used, if any.
    pub file: Option<String>,
}

/// What an agent with this role would be told at start, looked up the way `spawn_agent` does
/// (an empty folder means the default folder).
#[tauri::command]
pub fn role_brief(state: State<'_, Arc<AppState>>, role: String, cwd: Option<String>) -> RoleBrief {
    let cwd = cwd.filter(|c| !c.trim().is_empty()).or_else(|| state.reg().desktop.default_folder.clone()).map(|c| expand_home(c.trim()));
    let role = role.trim().to_string();
    let (text, file) = prompts::effective_brief(&state.tmux.session, "", if role.is_empty() { None } else { Some(&role) }, cwd.as_deref());
    RoleBrief { text, file: file.map(|p| p.display().to_string()) }
}

/// A role in Settings → Roles: built in, in use, or with a file in ~/.config/hubcodes/roles.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleEntry {
    pub role: String,
    /// orchestrator | reviewer | tester | worker: which generated brief it gets.
    pub kind: String,
    /// generated | extends | replaces
    pub mode: String,
    pub file: Option<String>,
}

/// Roles worth showing: the four kinds, every role an agent or tracker uses,
/// and every user role file.
#[tauri::command]
pub fn roles_list(state: State<'_, Arc<AppState>>) -> Vec<RoleEntry> {
    let reg = state.reg();
    let mut names: Vec<String> = ["orchestrator", "software engineer", "reviewer", "tester"].iter().map(|s| s.to_string()).collect();
    names.extend(reg.trackers.iter().map(|t| t.role.clone()));
    names.extend(state.tmux.panes().unwrap_or_default().into_iter().filter_map(|p| p.role));
    let dir = prompts::user_roles_dir();
    let files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "md").unwrap_or(false)).collect()).unwrap_or_default();
    names.extend(files.iter().filter_map(|f| f.file_stem().map(|s| s.to_string_lossy().replace('-', " "))));
    let mut seen = std::collections::HashSet::new();
    names
        .into_iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && seen.insert(prompts::role_slug(n)))
        .map(|role| {
            let file = prompts::role_file_in(&role, std::slice::from_ref(&dir));
            let mode = match &file {
                None => "generated",
                Some((_, t)) if prompts::replaces(t) => "replaces",
                Some(_) => "extends",
            };
            RoleEntry { kind: prompts::kind(&role).into(), mode: mode.into(), file: file.map(|(p, _)| p.display().to_string()), role }
        })
        .collect()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDetail {
    /// hubcodes' own text for the role's job (`{lead}` = the orchestrator).
    pub default: String,
    /// The text the role gets now: the default, or the user's version.
    pub text: String,
    /// The user's role file, when there is one.
    pub file: Option<String>,
}

#[tauri::command]
pub fn role_get(role: String) -> RoleDetail {
    let role = role.trim().to_string();
    let default = prompts::default_job(prompts::kind(&role));
    match prompts::role_file_in(&role, &[prompts::user_roles_dir()]) {
        Some((p, t)) => {
            let body = prompts::role_body(&t);
            // A file that adds to the default shows as the default followed by it.
            let text = if prompts::replaces(&t) { body } else { format!("{default}\n\n{body}") };
            RoleDetail { default, text, file: Some(p.display().to_string()) }
        }
        None => RoleDetail { text: default.clone(), default, file: None },
    }
}

/// Save what the role is told about its job as
/// ~/.config/hubcodes/roles/<slug>.md. The default text (or nothing) removes
/// the file, so the role follows hubcodes' default again.
#[tauri::command]
pub fn role_save(role: String, text: String) -> Result<Option<String>, String> {
    let slug = prompts::role_slug(&role);
    if slug.is_empty() {
        return Err(format!("\"{role}\" has no letters or digits to name a role file after"));
    }
    let path = prompts::user_roles_dir().join(format!("{slug}.md"));
    let body = text.trim();
    if body.is_empty() || body == prompts::default_job(prompts::kind(&role)).trim() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(err)?;
        }
        return Ok(None);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    std::fs::write(&path, format!("{}\n\n{body}\n", prompts::REPLACE_MARKER)).map_err(err)?;
    Ok(Some(path.display().to_string()))
}

/// Write the running canvas as the repo's team file (.hubcodes/team.toml).
#[tauri::command]
pub fn team_init(state: State<'_, Arc<AppState>>, cwd: String) -> Result<String, String> {
    let root = git::repo_root(Path::new(&expand_home(&cwd))).ok_or_else(|| format!("{cwd} is not inside a git repository"))?;
    let mut rc = repo::load_at(&root);
    let name = root.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "team".into());
    let tpl = team::capture(&state.tmux, &name).map_err(err)?;
    rc.templates.retain(|x| x.name != tpl.name);
    rc.templates.insert(0, tpl);
    if rc.test.command.is_empty() {
        if let Some(c) = repo::detect_test_command(&root) {
            rc.test.command = c;
        }
    }
    repo::save_at(&root, &rc).map(|p| p.display().to_string()).map_err(|e| format!("{e:#}"))
}
