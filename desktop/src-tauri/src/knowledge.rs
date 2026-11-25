//! Memory and knowledge-base lookups.

use super::*;

/// Test a memory config without saving it (Settings → Memory → Test).
#[tauri::command]
pub async fn memory_ping(state: State<'_, Arc<AppState>>, config: MemoryConfig) -> Result<MemoryPing, String> {
    let session = state.tmux.session.clone();
    tauri::async_runtime::spawn_blocking(move || memory::ping(&config, &session)).await.map_err(err)
}

#[tauri::command]
pub async fn memory_recall(state: State<'_, Arc<AppState>>, query: String, limit: usize) -> Result<Vec<Memory>, String> {
    let (cfg, session) = (state.reg().memory.clone(), state.tmux.session.clone());
    tauri::async_runtime::spawn_blocking(move || memory::recall(&cfg, &session, &query, limit.clamp(1, 50)).map_err(|e| format!("{e:#}"))).await.map_err(err)?
}

#[tauri::command]
pub async fn memory_retain(state: State<'_, Arc<AppState>>, text: String, tags: String) -> Result<String, String> {
    let (cfg, session, me) = (state.reg().memory.clone(), state.tmux.session.clone(), state.me());
    tauri::async_runtime::spawn_blocking(move || memory::retain(&cfg, &session, &me, &text, &tags).map_err(|e| format!("{e:#}"))).await.map_err(err)?
}

/// Test one knowledge source without saving.
#[tauri::command]
pub async fn kb_check(source: KbSource) -> Result<KbCheck, String> {
    tauri::async_runtime::spawn_blocking(move || kb::check(&source)).await.map_err(err)
}

#[tauri::command]
pub async fn kb_search(state: State<'_, Arc<AppState>>, query: String, source: Option<String>, limit: usize) -> Result<serde_json::Value, String> {
    let (kbs, session) = (state.reg().kbs.clone(), state.tmux.session.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let (hits, errors): (Vec<KbHit>, Vec<String>) = kb::search(&kbs, source.as_deref().filter(|s| !s.is_empty()), &query, limit.clamp(1, 50));
        kb::record(&session, "human", &query, &hits);
        serde_json::json!({ "hits": hits, "errors": errors })
    })
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn kb_get(state: State<'_, Arc<AppState>>, source: String, reference: String) -> Result<String, String> {
    let kbs = state.reg().kbs.clone();
    tauri::async_runtime::spawn_blocking(move || kb::get(&kbs, &source, &reference).map_err(|e| format!("{e:#}"))).await.map_err(err)?
}
