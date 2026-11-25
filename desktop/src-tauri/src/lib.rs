//! hubcodes desktop backend. tmux still owns every PTY; this process is a
//! client that lists panes, streams their output to the webview (xterm.js),
//! types into them, and keeps the shared canvas state (edges, messages) in
//! tmux session options exactly like the TUI and the CLI do.

mod agents;
mod board;
mod config;
mod connect;
mod knowledge;
mod loops;
mod panes;
mod peers;
mod remote_access;
mod rooms;
mod session;
mod snapshot;
mod stream;
mod system;
mod teams;
mod updater;

use anyhow::Result;
use hubcodes_core::db::{Db, MessageRow, Usage};
use hubcodes_core::fanout::{self, Fanout};
use hubcodes_core::git::{self, Changes};
use hubcodes_core::notes;
use hubcodes_core::prompts::{self, Prompt};
use hubcodes_core::serve::{Share, ServeConfig};
use hubcodes_core::tasks::{self, Task};
use hubcodes_core::team::{self, Template};
use hubcodes_core::tend;
use hubcodes_core::triggers::Trigger;
use hubcodes_core::kb::{self, Check as KbCheck, Hit as KbHit, KbSource};
use hubcodes_core::merge::{self, Merge};
use hubcodes_core::sessiondef::{self, SessionDef};
use hubcodes_core::testrun::{self, TestResult};
use hubcodes_core::sandbox;
use hubcodes_core::repo;
use hubcodes_core::memory::{self, Memory, MemoryConfig, Ping as MemoryPing};
use hubcodes_core::hooks;
use hubcodes_core::models::ProviderStatus;
use hubcodes_core::providers::{self, Provider, Registry, DEFAULT_WAITING_PATTERN};
use hubcodes_core::relay::{self, Awaiting, LoopState};
use hubcodes_core::remote::{self, Peer, RemoteSnapshot};
use std::collections::BTreeMap;
use hubcodes_core::tmux::{self, Tmux};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

const POLL: Duration = Duration::from_millis(500);
const PREVIEW_LINES: usize = 12;
use hubcodes_core::tmux::PLACEHOLDER_WINDOW;
/// Provider tag of a local pane that only shows a remote agent (`ssh … tmux attach`).
const PROXY_PROVIDER: &str = "proxy";
/// Fastest device poll; `AppState::peer_poll_wait` slows it down when nothing moves there.
const PEER_POLL: Duration = Duration::from_secs(3);

/// Modification time of the config file `reg` came from (or would come from).
/// When a peer was last polled and what it said.
type PeerEntry = (Instant, Result<RemoteSnapshot, String>);

struct Tracked {
    last_hash: u64,
    last_change: Instant,
}

pub struct AppState {
    tmux: Tmux,
    /// Absolute path of the `hubcodes` CLI agents call (`hubcodes send`).
    cli: Option<PathBuf>,
    cli_note: String,
    reg: RwLock<Registry>,
    waiting: Regex,
    tracked: Mutex<HashMap<String, Tracked>>,
    streams: Mutex<HashMap<String, stream::Handle>>,
    /// Pane ids that were *needs you* at the last poll (notification edge trigger).
    was_waiting: Mutex<HashSet<String>>,
    /// Provider discovery results (reading caches, running `opencode models`) for a minute.
    status_cache: Mutex<HashMap<String, (Instant, ProviderStatus)>>,
    /// Another device's providers and model catalogs, for a few minutes.
    peer_models: Mutex<HashMap<String, (Instant, Vec<ProviderStatus>)>>,
    /// Last `hubcodes ls --json` from every peer, refreshed by its own thread.
    peer_cache: Mutex<HashMap<String, PeerEntry>>,
    /// Devices followed through their federation listener (long poll)
    /// instead of the timed poll.
    fed_watch: Mutex<HashSet<String>>,
    /// Loop keys already reported as paused (one notification each).
    paused_seen: Mutex<HashSet<String>>,
    /// `git status` summary per agent folder, refreshed every 5 s (git is not free).
    changes_cache: Mutex<HashMap<String, (Instant, Option<Changes>)>>,
    tend_mem: Mutex<tend::Memory>,
    tend_at: Mutex<Instant>,
    /// Address the HTTP server listens on while it runs.
    serving: Mutex<Option<String>>,
    /// When hubcodes.toml last changed as far as `reg` knows; a newer file
    /// (`hubcodes peer add`, an edit by hand or by an agent) is read again.
    config_seen: Mutex<Option<std::time::SystemTime>>,
}

impl AppState {
    fn new() -> Result<Self> {
        let reg = Registry::load()?;
        // HUBCODES_SESSION wins; else the session chosen in the app (restart
        // into another project); else the default.
        let session = std::env::var("HUBCODES_SESSION")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::fs::read_to_string(hubcodes_core::db::state_dir().join("desktop-session")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
            .unwrap_or_else(|| tmux::DEFAULT_SESSION.to_string());
        let cli = cli_path();
        let cli_note = match &cli {
            Some(c) => install_cli_link(c),
            None => "hubcodes CLI not found: agents cannot message each other".into(),
        };
        eprintln!("[hubcodes] cli: {cli_note}");
        Ok(AppState {
            tmux: Tmux::named(&session),
            cli,
            cli_note,
            config_seen: Mutex::new(reg.config_mtime()),
            reg: RwLock::new(reg),
            waiting: Regex::new(DEFAULT_WAITING_PATTERN)?,
            tracked: Mutex::new(HashMap::new()),
            streams: Mutex::new(HashMap::new()),
            was_waiting: Mutex::new(HashSet::new()),
            status_cache: Mutex::new(HashMap::new()),
            peer_models: Mutex::new(HashMap::new()),
            peer_cache: Mutex::new(HashMap::new()),
            fed_watch: Mutex::new(HashSet::new()),
            paused_seen: Mutex::new(HashSet::new()),
            changes_cache: Mutex::new(HashMap::new()),
            tend_mem: Mutex::new(tend::Memory::default()),
            tend_at: Mutex::new(Instant::now()),
            serving: Mutex::new(None),
        })
    }

    /// A copy of the registry. A copy, not a guard: the poll thread used to
    /// hold a read guard while helpers took another read, and a Settings save
    /// waiting for the write lock in between froze both for good.
    fn reg(&self) -> Registry {
        self.reg.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Read hubcodes.toml again when it changed outside the app. Without
    /// this a device added with `hubcodes peer add` was never polled, the
    /// peer cache went stale, and briefs and the guard lost that device's
    /// agents. Returns whether it reloaded.
    pub(crate) fn reload_config_if_changed(&self) -> bool {
        let reg = self.reg();
        let now = reg.config_mtime();
        let mut seen = self.config_seen.lock().unwrap_or_else(|e| e.into_inner());
        if now.is_none() || now == *seen {
            return false;
        }
        *seen = now;
        drop(seen);
        match Registry::load() {
            Ok(next) => {
                *self.reg.write().unwrap_or_else(|e| e.into_inner()) = next;
                self.status_cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
                true
            }
            Err(e) => {
                log_line(&format!("hubcodes.toml changed but does not read: {e:#}"));
                false
            }
        }
    }

    /// Branch and counts for a folder, cached per agent for 5 s.
    fn changes_for(&self, id: &str, dir: &str) -> Option<Changes> {
        if dir.is_empty() {
            return None;
        }
        let mut cache = self.changes_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, c)) = cache.get(id) {
            if at.elapsed() < Duration::from_secs(5) {
                return c.clone();
            }
        }
        let c = git::repo_root(Path::new(dir)).and_then(|_| git::changes(Path::new(dir)).ok());
        cache.insert(id.to_string(), (Instant::now(), c.clone()));
        c
    }

    fn me(&self) -> String {
        format!("human:{}", std::env::var("USER").unwrap_or_else(|_| "me".into()))
    }
}

fn pane_cwd(pane: &str) -> Option<String> {
    let out = std::process::Command::new("tmux")
        .args(["display-message", "-p", "-t", pane, "#{pane_current_path}"])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn expand_home(p: &str) -> String {
    if p == "~" {
        return dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_else(|| p.to_string());
    }
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest).display().to_string();
        }
    }
    p.to_string()
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The bundled `hubcodes` CLI (Tauri sidecar next to the main binary), or
/// one on PATH when running unbundled (`pnpm tauri dev`).
fn cli_path() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let side = dir.join("hubcodes");
            if side.is_file() {
                return Some(side);
            }
        }
    }
    // Dev: the workspace target dir, two levels up from desktop/src-tauri.
    for rel in ["../../target/release/hubcodes", "../../target/debug/hubcodes"] {
        let p = std::env::current_dir().ok()?.join(rel);
        if p.is_file() {
            return p.canonicalize().ok();
        }
    }
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join("hubcodes")).find(|p| p.is_file()))
}

/// `~/.local/bin/hubcodes` → the bundled CLI, so agents and the user's shell
/// find it. Replaces only a missing link or one that points into an app
/// bundle (a `cargo install`ed binary is left alone). Returns what happened.
fn install_cli_link(cli: &Path) -> String {
    let Some(home) = dirs::home_dir() else { return "no home dir".into() };
    let bin = home.join(".local").join("bin");
    let link = bin.join("hubcodes");
    if let Err(e) = std::fs::create_dir_all(&bin) {
        return format!("cannot create {}: {e}", bin.display());
    }
    match std::fs::symlink_metadata(&link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let target = std::fs::read_link(&link).unwrap_or_default();
            if target == cli {
                return format!("{} → {}", link.display(), cli.display());
            }
            if !target.to_string_lossy().contains(".app/") && target.exists() {
                return format!("{} points at {} (left alone)", link.display(), target.display());
            }
            let _ = std::fs::remove_file(&link);
        }
        Ok(_) => return format!("{} is a real file (left alone)", link.display()),
        Err(_) => {}
    }
    match std::os::unix::fs::symlink(cli, &link) {
        Ok(()) => format!("{} → {}", link.display(), cli.display()),
        Err(e) => format!("cannot link {}: {e}", link.display()),
    }
}

/// Apps launched from Finder get the minimal `PATH` (`/usr/bin:/bin:…`), so
/// `tmux` from Homebrew and the agent CLIs from nvm, cargo or pipx are not
/// found. Ask the user's login shell for its `PATH` and merge it in.
fn adopt_login_path() {
    use std::process::{Command, Stdio};
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut child = match Command::new(&shell)
        .args(["-l", "-i", "-c", "printf '%s' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[hubcodes] login shell {shell} failed: {e}");
            return;
        }
    };
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut output = String::new();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                if let Some(mut out) = child.stdout.take() {
                    use std::io::Read;
                    let _ = out.read_to_string(&mut output);
                }
                break;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                eprintln!("[hubcodes] login shell did not answer in 4 s, keeping PATH as is");
                break;
            }
        }
    }
    let login = output.lines().rev().find(|l| l.contains('/')).unwrap_or("").trim().to_string();
    let current = std::env::var("PATH").unwrap_or_default();
    let mut merged: Vec<String> = Vec::new();
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
    let fallback = format!("/opt/homebrew/bin:/usr/local/bin:{home}/.cargo/bin:{home}/.local/bin");
    for dir in login.split(':').chain(current.split(':')).chain(fallback.split(':')) {
        if !dir.is_empty() && !merged.iter().any(|d| d == dir) {
            merged.push(dir.to_string());
        }
    }
    let merged = merged.join(":");
    eprintln!("[hubcodes] PATH from {shell}: {}", if login.is_empty() { "unavailable, using fallback dirs" } else { "ok" });
    std::env::set_var("PATH", merged);
}

/// Append one line to `~/.local/state/hubcodes/desktop.log` (and stderr).
fn log_line(msg: &str) {
    eprintln!("[hubcodes] {msg}");
    let dir = hubcodes_core::db::state_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("desktop.log")) {
        use std::io::Write;
        let _ = writeln!(f, "{} {msg}", hubcodes_core::db::now());
    }
}

/// Replace an older `~/.local/bin/hubcodes` (a plain file the Linux
/// installer wrote, not a link) with the CLI bundled in this app.
#[cfg(target_os = "linux")]
fn sync_linux_cli() -> Option<String> {
    let bundled = std::env::current_exe().ok()?.parent()?.join("hubcodes");
    let target = dirs::home_dir()?.join(".local/bin/hubcodes");
    let meta = std::fs::symlink_metadata(&target).ok()?;
    if !bundled.is_file() || !meta.file_type().is_file() {
        return None;
    }
    let out = std::process::Command::new(&target).arg("--version").output().ok()?;
    let have = String::from_utf8_lossy(&out.stdout).trim().trim_start_matches("hubcodes").trim().to_string();
    let want = env!("CARGO_PKG_VERSION");
    if !hubcodes_core::update::newer(want, &have) {
        return None;
    }
    let tmp = target.with_file_name(".hubcodes.new");
    std::fs::copy(&bundled, &tmp).ok()?;
    std::fs::rename(&tmp, &target).ok()?;
    Some(format!("updated {} from {have} to {want}", target.display()))
}

pub fn run() {
    adopt_login_path();
    if let Some(l) = tmux::ensure_utf8_locale() {
        eprintln!("[hubcodes] no locale in the environment, using {l}");
    }
    let state = Arc::new(AppState::new().expect("load hubcodes config"));
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            let st = state.clone();
            std::thread::spawn(move || loop {
                if tmux::available() {
                    // A panic here would stop every later poll and freeze the
                    // canvas; log it, tell the UI, and keep going.
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        match st.ensure_session().and_then(|_| st.snapshot()) {
                            Ok(snap) => {
                                st.notify_needs_you(&handle, &snap);
                                st.tend_loops(&handle, &snap);
                                st.tend(&handle, &snap);
                                let _ = handle.emit("snapshot", &snap);
                            }
                            Err(e) => log_line(&format!("snapshot: {e:#}")),
                        }
                    }));
                    if let Err(e) = r {
                        let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "unknown panic".into());
                        log_line(&format!("poll panicked: {msg}"));
                        let _ = handle.emit("tend", &serde_json::json!({ "kind": "error", "text": format!("Internal error while refreshing: {msg}") }));
                    }
                }
                std::thread::sleep(POLL);
            });
            // Linux: the AppImage updates itself but the CLI in ~/.local/bin
            // is a separate file; bring it to the app's version.
            #[cfg(target_os = "linux")]
            std::thread::spawn(|| {
                if let Some(msg) = sync_linux_cli() {
                    log_line(&msg);
                }
            });
            let st2 = state.clone();
            std::thread::spawn(move || loop {
                st2.reload_config_if_changed();
                if tmux::available() {
                    st2.poll_peers();
                }
                st2.wait_for_peers();
            });
            // When hubcodesd tends this session, its housekeeping events
            // (dispatch, deliveries, alerts) happen outside this process;
            // follow its socket so they still reach the activity panel and
            // the notifications, exactly like the app's own.
            let (h3, st3) = (app.handle().clone(), state.clone());
            std::thread::spawn(move || loop {
                if hubcodes_core::ipc::alive() {
                    let (h, st) = (h3.clone(), st3.clone());
                    let _ = hubcodes_core::ipc::subscribe(None, move |ev| {
                        if ev.event != "tend" || ev.session != st.tmux.session {
                            return;
                        }
                        let kind = ev.data.get("kind").and_then(|v| v.as_str()).unwrap_or("info").to_string();
                        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let _ = h.emit("tend", &tend::Event { kind: kind.clone(), text: text.clone() });
                        if kind == "budget" || kind == "error" {
                            let _ = h.notification().builder().title("hubcodes").body(text).show();
                        }
                    });
                }
                std::thread::sleep(Duration::from_secs(5));
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            session::session_info,
            system::doctor,
            system::diagnostics,
            system::log_tail,
            system::usage_log,
            system::daemon_status,
            system::daemon_set,
            remote_access::pending_decide,
            rooms::room_list,
            rooms::room_log,
            rooms::room_create,
            rooms::room_members,
            rooms::room_delete,
            rooms::room_post,
            remote_access::audit_log,
            session::sessions,
            session::switch_session,
            updater::update_check,
            updater::update_install,
            system::history,
            config::providers,
            config::provider_status,
            config::account_list,
            config::account_add,
            config::account_remove,
            config::account_login,
            config::login_terminal,
            config::get_config,
            config::save_config,
            snapshot::get_snapshot,
            agents::spawn_agent,
            agents::agent_diff,
            agents::commit_agent,
            agents::open_pr,
            agents::set_role,
            agents::set_private,
            agents::kill_agent,
            agents::instruct,
            agents::save_attachment,
            agents::compose_message,
            agents::send_message,
            agents::answer_prompt,
            board::task_add,
            board::task_set,
            board::board_columns,
            board::board_columns_set,
            board::task_update,
            board::task_place,
            board::task_history,
            board::task_assign,
            board::task_remove,
            board::task_resume,
            board::pr_approve,
            board::pr_ignore,
            board::tasks_all,
            board::notes_write,
            board::notes_add,
            board::fanout_start,
            board::fanouts,
            teams::templates,
            teams::team_start,
            teams::team_save,
            teams::prompt_list,
            teams::prompt_preview,
            teams::role_brief,
            teams::roles_list,
            teams::role_get,
            teams::role_save,
            remote_access::serve_start,
            agents::run_tests,
            agents::merge_enqueue,
            agents::merges_list,
            session::session_def,
            session::restore_session,
            session::discard_saved_session,
            session::restart_agents,
            session::switch_agent,
            board::task_approve,
            board::task_request_changes,
            config::provider_export,
            config::provider_import,
            board::playbook_start,
            board::playbook_approve,
            board::playbook_abort,
            board::playbook_runs,
            board::playbooks,
            board::tracker_check,
            board::tracker_pull,
            board::skills_list,
            board::skill_copy,
            board::skills_sync,
            board::skill_add,
            board::skills_pull,
            board::tracker_overlaps,
            connect::connection_check,
            connect::twg_profiles,
            connect::connection_choices,
            connect::mcp_discover,
            config::policy_test,
            teams::team_init,
            knowledge::memory_ping,
            knowledge::memory_recall,
            knowledge::memory_retain,
            knowledge::kb_check,
            knowledge::kb_search,
            knowledge::kb_get,
            remote_access::tailscale_info,
            loops::set_loop,
            loops::reset_loop,
            loops::start_loop,
            peers::check_peer,
            peers::device_status,
            peers::this_device,
            peers::create_ssh_key,
            peers::device_retry,
            peers::device_models,
            peers::open_remote,
            agents::connect_agents,
            agents::disconnect_agents,
            panes::attach_pane,
            panes::detach_pane,
            panes::write_pane,
            panes::resize_pane,
            panes::read_pane,
            session::load_layout,
            session::save_layout,
            system::open_path,
            system::test_notification,
            system::log_js
        ])
        .run(tauri::generate_context!())
        .expect("error while running hubcodes");
}
