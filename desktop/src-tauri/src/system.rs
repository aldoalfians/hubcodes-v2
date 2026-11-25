//! Usage, history, background daemon, doctor, logs and other app-level commands.

use super::*;

/// Token usage per turn for the cost page.
#[tauri::command]
pub fn usage_log(state: State<'_, Arc<AppState>>, days: i64) -> Result<Vec<hubcodes_core::db::UsageRow>, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    db.usage_log(days.clamp(1, 365)).map_err(err)
}

/// Transcript from SQLite, newest first.
#[tauri::command]
pub fn history(state: State<'_, Arc<AppState>>, limit: usize, query: Option<String>) -> Result<Vec<MessageRow>, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    db.messages(limit.clamp(1, 2000), query.as_deref().filter(|q| !q.trim().is_empty())).map_err(err)
}

/// Settings → General → Background: the launchd watcher.
#[tauri::command]
pub fn daemon_status(state: State<'_, Arc<AppState>>) -> hubcodes_core::daemon::Status {
    hubcodes_core::daemon::status(&state.tmux.session)
}

#[tauri::command]
pub fn daemon_set(state: State<'_, Arc<AppState>>, on: bool, serve: bool) -> Result<hubcodes_core::daemon::Status, String> {
    let session = state.tmux.session.clone();
    if on {
        let exe = state.cli.as_ref().map(|p| p.display().to_string()).ok_or_else(|| "the hubcodes CLI is not bundled; install it first".to_string())?;
        hubcodes_core::daemon::install(&session, &exe, serve).map_err(err)?;
    } else {
        hubcodes_core::daemon::uninstall(&session).map_err(err)?;
    }
    Ok(hubcodes_core::daemon::status(&session))
}

/// Settings → Doctor: environment checks plus the log tail.
#[tauri::command]
pub async fn doctor(state: State<'_, Arc<AppState>>) -> Result<hubcodes_core::doctor::Report, String> {
    let ctx = hubcodes_core::doctor::Context {
        cli: state.cli.clone(),
        bundle: std::env::current_exe().ok().and_then(|p| p.ancestors().find(|a| a.extension().map(|e| e == "app").unwrap_or(false)).map(|a| a.to_path_buf())),
        session: Some(state.tmux.session.clone()),
        peers: state.reg().peers.clone(),
        self_name: state.reg().sharing.name.clone(),
    };
    tauri::async_runtime::spawn_blocking(move || hubcodes_core::doctor::run(&ctx)).await.map_err(err)
}

/// The report as text, with the log tail, for a bug report.
#[tauri::command]
pub async fn diagnostics(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let r = doctor(state).await?;
    Ok(hubcodes_core::doctor::render(&r, 80))
}

#[tauri::command]
pub fn log_tail(lines: usize) -> String {
    hubcodes_core::doctor::log_tail(lines.clamp(1, 2000))
}

#[tauri::command]
pub fn log_js(msg: String) {
    eprintln!("[js] {msg}");
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let st = std::process::Command::new("open").arg(&path).status();
    #[cfg(not(target_os = "macos"))]
    let st = std::process::Command::new("xdg-open").arg(&path).status();
    st.map_err(err).and_then(|s| if s.success() { Ok(()) } else { Err("open failed".into()) })
}

/// Send a test notification (Settings → "Send a test notification").
#[tauri::command]
pub fn test_notification(app: AppHandle) -> Result<(), String> {
    app.notification()
        .builder()
        .title("reviewer needs you")
        .body("Allow opencode to run `cargo clippy`? (y/N)")
        .show()
        .map_err(err)
}
