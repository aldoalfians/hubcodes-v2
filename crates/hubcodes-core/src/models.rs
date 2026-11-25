//! What each provider can run: is the CLI installed, is it logged in, which
//! models does it know, which one is current. Read from the files the CLIs
//! keep themselves (no network), so the list is what the user would see in
//! the tool's own `/model` picker. Custom providers can set `models_command`.

use crate::providers::Provider;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// What the provider's model flag takes.
    pub id: String,
    pub label: String,
    /// Version, description, or where it comes from. Empty when unknown.
    pub note: String,
    /// The provider's own default right now.
    pub current: bool,
}

/// Setup steps shown when a provider is missing or not logged in.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    pub install: Option<String>,
    pub login: Option<String>,
    pub docs: Option<String>,
    /// How hubcodes finds the model list for this provider.
    pub models_from: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub name: String,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// `None` = hubcodes has no way to tell for this provider.
    pub logged_in: Option<bool>,
    pub config_path: Option<String>,
    pub current_model: Option<String>,
    pub models: Vec<ModelInfo>,
    /// Where `models` came from, for the UI ("~/.codex/models_cache.json").
    pub source: String,
    pub setup: Setup,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

fn tilde(p: &Path) -> String {
    let h = home();
    p.strip_prefix(&h).map(|r| format!("~/{}", r.display())).unwrap_or_else(|_| p.display().to_string())
}

fn read_json(p: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

fn which(bin: &str) -> Option<PathBuf> {
    if bin.starts_with('$') {
        return None;
    }
    if bin.contains('/') {
        return Path::new(bin).is_file().then(|| PathBuf::from(bin));
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(bin)).find(|p| p.is_file())
}

/// `cmd --version`, first line, with a time limit (some CLIs phone home).
fn version_of(bin: &str) -> Option<String> {
    run_capture(bin, &["--version"], Duration::from_secs(4))
        .and_then(|out| out.lines().next().map(|l| l.trim().to_string()))
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(60).collect())
}

fn run_capture(bin: &str, args: &[&str], limit: Duration) -> Option<String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let mut s = String::new();
                if let Some(mut o) = child.stdout.take() {
                    use std::io::Read;
                    let _ = o.read_to_string(&mut s);
                }
                return st.success().then_some(s);
            }
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

fn run_shell(cmd: &str, limit: Duration) -> Option<String> {
    run_capture("/bin/sh", &["-lc", cmd], limit)
}

/// Full status for one provider. Reads files and may run the CLI once
/// (`--version`, `opencode models`), so call it from a worker, not the UI.
pub fn status(p: &Provider) -> ProviderStatus {
    status_for(p, crate::accounts::DEFAULT)
}

/// [`status`] as seen from one account: the tool's files under that
/// account's folder (login, settings, model cache) instead of its home.
pub fn status_for(p: &Provider, account: &str) -> ProviderStatus {
    let paths = crate::accounts::paths(&p.name, p.hook_kind.as_deref(), account);
    let bin = p.command.split_whitespace().next().unwrap_or("");
    let path = which(bin);
    let installed = p.installed();
    let version = path.as_ref().and_then(|_| version_of(bin));
    let mut st = ProviderStatus {
        name: p.name.clone(),
        installed,
        path: path.map(|p| p.display().to_string()),
        version,
        logged_in: None,
        config_path: None,
        current_model: None,
        models: Vec::new(),
        source: String::new(),
        setup: Setup::default(),
    };
    match p.hook_kind.as_deref().unwrap_or(p.name.as_str()) {
        "claude" => claude(&mut st, paths.claude_dir.as_deref()),
        "codex" => codex(&mut st, paths.codex_dir.as_deref()),
        "opencode" => opencode(&mut st, installed, paths.opencode_data.as_deref()),
        "gemini" => gemini(&mut st, paths.gemini_dir.as_deref()),
        "agy" => agy(&mut st, installed),
        "hermes" => hermes(&mut st, None),
        "pi" => pi(&mut st, paths.pi_agent_dir.as_deref()),
        "omp" => omp(&mut st, paths.omp_profile.as_deref()),
        "shell" => {
            st.logged_in = Some(true);
            st.installed = true;
            st.setup.models_from = "A shell has no model.".into();
        }
        _ => {
            st.setup.models_from = match &p.models_command {
                Some(c) => format!("`{c}`, one model id per line"),
                None => "`models` in hubcodes.toml; set `models_command` to read them from the tool".into(),
            };
        }
    }
    // A command set in hubcodes.toml wins over the built-in reader.
    if let Some(cmd) = &p.models_command {
        if let Some(out) = run_shell(cmd, Duration::from_secs(10)) {
            let found: Vec<ModelInfo> = out
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| ModelInfo { id: l.into(), label: l.into(), note: String::new(), current: false })
                .collect();
            if !found.is_empty() {
                st.models = found;
                st.source = format!("`{cmd}`");
            }
        }
    }
    // Configured suggestions fill in when nothing was discovered.
    if st.models.is_empty() && !p.models.is_empty() {
        st.models = p.models.iter().map(|m| ModelInfo { id: m.clone(), label: m.clone(), note: String::new(), current: false }).collect();
        if st.source.is_empty() {
            st.source = "hubcodes.toml".into();
        }
    }
    if let Some(cur) = st.current_model.clone() {
        let mut marked = false;
        for m in &mut st.models {
            if m.id == cur {
                m.current = true;
                marked = true;
            }
        }
        if !marked {
            st.models.insert(0, ModelInfo { id: cur.clone(), label: cur, note: "current default".into(), current: true });
        }
    }
    if p.model_flag.is_none() && p.name != "shell" {
        st.setup.models_from = format!("{} No model_flag in hubcodes.toml, so a model cannot be passed.", st.setup.models_from).trim().to_string();
    }
    st
}

fn claude(st: &mut ProviderStatus, config_dir: Option<&Path>) {
    // Under CLAUDE_CONFIG_DIR everything sits in that one folder.
    let (state, settings) = match config_dir {
        Some(d) => (d.join(".claude.json"), d.join("settings.json")),
        None => (home().join(".claude.json"), home().join(".claude").join("settings.json")),
    };
    st.config_path = Some(tilde(&settings));
    st.setup = Setup {
        install: Some("npm install -g @anthropic-ai/claude-code".into()),
        login: Some("claude   # then /login in the prompt".into()),
        docs: Some("https://docs.anthropic.com/en/docs/claude-code".into()),
        models_from: "the model picker cache in ~/.claude.json plus the built-in aliases".into(),
    };
    let mut models = vec![
        ModelInfo { id: "default".into(), label: "Default".into(), note: "your account's recommended model".into(), current: false },
        ModelInfo { id: "opus".into(), label: "Opus".into(), note: "alias → latest Opus".into(), current: false },
        ModelInfo { id: "sonnet".into(), label: "Sonnet".into(), note: "alias → latest Sonnet".into(), current: false },
        ModelInfo { id: "haiku".into(), label: "Haiku".into(), note: "alias → latest Haiku".into(), current: false },
    ];
    if let Some(j) = read_json(&state) {
        st.logged_in = Some(j.get("oauthAccount").map(|a| !a.is_null()).unwrap_or(false) || std::env::var_os("ANTHROPIC_API_KEY").is_some());
        for group in ["additionalModelOptionsCache", "modelAccessCache"] {
            if let Some(arr) = j.get(group).and_then(|v| v.as_array()) {
                for m in arr {
                    let id = m.get("value").and_then(|v| v.as_str()).unwrap_or("");
                    if id.is_empty() || models.iter().any(|x| x.id == id) {
                        continue;
                    }
                    models.push(ModelInfo {
                        id: id.into(),
                        label: m.get("label").and_then(|v| v.as_str()).unwrap_or(id).into(),
                        note: m.get("description").and_then(|v| v.as_str()).unwrap_or("").into(),
                        current: false,
                    });
                }
            }
        }
        st.source = tilde(&state);
    } else {
        st.logged_in = Some(false);
        st.source = "built-in aliases".into();
    }
    if let Some(s) = read_json(&settings) {
        st.current_model = s.get("model").and_then(|v| v.as_str()).map(String::from);
    }
    st.models = models;
}

fn codex(st: &mut ProviderStatus, codex_home: Option<&Path>) {
    let dir = codex_home.map(Path::to_path_buf).unwrap_or_else(|| home().join(".codex"));
    st.config_path = Some(tilde(&dir.join("config.toml")));
    st.logged_in = Some(dir.join("auth.json").is_file());
    st.setup = Setup {
        install: Some("npm install -g @openai/codex".into()),
        login: Some("codex login".into()),
        docs: Some("https://github.com/openai/codex".into()),
        models_from: "~/.codex/models_cache.json (what Codex's /model shows) and `model` in config.toml".into(),
    };
    if let Some(j) = read_json(&dir.join("models_cache.json")) {
        if let Some(arr) = j.get("models").and_then(|v| v.as_array()) {
            for m in arr {
                if m.get("visibility").and_then(|v| v.as_str()) == Some("hide") {
                    continue;
                }
                let id = m.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    continue;
                }
                st.models.push(ModelInfo {
                    id: id.into(),
                    label: m.get("display_name").and_then(|v| v.as_str()).unwrap_or(id).into(),
                    note: m.get("description").and_then(|v| v.as_str()).unwrap_or("").into(),
                    current: false,
                });
            }
            st.source = tilde(&dir.join("models_cache.json"));
        }
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("config.toml")) {
        if let Ok(t) = text.parse::<toml::Table>() {
            st.current_model = t.get("model").and_then(|v| v.as_str()).map(String::from);
        }
    }
}

fn opencode(st: &mut ProviderStatus, installed: bool, data_dir: Option<&Path>) {
    let cfg = home().join(".config").join("opencode").join("opencode.json");
    st.config_path = Some(tilde(&cfg));
    let auth = data_dir.map(Path::to_path_buf).unwrap_or_else(|| home().join(".local").join("share")).join("opencode").join("auth.json");
    let j = read_json(&cfg);
    let configured = j.as_ref().and_then(|j| j.get("provider")).and_then(|v| v.as_object()).map(|o| !o.is_empty()).unwrap_or(false);
    st.logged_in = Some(auth.is_file() || configured);
    st.setup = Setup {
        install: Some("npm install -g opencode-ai   # or: brew install sst/tap/opencode".into()),
        login: Some("opencode auth login".into()),
        docs: Some("https://opencode.ai/docs".into()),
        models_from: "`opencode models` (every provider you logged in to) and `model` in opencode.json".into(),
    };
    st.current_model = j.and_then(|j| j.get("model").and_then(|v| v.as_str()).map(String::from));
    if installed {
        if let Some(out) = run_capture("opencode", &["models"], Duration::from_secs(12)) {
            st.models = out
                .lines()
                .map(str::trim)
                .filter(|l| l.contains('/') && !l.contains(' '))
                .map(|l| ModelInfo { id: l.into(), label: l.rsplit('/').next().unwrap_or(l).into(), note: l.rsplit_once('/').map(|(p, _)| p).unwrap_or("").into(), current: false })
                .collect();
            st.source = "`opencode models`".into();
        }
    }
}

fn gemini(st: &mut ProviderStatus, gemini_dir: Option<&Path>) {
    let dir = gemini_dir.map(Path::to_path_buf).unwrap_or_else(|| home().join(".gemini"));
    st.config_path = Some(tilde(&dir.join("settings.json")));
    st.logged_in = Some(dir.join("oauth_creds.json").is_file() || std::env::var_os("GEMINI_API_KEY").is_some());
    st.setup = Setup {
        install: Some("npm install -g @google/gemini-cli".into()),
        login: Some("gemini   # signs in with Google on first run, or export GEMINI_API_KEY".into()),
        docs: Some("https://github.com/google-gemini/gemini-cli".into()),
        models_from: "the built-in list; Gemini CLI keeps no local catalog".into(),
    };
    if let Some(j) = read_json(&dir.join("settings.json")) {
        st.current_model = j.get("model").and_then(|v| v.as_str()).map(String::from);
    }
    st.models = ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite"]
        .iter()
        .map(|m| ModelInfo { id: m.to_string(), label: m.to_string(), note: String::new(), current: false })
        .collect();
    st.source = "built-in list".into();
}

/// Antigravity CLI: signs in with Google on its first run (the login lives in
/// the system keyring, not a file); `agy models` lists what the account may
/// use, and answering at all is the sign that it is signed in.
fn agy(st: &mut ProviderStatus, installed: bool) {
    st.config_path = Some(tilde(&home().join(".gemini/antigravity-cli/settings.json")));
    st.setup = Setup {
        install: Some("curl -fsSL https://antigravity.google/cli/install.sh | bash".into()),
        login: Some("agy   # signs in with Google on first run".into()),
        docs: Some("https://antigravity.google/docs/getting-started/".into()),
        models_from: "`agy models` (needs a signed-in session)".into(),
    };
    if let Some(j) = read_json(&home().join(".gemini/antigravity-cli/settings.json")) {
        st.current_model = j.get("model").and_then(|v| v.as_str()).map(String::from);
    }
    if !installed {
        return;
    }
    match run_capture("agy", &["models"], Duration::from_secs(20)) {
        Some(out) => {
            st.models = agy_models(&out);
            st.logged_in = Some(true);
            st.source = "`agy models`".into();
        }
        None => st.logged_in = None,
    }
}

/// Hermes Agent keeps secrets in `~/.hermes/.env` (API keys, tokens) and
/// OAuth logins next to it; `hermes model` adds a provider and signs in. It
/// has no model catalog to read, so the list is the default it is set to.
fn hermes(st: &mut ProviderStatus, dir: Option<&Path>) {
    let dir = dir.map(Path::to_path_buf).or_else(|| std::env::var_os("HERMES_HOME").map(PathBuf::from)).unwrap_or_else(|| home().join(".hermes"));
    st.config_path = Some(tilde(&dir.join("config.yaml")));
    st.setup = Setup {
        install: Some("curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash".into()),
        login: Some("hermes model   # pick a provider, then sign in or paste its API key".into()),
        docs: Some("https://hermes-agent.nousresearch.com/docs/getting-started/quickstart".into()),
        models_from: "the default in ~/.hermes/config.yaml; any id the signed-in provider takes works".into(),
    };
    let env = std::fs::read_to_string(dir.join(".env")).unwrap_or_default();
    st.logged_in = Some(hermes_has_credential(&env) || dir.join("auth.json").is_file());
    if let Ok(cfg) = std::fs::read_to_string(dir.join("config.yaml")) {
        st.current_model = hermes_default_model(&cfg);
    }
    if let Some(m) = &st.current_model {
        st.models = vec![ModelInfo { id: m.clone(), label: m.rsplit('/').next().unwrap_or(m).into(), note: "Hermes default".into(), current: true }];
        st.source = "~/.hermes/config.yaml".into();
    }
}

/// A key or token with a value in Hermes' `.env` (comments and blanks skipped).
fn hermes_has_credential(env: &str) -> bool {
    env.lines().map(str::trim).filter(|l| !l.starts_with('#')).filter_map(|l| l.strip_prefix("export ").unwrap_or(l).split_once('=')).any(|(k, v)| {
        let k = k.trim();
        (k.ends_with("_API_KEY") || k.ends_with("_TOKEN") || k.ends_with("_KEY")) && !v.trim().trim_matches(['"', '\'']).is_empty()
    })
}

/// `model: id` or `model:` with a nested `default:` / `name:` in config.yaml.
fn hermes_default_model(cfg: &str) -> Option<String> {
    let clean = |v: &str| Some(v.trim().trim_matches(['"', '\'']).to_string()).filter(|s| !s.is_empty());
    let mut in_model = false;
    for line in cfg.lines() {
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented {
            in_model = false;
            if let Some(v) = line.strip_prefix("model:") {
                if let Some(m) = clean(v) {
                    return Some(m);
                }
                in_model = true;
            }
            continue;
        }
        if in_model {
            let l = line.trim();
            for k in ["default:", "name:", "model:"] {
                if let Some(v) = l.strip_prefix(k) {
                    if let Some(m) = clean(v) {
                        return Some(m);
                    }
                }
            }
        }
    }
    None
}

/// The command that signs a built-in provider in, as it is run in a pane
/// (the `login` hint without its comment). None for tools that sign in on
/// their first run inside the agent itself.
pub fn login_command(provider: &str) -> Option<&'static str> {
    match provider {
        "hermes" => Some("hermes model"),
        "codex" => Some("codex login"),
        "opencode" => Some("opencode auth login"),
        "claude" => Some("claude"),
        "gemini" => Some("gemini"),
        "agy" => Some("agy"),
        _ => None,
    }
}

/// Model ids out of `agy models`, whatever its layout (a list or a table):
/// the first id-shaped token of each line (`gemini-4-neon-xhigh`).
pub fn agy_models(out: &str) -> Vec<ModelInfo> {
    let mut seen: Vec<ModelInfo> = Vec::new();
    for line in out.lines() {
        let Some(id) = line
            .split(|c: char| c.is_whitespace() || c == '|' || c == ',')
            .map(|w| w.trim_matches(|c: char| matches!(c, '*' | '`' | '"' | '\'' | '•' | '-' | ':' | '(' | ')')))
            .find(|w| w.len() > 2 && w.contains('-') && w.chars().all(|c| c.is_ascii_alphanumeric() || "-._/".contains(c)) && w.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false))
        else {
            continue;
        };
        if !seen.iter().any(|m| m.id == id) {
            let label = line.trim().trim_start_matches(['*', '-', '•', ' ']).to_string();
            seen.push(ModelInfo { id: id.to_string(), label: if label == id { id.to_string() } else { label }, note: String::new(), current: false });
        }
    }
    seen
}

fn pi(st: &mut ProviderStatus, agent_dir: Option<&Path>) {
    let dir = agent_dir.map(Path::to_path_buf).unwrap_or_else(|| home().join(".pi").join("agent"));
    st.config_path = Some(tilde(&dir.join("settings.json")));
    st.logged_in = Some(dir.join("auth.json").is_file());
    st.setup = Setup {
        install: None,
        login: Some("pi   # /login inside, then `pi update` refreshes its model catalog".into()),
        docs: None,
        models_from: "~/.pi/agent/models.json and models-store.json (provider/id)".into(),
    };
    for file in ["models.json", "models-store.json"] {
        let Some(j) = read_json(&dir.join(file)) else { continue };
        let providers = j.get("providers").and_then(|v| v.as_object()).or_else(|| j.as_object());
        let Some(providers) = providers else { continue };
        for (pname, pv) in providers {
            let Some(models) = pv.get("models").and_then(|v| v.as_array()) else { continue };
            for m in models {
                let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    continue;
                }
                let full = if id.contains('/') && !pname.contains('/') && id.starts_with(pname.as_str()) { id.to_string() } else { format!("{pname}/{id}") };
                if st.models.iter().any(|x| x.id == full) {
                    continue;
                }
                st.models.push(ModelInfo {
                    id: full,
                    label: m.get("name").and_then(|v| v.as_str()).unwrap_or(id).into(),
                    note: m.get("contextWindow").and_then(|v| v.as_u64()).map(|c| format!("{}k context", c / 1000)).unwrap_or_default(),
                    current: false,
                });
            }
        }
        if st.source.is_empty() {
            st.source = tilde(&dir.join(file));
        }
    }
    if let Some(j) = read_json(&dir.join("settings.json")) {
        st.current_model = j.get("model").and_then(|v| v.as_str()).map(String::from);
    }
}

fn omp(st: &mut ProviderStatus, profile: Option<&str>) {
    let dir = match profile {
        Some(p) => home().join(".omp").join("profiles").join(p).join("agent"),
        None => home().join(".omp").join("agent"),
    };
    let cfg = dir.join("config.yml");
    st.config_path = Some(tilde(&cfg));
    st.logged_in = Some(cfg.is_file());
    st.setup = Setup {
        install: None,
        login: Some("omp   # configure providers inside".into()),
        docs: None,
        models_from: "modelRoles in ~/.omp/agent/config.yml, providers in models.yml, and its catalog in models.db".into(),
    };
    let mut push = |id: String, label: String, note: String| {
        if id.is_empty() {
            return;
        }
        if let Some(m) = st.models.iter_mut().find(|m| m.id == id) {
            // Same model under several omp roles: list them all.
            if note.starts_with("omp role: ") && m.note.starts_with("omp role") {
                m.note = format!("{}, {}", m.note.replacen("omp role:", "omp roles:", 1), note.trim_start_matches("omp role: "));
            }
            return;
        }
        st.models.push(ModelInfo { id, label, note, current: false });
    };
    // 1. The roles the user assigned (default, plan, smol…): what omp runs today.
    let mut role_providers: Vec<String> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&cfg) {
        let mut in_roles = false;
        for line in text.lines() {
            if line.starts_with("modelRoles:") {
                in_roles = true;
                continue;
            }
            if in_roles {
                if !line.starts_with(' ') {
                    in_roles = false;
                    continue;
                }
                if let Some((role, value)) = line.trim().split_once(':') {
                    let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
                    if value.is_empty() {
                        continue;
                    }
                    if role.trim() == "default" {
                        st.current_model = Some(value.to_string());
                    }
                    if let Some((prov, _)) = value.split_once('/') {
                        if !role_providers.iter().any(|p| p == prov) {
                            role_providers.push(prov.to_string());
                        }
                    }
                    push(value.to_string(), value.rsplit('/').next().unwrap_or(value).to_string(), format!("omp role: {}", role.trim()));
                }
            }
        }
    }
    // 2. Custom providers in models.yml (YAML with JSON blocks): `"id": "x"` under `  name:`.
    if let Ok(text) = std::fs::read_to_string(dir.join("models.yml")) {
        let mut provider = String::new();
        for line in text.lines() {
            if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') || (line.starts_with("  ") && !line.starts_with("   ") && line.contains(": {")) {
                provider = line.trim().split(':').next().unwrap_or("").trim().to_string();
                continue;
            }
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("\"id\":") {
                let id = rest.trim().trim_end_matches(',').trim_matches('"');
                if !provider.is_empty() && !id.is_empty() {
                    push(format!("{provider}/{id}"), id.to_string(), provider.clone());
                }
            }
        }
    }
    // 3. omp's catalog for the providers in use (the whole db has thousands of models).
    let db = dir.join("models.db");
    if db.is_file() && !role_providers.is_empty() {
        let list = role_providers.iter().map(|p| format!("'{}'", p.replace('\'', ""))).collect::<Vec<_>>().join(",");
        let sql = format!("select provider_id, models from model_cache where provider_id in ({list})");
        if let Some(out) = run_capture("sqlite3", &["-json", db.to_str().unwrap_or(""), &sql], Duration::from_secs(5)) {
            if let Ok(serde_json::Value::Array(rows)) = serde_json::from_str::<serde_json::Value>(&out) {
                for row in rows {
                    let prov = row.get("provider_id").and_then(|v| v.as_str()).unwrap_or("");
                    let Some(models) = row.get("models").and_then(|v| v.as_str()).and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok()) else { continue };
                    if let Some(arr) = models.as_array() {
                        for m in arr {
                            let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            let name = m.get("name").and_then(|v| v.as_str()).unwrap_or(id);
                            let ctx = m.get("contextWindow").and_then(|v| v.as_u64()).map(|c| format!("{}k context", c / 1000)).unwrap_or_default();
                            push(format!("{prov}/{id}"), name.to_string(), ctx);
                        }
                    }
                }
            }
        }
    }
    if !st.models.is_empty() {
        st.source = "~/.omp/agent (config.yml, models.yml, models.db)".into();
    }
}

#[cfg(test)]
mod hermes_tests {
    use super::{hermes_default_model, hermes_has_credential};

    #[test]
    fn hermes_default_model_reads_flat_or_nested() {
        assert_eq!(hermes_default_model("model: anthropic/claude-opus-4.6\n").as_deref(), Some("anthropic/claude-opus-4.6"));
        assert_eq!(hermes_default_model("# x\nmodel:\n  provider: openrouter\n  default: \"openai/gpt-5.5\"\nterminal:\n  backend: local\n").as_deref(), Some("openai/gpt-5.5"));
        assert_eq!(hermes_default_model("terminal:\n  model: nope\n"), None);
    }

    #[test]
    fn hermes_credential_needs_a_value() {
        assert!(hermes_has_credential("# c\nOPENROUTER_API_KEY=sk-or-1\n"));
        assert!(!hermes_has_credential("OPENROUTER_API_KEY=\n# NOUS_TOKEN=x\n"));
        assert!(!hermes_has_credential(""));
    }
}

#[cfg(test)]
mod agy_tests {
    use super::agy_models;

    #[test]
    fn agy_model_ids_come_out_of_a_list_or_a_table() {
        let list = "gemini-4-neon-xhigh\ngemini-3.8-flash\n";
        assert_eq!(agy_models(list).iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["gemini-4-neon-xhigh", "gemini-3.8-flash"]);
        let table = "MODEL                 NAME\n* gemini-4-neon-high   Gemini 4 Neon (High)\n  claude-sonnet-5-5    Claude Sonnet 5.5\n\nAvailable models: 2";
        let ids: Vec<String> = agy_models(table).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["gemini-4-neon-high", "claude-sonnet-5-5"]);
        assert!(agy_models("Sign in to list models.").is_empty());
    }
}
