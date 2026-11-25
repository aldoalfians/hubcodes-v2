//! Settings: providers, accounts, `hubcodes.toml` read and write, policy test.

use super::*;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDto {
    pub name: String,
    pub command: String,
    pub glyph: String,
    pub color: Option<u8>,
    pub installed: bool,
    /// Suggested models; empty when the provider takes no model flag.
    pub models: Vec<String>,
    pub takes_model: bool,
    pub takes_role: bool,
    /// Logins known for it: `default` plus the account folders.
    pub accounts: Vec<String>,
}

/// Editable view of a provider for the Settings dialog.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEditDto {
    pub name: String,
    pub command: String,
    pub glyph: String,
    pub color: Option<u8>,
    pub waiting_pattern: Option<String>,
    #[serde(default)]
    pub model_flag: Option<String>,
    #[serde(default)]
    pub role_flag: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub models_command: Option<String>,
    #[serde(default)]
    pub approve_keys: Vec<String>,
    #[serde(default)]
    pub deny_keys: Vec<String>,
    #[serde(default)]
    pub resume_flag: Option<String>,
    #[serde(default)]
    pub hook_kind: Option<String>,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub installed: bool,
}

/// `ServeConfig` for the UI: plain field names (`shares`, not the TOML's `share`).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServeDto {
    #[serde(default)]
    pub bind: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub shares: Vec<Share>,
    #[serde(default)]
    pub web_root: String,
}

impl From<&ServeConfig> for ServeDto {
    fn from(s: &ServeConfig) -> Self {
        ServeDto { bind: s.bind.clone(), port: s.port, token: s.token.clone(), shares: s.shares.clone(), web_root: s.web_root.clone() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDto {
    /// File `save_config` writes to.
    pub path: String,
    pub exists: bool,
    pub providers: Vec<ProviderEditDto>,
    pub notifications: bool,
    pub default_folder: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub max_rounds: u32,
    /// What a person types to an agent goes to the orchestrator first.
    #[serde(default = "yes")]
    pub route_through_orchestrator: bool,
    #[serde(default)]
    pub self_name: String,
    #[serde(default)]
    pub peers: Vec<Peer>,
    #[serde(default)]
    pub prompts: Vec<Prompt>,
    #[serde(default)]
    pub templates: Vec<Template>,
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    #[serde(default)]
    pub max_tokens_per_agent: i64,
    #[serde(default)]
    pub max_tokens_per_loop: i64,
    #[serde(default)]
    pub serve: ServeDto,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub kbs: Vec<KbSource>,
    #[serde(default)]
    pub sandbox: sandbox::SandboxConfig,
    #[serde(default)]
    pub alerts: providers::AlertsConfig,
    #[serde(default)]
    pub failover: hubcodes_core::failover::FailoverConfig,
    #[serde(default)]
    pub autoteam: hubcodes_core::autoteam::AutoTeamConfig,
    #[serde(default)]
    pub skills: hubcodes_core::skills::SkillsConfig,
    #[serde(default)]
    pub board: hubcodes_core::board::BoardConfig,
    #[serde(default)]
    pub policy: PolicyDto,
    #[serde(default)]
    pub playbooks: Vec<hubcodes_core::playbook::Playbook>,
    #[serde(default)]
    pub trackers: Vec<hubcodes_core::tracker::Tracker>,
    #[serde(default)]
    pub connections: Vec<hubcodes_core::connections::Connection>,
}

#[tauri::command]
pub fn providers(state: State<'_, Arc<AppState>>) -> Vec<ProviderDto> {
    state
        .reg()
        .providers
        .iter()
        .map(|p| ProviderDto {
            name: p.name.clone(),
            command: p.command.clone(),
            glyph: p.glyph.clone(),
            color: p.color_index(),
            installed: p.installed(),
            models: p.models.clone(),
            takes_model: p.model_flag.is_some(),
            takes_role: p.role_flag.is_some(),
            accounts: hubcodes_core::accounts::list(&p.name),
        })
        .collect()
}

/// Accounts of a provider (`default` first).
#[tauri::command]
pub fn account_list(provider: String) -> Vec<String> {
    hubcodes_core::accounts::list(&provider)
}

#[tauri::command]
pub fn account_add(state: State<'_, Arc<AppState>>, provider: String, name: String) -> Result<Vec<String>, String> {
    let reg = state.reg();
    let p = reg.get(&provider).ok_or_else(|| format!("unknown provider '{provider}'"))?;
    hubcodes_core::accounts::add(&provider, &name).map_err(err)?;
    hubcodes_core::accounts::prepare(&provider, p.hook_kind.as_deref(), &name).map_err(err)?;
    Ok(hubcodes_core::accounts::list(&provider))
}

#[tauri::command]
pub fn account_remove(state: State<'_, Arc<AppState>>, provider: String, name: String) -> Result<Vec<String>, String> {
    hubcodes_core::accounts::remove(&provider, &name).map_err(err)?;
    state.status_cache.lock().unwrap_or_else(|e| e.into_inner()).remove(&format!("{provider}@{name}"));
    Ok(hubcodes_core::accounts::list(&provider))
}

/// Open the tool under an account in its own window so the person can sign
/// in there; returns the pane's agent name.
#[tauri::command]
pub fn account_login(state: State<'_, Arc<AppState>>, provider: String, name: String) -> Result<String, String> {
    state.ensure_session().map_err(err)?;
    let reg = state.reg();
    let p = reg.get(&provider).ok_or_else(|| format!("unknown provider '{provider}'"))?;
    if hubcodes_core::accounts::dir(&provider, &name).is_none() {
        return Err("name an account other than default".into());
    }
    hubcodes_core::accounts::prepare(&provider, p.hook_kind.as_deref(), &name).map_err(err)?;
    let agent = format!("{provider}-{name}-login");
    if state.tmux.pane_by_name(&agent).is_ok() {
        return Ok(agent);
    }
    let path_part = state.cli.as_ref().and_then(|c| c.parent()).map(|d| format!("PATH={}:\"$PATH\" ", sh_quote(&d.display().to_string()))).unwrap_or_default();
    let cmd = format!("{path_part}{}{}", hubcodes_core::accounts::env_prefix(&provider, p.hook_kind.as_deref(), &name), p.command.trim());
    state.tmux.spawn_with_account(&agent, &provider, &cmd, None, None, None, Some(&name)).map_err(err)?;
    state.tmux.set_private(&agent, true).map_err(err)?;
    Ok(agent)
}

/// A sign-in that needs a terminal or a browser, opened as a pane on the
/// canvas: `github` (gh auth login --web), `twg` (twg login), or
/// `provider:<name>` for a coding tool's own sign-in (`hermes model`,
/// `codex login`…, see models::login_command). Only these, never a free command. (An MCP server's browser sign-in needs no pane: it
/// happens when the server is first reached, see `mcp_discover`.)
#[tauri::command]
pub fn login_terminal(state: State<'_, Arc<AppState>>, kind: String) -> Result<String, String> {
    state.ensure_session().map_err(err)?;
    let (agent, cmd) = match kind.as_str() {
        "github" => ("github-login".to_string(), "gh auth login --web --git-protocol https".to_string()),
        "twg" => ("twg-login".to_string(), "twg login".to_string()),
        k => match k.strip_prefix("provider:").and_then(|p| hubcodes_core::models::login_command(p).map(|c| (p, c))) {
            Some((p, c)) => (format!("{p}-login"), c.to_string()),
            None => return Err(format!("no sign-in for '{k}'")),
        },
    };
    if state.tmux.pane_by_name(&agent).is_ok() {
        return Ok(agent);
    }
    let path_part = state.cli.as_ref().and_then(|c| c.parent()).map(|d| format!("PATH={}:\"$PATH\" ", sh_quote(&d.display().to_string()))).unwrap_or_default();
    state.tmux.spawn_with_account(&agent, "shell", &format!("{path_part}{cmd}"), None, None, None, None).map_err(err)?;
    // Codes, keys and tokens get typed or shown here: never on a shared canvas.
    state.tmux.set_private(&agent, true).map_err(err)?;
    Ok(agent)
}

/// Installed? Logged in? Which models? Cached for 60 s; `refresh` re-reads.
#[tauri::command]
pub async fn provider_status(state: State<'_, Arc<AppState>>, name: String, refresh: bool, account: Option<String>) -> Result<ProviderStatus, String> {
    let account = account.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).unwrap_or_else(|| hubcodes_core::accounts::DEFAULT.into());
    let key = if account == hubcodes_core::accounts::DEFAULT { name.clone() } else { format!("{name}@{account}") };
    if !refresh {
        let cache = state.status_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, st)) = cache.get(&key) {
            if at.elapsed() < Duration::from_secs(60) {
                return Ok(st.clone());
            }
        }
    }
    let provider = state.reg().get(&name).cloned().ok_or_else(|| format!("unknown provider '{name}'"))?;
    let acct = account.clone();
    let st = tauri::async_runtime::spawn_blocking(move || hubcodes_core::models::status_for(&provider, &acct))
        .await
        .map_err(err)?;
    state.status_cache.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (Instant::now(), st.clone()));
    Ok(st)
}

fn yes() -> bool {
    true
}

#[tauri::command]
pub fn get_config(state: State<'_, Arc<AppState>>) -> ConfigDto {
    config_dto(&state.reg())
}

/// What Settings reads: the registry in the page's shape.
fn config_dto(reg: &Registry) -> ConfigDto {
    let path = reg.save_path();
    ConfigDto {
        exists: path.is_file(),
        path: path.display().to_string(),
        providers: reg
            .providers
            .iter()
            .map(|p| ProviderEditDto {
                name: p.name.clone(),
                command: p.command.clone(),
                glyph: p.glyph.clone(),
                color: p.color,
                waiting_pattern: p.waiting_pattern.clone(),
                model_flag: p.model_flag.clone(),
                role_flag: p.role_flag.clone(),
                models: p.models.clone(),
                models_command: p.models_command.clone(),
                approve_keys: p.approve_keys.clone(),
                deny_keys: p.deny_keys.clone(),
                resume_flag: p.resume_flag.clone(),
                hook_kind: p.hook_kind.clone(),
                builtin: providers::is_builtin(&p.name),
                installed: p.installed(),
            })
            .collect(),
        notifications: reg.desktop.notifications,
        default_folder: reg.desktop.default_folder.clone(),
        roles: reg.desktop.roles.clone(),
        max_rounds: reg.loop_cfg.max_rounds,
        route_through_orchestrator: reg.workspace.route_through_orchestrator,
        self_name: reg.sharing.name.clone(),
        peers: reg.peers.clone(),
        prompts: reg.prompts.clone(),
        templates: reg.templates.clone(),
        triggers: reg.triggers.clone(),
        max_tokens_per_agent: reg.budget.max_tokens_per_agent,
        max_tokens_per_loop: reg.budget.max_tokens_per_loop,
        serve: ServeDto::from(&reg.serve),
        memory: reg.memory.clone(),
        kbs: reg.kbs.clone(),
        sandbox: reg.sandbox.clone(),
        alerts: reg.alerts.clone(),
        failover: reg.failover.clone(),
        autoteam: reg.autoteam.clone(),
        skills: reg.skills.clone(),
        board: reg.board.clone(),
        policy: PolicyDto::from(&reg.policy),
        playbooks: reg.playbooks.clone(),
        trackers: reg.trackers.clone(),
        connections: reg.connections.clone(),
    }
}

/// What a save did: the file, and what was moved into the Keychain.
#[derive(Serialize)]
pub struct Saved {
    pub path: String,
    pub moved: Vec<String>,
}

/// Write `hubcodes.toml` and switch the running registry to it. Validation
/// errors come back as the message to show.
#[tauri::command]
pub fn save_config(state: State<'_, Arc<AppState>>, config: ConfigDto) -> Result<Saved, String> {
    let mut next = state.reg().clone();
    next.providers = config
        .providers
        .into_iter()
        .map(|p| Provider {
            name: p.name.trim().to_string(),
            command: p.command.trim().to_string(),
            glyph: p.glyph.trim().to_string(),
            color: p.color,
            waiting_pattern: p.waiting_pattern.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            model_flag: p.model_flag.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            role_flag: p.role_flag.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            models: p.models.into_iter().map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect(),
            models_command: p.models_command.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            approve_keys: p.approve_keys.into_iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect(),
            deny_keys: p.deny_keys.into_iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect(),
            resume_flag: p.resume_flag.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            hook_kind: p.hook_kind.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        })
        .collect();
    for p in &next.providers {
        if let Some(pat) = &p.waiting_pattern {
            Regex::new(pat).map_err(|e| format!("provider '{}': waiting pattern: {e}", p.name))?;
        }
    }
    next.desktop.notifications = config.notifications;
    next.desktop.default_folder = config.default_folder.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    next.desktop.roles = config.roles.into_iter().map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
    next.loop_cfg.max_rounds = config.max_rounds;
    next.workspace.route_through_orchestrator = config.route_through_orchestrator;
    next.sharing.name = config.self_name.trim().to_string();
    if next.sharing.name.is_empty() {
        next.sharing.name = remote::default_self_name();
    }
    next.prompts = config.prompts.into_iter().map(|p| Prompt { name: p.name.trim().trim_start_matches('/').to_string(), text: p.text, description: p.description.trim().to_string() }).filter(|p| !p.name.is_empty() && !p.text.trim().is_empty()).collect();
    next.templates = config.templates.into_iter().filter(|t| !t.name.trim().is_empty()).collect();
    next.triggers = config.triggers.into_iter().filter(|t| !t.name.trim().is_empty() && !t.on.trim().is_empty()).collect();
    next.budget.max_tokens_per_agent = config.max_tokens_per_agent.max(0);
    next.budget.max_tokens_per_loop = config.max_tokens_per_loop.max(0);
    next.serve = ServeConfig {
        bind: if config.serve.bind.trim().is_empty() { "tailscale".into() } else { config.serve.bind.trim().to_string() },
        port: if config.serve.port == 0 { 7433 } else { config.serve.port },
        token: config.serve.token.trim().to_string(),
        shares: config.serve.shares.into_iter().map(|s| Share { user: s.user.trim().to_string(), role: s.role }).filter(|s| !s.user.is_empty()).collect(),
        web_root: config.serve.web_root.trim().to_string(),
    };
    next.memory = config.memory;
    next.kbs = config.kbs.into_iter().filter(|k| !k.name.trim().is_empty()).collect();
    next.sandbox = config.sandbox;
    next.alerts = config.alerts;
    next.autoteam = hubcodes_core::autoteam::AutoTeamConfig { roles: config.autoteam.roles.into_iter().map(|(r, n)| (r.trim().to_string(), n)).filter(|(r, _)| !r.is_empty()).collect(), ..config.autoteam };
    next.failover = hubcodes_core::failover::FailoverConfig { chain: config.failover.chain.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(), ..config.failover };
    next.skills = hubcodes_core::skills::SkillsConfig { sync: config.skills.sync, sources: config.skills.sources.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect() };
    next.board = config.board.normalized().map_err(|e| format!("{e:#}"))?;
    next.policy = config.policy.into();
    next.playbooks = config.playbooks.into_iter().filter(|p| !p.name.trim().is_empty()).map(|mut p| { p.steps.retain(|st| !st.name.trim().is_empty()); p }).collect();
    for p in &next.playbooks {
        for st in &p.steps {
            if st.prompt.trim().is_empty() && st.action.trim().is_empty() {
                return Err(format!("playbook '{}': step '{}' needs a prompt or an action", p.name, st.name));
            }
            if !matches!(st.gate.as_str(), "" | "none" | "human" | "test" | "review") {
                return Err(format!("playbook '{}': step '{}': gate must be none, human, test or review", p.name, st.name));
            }
        }
    }
    next.trackers = config.trackers.into_iter().filter(|t| !t.name.trim().is_empty()).collect();
    next.peers = config
        .peers
        .into_iter()
        .map(|p| Peer { name: p.name.trim().to_string(), host: p.host.trim().to_string(), user: p.user.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()), session: if p.session.trim().is_empty() { "hubcodes".into() } else { p.session.trim().to_string() }, hubcodes: if p.hubcodes.trim().is_empty() { "hubcodes".into() } else { p.hubcodes.trim().to_string() }, max_agents: p.max_agents, allow: p.allow.into_iter().map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty()).collect(), transport: p.transport.trim().to_lowercase(), port: p.port })
        .collect();
    next.connections = config.connections.into_iter().filter(|c| !c.name.trim().is_empty()).map(|mut c| { c.name = c.name.trim().to_string(); c.url = c.url.trim().trim_end_matches('/').to_string(); c }).collect();
    // No plain secret is written: tokens go to the Keychain, the file keeps a reference.
    let moved = next.stash_secrets().map_err(|e| format!("{e:#}. Nothing was saved; use ${{ENV_VAR}} for this token instead."))?;
    let path = next.save().map_err(|e| format!("{e:#}"))?;
    next.source = Some(path.clone());
    *state.reg.write().unwrap_or_else(|e| e.into_inner()) = next;
    state.status_cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    Ok(Saved { path: path.display().to_string(), moved })
}

#[tauri::command]
pub fn policy_test(state: State<'_, Arc<AppState>>, policy: PolicyDto, command: String) -> hubcodes_core::policy::Decision {
    let _ = state;
    hubcodes_core::policy::decide_command(&policy.into(), &command)
}

/// The policy as the app sends and reads it: camelCase, every list always
/// present. The core type is snake_case (it is also the TOML shape) and
/// leaves empty lists out, which the page read as missing fields.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PolicyDto {
    pub mode: String,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub allow_tools: Vec<String>,
    pub deny_paths: Vec<String>,
}

impl From<&hubcodes_core::policy::PolicyConfig> for PolicyDto {
    fn from(p: &hubcodes_core::policy::PolicyConfig) -> Self {
        PolicyDto { mode: p.mode.clone(), allow: p.allow.clone(), deny: p.deny.clone(), allow_tools: p.allow_tools.clone(), deny_paths: p.deny_paths.clone() }
    }
}

impl From<PolicyDto> for hubcodes_core::policy::PolicyConfig {
    fn from(p: PolicyDto) -> Self {
        let clean = |v: Vec<String>| v.into_iter().map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
        hubcodes_core::policy::PolicyConfig {
            mode: if p.mode.trim().is_empty() { "off".into() } else { p.mode.trim().to_lowercase() },
            allow: clean(p.allow),
            deny: clean(p.deny),
            allow_tools: clean(p.allow_tools),
            deny_paths: clean(p.deny_paths),
        }
    }
}

#[cfg(test)]
mod policy_dto_tests {
    use super::*;

    /// Prints what Settings receives, so the page's types can be checked
    /// against it (`HUBCODES_DUMP_CONFIG=path cargo test dump`).
    #[test]
    fn dump_the_config_the_page_reads() {
        let Ok(out) = std::env::var("HUBCODES_DUMP_CONFIG") else { return };
        // Synthetic, so no real config (or secret) is read: one of every list.
        let toml = r#"
[[peer]]
name = "mini"
host = "mac-mini"
[[kb]]
name = "docs"
kind = "mcp"
url = "https://example.invalid/mcp"
search_tool = "search"
[[tracker]]
name = "jira"
kind = "jira"
[[playbook]]
name = "feature"
[[playbook.step]]
name = "build"
[[template]]
name = "duo"
[[template.agents]]
name = "dev"
provider = "claude"
[[trigger]]
name = "nightly"
on = "cron 0 2 * * *"
action = "send dev hi"
[[prompt]]
name = "review"
text = "Review this"
[memory]
provider = "hindsight"
[memory.hindsight]
url = "https://example.invalid"
api_key = "${HINDSIGHT_API_KEY}"
[policy]
mode = "auto"
allow_tools = ["Read"]
[serve]
token = "${SERVE_TOKEN}"
[[serve.share]]
user = "a@example.invalid"
role = "viewer"
"#;
        let file = std::env::temp_dir().join(format!("hubcodes-dump-{}.toml", std::process::id()));
        std::fs::write(&file, toml).unwrap();
        let reg = Registry::from_file(&file).expect("synthetic config parses");
        let _ = std::fs::remove_file(&file);
        std::fs::write(out, serde_json::to_string_pretty(&config_dto(&reg)).unwrap()).unwrap();
    }

    #[test]
    fn the_policy_crosses_to_the_page_in_camel_case_with_every_list() {
        let core = hubcodes_core::policy::PolicyConfig { mode: "auto".into(), allow: vec!["ls*".into()], deny: vec![], allow_tools: vec!["Read".into()], deny_paths: vec![] };
        let json = serde_json::to_value(PolicyDto::from(&core)).unwrap();
        assert_eq!(json["allowTools"], serde_json::json!(["Read"]));
        assert_eq!(json["deny"], serde_json::json!([]));
        assert_eq!(json["denyPaths"], serde_json::json!([]));
        let back: PolicyDto = serde_json::from_value(serde_json::json!({ "mode": "auto", "allow": [], "deny": [], "allowTools": ["Grep "], "denyPaths": ["*.env"] })).unwrap();
        let core2: hubcodes_core::policy::PolicyConfig = back.into();
        assert_eq!(core2.allow_tools, vec!["Grep".to_string()]);
        assert_eq!(core2.deny_paths, vec!["*.env".to_string()]);
    }
}

#[tauri::command]
pub fn provider_export(state: State<'_, Arc<AppState>>, name: String) -> Result<String, String> {
    let reg = state.reg();
    let p = reg.get(&name).ok_or_else(|| format!("no provider {name}"))?;
    providers::export_provider(p).map_err(err)
}

#[tauri::command]
pub fn provider_import(state: State<'_, Arc<AppState>>, text: String) -> Result<Vec<String>, String> {
    let paths = providers::import_provider_text(&text).map_err(|e| format!("{e:#}"))?;
    // Reload so the new provider shows up at once.
    if let Ok(next) = Registry::load() {
        *state.reg.write().unwrap_or_else(|e| e.into_inner()) = next;
    }
    state.status_cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    Ok(paths.into_iter().map(|p| p.display().to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI reads `serve.shares`; the TOML name is `share`.
    #[test]
    fn serve_dto_uses_ui_field_names() {
        let mut cfg = ServeConfig::default();
        cfg.shares.push(Share { user: "a@b".into(), role: "viewer".into() });
        let v = serde_json::to_value(ServeDto::from(&cfg)).unwrap();
        assert!(v["shares"].is_array());
        assert_eq!(v["shares"][0]["user"], "a@b");
        assert!(v.get("share").is_none());
    }
}
