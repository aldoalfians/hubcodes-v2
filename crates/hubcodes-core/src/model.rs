//! Domain model: agents (nodes), edges (connections), pulses (messages in
//! flight), participants (humans on the canvas) and the activity log.
//!
//! Everything here is backend-agnostic. The tmux backend and the demo backend
//! both mutate a [`Workspace`]; the canvas renderer only reads it.

use std::time::Instant;

pub type AgentId = usize;

/// Coarse agent state. The tmux backend infers it from pane activity; the
/// demo backend scripts it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Idle,
    Working,
    /// Blocked on a human: permission prompt, question, y/n.
    Waiting,
    Error,
    /// Window gone / process exited.
    Offline,
}

impl Status {
    pub fn glyph(self) -> &'static str {
        match self {
            Status::Idle => "○",
            Status::Working => "●",
            Status::Waiting => "◐",
            Status::Error => "✖",
            Status::Offline => "·",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Working => "working",
            Status::Waiting => "needs you",
            Status::Error => "error",
            Status::Offline => "offline",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    pub provider: String,
    pub role: Option<String>,
    pub status: Status,
    /// Canvas position in cells (top-left corner), independent of the camera.
    pub x: i32,
    pub y: i32,
    /// Last N lines of the agent's terminal, raw (may contain ANSI escapes).
    pub preview: Vec<String>,
    /// tmux pane id such as `%3`. `None` for demo agents.
    pub tmux_target: Option<String>,
    /// Shown next to the canvas (tiled into the canvas window).
    pub open: bool,
    pub last_change: Instant,
    pub last_hash: u64,
}

impl Agent {
    pub fn new(id: AgentId, name: impl Into<String>, provider: impl Into<String>) -> Self {
        Agent {
            id,
            name: name.into(),
            provider: provider.into(),
            role: None,
            status: Status::Idle,
            x: 0,
            y: 0,
            preview: Vec::new(),
            tmux_target: None,
            open: false,
            last_change: Instant::now(),
            last_hash: 0,
        }
    }
}

/// A message travelling along an edge. `progress` goes 0.0 → 1.0.
#[derive(Clone, Debug)]
pub struct Pulse {
    pub progress: f32,
    /// `true` when the message flows against the edge direction (to → from).
    pub reverse: bool,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: AgentId,
    pub to: AgentId,
    /// Short relationship label shown at the midpoint (e.g. "task", "diff").
    pub label: String,
    pub pulses: Vec<Pulse>,
    /// Number of messages that crossed this edge; drives the heavy-line style.
    pub traffic: u32,
    pub last_traffic: Option<Instant>,
}

/// A human looking at the canvas. The owner is `is_me`; guests come from the
/// sharing layer (phase 3) or from the demo backend.
#[derive(Clone, Debug)]
pub struct Participant {
    pub name: String,
    pub color_idx: usize,
    pub selected: Option<AgentId>,
    pub is_me: bool,
    pub read_only: bool,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub at: Instant,
    pub from: String,
    pub to: Option<String>,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Workspace {
    pub name: String,
    pub agents: Vec<Agent>,
    pub edges: Vec<Edge>,
    pub participants: Vec<Participant>,
    pub log: Vec<LogEntry>,
    next_id: AgentId,
}

/// Node footprint on the canvas at the "full" zoom level.
pub const NODE_W: i32 = 32;
pub const NODE_H: i32 = 9;
/// Node footprint at the "compact" zoom level.
pub const NODE_W_COMPACT: i32 = 22;
pub const NODE_H_COMPACT: i32 = 3;

impl Workspace {
    pub fn new(name: impl Into<String>) -> Self {
        Workspace {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn agent(&self, id: AgentId) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == id)
    }

    pub fn agent_mut(&mut self, id: AgentId) -> Option<&mut Agent> {
        self.agents.iter_mut().find(|a| a.id == id)
    }

    pub fn agent_by_name(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.name == name)
    }

    pub fn add_agent(&mut self, name: impl Into<String>, provider: impl Into<String>) -> AgentId {
        let id = self.next_id;
        self.next_id += 1;
        let mut a = Agent::new(id, name, provider);
        let (x, y) = self.free_slot();
        a.x = x;
        a.y = y;
        self.agents.push(a);
        id
    }

    pub fn remove_agent(&mut self, id: AgentId) {
        self.agents.retain(|a| a.id != id);
        self.edges.retain(|e| e.from != id && e.to != id);
        for p in &mut self.participants {
            if p.selected == Some(id) {
                p.selected = None;
            }
        }
    }

    /// First grid slot (4 columns wide) not occupied by an existing node.
    fn free_slot(&self) -> (i32, i32) {
        let gap_x = NODE_W + 8;
        let gap_y = NODE_H + 3;
        for row in 0..16 {
            for col in 0..4 {
                let x = 2 + col * gap_x;
                let y = 1 + row * gap_y;
                let taken = self.agents.iter().any(|a| {
                    (a.x - x).abs() < NODE_W / 2 && (a.y - y).abs() < NODE_H / 2
                });
                if !taken {
                    return (x, y);
                }
            }
        }
        (2, 1)
    }

    pub fn connect(&mut self, from: AgentId, to: AgentId, label: impl Into<String>) -> bool {
        if from == to || self.agent(from).is_none() || self.agent(to).is_none() {
            return false;
        }
        if self.edge_between(from, to).is_some() {
            return false;
        }
        self.edges.push(Edge {
            from,
            to,
            label: label.into(),
            pulses: Vec::new(),
            traffic: 0,
            last_traffic: None,
        });
        true
    }

    pub fn disconnect(&mut self, a: AgentId, b: AgentId) {
        self.edges
            .retain(|e| !((e.from == a && e.to == b) || (e.from == b && e.to == a)));
    }

    pub fn edge_between(&self, a: AgentId, b: AgentId) -> Option<usize> {
        self.edges
            .iter()
            .position(|e| (e.from == a && e.to == b) || (e.from == b && e.to == a))
    }

    /// Agents directly connected to `id`, in edge order.
    pub fn neighbours(&self, id: AgentId) -> Vec<AgentId> {
        self.edges
            .iter()
            .filter_map(|e| {
                if e.from == id {
                    Some(e.to)
                } else if e.to == id {
                    Some(e.from)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Record a message from one agent/human to another. Creates the edge on
    /// demand so a `hubcodes send` from the CLI always shows up on the canvas.
    pub fn send(&mut self, from: AgentId, to: AgentId, text: &str) {
        if self.edge_between(from, to).is_none() {
            self.connect(from, to, "msg");
        }
        if let Some(i) = self.edge_between(from, to) {
            let e = &mut self.edges[i];
            e.pulses.push(Pulse {
                progress: 0.0,
                reverse: e.from != from,
            });
            e.traffic += 1;
            e.last_traffic = Some(Instant::now());
        }
        let from_name = self.agent(from).map(|a| a.name.clone()).unwrap_or_default();
        let to_name = self.agent(to).map(|a| a.name.clone());
        self.log.push(LogEntry {
            at: Instant::now(),
            from: from_name,
            to: to_name,
            text: text.to_string(),
        });
    }

    pub fn log_human(&mut self, who: &str, text: &str) {
        self.log.push(LogEntry {
            at: Instant::now(),
            from: who.to_string(),
            to: None,
            text: text.to_string(),
        });
    }

    /// Advance pulses; drop the ones that arrived.
    pub fn tick_pulses(&mut self, dt: f32) {
        for e in &mut self.edges {
            for p in &mut e.pulses {
                p.progress += dt;
            }
            e.pulses.retain(|p| p.progress < 1.0);
        }
    }
}

pub fn short(s: &str, max: usize) -> String {
    let s = s.trim().replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
