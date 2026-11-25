//! Canvas snapshot: the poll that builds it, agent status, housekeeping and notifications.

use super::*;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentDto {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub status: String,
    pub cwd: String,
    pub command: String,
    pub dead: bool,
    /// Seconds since the pane last changed.
    pub quiet_secs: f32,
    /// Last non-empty line of the pane, ANSI stripped (what the agent is asking).
    pub last_line: String,
    pub role: Option<String>,
    pub model: Option<String>,
    /// Device the agent runs on; `None` = this one.
    pub peer: Option<String>,
    /// Last screen lines for remote agents (no live terminal over SSH).
    pub preview: Vec<String>,
    /// The agent's own git worktree, when it was started with one.
    pub worktree: Option<String>,
    /// Branch and change counts of the agent's folder (refreshed every few seconds).
    pub changes: Option<Changes>,
    /// Tokens from the agent's own transcript (Claude Code via hooks); `None` when unknown.
    pub usage: Option<Usage>,
    /// Last test run: ok, unix time, summary.
    pub test: Option<(bool, i64, String)>,
    /// Human typing into this agent right now (from any client).
    pub holder: Option<String>,
    /// Messages waiting in the outbox for this agent to finish its turn.
    #[serde(default)]
    pub queued: i64,
    /// Guests of the shared canvas see this agent's status, not its screen.
    #[serde(default)]
    pub private: bool,
    /// The login it runs under (`None` = the tool's own).
    pub account: Option<String>,
    /// Added by auto team; removed again when idle with nothing waiting.
    #[serde(default)]
    pub extra: bool,
    /// The model it really answered with (from its transcript), when known.
    pub model_seen: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PeerDto {
    pub name: String,
    pub host: String,
    pub ok: bool,
    pub error: Option<String>,
    pub agents: usize,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EdgeDto {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MessageDto {
    pub seq: u64,
    pub from: String,
    pub to: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub session: String,
    /// What a person types to an agent goes to the orchestrator first.
    pub route_through_orchestrator: bool,
    pub agents: Vec<AgentDto>,
    pub edges: Vec<EdgeDto>,
    pub last_message: Option<MessageDto>,
    /// Loops keyed `a|b` (unordered pair).
    pub loops: BTreeMap<String, LoopState>,
    pub awaiting: BTreeMap<String, Awaiting>,
    pub peers: Vec<PeerDto>,
    pub self_name: String,
    pub tasks: Vec<Task>,
    pub notes: String,
    pub serving: Option<String>,
    pub merges: Vec<Merge>,
    /// Saved session definition that could be restored (agents not running now).
    pub restorable: Vec<String>,
    /// Running agents started with an older team brief (restart them to apply the current one).
    pub stale_brief: Vec<String>,
    /// Open playbook runs.
    pub runs: Vec<hubcodes_core::playbook::Run>,
    /// Teammates on the web canvas right now (pointer in flow coordinates).
    #[serde(default)]
    pub presence: Vec<hubcodes_core::serve::Presence>,
    /// Contributor messages waiting for approval.
    #[serde(default)]
    pub pending: Vec<hubcodes_core::db::PendingSend>,
}

impl AppState {
    /// Status for any agent from a preview: hash changes mark activity.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn status_for(&self, id: &str, preview: &[String], is_shell: bool, dead: bool, provider: &str, now: Instant, hook: Option<(&str, i64, &str)>) -> (String, f32, String) {
        let mut tracked = self.tracked.lock().unwrap_or_else(|e| e.into_inner());
        let mut h = DefaultHasher::new();
        preview.hash(&mut h);
        let hash = h.finish();
        let t = tracked.entry(id.to_string()).or_insert(Tracked { last_hash: 0, last_change: now });
        if hash != t.last_hash {
            t.last_hash = hash;
            t.last_change = now;
        }
        let quiet = now.duration_since(t.last_change);
        let last_line = preview.iter().rev().find(|l| !l.trim().is_empty()).map(|l| strip_ansi(l)).unwrap_or_default().trim().to_string();
        let waits = self
            .reg()
            .get(provider)
            .and_then(|pr| pr.waiting_pattern.as_deref())
            .and_then(|pat| Regex::new(pat).ok())
            .map(|re| re.is_match(&last_line))
            .unwrap_or_else(|| self.waiting.is_match(&last_line));
        // A fresh report from the agent's own hooks beats the screen heuristic.
        let fresh_hook = hook.filter(|(_, at, _)| hubcodes_core::db::now() - at < hooks::FRESH_SECS);
        let status: &str = if dead {
            "offline"
        } else if let Some((st, _, _)) = fresh_hook {
            match st {
                "waiting" => "waiting",
                "working" => "working",
                "idle" if quiet < Duration::from_millis(2500) => "working",
                "idle" => "idle",
                "offline" => "offline",
                _ => "idle",
            }
        } else if quiet < Duration::from_millis(2500) {
            "working"
        } else if waits && !is_shell {
            "waiting"
        } else {
            "idle"
        };
        let detail = fresh_hook.map(|(_, _, d)| d.to_string()).filter(|d| !d.is_empty());
        (status.into(), quiet.as_secs_f32(), detail.unwrap_or(last_line))
    }

    /// Make sure the session exists (detached, no canvas window needed: the
    /// desktop app *is* the canvas). Idempotent.
    pub(crate) fn ensure_session(&self) -> Result<()> {
        self.tmux.ensure()
    }

    pub(crate) fn snapshot(&self) -> Result<Snapshot> {
        let panes = self.tmux.panes()?;
        let canvas = self.tmux.canvas_pane().ok().flatten();
        let reg = self.reg();
        let mut agents = Vec::new();
        let now = Instant::now();
        let db = Db::open(&self.tmux.session).ok();
        let usage_map: HashMap<String, Usage> = db.as_ref().and_then(|d| d.all_usage().ok()).unwrap_or_default().into_iter().collect();
        let extras = hubcodes_core::autoteam::extras(&self.tmux.session);
        let queued: HashMap<String, i64> = db.as_ref().and_then(|d| d.outbox_counts().ok()).unwrap_or_default().into_iter().collect();
        // The placeholder window from ensure_session is only needed while the
        // session would otherwise be empty; drop it once a real agent exists.
        let placeholder: Vec<String> = panes
            .iter()
            .filter(|p| p.provider.is_none() && p.name == PLACEHOLDER_WINDOW)
            .map(|p| p.id.clone())
            .collect();
        let real = panes.iter().filter(|p| p.provider.is_some()).count();
        if real > 0 {
            for id in &placeholder {
                let _ = self.tmux.kill(id);
            }
        }
        for p in panes {
            if canvas.as_deref() == Some(p.id.as_str()) || p.current_command == "hubcodes" {
                continue;
            }
            if placeholder.contains(&p.id) || p.provider.as_deref() == Some(PROXY_PROVIDER) {
                continue;
            }
            let preview = self.tmux.capture(&p.id, PREVIEW_LINES).unwrap_or_default();
            let provider = p.provider.clone().unwrap_or_else(|| "shell".into());
            let is_shell = matches!(p.current_command.as_str(), "zsh" | "bash" | "fish" | "sh");
            let hook = match (&p.hook_status, p.hook_at) {
                (Some(st), Some(at)) => Some((st.as_str(), at, p.hook_detail.as_deref().unwrap_or(""))),
                _ => None,
            };
            let (status, quiet_secs, last_line) = self.status_for(&p.id, &preview, is_shell, p.dead || p.exited, &provider, now, hook);
            let cwd = pane_cwd(&p.id).unwrap_or_default();
            let changes = self.changes_for(&p.id, p.worktree.as_deref().unwrap_or(&cwd));
            let usage = usage_map.get(&p.name).cloned();
            let extra = extras.contains_key(&p.name);
            agents.push(AgentDto {
                id: p.id.clone(),
                name: p.name.clone(),
                provider,
                status,
                cwd,
                command: p.current_command.clone(),
                dead: p.dead || p.exited,
                quiet_secs,
                last_line: last_line.chars().take(160).collect(),
                role: p.role.clone(),
                model: p.model.clone(),
                peer: None,
                preview: Vec::new(),
                worktree: p.worktree.clone(),
                changes,
                usage,
                test: p.test.as_deref().and_then(testrun::parse_opt),
                holder: p.holder.clone(),
                queued: queued.get(&p.name).copied().unwrap_or(0),
                private: p.private,
                account: p.account.clone(),
                extra,
                model_seen: p.model_seen.clone(),
            });
        }
        // Remote agents, from the peer thread's cache.
        let mut peers_dto = Vec::new();
        {
            let cfg_peers: Vec<Peer> = reg.peers.clone();
            let cache = self.peer_cache.lock().unwrap_or_else(|e| e.into_inner());
            for peer in cfg_peers {
                match cache.get(&peer.name) {
                    Some((_, Ok(snap))) => {
                        peers_dto.push(PeerDto { name: peer.name.clone(), host: peer.host.clone(), ok: true, error: None, agents: snap.agents.len() });
                        for a in &snap.agents {
                            let id = format!("{}/{}", peer.name, a.name);
                            let is_shell = matches!(a.command.as_str(), "zsh" | "bash" | "fish" | "sh");
                            let (status, quiet_secs, last_line) = self.status_for(&id, &a.preview, is_shell, a.dead, &a.provider, now, None);
                            agents.push(AgentDto {
                                id: id.clone(),
                                name: id,
                                provider: a.provider.clone(),
                                status,
                                cwd: a.cwd.clone(),
                                command: a.command.clone(),
                                dead: a.dead,
                                quiet_secs,
                                last_line: last_line.chars().take(160).collect(),
                                role: a.role.clone(),
                                model: a.model.clone(),
                                peer: Some(peer.name.clone()),
                                preview: a.preview.clone(),
                                worktree: None,
                                changes: None,
                                usage: None,
                                test: None,
                                holder: None,
                                queued: 0,
                                private: a.private,
                                account: a.account.clone(),
                                extra: false,
                                model_seen: None,
                            });
                        }
                    }
                    Some((_, Err(e))) => peers_dto.push(PeerDto { name: peer.name.clone(), host: peer.host.clone(), ok: false, error: Some(e.clone()), agents: 0 }),
                    None => peers_dto.push(PeerDto { name: peer.name.clone(), host: peer.host.clone(), ok: false, error: Some("checking…".into()), agents: 0 }),
                }
            }
        }
        let edges = self
            .tmux
            .edges()?
            .into_iter()
            .map(|(from, to, label)| EdgeDto { from, to, label })
            .collect();
        let last_message = self
            .tmux
            .last_message()?
            .map(|(seq, from, to, text)| MessageDto { seq, from, to, text });
        let restorable: Vec<String> = sessiondef::load(&self.tmux.session)
            .map(|d| d.agents.into_iter().map(|a| a.name).filter(|n| !agents.iter().any(|a| &a.name == n)).collect())
            .unwrap_or_default();
        let stale_brief: Vec<String> = self
            .tmux
            .panes()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| !p.dead && !p.exited && p.role.is_some() && p.provider.as_deref().map(|v| v != "shell" && v != "proxy").unwrap_or(false))
            .filter(|p| p.brief_v.as_deref() != Some(hubcodes_core::prompts::BRIEF_VERSION))
            .map(|p| p.name)
            .collect();
        Ok(Snapshot {
            session: self.tmux.session.clone(),
            route_through_orchestrator: self.reg().workspace.route_through_orchestrator,
            agents,
            edges,
            last_message,
            loops: relay::loops(&self.tmux),
            awaiting: relay::awaiting(&self.tmux),
            peers: peers_dto,
            self_name: reg.sharing.name.clone(),
            tasks: db.as_ref().and_then(|d| tasks::list(d, false).ok()).unwrap_or_default(),
            notes: notes::read(&self.tmux.session),
            serving: self.serving.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            merges: db.as_ref().and_then(|d| merge::list(d, 10).ok()).unwrap_or_default(),
            restorable,
            stale_brief,
            runs: Db::open(&self.tmux.session).ok().and_then(|d| hubcodes_core::playbook::list(&d, true).ok()).unwrap_or_default(),
            presence: hubcodes_core::serve::presence(),
            pending: db.as_ref().and_then(|d| d.pending_open().ok()).unwrap_or_default(),
        })
    }

    /// Core housekeeping every 5 s: task dispatch, reminders, budgets, triggers.
    pub(crate) fn tend(&self, app: &AppHandle, snap: &Snapshot) {
        {
            let mut at = self.tend_at.lock().unwrap_or_else(|e| e.into_inner());
            if at.elapsed() < Duration::from_secs(5) {
                return;
            }
            *at = Instant::now();
        }
        // A background `hubcodes watch` (launchd) may own dispatch and triggers; one tender per session.
        if !hubcodes_core::daemon::try_tend(&self.tmux.session, "desktop") {
            return;
        }
        let agents: Vec<tend::AgentState> = snap
            .agents
            .iter()
            .map(|a| tend::AgentState { name: a.name.clone(), role: a.role.clone().unwrap_or_default(), status: a.status.clone(), quiet_secs: a.quiet_secs, remote: a.peer.is_some(), dead: a.dead, last_line: a.last_line.clone(), dir: a.worktree.clone().unwrap_or_else(|| a.cwd.clone()) })
            .collect();
        let reg = self.reg().clone();
        let mut mem = self.tend_mem.lock().unwrap_or_else(|e| e.into_inner());
        for ev in tend::tend(&self.tmux, &reg, &agents, &mut mem) {
            eprintln!("[hubcodes] {}", ev.text);
            let _ = app.emit("tend", &ev);
            if ev.kind == "budget" || ev.kind == "error" {
                let _ = app.notification().builder().title("hubcodes").body(ev.text.clone()).show();
            }
        }
    }

    /// Loop housekeeping each poll: remind an agent that owes a reply and went
    /// quiet, and tell the user once when a loop pauses at its round limit.
    pub(crate) fn tend_loops(&self, app: &AppHandle, snap: &Snapshot) {
        let mut seen = self.paused_seen.lock().unwrap_or_else(|e| e.into_inner());
        for (key, st) in &snap.loops {
            if st.paused && !seen.contains(key) {
                seen.insert(key.clone());
                let _ = app
                    .notification()
                    .builder()
                    .title(format!("Loop {} paused", key.replace('|', " ↔ ")))
                    .body(format!("{} rounds reached. Reset the loop on the canvas to continue.", st.max))
                    .show();
            }
            if !st.paused {
                seen.remove(key);
            }
        }
    }

    /// OS notification for every agent that just entered *needs you*, when the
    /// window is not focused (the top bar already says so when it is), and a
    /// Dock badge with the count either way.
    pub(crate) fn notify_needs_you(&self, app: &AppHandle, snap: &Snapshot) {
        let now: HashSet<String> = snap
            .agents
            .iter()
            .filter(|a| a.status == "waiting")
            .map(|a| a.id.clone())
            .collect();
        let fresh: Vec<&AgentDto> = {
            let mut was = self.was_waiting.lock().unwrap_or_else(|e| e.into_inner());
            let fresh = snap
                .agents
                .iter()
                .filter(|a| now.contains(&a.id) && !was.contains(&a.id))
                .collect();
            *was = now.clone();
            fresh
        };
        let window = app.get_webview_window("main");
        if let Some(w) = &window {
            let n = now.len() as i64;
            let _ = w.set_badge_count(if n > 0 { Some(n) } else { None });
        }
        if fresh.is_empty() || !self.reg().desktop.notifications {
            return;
        }
        let focused = window.and_then(|w| w.is_focused().ok()).unwrap_or(false);
        eprintln!(
            "[hubcodes] needs you: {} (window focused: {focused})",
            fresh.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
        );
        if focused {
            return;
        }
        for a in fresh {
            let body = if a.last_line.is_empty() { a.provider.clone() } else { a.last_line.clone() };
            if let Err(e) = app
                .notification()
                .builder()
                .title(format!("{} needs you", a.name))
                .body(body)
                .show()
            {
                eprintln!("[hubcodes] notification failed: {e}");
            }
        }
    }
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, Arc<AppState>>) -> Result<Snapshot, String> {
    state.ensure_session().map_err(err)?;
    state.snapshot().map_err(err)
}
