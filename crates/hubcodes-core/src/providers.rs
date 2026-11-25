//! Provider registry: which CLI to launch for each agent kind, plus the colour
//! and glyph the canvas uses for it. Loaded from `hubcodes.toml` when present,
//! otherwise the built-in defaults below. The desktop Settings dialog writes
//! the same file back through [`Registry::save`].

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Provider {
    pub name: String,
    /// Shell command used to start the agent inside its tmux window.
    pub command: String,
    #[serde(default)]
    pub glyph: String,
    /// 256-colour index used for the node accent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<u8>,
    /// Regex matched against the last lines of the pane to detect "waiting
    /// for a human" (permission prompts, y/n questions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_pattern: Option<String>,
    /// Appended to the command when an agent is started with a model, with
    /// `{model}` replaced: `--model {model}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_flag: Option<String>,
    /// Appended when an agent has a role, with `{role}` replaced, so the
    /// agent knows its job: `--append-system-prompt 'You are the {role}…'`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_flag: Option<String>,
    /// Model names offered in the New agent dialog (free text still allowed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    /// Shell command printing one model id per line; wins over the built-in
    /// reader and over `models`: `models_command = "mytool models --plain"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_command: Option<String>,
    /// tmux key names that answer the tool's permission prompt with yes / no
    /// (`Enter`, `Escape`, `y`, `n`…). Used by the approval inbox.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approve_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny_keys: Vec<String>,
    /// Flag that continues the previous conversation in the same folder
    /// (`-c` for Claude Code, `resume --last` for Codex); used by restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_flag: Option<String>,
    /// Which hook mechanism this provider speaks: `claude` | `codex` |
    /// `opencode` | `pi` | `omp` | none. Lets a custom provider (a wrapper
    /// around claude, say) keep hook status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_kind: Option<String>,
}

/// Keys for a provider that did not set its own.
pub fn default_keys(provider: &str) -> (Vec<String>, Vec<String>) {
    let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match provider {
        "claude" => (v(&["Enter"]), v(&["Escape"])),
        "codex" => (v(&["y"]), v(&["n"])),
        "opencode" => (v(&["Enter"]), v(&["Escape"])),
        _ => (v(&["y", "Enter"]), v(&["n", "Enter"])),
    }
}

impl Provider {
    pub fn keys_for(&self, approve: bool) -> Vec<String> {
        let (a, d) = default_keys(&self.name);
        let own = if approve { &self.approve_keys } else { &self.deny_keys };
        if own.is_empty() { if approve { a } else { d } } else { own.clone() }
    }
}

/// Single-quote `s` for `sh`.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl Provider {
    /// [`Provider::launch_command`] plus extra system-prompt text (memory
    /// recall, the start brief) appended through the role flag's `{extra}` slot.
    pub fn launch_command_with(&self, model: Option<&str>, role: Option<&str>, extra: &str) -> String {
        let extra = extra.trim();
        if extra.is_empty() || self.role_flag.is_none() {
            return self.launch_command(model, role);
        }
        // The role flag is appended only when there is a role; "teammate" stands in.
        let role = role.filter(|r| !r.trim().is_empty()).unwrap_or("teammate");
        let mut cmd = self.launch_command(model, None);
        cmd.push(' ');
        cmd.push_str(&self.role_part(role, extra, model));
        cmd
    }

    /// The command to run for an agent with this model and role.
    pub fn launch_command(&self, model: Option<&str>, role: Option<&str>) -> String {
        let mut cmd = self.command.trim().to_string();
        if let (Some(m), Some(flag)) = (model.filter(|m| !m.trim().is_empty()), &self.model_flag) {
            let safe = clean_model(m);
            if !safe.is_empty() {
                cmd.push(' ');
                cmd.push_str(&flag.replace("{model}", &safe));
            }
        }
        if let (Some(r), Some(_)) = (role.filter(|r| !r.trim().is_empty()), &self.role_flag) {
            cmd.push(' ');
            cmd.push_str(&self.role_part(r.trim(), "", model));
        }
        cmd
    }

    /// The role flag with its slots filled. Slots: `{role}` and `{extra}`
    /// (shell-escaped for a single-quoted argument), `{slug}` (`hub-<role>`,
    /// an agent name), `{agent}` (same, but the role prompt is also written
    /// as that provider's agent file, opencode), `{toml}` (the whole role
    /// prompt as a TOML string, codex `-c developer_instructions=`).
    /// `model` is the one picked for the agent: a role flag that defines an
    /// agent (Claude's `--agents`, opencode's agent file) names it too,
    /// because such an agent's own model wins over `--model`.
    fn role_part(&self, role: &str, extra: &str, model: Option<&str>) -> String {
        let model = model.map(clean_model).filter(|m| !m.is_empty());
        let flag = self.role_flag.as_deref().unwrap_or_default();
        let sq = |s: &str| s.replace('\'', "'\\''");
        let extra_block = if extra.is_empty() { String::new() } else { format!("\n\n{extra}") };
        let prompt = ROLE_PROMPT.replace("{role}", role).replace("{extra}", &extra_block);
        let mut filled = flag.replace("{role}", &sq(role)).replace("{extra}", &sq(&extra_block));
        // `{agent_model}`: `,"model":"<id>"` inside a JSON agent definition, or nothing.
        filled = filled.replace("{agent_model}", &model.as_deref().map(|m| format!(",\"model\":\"{m}\"")).unwrap_or_default());
        if flag.contains("{slug}") {
            filled = filled.replace("{slug}", &role_agent_name(role));
        }
        if flag.contains("{agent}") {
            filled = filled.replace("{agent}", &self.role_agent(role, &prompt, model.as_deref()));
        }
        if flag.contains("{toml}") {
            filled = filled.replace("{toml}", &sq(&toml_string(&prompt)));
        }
        filled
    }

    /// For providers whose role flag names an *agent* (`{agent}`, opencode):
    /// write the role prompt as that provider's agent definition and return
    /// the agent name. Falls back to the provider's default agent when the
    /// file cannot be written.
    fn role_agent(&self, role: &str, prompt: &str, model: Option<&str>) -> String {
        match write_opencode_agent_with(&opencode_agent_dir(), role, prompt, model) {
            Ok(name) => name,
            Err(e) => {
                eprintln!("[hubcodes] could not write the opencode agent for role {role}: {e}; using the default agent");
                "build".into()
            }
        }
    }

    /// 256-colour index for the node accent (`None` = terminal grey).
    pub fn color_index(&self) -> Option<u8> {
        self.color
    }

    /// `true` when the provider's command is on `PATH` (used by setup checks).
    pub fn installed(&self) -> bool {
        let Some(bin) = self.command.split_whitespace().next() else { return false };
        if bin.starts_with('$') {
            // `$SHELL` and friends: resolved by the shell, always present.
            return true;
        }
        if bin.contains('/') {
            return Path::new(bin).is_file();
        }
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|d| d.join(bin).is_file()))
            .unwrap_or(false)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct AgentConfig {
    pub name: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub agents: Vec<AgentConfig>,
    /// `["planner", "builder"]` pairs, from → to.
    #[serde(default)]
    pub edges: Vec<[String; 2]>,
    /// Orchestrator guard: an agent whose role is an orchestrator kind is
    /// refused file edits, builds, tests and git writes by the provider
    /// hooks, with a reason that names its engineers. `false` turns it off.
    #[serde(default = "default_true")]
    pub guard_orchestrator: bool,
    /// What a person types to an agent goes to the session's orchestrator
    /// first, which answers it or hands it on, so no role takes on work that
    /// is not its own. `hubcodes send --direct` (⌥Enter in the app) skips it.
    #[serde(default = "default_true")]
    pub route_through_orchestrator: bool,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        WorkspaceConfig { agents: Vec::new(), edges: Vec::new(), guard_orchestrator: true, route_through_orchestrator: true }
    }
}

/// Settings that only the desktop app uses. Kept in the same file so there is
/// one place to look.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DesktopConfig {
    /// OS notification when an agent enters *needs you* while the window is
    /// not focused.
    #[serde(default = "default_true")]
    pub notifications: bool,
    /// Folder a new agent starts in when the dialog leaves it empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_folder: Option<String>,
    /// Role suggestions in New agent; any text is still accepted.
    #[serde(default = "default_roles")]
    pub roles: Vec<String>,
}

pub fn default_roles() -> Vec<String> {
    ["orchestrator", "software engineer", "reviewer", "pr-fixer", "tests", "docs", "research"].iter().map(|s| s.to_string()).collect()
}

fn default_true() -> bool {
    true
}

impl Default for DesktopConfig {
    fn default() -> Self {
        DesktopConfig { notifications: true, default_folder: None, roles: default_roles() }
    }
}

/// Limits for agent-to-agent loops.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LoopConfig {
    /// Rounds before a loop pauses and asks you (one round = one message along the loop).
    /// 0: no round limit; the loop runs until DONE, a pause, or a token budget.
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u32,
}

fn default_max_rounds() -> u32 {
    crate::relay::DEFAULT_MAX_ROUNDS
}

impl Default for LoopConfig {
    fn default() -> Self {
        LoopConfig { max_rounds: default_max_rounds() }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default, rename = "provider")]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub desktop: DesktopConfig,
    #[serde(default, rename = "loop")]
    pub loop_cfg: LoopConfig,
    #[serde(default)]
    pub sharing: crate::remote::Sharing,
    #[serde(default, rename = "peer", skip_serializing_if = "Vec::is_empty")]
    pub peers: Vec<crate::remote::Peer>,
    #[serde(default, rename = "prompt", skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<crate::prompts::Prompt>,
    #[serde(default, rename = "template", skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<crate::team::Template>,
    #[serde(default)]
    pub budget: BudgetConfig,
    #[serde(default, rename = "trigger", skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<crate::triggers::Trigger>,
    #[serde(default)]
    pub serve: crate::serve::ServeConfig,
    #[serde(default)]
    pub memory: crate::memory::MemoryConfig,
    #[serde(default, rename = "kb", skip_serializing_if = "Vec::is_empty")]
    pub kbs: Vec<crate::kb::KbSource>,
    #[serde(default)]
    pub sandbox: crate::sandbox::SandboxConfig,
    #[serde(default)]
    pub alerts: AlertsConfig,
    /// `[failover]`: move an agent whose provider ran out to the next one.
    #[serde(default)]
    pub failover: crate::failover::FailoverConfig,
    /// `[autoteam]`: extra agents for a role whose queue backs up.
    #[serde(default)]
    pub autoteam: crate::autoteam::AutoTeamConfig,
    /// `[skills]`: keep every tool's skill folder in sync.
    #[serde(default)]
    pub skills: crate::skills::SkillsConfig,
    /// The board's columns (`[board] columns`).
    #[serde(default)]
    pub board: crate::board::BoardConfig,
    #[serde(default)]
    pub policy: crate::policy::PolicyConfig,
    #[serde(default, rename = "playbook", skip_serializing_if = "Vec::is_empty")]
    pub playbooks: Vec<crate::playbook::Playbook>,
    #[serde(default, rename = "tracker", skip_serializing_if = "Vec::is_empty")]
    pub trackers: Vec<crate::tracker::Tracker>,
    #[serde(default, rename = "connection", skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<crate::connections::Connection>,
}

/// When to tell the user something is off.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AlertsConfig {
    /// An agent holding a `doing` task but idle this long (minutes) gets flagged. 0 = off.
    #[serde(default = "d_idle")]
    pub idle_minutes: u32,
    /// A running loop with this many rounds and no change in any diff gets flagged. 0 = off.
    #[serde(default = "d_stale_rounds")]
    pub stale_loop_rounds: u32,
    /// Same error line seen this many polls in a row gets flagged. 0 = off.
    #[serde(default = "d_repeat")]
    pub repeated_error_polls: u32,
    /// Also post every alert here: a Slack or Discord incoming webhook, or
    /// an ntfy topic URL for the phone. Held in the Keychain (`${keychain:…}`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub webhook: String,
}

fn d_idle() -> u32 {
    10
}
fn d_stale_rounds() -> u32 {
    6
}
fn d_repeat() -> u32 {
    3
}

impl Default for AlertsConfig {
    fn default() -> Self {
        AlertsConfig { idle_minutes: 10, stale_loop_rounds: 6, repeated_error_polls: 3, webhook: String::new() }
    }
}

/// Token limits. 0 = no limit. Counted from the agents' own transcripts
/// (Claude Code today), so providers without a transcript show no usage.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct BudgetConfig {
    /// Output + input tokens an agent may spend before hubcodes pauses its loops and tells you.
    #[serde(default)]
    pub max_tokens_per_agent: i64,
    /// Same, summed over both agents of a loop.
    #[serde(default)]
    pub max_tokens_per_loop: i64,
}

/// The role prompt for `role`, with `extra` (the brief) appended, as plain
/// text: what a tool without a system-prompt flag is told in its first message.
pub fn role_prompt(role: &str, extra: &str) -> String {
    let extra_block = if extra.trim().is_empty() { String::new() } else { format!("\n\n{}", extra.trim()) };
    ROLE_PROMPT.replace("{role}", role).replace("{extra}", &extra_block)
}

pub const ROLE_PROMPT: &str = "You are the {role} of a team of coding agents on hubcodes. Other agents message you as [from name]; answer them with: hubcodes send <name> \"text\". Team tools: hubcodes task add/done, hubcodes note show/add (shared notes), hubcodes memory recall/retain (team memory), hubcodes kb search/get (knowledge base).{extra}";

/// A TOML basic string (`"…"`) for `codex -c key=value`.
pub fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `hub-<role>`: the agent name for a hubcodes role (opencode agent file, claude `--agent`).
pub fn role_agent_name(role: &str) -> String {
    let mut slug = String::new();
    for c in role.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() { slug.push(c); } else if !slug.ends_with('-') && !slug.is_empty() { slug.push('-'); }
    }
    let slug = slug.trim_end_matches('-');
    format!("hub-{}", if slug.is_empty() { "teammate" } else { slug })
}

/// Where opencode reads user-level agents from.
pub fn opencode_agent_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".config/opencode/agent")
}

/// opencode has no system-prompt flag, but an agent is a markdown file whose
/// body *is* the system prompt. So the hubcodes role becomes an opencode
/// agent, rewritten at every start (the brief lists the team of the
/// moment), and the command runs `opencode --agent hub-<role>`: what you
/// pick in hubcodes is what opencode shows and obeys.
pub fn write_opencode_agent(dir: &Path, role: &str, prompt: &str) -> Result<String> {
    write_opencode_agent_with(dir, role, prompt, None)
}

/// [`write_opencode_agent`] for an agent with a picked model: the agent file
/// names it (an agent's own model wins in opencode) and gets its own name per
/// model, so two agents of one role on different models never share a file.
pub fn write_opencode_agent_with(dir: &Path, role: &str, prompt: &str, model: Option<&str>) -> Result<String> {
    let model = model.map(clean_model).filter(|m| !m.is_empty());
    let name = match &model {
        Some(m) => format!("{}--{}", role_agent_name(role), m.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect::<String>()),
        None => role_agent_name(role),
    };
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let body = format!(
        "---\ndescription: hubcodes role \"{}\" (managed by hubcodes; rewritten at every start)\nmode: primary\n{}---\n{}\n",
        role.trim().replace('"', "'"),
        model.as_deref().map(|m| format!("model: {m}\n")).unwrap_or_default(),
        prompt.trim()
    );
    let path = dir.join(format!("{name}.md"));
    std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(name)
}

/// A model id as it goes on a command line: plain tokens only; anything
/// else is dropped rather than quoted.
pub fn clean_model(m: &str) -> String {
    m.trim().chars().filter(|c| c.is_alphanumeric() || "._:/-[]".contains(*c)).collect()
}

pub fn default_providers() -> Vec<Provider> {
    let p = |name: &str, command: &str, glyph: &str, color: u8, model_flag: Option<&str>, role_flag: Option<&str>, models: &[&str]| Provider {
        name: name.into(),
        command: command.into(),
        glyph: glyph.into(),
        color: Some(color),
        waiting_pattern: None,
        model_flag: model_flag.map(Into::into),
        role_flag: role_flag.map(Into::into),
        models: models.iter().map(|m| m.to_string()).collect(),
        models_command: None,
        approve_keys: vec![],
        deny_keys: vec![],
        resume_flag: None,
        hook_kind: None,
    };
    // claude: a named session agent (shows as hub-<role>) plus the role prompt
    // appended to its normal system prompt (`--agent` keeps the defaults).
    let claude_role = format!("--agents '{{\"{{slug}}\":{{\"description\":\"hubcodes role\",\"prompt\":\"You are the {{slug}} agent of team hubcodes.\"{{agent_model}}}}}}' --agent {{slug}} --append-system-prompt '{ROLE_PROMPT}'");
    let pi_role = format!("--append-system-prompt '{ROLE_PROMPT}'");
    let omp_role = format!("--append-system-prompt='{ROLE_PROMPT}'");
    // No system-prompt flag (gemini, agy, hermes): the role goes in as the first
    // message once the tool is up (tend::brief_by_message), with a one-word
    // reply asked for so the turn stays cheap.
    // opencode: the role is an agent file (see write_opencode_agent).
    let opencode_role = "--agent {agent}".to_string();
    // codex: developer instructions through its config override.
    let codex_role = "-c 'developer_instructions={toml}'".to_string();
    let mut v = vec![
        p("claude", "claude", "✦", 215, Some("--model {model}"), Some(&claude_role), &["opus", "sonnet", "haiku"]),
        p("codex", "codex", "◇", 252, Some("--model {model}"), Some(&codex_role), &["gpt-5-codex", "gpt-5"]),
        p("opencode", "opencode", "◈", 80, Some("--model {model}"), Some(&opencode_role), &[]),
        p("pi", "pi", "π", 176, Some("--model {model}"), Some(&pi_role), &[]),
        p("omp", "omp", "◆", 114, Some("--model={model}"), Some(&omp_role), &[]),
        p("gemini", "gemini", "✧", 75, Some("--model {model}"), None, &["gemini-2.5-pro", "gemini-2.5-flash"]),
        // Antigravity CLI, Google's successor to the Gemini CLI for personal accounts.
        p("agy", "agy", "◎", 69, Some("--model {model}"), None, &[]),
        // Hermes Agent (Nous Research): any provider it is signed in to, picked with `hermes model`.
        p("hermes", "hermes chat", "☤", 178, Some("--model {model}"), None, &[]),
        p("shell", "$SHELL", "$", 245, None, None, &[]),
    ];
    for x in &mut v {
        match x.name.as_str() {
            "claude" => {
                x.resume_flag = Some("-c".into());
                x.hook_kind = Some("claude".into());
            }
            "codex" => {
                x.resume_flag = Some("resume --last".into());
                x.hook_kind = Some("codex".into());
            }
            "opencode" => {
                x.resume_flag = Some("-c".into());
                x.hook_kind = Some("opencode".into());
            }
            // `-c` / `--continue`: the folder's most recent conversation.
            "agy" => x.resume_flag = Some("-c".into()),
            // `hermes chat -c`: the most recent session.
            "hermes" => x.resume_flag = Some("-c".into()),
            // pi and omp: a managed extension file (hooks::ensure_shim).
            "pi" => x.hook_kind = Some("pi".into()),
            "omp" => x.hook_kind = Some("omp".into()),
            // gemini: hook groups merged into ~/.gemini/settings.json (hooks::ensure_gemini_hooks).
            "gemini" => x.hook_kind = Some("gemini".into()),
            _ => {}
        }
    }
    v
}

/// `true` when `name` is one of the built-in providers (they can be edited,
/// not removed).
pub fn is_builtin(name: &str) -> bool {
    default_providers().iter().any(|p| p.name == name)
}

pub const DEFAULT_WAITING_PATTERN: &str =
    r"(?i)(\(y/n\)|\[y/n\]|\by/n\b|allow|approve|permission|press enter|continue\?|\?\s*$|❯\s*$|>\s*$)";

#[derive(Clone, Debug)]
pub struct Registry {
    pub providers: Vec<Provider>,
    pub workspace: WorkspaceConfig,
    pub desktop: DesktopConfig,
    pub loop_cfg: LoopConfig,
    pub sharing: crate::remote::Sharing,
    pub peers: Vec<crate::remote::Peer>,
    /// Saved prompts: user-defined first, then the built-ins not overridden.
    pub prompts: Vec<crate::prompts::Prompt>,
    pub templates: Vec<crate::team::Template>,
    pub budget: BudgetConfig,
    pub triggers: Vec<crate::triggers::Trigger>,
    pub serve: crate::serve::ServeConfig,
    pub memory: crate::memory::MemoryConfig,
    pub kbs: Vec<crate::kb::KbSource>,
    pub sandbox: crate::sandbox::SandboxConfig,
    pub alerts: AlertsConfig,
    pub failover: crate::failover::FailoverConfig,
    pub autoteam: crate::autoteam::AutoTeamConfig,
    pub skills: crate::skills::SkillsConfig,
    pub board: crate::board::BoardConfig,
    /// Auto-approve rules (user's, merged with the repo's by [`Registry::for_dir`]).
    pub policy: crate::policy::PolicyConfig,
    /// Playbooks: user-defined first, then built-ins not overridden.
    pub playbooks: Vec<crate::playbook::Playbook>,
    pub trackers: Vec<crate::tracker::Tracker>,
    /// Accounts connected once ([[connection]]).
    pub connections: Vec<crate::connections::Connection>,
    /// Settings of the repository the current work is in (`.hubcodes/team.toml`), when loaded with [`Registry::for_dir`].
    pub repo: crate::repo::RepoConfig,
    /// File the registry was read from, `None` for built-in defaults.
    pub source: Option<PathBuf>,
}

impl Registry {
    /// Modification time of the file this registry was read from (the user
    /// config when it came from defaults), so long-lived processes can
    /// notice an edit and load again.
    pub fn config_mtime(&self) -> Option<std::time::SystemTime> {
        let path = self.source.clone().unwrap_or_else(user_config_path);
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    pub fn load() -> Result<Self> {
        for candidate in config_candidates() {
            if candidate.is_file() {
                return Self::from_file(&candidate);
            }
        }
        Ok(Registry {
            providers: default_providers(),
            workspace: WorkspaceConfig::default(),
            desktop: DesktopConfig::default(),
            loop_cfg: LoopConfig::default(),
            sharing: crate::remote::Sharing::default(),
            peers: Vec::new(),
            prompts: crate::prompts::default_prompts(),
            templates: crate::team::default_templates(),
            budget: BudgetConfig::default(),
            triggers: Vec::new(),
            serve: crate::serve::ServeConfig::default(),
            memory: crate::memory::MemoryConfig::default(),
            kbs: Vec::new(),
            sandbox: crate::sandbox::SandboxConfig::default(),
            alerts: AlertsConfig::default(),
            failover: Default::default(),
            autoteam: Default::default(),
            skills: Default::default(),
            board: crate::board::BoardConfig::default(),
            policy: crate::policy::PolicyConfig::default(),
            playbooks: crate::playbook::default_playbooks(),
            trackers: Vec::new(),
            connections: Vec::new(),
            repo: crate::repo::RepoConfig::default(),
            source: None,
        })
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read config {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parse config {}", path.display()))?;
        Ok(Self::from_config(cfg, Some(path.to_path_buf())))
    }

    pub fn from_config(cfg: Config, source: Option<PathBuf>) -> Self {
        // User-defined providers override defaults with the same name;
        // provider files (`providers/*.toml`) sit between the two.
        let mut providers = default_providers();
        for p in plugin_providers() {
            if let Some(slot) = providers.iter_mut().find(|d| d.name == p.name) {
                *slot = p;
            } else {
                providers.push(p);
            }
        }
        for p in cfg.providers {
            if let Some(slot) = providers.iter_mut().find(|d| d.name == p.name) {
                *slot = p;
            } else {
                providers.push(p);
            }
        }
        // User prompts/templates override built-ins with the same name.
        let mut prompts = cfg.prompts;
        for d in crate::prompts::default_prompts() {
            if !prompts.iter().any(|p| p.name == d.name) {
                prompts.push(d);
            }
        }
        let mut templates = cfg.templates;
        for d in crate::team::default_templates() {
            if !templates.iter().any(|t| t.name == d.name) {
                templates.push(d);
            }
        }
        let mut playbooks = cfg.playbooks;
        for d in crate::playbook::default_playbooks() {
            if !playbooks.iter().any(|p| p.name == d.name) {
                playbooks.push(d);
            }
        }
        Registry {
            providers,
            workspace: cfg.workspace,
            desktop: cfg.desktop,
            loop_cfg: cfg.loop_cfg,
            sharing: cfg.sharing,
            peers: cfg.peers,
            prompts,
            templates,
            budget: cfg.budget,
            triggers: cfg.triggers,
            serve: cfg.serve,
            memory: cfg.memory,
            kbs: cfg.kbs,
            sandbox: cfg.sandbox,
            alerts: cfg.alerts,
            failover: cfg.failover,
            autoteam: cfg.autoteam,
            skills: cfg.skills,
            board: cfg.board.normalized().unwrap_or_default(),
            policy: cfg.policy,
            playbooks,
            trackers: cfg.trackers,
            connections: cfg.connections,
            repo: crate::repo::RepoConfig::default(),
            source,
        }
    }

    /// This registry plus the repository settings of `dir`: repo templates
    /// first, repo knowledge sources appended, `repo` filled.
    pub fn for_dir(&self, dir: &std::path::Path) -> Registry {
        let rc = crate::repo::load(dir);
        let mut out = self.clone();
        if rc.root.is_none() {
            return out;
        }
        let mut templates = rc.templates.clone();
        for t in &self.templates {
            if !templates.iter().any(|x| x.name == t.name) {
                templates.push(t.clone());
            }
        }
        out.templates = templates;
        for k in &rc.kbs {
            if !out.kbs.iter().any(|x| x.name == k.name) {
                out.kbs.push(k.clone());
            }
        }
        let mut playbooks = rc.playbooks.clone();
        for p in &self.playbooks {
            if !playbooks.iter().any(|x| x.name == p.name) {
                playbooks.push(p.clone());
            }
        }
        out.playbooks = playbooks;
        out.policy = self.policy.merged(&rc.policy);
        out.repo = rc;
        out
    }

    pub fn get(&self, name: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.name == name)
    }

    pub fn names(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.name.clone()).collect()
    }

    /// Where [`Registry::save`] writes: the file this registry came from, or
    /// the user-level config when there is none yet.
    pub fn save_path(&self) -> PathBuf {
        self.source.clone().unwrap_or_else(user_config_path)
    }

    /// The config as it would be written: built-in providers only when they
    /// differ from the defaults, so future default changes still apply.
    pub fn to_config(&self) -> Config {
        let defaults = default_providers();
        let providers = self
            .providers
            .iter()
            .filter(|p| defaults.iter().all(|d| d != *p))
            .cloned()
            .collect();
        Config {
            providers,
            workspace: self.workspace.clone(),
            desktop: self.desktop.clone(),
            loop_cfg: self.loop_cfg.clone(),
            sharing: self.sharing.clone(),
            peers: self.peers.clone(),
            prompts: self.prompts.iter().filter(|p| !crate::prompts::default_prompts().contains(p)).cloned().collect(),
            templates: self.templates.iter().filter(|t| !crate::team::default_templates().contains(t)).cloned().collect(),
            budget: self.budget.clone(),
            triggers: self.triggers.clone(),
            serve: self.serve.clone(),
            memory: self.memory.clone(),
            kbs: self.kbs.clone(),
            sandbox: self.sandbox.clone(),
            alerts: self.alerts.clone(),
            failover: self.failover.clone(),
            autoteam: self.autoteam.clone(),
            skills: self.skills.clone(),
            board: self.board.clone(),
            policy: self.policy.clone(),
            playbooks: self.playbooks.iter().filter(|p| !crate::playbook::default_playbooks().contains(p)).cloned().collect(),
            trackers: self.trackers.clone(),
            connections: self.connections.clone(),
        }
    }

    /// Move every plain token, API key and credential header into the
    /// Keychain, leaving `${keychain:…}` in their place, so the file written
    /// next holds no secret. Returns what moved, for the person to see.
    pub fn stash_secrets(&mut self) -> Result<Vec<String>> {
        use crate::secrets::{stash, stash_headers};
        let mut moved = Vec::new();
        for c in &mut self.connections {
            if stash(&mut c.token, &crate::connections::secret_name(&c.name))? {
                moved.push(format!("connection {}", c.name));
            }
        }
        for t in &mut self.trackers {
            let p = format!("tracker:{}", t.name.trim());
            if stash(&mut t.token, &format!("{p}:token"))? | stash(&mut t.api_key, &format!("{p}:api_key"))? | (stash_headers(&mut t.headers, &p)? > 0) {
                moved.push(format!("tracker {}", t.name));
            }
        }
        for k in &mut self.kbs {
            let p = format!("kb:{}", k.name.trim());
            if stash(&mut k.token, &format!("{p}:token"))? | (stash_headers(&mut k.headers, &p)? > 0) {
                moved.push(format!("knowledge source {}", k.name));
            }
        }
        if stash(&mut self.alerts.webhook, "alerts:webhook")? {
            moved.push("alert webhook".into());
        }
        if stash(&mut self.memory.hindsight.api_key, "memory:hindsight:api_key")? | (stash_headers(&mut self.memory.mcp.target.headers, "memory:mcp")? > 0) {
            moved.push("team memory".into());
        }
        Ok(moved)
    }

    /// Validate and write `hubcodes.toml` atomically. Returns the path written.
    pub fn save(&self) -> Result<PathBuf> {
        self.validate()?;
        let path = self.save_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let body = toml::to_string_pretty(&self.to_config()).context("serialize config")?;
        let text = format!(
            "# hubcodes configuration. Written by the desktop Settings dialog;\n\
             # hand edits are kept, comments are not. Docs: hubcodes.example.toml\n\n{body}"
        );
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("rename to {}", path.display()))?;
        Ok(path)
    }

    pub fn validate(&self) -> Result<()> {
        let mut peers = std::collections::HashSet::new();
        for p in &self.peers {
            let n = p.name.trim();
            if n.is_empty() || n.contains('/') || n.contains(char::is_whitespace) {
                anyhow::bail!("device name '{n}' must be one word without '/'");
            }
            if !peers.insert(n) {
                anyhow::bail!("device '{n}' is listed twice");
            }
            if p.host.trim().is_empty() {
                anyhow::bail!("device '{n}' has no host");
            }
        }
        let mut seen = std::collections::HashSet::new();
        for p in &self.providers {
            let name = p.name.trim();
            if name.is_empty() {
                anyhow::bail!("a provider has no name");
            }
            if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                anyhow::bail!("provider name '{name}' may only use letters, digits, - and _");
            }
            if !seen.insert(name) {
                anyhow::bail!("provider '{name}' is listed twice");
            }
            if p.command.trim().is_empty() {
                anyhow::bail!("provider '{name}' has no command");
            }
            if let Some(pat) = &p.waiting_pattern {
                regex_check(pat).with_context(|| format!("provider '{name}': waiting pattern"))?;
            }
        }
        Ok(())
    }
}

/// Cheap syntax check without pulling `regex` into core: balanced groups and
/// brackets. The clients compile the pattern for real.
fn regex_check(pat: &str) -> Result<()> {
    let mut depth = 0i32;
    let mut in_class = false;
    let mut escaped = false;
    for c in pat.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '[' if !in_class => in_class = true,
            ']' if in_class => in_class = false,
            '(' if !in_class => depth += 1,
            ')' if !in_class => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            anyhow::bail!("unbalanced ')' in '{pat}'");
        }
    }
    if depth != 0 || in_class || escaped {
        anyhow::bail!("unbalanced pattern '{pat}'");
    }
    Ok(())
}

/// Folder of shareable provider files: `~/.config/hubcodes/providers/<name>.toml`.
/// Each file is one provider (a plain table) or several (`[[provider]]`).
pub fn providers_dir() -> PathBuf {
    user_config_path().parent().map(|d| d.join("providers")).unwrap_or_else(|| PathBuf::from("providers"))
}

/// Providers from every `*.toml` in [`providers_dir`].
pub fn plugin_providers() -> Vec<Provider> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(providers_dir()) else { return out };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "toml").unwrap_or(false)).collect();
    files.sort();
    for f in files {
        match parse_provider_file(&f) {
            Ok(list) => out.extend(list),
            Err(e) => eprintln!("[hubcodes] {}: {e:#}", f.display()),
        }
    }
    out
}

#[derive(Deserialize)]
struct ProviderFile {
    #[serde(default, rename = "provider")]
    providers: Vec<Provider>,
}

/// One file → providers. A single-table file (`name = …` at top level) also works.
pub fn parse_provider_file(path: &Path) -> Result<Vec<Provider>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    parse_provider_text(&text).with_context(|| format!("parse {}", path.display()))
}

pub fn parse_provider_text(text: &str) -> Result<Vec<Provider>> {
    if let Ok(single) = toml::from_str::<Provider>(text) {
        return Ok(vec![single]);
    }
    let f: ProviderFile = toml::from_str(text)?;
    if f.providers.is_empty() {
        anyhow::bail!("no provider found (expected `name = …` fields or [[provider]] tables)");
    }
    Ok(f.providers)
}

/// A provider as a shareable TOML file (every field, including the ones at
/// their defaults, so the receiving machine needs nothing else).
pub fn export_provider(p: &Provider) -> Result<String> {
    let body = toml::to_string_pretty(p).context("serialize provider")?;
    Ok(format!("# hubcodes provider file. Install with: hubcodes provider import <this file>\n# or drop it into {}\n\n{body}", providers_dir().display()))
}

/// Write a provider file into [`providers_dir`]; returns the path.
pub fn import_provider_text(text: &str) -> Result<Vec<PathBuf>> {
    let list = parse_provider_text(text)?;
    let dir = providers_dir();
    std::fs::create_dir_all(&dir)?;
    let mut paths = Vec::new();
    for p in list {
        if p.name.trim().is_empty() || p.command.trim().is_empty() {
            anyhow::bail!("provider needs a name and a command");
        }
        let path = dir.join(format!("{}.toml", p.name.trim()));
        std::fs::write(&path, toml::to_string_pretty(&p)?)?;
        paths.push(path);
    }
    Ok(paths)
}

/// `$XDG_CONFIG_HOME/hubcodes/hubcodes.toml`, default `~/.config/hubcodes/hubcodes.toml`.
pub fn user_config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("hubcodes").join("hubcodes.toml")
}

fn config_candidates() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("hubcodes.toml"), user_config_path()];
    // Platform config dir (`~/Library/Application Support` on macOS), for
    // people who put it there.
    if let Some(dir) = dirs::config_dir() {
        let p = dir.join("hubcodes").join("hubcodes.toml");
        if !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_custom_and_changed_providers_only() {
        let mut reg = Registry::from_config(Config::default(), None);
        reg.providers.push(Provider {
            name: "aider".into(),
            command: "aider --no-git".into(),
            glyph: "a".into(),
            color: Some(99),
            waiting_pattern: Some(r"\(y/n\)".into()),
            model_flag: None,
            role_flag: None,
            models: vec![],
            models_command: None,
            approve_keys: vec![],
            deny_keys: vec![],
            resume_flag: None,
            hook_kind: None,
        });
        reg.providers.iter_mut().find(|p| p.name == "claude").unwrap().command = "claude --dangerously-skip-permissions".into();
        reg.desktop.notifications = false;
        let text = toml::to_string_pretty(&reg.to_config()).unwrap();
        assert!(text.contains("aider --no-git"));
        assert!(text.contains("dangerously-skip-permissions"));
        assert!(!text.contains("\"codex\""), "unchanged defaults are not written:\n{text}");
        let back = Registry::from_config(toml::from_str(&text).unwrap(), None);
        assert_eq!(back.providers.len(), 10, "nine built in (agy and hermes included) and aider");
        assert!(!back.desktop.notifications);
        assert_eq!(back.get("claude").unwrap().command, "claude --dangerously-skip-permissions");
    }

    #[test]
    fn provider_file_round_trip() {
        let reg = Registry::from_config(Config::default(), None);
        let text = export_provider(reg.get("claude").unwrap()).unwrap();
        let back = parse_provider_text(&text).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "claude");
        assert_eq!(back[0].hook_kind.as_deref(), Some("claude"));
        let multi = parse_provider_text("[[provider]]\nname = \"a\"\ncommand = \"a\"\n[[provider]]\nname = \"b\"\ncommand = \"b\"\n").unwrap();
        assert_eq!(multi.len(), 2);
        assert!(parse_provider_text("just = 1").is_err());
    }

    #[test]
    fn builtin_hook_kinds() {
        let kinds: Vec<(String, Option<String>)> = default_providers().into_iter().map(|p| (p.name, p.hook_kind)).collect();
        for (name, want) in [("claude", Some("claude")), ("codex", Some("codex")), ("opencode", Some("opencode")), ("pi", Some("pi")), ("omp", Some("omp")), ("gemini", Some("gemini")), ("shell", None)] {
            let got = kinds.iter().find(|(n, _)| n == name).map(|(_, k)| k.as_deref()).unwrap_or_else(|| panic!("provider {name} missing"));
            assert_eq!(got, want, "hook kind of {name}");
        }
    }

    #[test]
    fn validate_rejects_duplicates_and_bad_patterns() {
        let mut reg = Registry::from_config(Config::default(), None);
        reg.providers.push(Provider { name: "claude".into(), command: "x".into(), glyph: String::new(), color: None, waiting_pattern: None, model_flag: None, role_flag: None, models: vec![], models_command: None, approve_keys: vec![], deny_keys: vec![], resume_flag: None, hook_kind: None });
        assert!(reg.validate().is_err());
        reg.providers.pop();
        reg.providers[0].waiting_pattern = Some("(abc".into());
        assert!(reg.validate().is_err());
        reg.providers[0].waiting_pattern = Some(r"(?i)\[y/n\]".into());
        assert!(reg.validate().is_ok());
    }

    #[test]
    fn launch_command_adds_model_and_role_flags() {
        let reg = Registry::from_config(Config::default(), None);
        let claude = reg.get("claude").unwrap();
        assert_eq!(claude.launch_command(None, None), "claude");
        assert_eq!(claude.launch_command(Some("opus"), None), "claude --model opus");
        assert_eq!(claude.launch_command(Some("claude-fable-5-1[1m]"), None), "claude --model claude-fable-5-1[1m]");
        assert_eq!(claude.launch_command(Some("x; rm -rf /"), None), "claude --model xrm-rf/");
        let with_role = claude.launch_command(Some("opus"), Some("orchestrator"));
        assert!(with_role.starts_with("claude --model opus --agents '{\"hub-orchestrator\":{"), "{with_role}");
        assert!(with_role.contains(" --agent hub-orchestrator --append-system-prompt 'You are the orchestrator of a team"), "{with_role}");
        // The session agent names the picked model too: its own model would win over --model.
        assert!(with_role.contains("agent of team hubcodes.\",\"model\":\"opus\"}}' --agent hub-orchestrator"), "{with_role}");
        let no_model = claude.launch_command(None, Some("reviewer"));
        assert!(no_model.contains("agent of team hubcodes.\"}}' --agent hub-reviewer") && !no_model.contains("\"model\""), "{no_model}");
        let json = with_role.split("--agents '").nth(1).and_then(|r| r.split("' --agent ").next()).unwrap();
        let v: serde_json::Value = serde_json::from_str(json).expect("the --agents argument is valid JSON");
        assert_eq!(v["hub-orchestrator"]["model"], "opus");
        let fable = claude.launch_command(Some("claude-fable-5-1[1m]"), Some("reviewer"));
        assert!(fable.contains("\"model\":\"claude-fable-5-1[1m]\""), "{fable}");
        let oc = reg.get("opencode").unwrap().launch_command(None, Some("reviewer"));
        assert_eq!(oc, "opencode --agent hub-reviewer");
        let cx = reg.get("codex").unwrap().launch_command(Some("gpt-5"), Some("reviewer"));
        assert!(cx.starts_with("codex --model gpt-5 -c 'developer_instructions=\"You are the reviewer of a team"), "{cx}");
        assert!(cx.contains("hubcodes send <name> \\\"text\\\""), "quotes inside the TOML string are escaped: {cx}");
        let cx2 = reg.get("codex").unwrap().launch_command_with(None, Some("reviewer"), "Your team right now: dev.\nSecond line");
        assert!(cx2.contains("\\n\\nYour team right now: dev.\\nSecond line\"'"), "{cx2}");
        assert_eq!(toml_string("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
        let pi = reg.get("pi").unwrap().launch_command(None, Some("tests"));
        assert!(pi.starts_with("pi --append-system-prompt 'You are the tests of a team"), "{pi}");
        let shell = reg.get("shell").unwrap();
        assert_eq!(shell.launch_command(Some("x"), Some("y")), "$SHELL", "no flags configured: nothing appended");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn an_opencode_agent_with_a_model_names_it_in_its_own_file() {
        let dir = std::env::temp_dir().join(format!("hubcodes-oc-agent-{}", std::process::id()));
        let plain = write_opencode_agent_with(&dir, "reviewer", "Review.", None).unwrap();
        let picked = write_opencode_agent_with(&dir, "reviewer", "Review.", Some("openai/gpt-5.6")).unwrap();
        assert_eq!(plain, "hub-reviewer");
        assert_eq!(picked, "hub-reviewer--openai-gpt-5-6");
        let body = std::fs::read_to_string(dir.join(format!("{picked}.md"))).unwrap();
        assert!(body.contains("\nmodel: openai/gpt-5.6\n") && body.starts_with("---\n"), "{body}");
        assert!(!std::fs::read_to_string(dir.join("hub-reviewer.md")).unwrap().contains("model:"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opencode_role_becomes_an_agent_file() {
        assert_eq!(role_agent_name("Software Engineer"), "hub-software-engineer");
        assert_eq!(role_agent_name("  "), "hub-teammate");
        let dir = std::env::temp_dir().join(format!("hubcodes-agent-test-{}", std::process::id()));
        let name = write_opencode_agent(&dir, "reviewer", "You are the reviewer. Reply with hubcodes send.").unwrap();
        assert_eq!(name, "hub-reviewer");
        let text = std::fs::read_to_string(dir.join("hub-reviewer.md")).unwrap();
        assert!(text.starts_with("---\ndescription: hubcodes role \"reviewer\""));
        assert!(text.contains("mode: primary"));
        assert!(text.trim_end().ends_with("Reply with hubcodes send."));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
