//! Other devices: peer polling, device status, the remote queue, remote terminals.

use super::*;

impl AppState {
    /// Refresh every peer's agent list (slow: SSH), called from the peer thread.
    pub(crate) fn poll_peers(self: &Arc<Self>) {
        let peers: Vec<Peer> = self.reg().peers.clone();
        for p in peers.iter().filter(|p| remote::transport_in_use(p) == "http") {
            self.follow(p.name.clone());
        }
        let followed = self.fed_watch.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for p in peers.into_iter().filter(|p| !followed.contains(&p.name)) {
            let res = remote::snapshot(&p).map_err(|e| format!("{e:#}"));
            self.peer_cache.lock().unwrap_or_else(|e| e.into_inner()).insert(p.name.clone(), (Instant::now(), res));
        }
        let names: HashSet<String> = self.reg().peers.iter().map(|p| p.name.clone()).collect();
        self.peer_cache.lock().unwrap_or_else(|e| e.into_inner()).retain(|k, _| names.contains(k));
        self.write_peer_file();
    }

    /// The CLI (briefs, guard, `task done mac:3`) and the daemon read this file.
    fn write_peer_file(&self) {
        let good: std::collections::BTreeMap<String, RemoteSnapshot> = self
            .peer_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|(k, (_, r))| r.as_ref().ok().map(|s| (k.clone(), s.clone())))
            .collect();
        if !self.reg().peers.is_empty() {
            let _ = remote::cache_write(&self.tmux.session, &good);
        }
    }

    /// Follow a device through its federation listener: each answer of the
    /// long poll is a change there, so its cards move without the timed
    /// poll. Ends when the device is removed, leaves `http`, or its listener
    /// is gone (the timed poll, over SSH, takes over).
    fn follow(self: &Arc<Self>, name: String) {
        if !self.fed_watch.lock().unwrap_or_else(|e| e.into_inner()).insert(name.clone()) {
            return;
        }
        let st = self.clone();
        std::thread::spawn(move || {
            let mut etag = String::new();
            while let Some(peer) = st.reg().peers.iter().find(|p| p.name == name).cloned().filter(|p| remote::transport_in_use(p) == "http") {
                match hubcodes_core::fed::watch(&peer, &etag) {
                    Ok((tag, snap)) => {
                        etag = tag;
                        st.peer_cache.lock().unwrap_or_else(|e| e.into_inner()).insert(name.clone(), (Instant::now(), Ok(snap)));
                        st.write_peer_file();
                    }
                    Err(hubcodes_core::fed::CallError::Unavailable(why)) => {
                        hubcodes_core::fed::remember_fallback(&peer, &why);
                        break;
                    }
                    Err(e) => {
                        st.peer_cache.lock().unwrap_or_else(|e| e.into_inner()).insert(name.clone(), (Instant::now(), Err(format!("{:#}", e.into_error(&peer)))));
                        etag.clear();
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            }
            st.fed_watch.lock().unwrap_or_else(|e| e.into_inner()).remove(&name);
        });
    }

    /// Wait until the next device poll. While a device does not answer,
    /// watch Tailscale every 3 s instead of sleeping blind: when Tailscale
    /// comes back here or the device comes online, poll it right away.
    pub(crate) fn wait_for_peers(&self) {
        let wait = self.peer_poll_wait();
        let away = self.peer_cache.lock().unwrap_or_else(|e| e.into_inner()).values().any(|(_, r)| r.is_err());
        let hosts: Vec<String> = self.reg().peers.iter().map(|p| p.host.clone()).collect();
        let before = if away { remote::tailnet_state(&hosts) } else { None };
        let Some(before) = before else {
            std::thread::sleep(wait);
            return;
        };
        let until = Instant::now() + wait;
        while let Some(left) = until.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
            std::thread::sleep(left.min(PEER_POLL));
            if remote::tailnet_state(&hosts).is_some_and(|now| now != before) {
                log_line("devices: Tailscale changed, polling now");
                return;
            }
        }
    }

    /// 3 s while an agent over there is busy or just stopped, 15 s when all
    /// sit quiet, 60 s when a device does not answer (SSH timeouts are slow).
    pub(crate) fn peer_poll_wait(&self) -> Duration {
        let cache = self.peer_cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.is_empty() {
            return PEER_POLL;
        }
        let unreachable = cache.values().any(|(_, r)| r.is_err());
        let mut peers = std::collections::BTreeMap::new();
        for (k, (_, r)) in cache.iter() {
            if let Ok(s) = r {
                peers.insert(k.clone(), s.clone());
            }
        }
        Duration::from_secs(remote::next_poll_secs(Some(&remote::PeerCache { at: 0, peers }), unreachable))
    }
}

/// `ssh host hubcodes --version` for Settings → Devices.
/// What Settings → Devices shows for one device: reachability, last poll,
/// agents, and the reports waiting for it in the remote queue.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatus {
    pub name: String,
    pub host: String,
    pub ok: bool,
    pub error: Option<String>,
    pub agents: usize,
    /// Seconds since the last poll answered or failed; `None` before the first.
    pub polled_secs_ago: Option<u64>,
    pub next_poll_secs: u64,
    pub queued: Vec<remote::QueuedCommand>,
    /// What is off about the pairing (version skew, names), one line each.
    pub issues: Vec<String>,
    /// The device's hubcodes version, when its answer carries one.
    pub version: Option<String>,
}

#[tauri::command]
pub fn device_status(state: State<'_, Arc<AppState>>, name: String) -> Result<DeviceStatus, String> {
    let reg = state.reg();
    let peer = reg.peers.iter().find(|p| p.name == name).ok_or_else(|| format!("unknown device '{name}'"))?;
    let (ok, error, agents, polled_secs_ago) = {
        let cache = state.peer_cache.lock().unwrap_or_else(|e| e.into_inner());
        match cache.get(&name) {
            Some((at, Ok(s))) => (true, None, s.agents.len(), Some(at.elapsed().as_secs())),
            Some((at, Err(e))) => (false, Some(e.clone()), 0, Some(at.elapsed().as_secs())),
            None => (false, Some("not polled yet".into()), 0, None),
        }
    };
    let queued: Vec<remote::QueuedCommand> = remote::queued(&state.tmux.session).into_iter().filter(|c| c.peer == name).collect();
    let (issues, version) = {
        let cache = state.peer_cache.lock().unwrap_or_else(|e| e.into_inner());
        match cache.get(&name) {
            Some((_, Ok(s))) => (remote::pairing_issues(s, peer, &reg.sharing.name, env!("CARGO_PKG_VERSION")), s.version.clone()),
            _ => (Vec::new(), None),
        }
    };
    Ok(DeviceStatus { name: peer.name.clone(), host: peer.host.clone(), ok, error, agents, polled_secs_ago, next_poll_secs: state.peer_poll_wait().as_secs(), queued, issues, version })
}

/// Poll every device now and retry everything queued for them. Returns
/// what was delivered or dropped, one line each.
#[tauri::command]
pub async fn device_retry(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let st: Arc<AppState> = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        st.poll_peers();
        let peers = st.reg().peers.clone();
        Ok(remote::flush_queue(&st.tmux.session, &peers)
            .into_iter()
            .map(|f| match f {
                remote::Flushed::Delivered(c) => format!("delivered to {}: hubcodes {}", c.peer, c.args.iter().take(3).cloned().collect::<Vec<_>>().join(" ")),
                remote::Flushed::Expired(c) => format!("dropped (too old) for {}: hubcodes {}", c.peer, c.args.iter().take(3).cloned().collect::<Vec<_>>().join(" ")),
            })
            .collect())
    })
    .await
    .map_err(err)?
}

/// The providers and model catalogs on another device, cached for 5 minutes.
#[tauri::command]
pub async fn device_models(state: State<'_, Arc<AppState>>, name: String, refresh: bool) -> Result<Vec<ProviderStatus>, String> {
    if !refresh {
        let cache = state.peer_models.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, v)) = cache.get(&name) {
            if at.elapsed() < Duration::from_secs(300) {
                return Ok(v.clone());
            }
        }
    }
    let st: Arc<AppState> = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let peer = st.reg().peers.iter().find(|p| p.name == name).cloned().ok_or_else(|| format!("unknown device '{name}'"))?;
        let v = remote::provider_statuses(&peer).map_err(|e| format!("{e:#}"))?;
        st.peer_models.lock().unwrap_or_else(|e| e.into_inner()).insert(name, (Instant::now(), v.clone()));
        Ok(v)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn check_peer(state: State<'_, Arc<AppState>>, peer: Peer) -> Result<String, String> {
    let st: Arc<AppState> = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let v = remote::check(&peer).map_err(|e| format!("{e:#}"))?;
        let n = remote::snapshot(&peer).map(|s| s.agents.len()).map_err(|e| format!("{e:#}"))?;
        st.peer_cache.lock().unwrap_or_else(|e| e.into_inner()).remove(&peer.name);
        Ok(format!("{v} · {n} agents"))
    })
    .await
    .map_err(err)?
}

/// A local pane showing a remote agent's terminal: `ssh -t host tmux attach`.
/// Returns the pane id to stream; the pane is tagged so it is not an agent.
#[tauri::command]
pub fn open_remote(state: State<'_, Arc<AppState>>, id: String) -> Result<String, String> {
    let (peer_name, agent) = id.split_once('/').ok_or("not a remote agent")?;
    let reg = state.reg();
    let peer = reg.peers.iter().find(|p| p.name == peer_name).ok_or_else(|| format!("unknown device {peer_name}"))?;
    let window = format!("proxy:{id}");
    // Reuse an existing proxy pane.
    if let Ok(panes) = state.tmux.panes() {
        if let Some(p) = panes.iter().find(|p| p.name == window && !p.dead) {
            return Ok(p.id.clone());
        }
    }
    let attach = format!("tmux attach -t {}:{}", sh_quote(&peer.session), sh_quote(agent));
    let cmd = if peer.host == "local" {
        attach
    } else {
        let target = match &peer.user {
            Some(u) if !u.is_empty() => format!("{u}@{}", peer.host),
            _ => peer.host.clone(),
        };
        // The far side's bare SSH PATH has no Homebrew, so no tmux.
        let attach = format!("{}{attach}", hubcodes_core::remote::FAR_PATH);
        format!("ssh -t -o BatchMode=yes -o StrictHostKeyChecking=accept-new {} {}", sh_quote(&target), sh_quote(&attach))
    };
    state.tmux.spawn(&window, PROXY_PROVIDER, &cmd, None).map_err(err)
}

/// Settings → Devices → This Mac: what another device needs to reach this one.
#[tauri::command]
pub async fn this_device() -> Result<remote::ThisDevice, String> {
    tauri::async_runtime::spawn_blocking(remote::this_device).await.map_err(err)
}

/// Settings → Devices → This Mac → Create key (only when there is none).
#[tauri::command]
pub async fn create_ssh_key() -> Result<remote::ThisDevice, String> {
    tauri::async_runtime::spawn_blocking(|| remote::create_key().map(|_| remote::this_device()).map_err(|e| format!("{e:#}"))).await.map_err(err)?
}
