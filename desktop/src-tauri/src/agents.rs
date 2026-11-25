//! Agent lifecycle and messaging: spawn, kill, instruct, links, diff, commit, PR, tests, merge queue.

use super::*;

/// Start an agent off the main thread: starting a tool takes a moment, and a
/// command on the main thread freezes the window (the busy cursor) meanwhile.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn spawn_agent(
    state: State<'_, Arc<AppState>>,
    name: String,
    provider: String,
    cwd: Option<String>,
    role: Option<String>,
    model: Option<String>,
    worktree: Option<String>,
    sandbox: Option<bool>,
    device: Option<String>,
    account: Option<String>,
) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || spawn_agent_now(&st, name, provider, cwd, role, model, worktree, sandbox, device, account)).await.map_err(err)?
}

#[allow(clippy::too_many_arguments)]
fn spawn_agent_now(
    state: &Arc<AppState>,
    name: String,
    provider: String,
    cwd: Option<String>,
    role: Option<String>,
    model: Option<String>,
    worktree: Option<String>,
    sandbox: Option<bool>,
    device: Option<String>,
    account: Option<String>,
) -> Result<String, String> {
    state.ensure_session().map_err(err)?;
    let sandbox = sandbox.unwrap_or(false);
    let account = account.map(|a| a.trim().to_string()).filter(|a| !a.is_empty() && a != hubcodes_core::accounts::DEFAULT);
    if let Some(a) = &account {
        if !hubcodes_core::accounts::valid(a) {
            return Err(format!("bad account name '{a}'"));
        }
    }
    let role = role.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    let worktree = worktree.map(|w| w.trim().to_string()).filter(|w| !w.is_empty());
    let model = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
    // On another device: its own hubcodes starts the agent; it shows here as device/name.
    if let Some(device) = device.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()) {
        let reg = state.reg();
        let peer = reg.peers.iter().find(|p| p.name == device).ok_or_else(|| format!("unknown device '{device}'"))?;
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("name is empty".into());
        }
        let full = remote::spawn_account(peer, &name, &provider, cwd.as_deref().map(str::trim).filter(|c| !c.is_empty()), role.as_deref(), model.as_deref(), account.as_deref()).map_err(|e| format!("{e:#}"))?;
        if let Ok(db) = Db::open(&state.tmux.session) {
            let _ = db.add_event(&full, "spawn", &format!("{provider}{} on {device}", role.map(|r| format!(" {r}")).unwrap_or_default()));
        }
        state.poll_peers();
        return Ok(full);
    }
    let (cmd, default_folder) = {
        let reg = state.reg();
        let p = reg
            .get(&provider)
            .ok_or_else(|| format!("unknown provider '{provider}'"))?;
        let extra = hubcodes_core::prompts::start_extra(&reg.memory, &state.tmux.session, &name, role.as_deref(), cwd.as_deref());
        (p.launch_command_with(model.as_deref(), role.as_deref(), &extra), reg.desktop.default_folder.clone())
    };
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name is empty".into());
    }
    if let Some(r) = &role {
        if r.contains('\'') {
            return Err("role may not contain a quote".into());
        }
    }
    // Hooks: Claude Code `--settings <file>`, Codex `-c notify=[…]`, both calling the bundled CLI.
    let acct = account.clone().unwrap_or_else(|| hubcodes_core::accounts::DEFAULT.into());
    let (hook_kind_opt, account_part) = {
        let reg = state.reg();
        let hk = reg.get(&provider).and_then(|p| p.hook_kind.clone());
        hubcodes_core::accounts::prepare(&provider, hk.as_deref(), &acct).map_err(err)?;
        let prefix = hubcodes_core::accounts::env_prefix(&provider, hk.as_deref(), &acct);
        (hk, prefix)
    };
    let hook_args = match state.cli.as_ref() {
        Some(cli) => {
            let reg = state.reg();
            let (kind, command) = reg.get(&provider).map(|p| (p.hook_kind.clone().unwrap_or_else(|| provider.clone()), p.command.clone())).unwrap_or((provider.clone(), String::new()));
            let paths = hubcodes_core::accounts::paths(&provider, hook_kind_opt.as_deref(), &acct);
            hooks::launch_args_account(&kind, &state.tmux.session, &cli.display().to_string(), &command, &paths).unwrap_or_default()
        }
        None => vec![],
    };
    let hook_part = hook_args.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" ");
    // The bundled CLI first on PATH, so `hubcodes send` works in every agent.
    let path_part = state
        .cli
        .as_ref()
        .and_then(|c| c.parent())
        .map(|d| format!("PATH={}:\"$PATH\" ", sh_quote(&d.display().to_string())))
        .unwrap_or_default();
    let cmd = format!(
        "{path_part}{account_part}HUBCODES_AGENT='{}' HUBCODES_SESSION='{}' HUBCODES_ROLE='{}' HUBCODES_MODEL='{}' HUBCODES_ACCOUNT='{acct}' {}{}{}",
        name,
        state.tmux.session,
        role.as_deref().unwrap_or(""),
        model.as_deref().unwrap_or(""),
        cmd,
        if hook_part.is_empty() { "" } else { " " },
        hook_part
    );
    let cwd = cwd
        .filter(|c| !c.trim().is_empty())
        .or(default_folder)
        .map(|c| expand_home(c.trim()));
    if let Some(c) = &cwd {
        if !std::path::Path::new(c).is_dir() {
            return Err(format!("folder {c} does not exist"));
        }
    }
    // Own branch: a worktree under <repo>/.hubcodes/worktrees/<name> becomes the agent's folder.
    let mut wt_path: Option<String> = None;
    let cwd = match &worktree {
        Some(branch) => {
            let base = cwd.clone().or_else(|| dirs::home_dir().map(|h| h.display().to_string())).unwrap_or_default();
            let dir = git::add_worktree(Path::new(&base), &name, branch).map_err(|e| format!("{e:#}"))?;
            wt_path = Some(dir.display().to_string());
            wt_path.clone()
        }
        None => cwd,
    };
    let cmd = if sandbox {
        let dir = cwd.clone().or_else(|| dirs::home_dir().map(|h| h.display().to_string())).unwrap_or_default();
        sandbox::wrap(&state.reg().sandbox, &cmd, Path::new(&dir))
    } else {
        cmd
    };
    let id = state
        .tmux
        .spawn_with_account(&name, &provider, &cmd, cwd.as_deref(), role.as_deref(), model.as_deref(), account.as_deref())
        .map_err(err)?;
    if let Some(w) = &wt_path {
        let _ = state.tmux.set_pane_option(&id, tmux::OPT_WORKTREE, w);
    }
    if sandbox {
        let _ = state.tmux.set_pane_option(&id, "@hubcodes_sandbox", "1");
    }
    if let Ok(db) = Db::open(&state.tmux.session) {
        let _ = db.add_event(&name, "spawn", &format!("{provider}{}{}", role.map(|r| format!(" {r}")).unwrap_or_default(), wt_path.map(|w| format!(" worktree {w}")).unwrap_or_default()));
    }
    Ok(id)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiffDto {
    pub dir: String,
    pub changes: Changes,
    pub diff: String,
}

pub(crate) fn agent_dir(state: &AppState, id: &str) -> Result<String, String> {
    let pane = state
        .tmux
        .panes()
        .ok()
        .and_then(|ps| ps.into_iter().find(|p| p.id == id))
        .ok_or("agent not found")?;
    Ok(pane.worktree.unwrap_or_else(|| pane_cwd(id).unwrap_or_default()))
}

/// Branch, status and the unified diff of the agent's folder.
#[tauri::command]
pub fn agent_diff(state: State<'_, Arc<AppState>>, id: String) -> Result<DiffDto, String> {
    let dir = agent_dir(&state, &id)?;
    git::repo_root(Path::new(&dir)).ok_or_else(|| format!("{dir} is not a git repository"))?;
    let changes = git::changes(Path::new(&dir)).map_err(err)?;
    let diff = git::diff(Path::new(&dir), 400_000).map_err(err)?;
    Ok(DiffDto { dir, changes, diff })
}

#[tauri::command]
pub fn commit_agent(state: State<'_, Arc<AppState>>, id: String, message: String) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err("commit message is empty".into());
    }
    let dir = agent_dir(&state, &id)?;
    let sha = git::commit_all(Path::new(&dir), message.trim()).map_err(err)?;
    state.changes_cache.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    Ok(sha)
}

/// Push the branch and open the GitHub PR form in the browser (`gh pr create --web`).
#[tauri::command]
pub async fn open_pr(state: State<'_, Arc<AppState>>, id: String) -> Result<String, String> {
    let dir = agent_dir(&state, &id)?;
    tauri::async_runtime::spawn_blocking(move || git::open_pr(Path::new(&dir)).map_err(|e| format!("{e:#}"))).await.map_err(err)?
}

/// Change the role label of a running agent (the model needs a restart).
#[tauri::command]
pub fn set_role(state: State<'_, Arc<AppState>>, id: String, role: String) -> Result<(), String> {
    state.tmux.set_role(&id, &role).map_err(err)
}

/// Hide (or show again) an agent's screen to guests of the shared canvas;
/// they then see its status only. `name` is the agent's name or pane id.
#[tauri::command]
pub fn set_private(state: State<'_, Arc<AppState>>, name: String, on: bool) -> Result<(), String> {
    if name.contains('/') {
        return Err("mark it private on its own device".into());
    }
    state.tmux.set_private(&name, on).map_err(err)
}

#[tauri::command]
pub fn kill_agent(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    if id.contains('/') {
        return Err("remove it on its own device".into());
    }
    let mut streams = state.streams.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(h) = streams.remove(&id) {
        h.stop();
    }
    let name = state
        .tmux
        .panes()
        .ok()
        .and_then(|ps| ps.into_iter().find(|p| p.id == id))
        .map(|p| p.name);
    let worktree = state.tmux.panes().ok().and_then(|ps| ps.into_iter().find(|p| p.id == id)).and_then(|p| p.worktree);
    state.tmux.kill(&id).map_err(err)?;
    if let Some(n) = &name {
        let _ = state.tmux.remove_edges_of(n);
        // Removed on purpose: Restore must not bring it back.
        let _ = sessiondef::forget(&state.tmux.session, n);
    }
    // The branch stays; only the checkout under .hubcodes/worktrees goes.
    if let Some(w) = worktree {
        if w.contains("/.hubcodes/worktrees/") {
            if let Err(e) = git::remove_worktree(Path::new(&w)) {
                eprintln!("[hubcodes] worktree {w} not removed: {e:#}");
            }
        }
    }
    if let (Some(n), Ok(db)) = (name, Db::open(&state.tmux.session)) {
        let _ = db.add_event(&n, "kill", "removed from the canvas");
    }
    Ok(())
}

/// Human → agent: the text lands in the terminal as typed (no `[from]` prefix).
/// `id` is a local pane id or `peer/agent`.
/// `/review …` → the saved prompt, variables filled for this agent.
pub(crate) fn expand_prompt(state: &AppState, id: &str, text: &str) -> String {
    if !text.trim_start().starts_with('/') {
        return text.to_string();
    }
    let reg = state.reg();
    let pane = state.tmux.panes().ok().and_then(|ps| ps.into_iter().find(|p| p.id == id || p.name == id));
    let ctx = prompts::Context {
        agent: pane.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| id.to_string()),
        role: pane.as_ref().and_then(|p| p.role.clone()).unwrap_or_default(),
        model: pane.as_ref().and_then(|p| p.model.clone()).unwrap_or_default(),
        cwd: pane.as_ref().and_then(|p| p.worktree.clone().or_else(|| pane_cwd(&p.id))).unwrap_or_default(),
        goal: pane
            .as_ref()
            .map(|p| relay::loops(&state.tmux).iter().find(|(k, _)| k.split('|').any(|n| n == p.name)).map(|(_, l)| l.goal.clone()).unwrap_or_default())
            .unwrap_or_default(),
        notes: notes::read(&state.tmux.session),
    };
    prompts::apply_slash(text, &reg.prompts, &ctx).unwrap_or_else(|| text.to_string())
}

/// What the person typed into an agent's composer. It goes to the
/// session's orchestrator first unless `direct` (⌥Enter) or routing is off;
/// returns that orchestrator's name when it did.
#[tauri::command]
pub fn instruct(state: State<'_, Arc<AppState>>, id: String, text: String, direct: Option<bool>) -> Result<Option<String>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let text = expand_prompt(&state, &id, &text);
    let reg = state.reg();
    let name = if id.contains('/') {
        id.clone()
    } else {
        state.tmux.panes().ok().and_then(|ps| ps.into_iter().find(|p| p.id == id)).map(|p| p.name).unwrap_or_else(|| id.clone())
    };
    if !direct.unwrap_or(false) && relay::route_for(&state.tmux, &reg, &name).is_some() {
        let (_, lead) = relay::deliver_from_person(&state.tmux, &reg, &state.me(), &name, &text, false, false).map_err(|e| format!("{e:#}"))?;
        return Ok(lead);
    }
    if id.contains('/') {
        relay::deliver(&state.tmux, &reg, &state.me(), &id, &text).map_err(|e| format!("{e:#}"))?;
        return Ok(None);
    }
    state.tmux.send_text(&id, &text).map_err(err)?;
    let _ = state.tmux.record_message(&state.me(), &name, &text);
    Ok(None)
}

/// Agent → agent on the user's behalf (composer `@name`): same path as
/// `hubcodes send`, so loops and remote devices work the same way.
#[tauri::command]
pub fn send_message(state: State<'_, Arc<AppState>>, from: String, to: String, text: String) -> Result<u64, String> {
    let reg = state.reg();
    let d = relay::deliver(&state.tmux, &reg, &from, &to, &text).map_err(|e| format!("{e:#}"))?;
    let _ = state.tmux.add_edge(&from, &to, "msg");
    Ok(d.seq)
}

/// Approval inbox: yes / no to the agent's permission prompt.
#[tauri::command]
pub fn answer_prompt(state: State<'_, Arc<AppState>>, id: String, approve: bool) -> Result<(), String> {
    let name = if id.contains('/') {
        id.clone()
    } else {
        state
            .tmux
            .panes()
            .ok()
            .and_then(|ps| ps.into_iter().find(|p| p.id == id))
            .map(|p| p.name)
            .ok_or("agent not found")?
    };
    let reg = state.reg();
    relay::answer(&state.tmux, &reg, &name, approve, &state.me()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn connect_agents(state: State<'_, Arc<AppState>>, from: String, to: String, label: Option<String>) -> Result<(), String> {
    state
        .tmux
        .add_edge(&from, &to, label.as_deref().unwrap_or("link"))
        .map_err(err)
}

#[tauri::command]
pub fn disconnect_agents(state: State<'_, Arc<AppState>>, a: String, b: String) -> Result<(), String> {
    state.tmux.remove_edge(&a, &b).map_err(err)
}

/// Run the repository's tests in the agent's folder (long: 15 min cap).
#[tauri::command]
pub async fn run_tests(state: State<'_, Arc<AppState>>, id: String) -> Result<TestResult, String> {
    let dir = agent_dir(&state, &id)?;
    let name = state.tmux.panes().ok().and_then(|ps| ps.into_iter().find(|p| p.id == id)).map(|p| p.name).unwrap_or(id.clone());
    let t = state.tmux.clone();
    state.changes_cache.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    tauri::async_runtime::spawn_blocking(move || testrun::run_for_agent(&t, &name, Path::new(&dir), None).map_err(|e| format!("{e:#}"))).await.map_err(err)?
}

/// Queue the agent's branch for a merge into the repo's target branch.
#[tauri::command]
pub fn merge_enqueue(state: State<'_, Arc<AppState>>, id: String, into: Option<String>) -> Result<Merge, String> {
    let dir = agent_dir(&state, &id)?;
    let name = state.tmux.panes().ok().and_then(|ps| ps.into_iter().find(|p| p.id == id)).map(|p| p.name).unwrap_or(id.clone());
    let branch = git::changes(Path::new(&dir)).map(|c| c.branch).map_err(err)?;
    let rc = repo::load(Path::new(&dir));
    let into = into.filter(|s| !s.trim().is_empty()).unwrap_or(rc.merge.into);
    if branch == into {
        return Err(format!("{name} is on {into} itself; give it its own branch first"));
    }
    let db = Db::open(&state.tmux.session).map_err(err)?;
    merge::enqueue(&db, Path::new(&dir), &branch, &into, &name).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn merges_list(state: State<'_, Arc<AppState>>) -> Result<Vec<Merge>, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    merge::list(&db, 30).map_err(err)
}

/// A file pasted or picked in the message dock (base64), written to this
/// Mac's attachments folder; returns its path.
#[tauri::command]
pub async fn save_attachment(name: String, data: String) -> Result<String, String> {
    use base64::Engine;
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|e| format!("{name}: {e}"))?;
        hubcodes_core::attach::save(&name, &bytes).map(|p| p.display().to_string()).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(err)?
}

/// The message agent `to` gets for this text and these local files: the
/// files copied to its device first when it runs on another one, their
/// paths listed after the text (as `hubcodes send --file` does).
#[tauri::command]
pub async fn compose_message(state: State<'_, Arc<AppState>>, to: String, text: String, paths: Vec<String>) -> Result<String, String> {
    let peers = state.reg().peers;
    tauri::async_runtime::spawn_blocking(move || {
        let files: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
        let far = hubcodes_core::attach::for_agent(&peers, &to, &files).map_err(|e| format!("{e:#}"))?;
        Ok(hubcodes_core::attach::with_files(&text, &far))
    })
    .await
    .map_err(err)?
}
