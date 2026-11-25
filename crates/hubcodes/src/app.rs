//! Application state, input handling and the frame layout around the canvas.

use crate::canvas::{Camera, CanvasView, Zoom};
use crate::demo::Demo;
use crate::layout::SavedLayout;
use hubcodes_core::model::{AgentId, Participant, Status, Workspace};
use hubcodes_core::providers::{Registry, DEFAULT_WAITING_PATTERN};
use crate::theme;
use hubcodes_core::tmux::Tmux;
use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

const PREVIEW_LINES: usize = 12;
const TICK: Duration = Duration::from_millis(100);
const BACKEND_EVERY_TICKS: u64 = 5;
const PULSE_SPEED: f32 = 0.07;
/// How often (in ticks) the layout file is compared and, if changed, written.
const LAYOUT_SAVE_EVERY_TICKS: u64 = 10;

pub enum Backend {
    Demo(Demo),
    Tmux {
        tmux: Tmux,
        /// Pane running this canvas (`$TMUX_PANE`); skipped in sync.
        canvas_pane: Option<String>,
        /// Window that holds the canvas pane; agent panes in it are "open".
        canvas_window: Option<String>,
        last_seq: u64,
        waiting: Regex,
    },
}

impl Backend {
    fn label(&self) -> String {
        match self {
            Backend::Demo(_) => "demo".into(),
            Backend::Tmux { tmux, .. } => format!("tmux:{}", tmux.session),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Mode {
    Normal,
    Help,
    Connect { from: AgentId },
    /// Agent → agent message (from the selected agent to a neighbour).
    Message { from: AgentId, to: AgentId, input: String },
    Spawn { name: String, provider_idx: usize },
    ConfirmKill { id: AgentId },
}

struct Drag {
    id: AgentId,
    dx: i32,
    dy: i32,
}

pub struct App {
    pub ws: Workspace,
    pub cam: Camera,
    pub reg: Registry,
    pub backend: Backend,
    pub selected: Option<AgentId>,
    pub mode: Mode,
    pub show_log: bool,
    me: String,
    drag: Option<Drag>,
    pan_from: Option<(i32, i32, i32, i32)>,
    /// Agents marked for a group action: Space toggles, Shift+click toggles,
    /// Shift+drag on empty canvas marks what the box covers; Enter sends the
    /// draft to all of them; Esc clears.
    marked: BTreeSet<AgentId>,
    /// Box select in progress: screen start, screen current.
    box_sel: Option<((i32, i32), (i32, i32))>,
    toast: Option<(String, Instant)>,
    frame: u64,
    canvas_area: Rect,
    running: bool,
    /// Saved layout for the tmux session; `None` for the demo (never persisted).
    layout: Option<SavedLayout>,
    /// Per-node composer text. Typing on the canvas edits the selected node's draft;
    /// `Enter` pastes it into that agent's terminal.
    drafts: HashMap<AgentId, String>,
    /// Status seen at the previous tick, to ring the bell once per
    /// transition into `Waiting` (not on every tick the agent stays there).
    last_status: HashMap<AgentId, Status>,
}

impl App {
    pub fn new(reg: Registry, backend: Backend) -> Self {
        let me = std::env::var("USER").unwrap_or_else(|_| "me".into());
        let mut ws = Workspace::new("workspace");
        ws.participants.push(Participant {
            name: me.clone(),
            color_idx: 4,
            selected: None,
            is_me: true,
            read_only: false,
        });
        App {
            ws,
            cam: Camera::default(),
            reg,
            backend,
            selected: None,
            mode: Mode::Normal,
            show_log: true,
            me,
            drag: None,
            pan_from: None,
            marked: BTreeSet::new(),
            box_sel: None,
            toast: None,
            frame: 0,
            canvas_area: Rect::default(),
            running: true,
            layout: None,
            drafts: HashMap::new(),
            last_status: HashMap::new(),
        }
    }

    pub fn demo(reg: Registry) -> Self {
        let mut app = App::new(reg, Backend::Demo(Demo::new(&mut Workspace::new("tmp"))));
        // Rebuild the demo on the real workspace so ids line up.
        let demo = Demo::new(&mut app.ws);
        app.backend = Backend::Demo(demo);
        app.selected = app.ws.agents.first().map(|a| a.id);
        app
    }

    pub fn tmux(reg: Registry, tmux: Tmux) -> Result<Self> {
        let canvas_pane = std::env::var("TMUX_PANE").ok().filter(|s| !s.is_empty());
        let canvas_window = canvas_pane.as_deref().and_then(|p| tmux.window_of(p).ok());
        if let Some(p) = &canvas_pane {
            if let Err(e) = tmux.set_canvas_pane(p) {
                eprintln!("warn: could not record canvas pane: {e}");
            }
        }
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "hubcodes".into());
        if let Err(e) = tmux.bind_return_keys(&exe) {
            eprintln!("warn: could not bind return keys: {e}");
        }
        let waiting = Regex::new(DEFAULT_WAITING_PATTERN)?;
        let last_seq = tmux.last_message()?.map(|m| m.0).unwrap_or(0);
        let mut app = App::new(
            reg,
            Backend::Tmux {
                tmux: tmux.clone(),
                canvas_pane,
                canvas_window,
                last_seq,
                waiting,
            },
        );
        app.ws.name = tmux.session.clone();
        let saved = SavedLayout::load(&tmux.session);
        app.cam = (&saved.camera).into();
        app.layout = Some(saved);
        app.sync_tmux();
        // Apply the workspace section of hubcodes.toml on first run only.
        let cfg = app.reg.workspace.clone();
        for a in &cfg.agents {
            if app.ws.agent_by_name(&a.name).is_none() {
                app.spawn(&a.name, &a.provider, a.cwd.as_deref());
            }
        }
        app.sync_tmux();
        for a in &cfg.agents {
            if let Some(role) = &a.role {
                if let Some(ag) = app.ws.agents.iter_mut().find(|x| x.name == a.name) {
                    ag.role = Some(role.clone());
                }
            }
        }
        if let Some(src) = &app.reg.source {
            app.toast = Some((format!("config: {}", src.display()), Instant::now()));
        }
        for [from, to] in &cfg.edges {
            if let (Some(f), Some(t)) = (app.ws.agent_by_name(from), app.ws.agent_by_name(to)) {
                let (f, t) = (f.id, t.id);
                app.connect(f, t);
            }
        }
        app.selected = app.ws.agents.first().map(|a| a.id);
        Ok(app)
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut last_tick = Instant::now();
        while self.running {
            terminal.draw(|f| self.draw(f))?;
            let timeout = TICK.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                match event::read()? {
                    Event::Key(k) => self.on_key(k),
                    Event::Mouse(m) => self.on_mouse(m),
                    _ => {}
                }
            }
            if last_tick.elapsed() >= TICK {
                self.tick();
                last_tick = Instant::now();
            }
        }
        self.save_layout();
        Ok(())
    }

    // ------------------------------------------------------------------ tick

    fn tick(&mut self) {
        self.frame += 1;
        self.ws.tick_pulses(PULSE_SPEED);
        if let Some((msg, at)) = &self.toast {
            let keep = if msg.contains("failed") || msg.contains("unknown") { 8 } else { 3 };
            if at.elapsed() > Duration::from_secs(keep) {
                self.toast = None;
            }
        }
        if self.frame.is_multiple_of(BACKEND_EVERY_TICKS) {
            match &mut self.backend {
                Backend::Demo(d) => {
                    if self.frame.is_multiple_of(BACKEND_EVERY_TICKS * 2) {
                        d.tick(&mut self.ws);
                    }
                }
                Backend::Tmux { .. } => self.sync_tmux(),
            }
            self.ring_on_needs_you();
        }
        if let Some(me) = self.ws.participants.iter_mut().find(|p| p.is_me) {
            me.selected = self.selected;
        }
        if self.frame.is_multiple_of(LAYOUT_SAVE_EVERY_TICKS) {
            self.save_layout();
        }
    }

    /// Terminal bell once per agent when it enters `Waiting` (needs you).
    /// Steady states and leaving the state stay silent.
    fn ring_on_needs_you(&mut self) {
        let mut ring = false;
        let mut now: HashMap<AgentId, Status> = HashMap::with_capacity(self.ws.agents.len());
        for a in &self.ws.agents {
            now.insert(a.id, a.status);
            let was = self.last_status.get(&a.id).copied();
            if a.status == Status::Waiting && was.is_some() && was != Some(Status::Waiting) {
                ring = true;
            }
        }
        self.last_status = now;
        if ring {
            let mut out = std::io::stdout();
            let _ = crossterm::execute!(out, crossterm::style::Print("\x07"));
        }
    }

    /// Select the next agent (after the current selection) whose status is
    /// `Waiting` and pan so it is on screen.
    fn jump_to_needs_you(&mut self) {
        let waiting: Vec<AgentId> = self.ws.agents.iter().filter(|a| a.status == Status::Waiting).map(|a| a.id).collect();
        if waiting.is_empty() {
            self.toast = Some(("nobody needs you right now".into(), Instant::now()));
            return;
        }
        let cur = self.selected.and_then(|id| self.ws.agents.iter().position(|a| a.id == id)).unwrap_or(0);
        let next = self
            .ws
            .agents
            .iter()
            .enumerate()
            .cycle()
            .skip(cur + 1)
            .take(self.ws.agents.len())
            .find(|(_, a)| a.status == Status::Waiting)
            .map(|(_, a)| a.id)
            .unwrap_or(waiting[0]);
        self.selected = Some(next);
        self.ensure_visible(next);
    }

    /// Pan the camera so the node is fully inside the canvas (centred when
    /// it was off screen); no-op when it already is.
    fn ensure_visible(&mut self, id: AgentId) {
        let Some(a) = self.ws.agent(id) else { return };
        let area = self.canvas_area;
        if area.width == 0 || area.height == 0 {
            return;
        }
        let r = self.cam.node_rect(a);
        let (w, h) = (area.width as i32, area.height as i32);
        if r.x >= 0 && r.y >= 0 && r.right() <= w && r.bottom() <= h {
            return;
        }
        self.cam.ox += r.x - (w - r.w) / 2;
        self.cam.oy += r.y - (h - r.h) / 2;
    }

    /// Write the layout file when positions or camera changed since the last
    /// write. Keeps saved positions of agents that are not currently running
    /// so a killed-and-respawned agent lands where it was.
    fn save_layout(&mut self) {
        let Some(saved) = self.layout.as_mut() else { return };
        let mut now = SavedLayout::capture(&self.ws, self.cam);
        for (name, pos) in &saved.nodes {
            now.nodes.entry(name.clone()).or_insert(*pos);
        }
        if now != *saved {
            if let Err(e) = now.save(&self.ws.name) {
                self.toast = Some((format!("layout not saved: {e}"), Instant::now()));
            }
            *saved = now;
        }
    }

    /// Reconcile the workspace with tmux: windows → agents, session options →
    /// edges and messages, pane captures → previews and status.
    fn sync_tmux(&mut self) {
        let Backend::Tmux {
            tmux,
            canvas_pane,
            canvas_window,
            last_seq,
            waiting,
        } = &mut self.backend
        else {
            return;
        };
        let Ok(panes) = tmux.panes() else {
            self.toast = Some(("tmux session gone".into(), Instant::now()));
            return;
        };
        let mut seen: Vec<AgentId> = Vec::new();
        for w in panes {
            if canvas_pane.as_deref() == Some(w.id.as_str()) {
                continue;
            }
            // Only the canvas pane itself counts as "the canvas": another
            // plain pane in that window is a user's split, not an agent.
            let open = canvas_window.as_deref() == Some(w.window_id.as_str());
            if open && w.provider.is_none() && canvas_pane.is_some() && w.current_command == "hubcodes" {
                continue;
            }
            let id = match self.ws.agents.iter().find(|a| a.tmux_target.as_deref() == Some(&w.id)) {
                Some(a) => a.id,
                None => {
                    let provider = w.provider.clone().unwrap_or_else(|| "shell".into());
                    let id = self.ws.add_agent(w.name.clone(), provider);
                    let a = self.ws.agent_mut(id).unwrap();
                    a.tmux_target = Some(w.id.clone());
                    if let Some((x, y)) = self.layout.as_ref().and_then(|l| l.position_of(&w.name)) {
                        a.x = x;
                        a.y = y;
                    }
                    id
                }
            };
            seen.push(id);
            let preview = tmux.capture(&w.id, PREVIEW_LINES).unwrap_or_default();
            let mut h = DefaultHasher::new();
            preview.hash(&mut h);
            let hash = h.finish();
            let a = self.ws.agent_mut(id).unwrap();
            a.name = w.name.clone();
            a.open = open;
            if let Some(p) = &w.provider {
                a.provider = p.clone();
            }
            let now = Instant::now();
            if hash != a.last_hash {
                a.last_hash = hash;
                a.last_change = now;
                a.preview = squeeze_blank_lines(preview);
            }
            let quiet = now.duration_since(a.last_change);
            let last_line = a
                .preview
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(|l| crate::canvas::strip_ansi(l))
                .unwrap_or_default();
            let is_shell = matches!(w.current_command.as_str(), "zsh" | "bash" | "fish" | "sh");
            let provider_waiting = self
                .reg
                .get(&a.provider)
                .and_then(|p| p.waiting_pattern.as_deref())
                .and_then(|pat| Regex::new(pat).ok());
            let waits = match &provider_waiting {
                Some(re) => re.is_match(last_line.trim()),
                None => waiting.is_match(last_line.trim()),
            };
            a.status = if w.dead {
                Status::Offline
            } else if quiet < Duration::from_millis(2500) {
                Status::Working
            } else if waits && !is_shell {
                Status::Waiting
            } else {
                Status::Idle
            };
        }
        let gone: Vec<AgentId> = self
            .ws
            .agents
            .iter()
            .filter(|a| a.tmux_target.is_some() && !seen.contains(&a.id))
            .map(|a| a.id)
            .collect();
        for id in gone {
            self.ws.remove_agent(id);
            if self.selected == Some(id) {
                self.selected = None;
            }
            self.drafts.remove(&id);
        }
        if self.selected.is_none() {
            self.selected = self.ws.agents.first().map(|a| a.id);
        }
        self.marked.retain(|m| self.ws.agents.iter().any(|a| a.id == *m));
        if let Ok(edges) = tmux.edges() {
            for (from, to, label) in edges {
                if let (Some(f), Some(t)) = (self.ws.agent_by_name(&from), self.ws.agent_by_name(&to)) {
                    let (f, t) = (f.id, t.id);
                    self.ws.connect(f, t, label);
                }
            }
        }
        if let Ok(Some((seq, from, to, text))) = tmux.last_message() {
            if seq > *last_seq {
                *last_seq = seq;
                match (self.ws.agent_by_name(&from), self.ws.agent_by_name(&to)) {
                    (Some(f), Some(t)) => {
                        let (f, t) = (f.id, t.id);
                        self.ws.send(f, t, &text);
                    }
                    _ => self.ws.log_human(&from, &format!("→ {to}: {text}")),
                }
            }
        }
    }

    // --------------------------------------------------------------- actions

    fn connect(&mut self, from: AgentId, to: AgentId) {
        if self.ws.connect(from, to, "link") {
            if let Backend::Tmux { tmux, .. } = &self.backend {
                let (Some(f), Some(t)) = (self.ws.agent(from), self.ws.agent(to)) else {
                    return;
                };
                if let Err(e) = tmux.add_edge(&f.name, &t.name, "link") {
                    self.toast = Some((format!("tmux: {e}"), Instant::now()));
                }
            }
            self.toast = Some(("connected".into(), Instant::now()));
        } else if self.ws.edge_between(from, to).is_some() {
            self.ws.disconnect(from, to);
            if let Backend::Tmux { tmux, .. } = &self.backend {
                if let (Some(f), Some(t)) = (self.ws.agent(from), self.ws.agent(to)) {
                    let _ = tmux.remove_edge(&f.name, &t.name);
                }
            }
            self.toast = Some(("disconnected".into(), Instant::now()));
        }
    }

    fn send_message(&mut self, from: AgentId, to: AgentId, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if let Backend::Tmux { tmux, last_seq, .. } = &mut self.backend {
            let (Some(f), Some(t)) = (self.ws.agent(from), self.ws.agent(to)) else {
                return;
            };
            let Some(target) = t.tmux_target.clone() else {
                return;
            };
            let (fname, tname) = (f.name.clone(), t.name.clone());
            // Plain shells would try to execute "[from x]"; only agents get the prefix.
            let prefixed = if t.provider == "shell" { text.to_string() } else { format!("[from {fname}] {text}") };
            if let Err(e) = tmux.send_text(&target, &prefixed) {
                self.toast = Some((format!("send failed: {e}"), Instant::now()));
                return;
            }
            if let Ok(seq) = tmux.record_message(&fname, &tname, text) {
                *last_seq = seq;
            }
        }
        self.ws.send(from, to, text);
    }

    fn instruct(&mut self, to: AgentId, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let me = self.me.clone();
        if let Backend::Tmux { tmux, .. } = &self.backend {
            let Some(t) = self.ws.agent(to) else { return };
            let Some(target) = t.tmux_target.clone() else { return };
            // It checks with its orchestrator before acting (route_through_orchestrator).
            if let Some(lead) = hubcodes_core::relay::route_for(tmux, &self.reg, &t.name) {
                let name = t.name.clone();
                match hubcodes_core::relay::deliver_from_person(tmux, &self.reg, &format!("human:{me}"), &name, text, false, false) {
                    Ok(_) => self.toast = Some((format!("sent to {name}; it checks with {lead} before acting"), Instant::now())),
                    Err(e) => self.toast = Some((format!("send failed: {e:#}"), Instant::now())),
                }
                self.ws.log_human(&me, &format!("→ {name} (checks with {lead}): {}", text.trim_end().replace('\n', " ⏎ ")));
                return;
            }
            if let Err(e) = tmux.send_text(&target, text) {
                self.toast = Some((format!("send failed: {e}"), Instant::now()));
                return;
            }
        } else if let Some(a) = self.ws.agent_mut(to) {
            for (i, line) in text.lines().enumerate() {
                if i == 0 {
                    a.preview.push(format!("\x1b[36m{me}>\x1b[0m {line}"));
                } else {
                    a.preview.push(format!("   {line}"));
                }
            }
            a.status = Status::Working;
            a.last_change = Instant::now();
        }
        let name = self.ws.agent(to).map(|a| a.name.clone()).unwrap_or_default();
        // The activity panel is one entry per line; show newlines as ⏎.
        let flat = text.trim_end().replace('\n', " ⏎ ");
        self.ws.log_human(&me, &format!("→ {name}: {flat}"));
    }

    fn spawn(&mut self, name: &str, provider: &str, cwd: Option<&str>) {
        let Some(p) = self.reg.get(provider).cloned() else {
            self.toast = Some((format!("unknown provider {provider}"), Instant::now()));
            return;
        };
        match &self.backend {
            Backend::Tmux { tmux, .. } => {
                let cmd = format!(
                    "HUBCODES_AGENT='{}' HUBCODES_SESSION='{}' {}",
                    name, tmux.session, p.command
                );
                let me = self.me.clone();
                match tmux.spawn(name, provider, &cmd, cwd) {
                    Ok(_) => {
                        self.toast = Some((format!("spawned {name} ({provider})"), Instant::now()));
                        self.ws.log_human(&me, &format!("spawned {name} ({provider})"));
                    }
                    Err(e) => {
                        self.toast = Some((format!("spawn failed: {e}"), Instant::now()));
                        self.ws.log_human("hubcodes", &format!("spawn failed: {e}"));
                    }
                }
            }
            Backend::Demo(_) => {
                let id = self.ws.add_agent(name, provider);
                self.selected = Some(id);
            }
        }
    }

    /// Open: tile the agent's real terminal next to the canvas and give it
    /// the keyboard. Already open → just focus it.
    fn open(&mut self, id: AgentId) {
        let Some(a) = self.ws.agent(id) else { return };
        let (target, already) = (a.tmux_target.clone(), a.open);
        match &self.backend {
            Backend::Tmux { tmux, .. } => {
                let Some(target) = target else { return };
                let r = if already { tmux.focus(&target) } else { tmux.open(&target) };
                if let Err(e) = r {
                    self.toast = Some((format!("open failed: {e}"), Instant::now()));
                    return;
                }
                if let Some(a) = self.ws.agent_mut(id) {
                    a.open = true;
                }
            }
            Backend::Demo(_) => {
                if let Some(a) = self.ws.agent_mut(id) {
                    a.open = true;
                }
                self.toast = Some(("demo: the agent's terminal would be tiled on the right".into(), Instant::now()));
            }
        }
    }

    /// Back: every open agent returns to its own window; canvas is full again.
    fn back(&mut self) {
        if let Backend::Tmux { tmux, .. } = &self.backend {
            if let Err(e) = tmux.back() {
                self.toast = Some((format!("back failed: {e}"), Instant::now()));
                return;
            }
        }
        for a in &mut self.ws.agents {
            a.open = false;
        }
    }

    /// Close one tiled agent (keyboard stays on the canvas).
    fn close(&mut self, id: AgentId) {
        let Some(a) = self.ws.agent(id) else { return };
        if !a.open {
            return;
        }
        let (target, name) = (a.tmux_target.clone(), a.name.clone());
        if let (Backend::Tmux { tmux, .. }, Some(t)) = (&self.backend, target) {
            if let Err(e) = tmux.close(&t, &name) {
                self.toast = Some((format!("close failed: {e}"), Instant::now()));
                return;
            }
        }
        if let Some(a) = self.ws.agent_mut(id) {
            a.open = false;
        }
    }

    /// Tile: open every agent; when all are open already, cycle tmux layouts.
    fn tile(&mut self) {
        let closed: Vec<AgentId> = self.ws.agents.iter().filter(|a| !a.open).map(|a| a.id).collect();
        if closed.is_empty() {
            if let Backend::Tmux { tmux, .. } = &self.backend {
                if let Err(e) = tmux.next_layout() {
                    self.toast = Some((format!("layout: {e}"), Instant::now()));
                }
            }
            return;
        }
        for id in closed {
            self.open(id);
        }
        if let (Backend::Tmux { tmux, .. }, Some(sel)) = (&self.backend, self.selected) {
            if let Some(t) = self.ws.agent(sel).and_then(|a| a.tmux_target.clone()) {
                let _ = tmux.focus(&t);
            }
        }
    }

    fn any_open(&self) -> bool {
        self.ws.agents.iter().any(|a| a.open)
    }

    fn kill(&mut self, id: AgentId) {
        if let Backend::Tmux { tmux, .. } = &self.backend {
            if let Some(target) = self.ws.agent(id).and_then(|a| a.tmux_target.clone()) {
                let _ = tmux.kill(&target);
            }
        }
        self.ws.remove_agent(id);
        if self.selected == Some(id) {
            self.selected = self.ws.agents.first().map(|a| a.id);
        }
    }

    fn cycle_selection(&mut self, dir: i32) {
        if self.ws.agents.is_empty() {
            self.selected = None;
            return;
        }
        let n = self.ws.agents.len() as i32;
        let cur = self
            .selected
            .and_then(|id| self.ws.agents.iter().position(|a| a.id == id))
            .map(|p| p as i32)
            .unwrap_or(-1);
        let next = (cur + dir).rem_euclid(n) as usize;
        self.selected = Some(self.ws.agents[next].id);
    }

    fn neighbour_after(&self, from: AgentId, current: AgentId) -> AgentId {
        let n = self.ws.neighbours(from);
        if n.is_empty() {
            return current;
        }
        let pos = n.iter().position(|&x| x == current).unwrap_or(0);
        n[(pos + 1) % n.len()]
    }

    // ----------------------------------------------------------------- input

    fn on_key(&mut self, k: KeyEvent) {
        if k.kind != KeyEventKind::Press {
            return;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.running = false;
            return;
        }
        let mode = self.mode.clone();
        match mode {
            Mode::Normal => self.on_key_normal(k),
            Mode::Help => {
                self.mode = Mode::Normal;
            }
            Mode::Connect { from } => match k.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('j') | KeyCode::Char('l') => self.cycle_selection(1),
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('k') | KeyCode::Char('h') => self.cycle_selection(-1),
                KeyCode::Enter => {
                    if let Some(to) = self.selected {
                        self.connect(from, to);
                    }
                    self.selected = Some(from);
                    self.mode = Mode::Normal;
                }
                _ => {}
            },
            Mode::Message { from, to, mut input } => match k.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Tab => {
                    let to = self.neighbour_after(from, to);
                    self.mode = Mode::Message { from, to, input };
                }
                KeyCode::Enter => {
                    self.send_message(from, to, &input);
                    self.mode = Mode::Normal;
                }
                KeyCode::Backspace => {
                    input.pop();
                    self.mode = Mode::Message { from, to, input };
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    self.mode = Mode::Message { from, to, input };
                }
                _ => {}
            },
            Mode::Spawn { mut name, mut provider_idx } => match k.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Tab => {
                    provider_idx = (provider_idx + 1) % self.reg.providers.len().max(1);
                    self.mode = Mode::Spawn { name, provider_idx };
                }
                KeyCode::BackTab => {
                    let n = self.reg.providers.len().max(1);
                    provider_idx = (provider_idx + n - 1) % n;
                    self.mode = Mode::Spawn { name, provider_idx };
                }
                KeyCode::Enter => {
                    let provider = self.reg.providers.get(provider_idx).map(|p| p.name.clone());
                    let name = if name.trim().is_empty() {
                        format!("agent-{}", self.ws.agents.len() + 1)
                    } else {
                        name.trim().to_string()
                    };
                    if let Some(p) = provider {
                        self.spawn(&name, &p, None);
                    }
                    self.mode = Mode::Normal;
                }
                KeyCode::Backspace => {
                    name.pop();
                    self.mode = Mode::Spawn { name, provider_idx };
                }
                KeyCode::Char(c) if c.is_ascii_alphanumeric() || c == '-' || c == '_' => {
                    name.push(c);
                    self.mode = Mode::Spawn { name, provider_idx };
                }
                _ => {}
            },
            Mode::ConfirmKill { id } => match k.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.kill(id);
                    self.mode = Mode::Normal;
                }
                _ => self.mode = Mode::Normal,
            },
        }
    }

    /// Normal mode is type-first: printable keys edit the selected node's
    /// draft, `Enter` sends it, commands live on `Ctrl` + key (see `F1`).
    fn on_key_normal(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let pan = if k.modifiers.contains(KeyModifiers::SHIFT) { 8 } else { 2 };
        if ctrl {
            match k.code {
                KeyCode::Char('q') => self.running = false,
                KeyCode::Char('n') => self.mode = Mode::Spawn { name: String::new(), provider_idx: 0 },
                KeyCode::Char('k') => {
                    if let Some(from) = self.selected {
                        self.mode = Mode::Connect { from };
                        self.cycle_selection(1);
                    }
                }
                KeyCode::Char('g') => {
                    if let Some(from) = self.selected {
                        let n = self.ws.neighbours(from);
                        if let Some(&to) = n.first() {
                            self.mode = Mode::Message { from, to, input: String::new() };
                        } else {
                            self.toast = Some(("no connections — Ctrl+K to connect first".into(), Instant::now()));
                        }
                    }
                }
                KeyCode::Char('x') => {
                    if let Some(id) = self.selected {
                        self.mode = Mode::ConfirmKill { id };
                    }
                }
                KeyCode::Char('z') => self.cam.toggle_zoom(),
                KeyCode::Char('l') => self.show_log = !self.show_log,
                KeyCode::Char('o') => self.cam = Camera { zoom: self.cam.zoom, ..Camera::default() },
                KeyCode::Char('u') => {
                    if let Some(id) = self.selected {
                        self.drafts.remove(&id);
                    }
                }
                KeyCode::Char('t') => self.tile(),
                KeyCode::Char('w') => {
                    if let Some(id) = self.selected {
                        self.close(id);
                    }
                }
                KeyCode::Char('b') => self.back(),
                KeyCode::Char('y') => self.jump_to_needs_you(),
                // Ctrl+J arrives as a raw LF: newline in the composer.
                KeyCode::Char('j') | KeyCode::Enter => self.draft_push('\n'),
                _ => {}
            }
            return;
        }
        // Alt+Enter (ESC CR), and Shift+Enter where the terminal reports it
        // (kitty keyboard protocol), add a line to the draft.
        if k.code == KeyCode::Enter && k.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) {
            self.draft_push('\n');
            return;
        }
        match k.code {
            KeyCode::F(1) => self.mode = Mode::Help,
            KeyCode::F(2) => self.mode = Mode::Spawn { name: String::new(), provider_idx: 0 },
            KeyCode::Tab => self.cycle_selection(1),
            KeyCode::BackTab => self.cycle_selection(-1),
            KeyCode::Left => self.cam.ox -= pan,
            KeyCode::Right => self.cam.ox += pan,
            KeyCode::Up => self.cam.oy -= pan / 2,
            KeyCode::Down => self.cam.oy += pan / 2,
            KeyCode::Esc => {
                if !self.draft().is_empty() {
                    if let Some(id) = self.selected {
                        self.drafts.remove(&id);
                    }
                } else if !self.marked.is_empty() {
                    self.marked.clear();
                } else if self.any_open() {
                    self.back();
                }
            }
            KeyCode::Backspace => {
                if let Some(d) = self.selected.and_then(|id| self.drafts.get_mut(&id)) {
                    d.pop();
                }
            }
            KeyCode::Enter => {
                let Some(id) = self.selected else { return };
                match self.drafts.remove(&id).filter(|d| !d.trim().is_empty()) {
                    Some(draft) if !self.marked.is_empty() => {
                        // One draft, every marked agent (the selected one included).
                        let mut targets: Vec<AgentId> = self.marked.iter().copied().filter(|m| self.ws.agent(*m).is_some()).collect();
                        if !targets.contains(&id) {
                            targets.push(id);
                        }
                        let n = targets.len();
                        for to in targets {
                            self.instruct(to, &draft);
                        }
                        self.toast = Some((format!("sent to {n} agents"), Instant::now()));
                    }
                    Some(draft) => self.instruct(id, &draft),
                    None => self.open(id),
                }
            }
            KeyCode::Char(' ') if self.draft().is_empty() => {
                if let Some(id) = self.selected {
                    if !self.marked.remove(&id) {
                        self.marked.insert(id);
                    }
                }
            }
            KeyCode::Char('?') if self.draft().is_empty() => self.mode = Mode::Help,
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::ALT) => self.draft_push(c),
            _ => {}
        }
    }

    /// Append a character (or newline) to the selected node's draft,
    /// selecting the first agent when nothing is selected yet.
    fn draft_push(&mut self, c: char) {
        let Some(id) = self.selected.or_else(|| self.ws.agents.first().map(|a| a.id)) else {
            self.toast = Some(("no agents yet — Ctrl+N spawns one".into(), Instant::now()));
            return;
        };
        self.selected = Some(id);
        self.drafts.entry(id).or_default().push(c);
    }

    /// Draft text of the selected node ("" when none).
    fn draft(&self) -> &str {
        self.selected
            .and_then(|id| self.drafts.get(&id))
            .map(String::as_str)
            .unwrap_or("")
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let area = self.canvas_area;
        let inside = m.column >= area.x
            && m.column < area.right()
            && m.row >= area.y
            && m.row < area.bottom();
        let sx = m.column as i32 - area.x as i32;
        let sy = m.row as i32 - area.y as i32;
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                let shift = m.modifiers.contains(KeyModifiers::SHIFT);
                if let Some(id) = CanvasView::hit_test(&self.ws, self.cam, area, m.column, m.row) {
                    if let Mode::Connect { from } = self.mode.clone() {
                        self.connect(from, id);
                        self.selected = Some(from);
                        self.mode = Mode::Normal;
                        return;
                    }
                    if shift {
                        if !self.marked.remove(&id) {
                            self.marked.insert(id);
                        }
                        self.selected = Some(id);
                        return;
                    }
                    self.selected = Some(id);
                    let (cx, cy) = self.cam.to_canvas(sx, sy);
                    let a = self.ws.agent(id).unwrap();
                    self.drag = Some(Drag { id, dx: cx - a.x, dy: cy - a.y });
                } else if self.ws.agents.is_empty() && matches!(self.mode, Mode::Normal) {
                    self.mode = Mode::Spawn { name: String::new(), provider_idx: 0 };
                } else if shift {
                    self.box_sel = Some(((sx, sy), (sx, sy)));
                } else {
                    self.pan_from = Some((sx, sy, self.cam.ox, self.cam.oy));
                }
            }
            MouseEventKind::Down(MouseButton::Right) if inside => {
                if let (Some(from), Some(to)) = (
                    self.selected,
                    CanvasView::hit_test(&self.ws, self.cam, area, m.column, m.row),
                ) {
                    if from != to {
                        self.connect(from, to);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(d) = &self.drag {
                    let (cx, cy) = self.cam.to_canvas(sx, sy);
                    let (id, dx, dy) = (d.id, d.dx, d.dy);
                    if let Some(a) = self.ws.agent_mut(id) {
                        a.x = (cx - dx).max(0);
                        a.y = (cy - dy).max(0);
                    }
                } else if let Some((px, py, ox, oy)) = self.pan_from {
                    self.cam.ox = ox - (sx - px);
                    self.cam.oy = oy - (sy - py);
                } else if let Some((start, _)) = self.box_sel {
                    self.box_sel = Some((start, (sx, sy)));
                }
            }
            MouseEventKind::Up(_) => {
                self.drag = None;
                self.pan_from = None;
                if let Some(((x0, y0), (x1, y1))) = self.box_sel.take() {
                    // Everything the box touches is marked (canvas coordinates).
                    let (ax, ay) = self.cam.to_canvas(x0.min(x1), y0.min(y1));
                    let (bx, by) = self.cam.to_canvas(x0.max(x1), y0.max(y1));
                    for a in &self.ws.agents {
                        let r = self.cam.node_rect(a);
                        let (rx0, ry0, rx1, ry1) = (r.x, r.y, r.x + r.w, r.y + r.h);
                        if rx0 <= bx && rx1 >= ax && ry0 <= by && ry1 >= ay {
                            self.marked.insert(a.id);
                        }
                    }
                    if let Some(first) = self.marked.iter().next() {
                        self.selected = Some(*first);
                    }
                }
            }
            MouseEventKind::ScrollDown => self.cam.oy += 2,
            MouseEventKind::ScrollUp => self.cam.oy -= 2,
            MouseEventKind::ScrollLeft => self.cam.ox -= 4,
            MouseEventKind::ScrollRight => self.cam.ox += 4,
            _ => {}
        }
    }

    // ------------------------------------------------------------------ draw

    fn draw(&mut self, f: &mut Frame) {
        if self.any_open() {
            self.draw_sidebar(f);
            return;
        }
        let [top, body, bottom] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)])
                .areas(f.area());
        let (canvas, log) = if self.show_log && body.width > 70 {
            let [c, l] = Layout::horizontal([Constraint::Min(30), Constraint::Length(34)]).areas(body);
            (c, Some(l))
        } else {
            (body, None)
        };
        self.canvas_area = canvas;

        self.draw_top(f, top);
        let connect_from = match &self.mode {
            Mode::Connect { from } => Some(*from),
            _ => None,
        };
        f.render_widget(
            CanvasView {
                ws: &self.ws,
                cam: self.cam,
                reg: &self.reg,
                selected: self.selected,
                marked: &self.marked,
                box_sel: self.box_sel,
                connect_from,
                draft: self.draft(),
                now: Instant::now(),
                frame: self.frame,
            },
            canvas,
        );
        if let Some(l) = log {
            self.draw_log(f, l);
        }
        self.draw_bottom(f, bottom);
        self.draw_overlay(f, canvas);
    }

    /// Narrow view used while agents are tiled next to the canvas: agent
    /// list with status, the selected agent's composer, and the activity log.
    fn draw_sidebar(&mut self, f: &mut Frame) {
        let area = f.area();
        self.canvas_area = Rect::default();
        let n = self.ws.agents.len() as u16;
        let draft_rows = crate::canvas::wrap_draft(self.draft(), (area.width as usize).saturating_sub(5).max(1))
            .len()
            .clamp(1, crate::canvas::COMPOSER_MAX_LINES) as u16;
        let [top, list, composer, log, bottom] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(n.max(1) + 2),
            Constraint::Length(draft_rows + 3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .areas(area);

        let open_n = self.ws.agents.iter().filter(|a| a.open).count();
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" hubcodes ", theme::accent()),
                Span::styled(format!("· {open_n} open "), theme::dim()),
                Span::styled("· Esc = back", theme::dim()),
            ]))
            .style(theme::bar()),
            top,
        );

        let mut rows: Vec<Line> = Vec::new();
        for a in &self.ws.agents {
            let sel = self.selected == Some(a.id);
            let color = self.reg.get(&a.provider).and_then(|p| p.color_index()).map(ratatui::style::Color::Indexed).unwrap_or(ratatui::style::Color::Gray);
            let marker = if sel { "▶ " } else { "  " };
            let tile = if a.open { "▣ " } else { "□ " };
            let name_style = if sel { theme::node_title(color).add_modifier(Modifier::BOLD) } else { theme::node_title(color) };
            rows.push(Line::from(vec![
                Span::styled(marker, theme::accent()),
                Span::styled(tile, if a.open { theme::accent() } else { theme::dim() }),
                Span::styled(format!("{} ", a.status.glyph()), theme::status(a.status)),
                Span::styled(a.name.clone(), name_style),
                Span::styled(format!("  {}", a.status.label()), theme::dim()),
            ]));
        }
        if rows.is_empty() {
            rows.push(Line::from(Span::styled("no agents", theme::dim())));
        }
        f.render_widget(
            Paragraph::new(rows).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(theme::dim())
                    .title(Line::from(Span::styled(" agents ", theme::dim()))),
            ),
            list,
        );

        let to = self.selected.and_then(|id| self.ws.agent(id)).map(|a| a.name.clone()).unwrap_or_default();
        let draft = self.draft();
        let mut body: Vec<Line> = if draft.is_empty() {
            vec![Line::from(vec![Span::styled("› ", theme::accent()), Span::styled("type here · Enter = jump in", theme::dim())])]
        } else {
            let room = (composer.width as usize).saturating_sub(5).max(1);
            let wrapped = crate::canvas::wrap_draft(draft, room);
            crate::canvas::composer_lines(&wrapped, draft_rows as usize)
        };
        body.push(Line::from(Span::styled("Enter = send · Alt+Enter = newline · Tab = other agent", theme::dim())));
        f.render_widget(
            Paragraph::new(body).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(theme::accent())
                    .title(Line::from(Span::styled(format!(" → {to} "), theme::accent()))),
            ),
            composer,
        );

        self.draw_log(f, log);
        self.draw_bottom(f, bottom);
        self.draw_overlay(f, area);
    }

    fn draw_top(&self, f: &mut Frame, area: Rect) {
        let mut left = vec![
            Span::styled(" hubcodes ", theme::accent()),
            Span::styled(format!("· {} ", self.ws.name), theme::text()),
            Span::styled(format!("· {} ", self.backend.label()), theme::dim()),
            Span::styled(
                format!("· {} agents · {} links ", self.ws.agents.len(), self.ws.edges.len()),
                theme::dim(),
            ),
        ];
        if self.cam.zoom == Zoom::Compact {
            left.push(Span::styled("· zoomed out ", theme::dim()));
        }
        let mut right: Vec<Span> = Vec::new();
        for p in &self.ws.participants {
            let glyph = if p.is_me { "●" } else { "◆" };
            let mut label = format!(" {glyph} {}", p.name);
            if p.read_only {
                label.push_str(" (ro)");
            }
            label.push(' ');
            right.push(Span::styled(label, Style::default().fg(theme::guest(p.color_idx))));
        }
        let right_w: usize = right.iter().map(|s| s.content.chars().count()).sum();
        f.render_widget(Paragraph::new(Line::from(left)).style(theme::bar()), area);
        let rx = area.right().saturating_sub(right_w as u16);
        let ra = Rect { x: rx, y: area.y, width: right_w as u16, height: 1 };
        f.render_widget(Paragraph::new(Line::from(right)).style(theme::bar()), ra);
    }

    fn draw_bottom(&self, f: &mut Frame, area: Rect) {
        let keys: Vec<(&str, &str)> = match &self.mode {
            Mode::Normal if !self.draft().is_empty() && !self.marked.is_empty() => vec![
                ("Enter", "send to every marked agent"),
                ("Alt+Enter", "newline"),
                ("Esc", "clear"),
            ],
            Mode::Normal if !self.draft().is_empty() => vec![
                ("Enter", "send to agent"),
                ("Alt+Enter", "newline"),
                ("Esc", "clear"),
                ("Tab", "other agent"),
            ],
            Mode::Normal if self.any_open() => vec![
                ("Esc", "back to canvas"),
                ("Enter", "open / jump in"),
                ("Tab", "select"),
                ("^T", "tile all / layout"),
                ("^W", "close one"),
                ("F1", "help"),
            ],
            Mode::Normal if !self.marked.is_empty() => vec![
                ("type", "instruct all marked"),
                ("Space", "unmark"),
                ("Shift+click / Shift+drag", "mark more"),
                ("Esc", "clear marks"),
            ],
            Mode::Normal => vec![
                ("type", "instruct"),
                ("Space", "mark for a group message"),
                ("Tab", "select"),
                ("Enter", "open"),
                ("^T", "tile all"),
                ("^N/F2", "new"),
                ("^K", "connect"),
                ("^G", "message"),
                ("^Y", "needs you"),
                ("^X", "kill"),
                ("^Z", "zoom"),
                ("^L", "log"),
                ("F1", "help"),
                ("^Q", "quit"),
            ],
            Mode::Connect { .. } => vec![("Tab/←→", "pick target"), ("Enter", "connect / toggle"), ("Esc", "cancel")],
            Mode::Message { .. } => vec![("Tab", "next recipient"), ("Enter", "send"), ("Esc", "cancel")],
            Mode::Spawn { .. } => vec![("Tab", "provider"), ("Enter", "spawn"), ("Esc", "cancel")],
            Mode::ConfirmKill { .. } => vec![("y", "kill window"), ("any", "cancel")],
            Mode::Help => vec![("any key", "close")],
        };
        let mut spans: Vec<Span> = Vec::new();
        for (k, v) in keys {
            spans.push(Span::styled(format!(" {k} "), theme::key()));
            spans.push(Span::styled(format!(" {v}  "), theme::dim()));
        }
        if let Some((msg, _)) = &self.toast {
            spans.push(Span::styled(format!("  {msg}"), theme::accent()));
        }
        f.render_widget(Paragraph::new(Line::from(spans)).style(theme::bar()), area);
    }

    fn draw_log(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(theme::dim())
            .title(Line::from(Span::styled(" activity ", theme::dim())));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let now = Instant::now();
        let mut lines: Vec<Line> = Vec::new();
        for e in self.ws.log.iter().rev().take(inner.height as usize * 2) {
            let age = now.duration_since(e.at).as_secs();
            let age_s = if age < 60 { format!("{age:>2}s") } else { format!("{:>2}m", age / 60) };
            let head = match &e.to {
                Some(to) => format!("{} → {}", e.from, to),
                None => e.from.clone(),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{age_s} "), theme::dim()),
                Span::styled(head, theme::accent().remove_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(Span::styled(format!("    {}", hubcodes_core::model::short(&e.text, 60)), theme::text())));
        }
        if lines.is_empty() {
            lines.push(Line::from(Span::styled("no activity yet", theme::dim())));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    fn draw_overlay(&self, f: &mut Frame, canvas: Rect) {
        let popup = |title: &str, body: Vec<Line<'static>>, w: u16, h: u16| -> (Block<'static>, Rect, Paragraph<'static>) {
            let w = w.min(canvas.width.saturating_sub(2)).max(10);
            let h = h.min(canvas.height.saturating_sub(2)).max(3);
            let x = canvas.x + (canvas.width - w) / 2;
            let y = canvas.y + (canvas.height - h) / 2;
            let r = Rect { x, y, width: w, height: h };
            let b = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(theme::accent())
                .title(Line::from(Span::styled(format!(" {title} "), theme::accent())))
                .style(theme::popup());
            (b, r, Paragraph::new(body).wrap(Wrap { trim: false }))
        };
        let name = |id: AgentId| self.ws.agent(id).map(|a| a.name.clone()).unwrap_or_default();
        let empty_state = |session: String| {
            popup(
                "no agents yet",
                vec![
                    Line::from(vec![Span::styled("Ctrl+N", theme::key()), Span::raw("  or  "), Span::styled("F2", theme::key()), Span::raw("  or click here to spawn an agent")]),
                    Line::from(""),
                    Line::from(Span::styled("from any shell:", theme::dim())),
                    Line::from(format!("  hubcodes --session {session} spawn builder -p claude --cwd ~/code/app")),
                    Line::from(""),
                    Line::from(Span::styled("F1 lists every key", theme::dim())),
                ],
                70,
                8,
            )
        };
        let rendered = match &self.mode {
            Mode::Normal if self.ws.agents.is_empty() => Some(empty_state(self.ws.name.clone())),
            Mode::Help => Some(popup(
                "hubcodes · keys",
                vec![
                    Line::from("type              instruct: text goes into the selected agent's input"),
                    Line::from("Enter             send the draft; empty draft: open the agent next to the canvas"),
                    Line::from("Alt+Enter, Ctrl+J newline in the draft (the composer grows up to 6 rows)"),
                    Line::from("Ctrl+Y            jump to the next agent that needs you (off-screen: edge marker + bell)"),
                    Line::from("Esc               clear the draft; empty draft: back (close all open agents)"),
                    Line::from("Ctrl+T            tile every agent next to the canvas; again: cycle layouts"),
                    Line::from("Ctrl+W / Ctrl+B   close the selected open agent / back"),
                    Line::from("Alt+Esc, prefix g back to the canvas from inside any agent pane"),
                    Line::from("Tab / Shift-Tab   select next / previous agent (drafts are kept per node)"),
                    Line::from("click / drag      select / move a node; drag empty space to pan"),
                    Line::from("Ctrl+N / F2       spawn a new agent (Tab cycles provider)"),
                    Line::from("Ctrl+K            connect selected → pick target (Enter toggles)"),
                    Line::from("right-click       connect selected → clicked node"),
                    Line::from("Ctrl+G            agent → agent message along a link (Tab = recipient)"),
                    Line::from("Ctrl+X            kill the selected agent's window"),
                    Line::from("Ctrl+Z            toggle zoom (full ↔ compact)"),
                    Line::from("arrows            pan (Shift = faster); Ctrl+O resets the camera"),
                    Line::from("Ctrl+L            toggle activity panel"),
                    Line::from("F1 / ?            this help;  Ctrl+Q / Ctrl+C quit (agents keep running)"),
                    Line::from(""),
                    Line::from(Span::styled("CLI from inside an agent:  hubcodes send <agent> \"text\"", theme::dim())),
                ],
                84,
                22,
            )),
            Mode::Message { from, to, input } => Some(popup(
                &format!("message  {} → {}", name(*from), name(*to)),
                vec![
                    Line::from(vec![Span::styled("> ", theme::accent()), Span::raw(input.clone()), Span::styled("▏", theme::accent())]),
                    Line::from(Span::styled("pasted into the recipient's terminal as “[from …] text”", theme::dim())),
                ],
                64,
                5,
            )),
            Mode::Spawn { name: n, provider_idx } => {
                let provs: Vec<Span> = self
                    .reg
                    .providers
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let s = if i == *provider_idx { theme::key() } else { theme::dim() };
                        Span::styled(format!(" {}{} ", p.glyph, p.name), s)
                    })
                    .collect();
                Some(popup(
                    "new agent",
                    vec![
                        Line::from(vec![Span::styled("name     ", theme::dim()), Span::raw(n.clone()), Span::styled("▏", theme::accent())]),
                        Line::from(vec![Span::styled("provider ", theme::dim())].into_iter().chain(provs).collect::<Vec<_>>()),
                        Line::from(Span::styled("Tab cycles provider · Enter spawns a tmux window running it", theme::dim())),
                    ],
                    70,
                    6,
                ))
            }
            Mode::ConfirmKill { id } => Some(popup(
                "kill agent?",
                vec![Line::from(format!("Kill tmux window of '{}' and remove it from the canvas? [y/N]", name(*id)))],
                60,
                4,
            )),
            _ => None,
        };
        if let Some((block, rect, para)) = rendered {
            f.render_widget(Clear, rect);
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            f.render_widget(para, inner);
        }
    }
}

/// Collapse runs of blank lines so a mostly-empty TUI (Claude Code's prompt
/// box, for example) does not push the interesting lines out of the preview.
fn squeeze_blank_lines(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for l in lines {
        let blank = crate::canvas::strip_ansi(&l).trim().is_empty();
        if blank && out.last().map(|p: &String| crate::canvas::strip_ansi(p).trim().is_empty()).unwrap_or(true) {
            continue;
        }
        out.push(l);
    }
    out
}
