//! Settings → Accounts and the pickers that read from them: check a
//! connection, list its projects / statuses / spaces / repos / labels, and
//! find an MCP server's tools so nobody types tool names or JSON.

use super::*;
use hubcodes_core::connections::{self, Connection};
use hubcodes_core::mcp::{self, McpTarget};

/// Who the connection signs in as, or why it does not.
#[tauri::command]
pub async fn connection_check(connection: Connection) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || connections::check(&connection).map_err(|e| format!("{e:#}"))).await.map_err(err)?
}

/// The Atlassian sites the twg CLI is signed in to here; empty without twg.
#[tauri::command]
pub async fn twg_profiles() -> Result<Vec<hubcodes_core::twg::Profile>, String> {
    tauri::async_runtime::spawn_blocking(|| hubcodes_core::twg::profiles().unwrap_or_default()).await.map_err(err)
}

/// Choices for a picker: `projects`, `statuses` (arg = project), `spaces`,
/// `repos`, `labels` (arg = repo). `{ok, choices, error}`.
#[tauri::command]
pub async fn connection_choices(connection: Connection, what: String, arg: Option<String>) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || connections::choices_json(&connection, &what, arg.as_deref().unwrap_or(""))).await.map_err(err)
}

#[derive(Serialize)]
pub struct Discovered {
    pub tools: Vec<mcp::Tool>,
    /// purpose → (tool, arguments)
    pub picks: std::collections::BTreeMap<String, (String, serde_json::Value)>,
}

/// Connect to an MCP server, list its tools and pick one per purpose.
#[tauri::command]
pub async fn mcp_discover(target: McpTarget, purposes: Vec<String>) -> Result<Discovered, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p: Vec<&str> = purposes.iter().map(String::as_str).collect();
        let (tools, picks) = mcp::discover(&target, &p).map_err(|e| format!("{e:#}"))?;
        Ok(Discovered { tools, picks: picks.into_iter().map(|(purpose, n, a)| (purpose, (n, a))).collect() })
    })
    .await
    .map_err(err)?
}
