//! Remote access: the HTTP server, contributor approvals, audit log, Tailscale.

use super::*;

#[tauri::command]
pub fn serve_start(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<String, String> {
    if let Some(addr) = state.serving.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Ok(addr);
    }
    let reg = state.reg().clone();
    let addr = hubcodes_core::serve::bind_addr(&reg.serve).map_err(|e| format!("{e:#}"))?;
    let t = state.tmux.clone();
    let mut cfg = reg.serve.clone();
    // The built UI ships as a resource folder (`web/`), so teammates get the full canvas at /canvas/.
    if cfg.web_root.is_empty() {
        if let Ok(dir) = app.path().resource_dir() {
            let web = dir.join("web");
            if web.join("index.html").exists() {
                cfg.web_root = web.display().to_string();
            }
        }
    }
    let (t3, reg3) = (t.clone(), reg.clone());
    let sink: hubcodes_core::serve::WebhookSink = Arc::new(move |name: &str, body: &str| {
        for tr in reg3.triggers.iter().filter(|x| x.enabled) {
            if hubcodes_core::triggers::kind(tr) == Some(hubcodes_core::triggers::Kind::Webhook(name.to_string())) {
                let action = hubcodes_core::triggers::fill(&tr.action, &[("body", body.trim())]);
                let ev = tend::run_action(&t3, &reg3, tr, &action);
                eprintln!("[hubcodes] {}", ev.text);
            }
        }
    });
    *state.serving.lock().unwrap_or_else(|e| e.into_inner()) = Some(addr.clone());
    let st = state.inner().clone();
    std::thread::spawn(move || {
        if let Err(e) = hubcodes_core::serve::run(t, reg, cfg, Some(sink)) {
            eprintln!("[hubcodes] serve stopped: {e:#}");
        }
        *st.serving.lock().unwrap_or_else(|e| e.into_inner()) = None;
    });
    Ok(addr)
}

/// Inbox: approve or deny a contributor's message from the web page.
#[tauri::command]
pub fn pending_decide(state: State<'_, Arc<AppState>>, id: i64, approve: bool) -> Result<String, String> {
    let reg = state.reg();
    hubcodes_core::serve::decide_pending(&state.tmux, &reg, id, approve, &state.me()).map_err(|e| format!("{e:#}"))
}

/// Settings → Remote access: who did what through the web page.
#[tauri::command]
pub fn audit_log(state: State<'_, Arc<AppState>>, limit: usize) -> Result<Vec<hubcodes_core::db::EventRow>, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    db.audit(limit.clamp(1, 500)).map_err(err)
}

#[tauri::command]
pub fn tailscale_info() -> serde_json::Value {
    serde_json::json!({ "ip": hubcodes_core::serve::tailscale_ip().map(|i| i.to_string()) })
}
