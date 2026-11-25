//! Scripted demo backend. No tmux, no real agents: four fake agents emit
//! plausible output, change status, and message each other so the canvas UX
//! can be judged on any machine.

use hubcodes_core::model::{AgentId, Status, Workspace};

pub struct Demo {
    step: u64,
    planner: AgentId,
    builder: AgentId,
    reviewer: AgentId,
    tester: AgentId,
    /// Far off the initial viewport; goes `Waiting` each loop so the
    /// edge marker, the bell and `Ctrl+Y` can be exercised.
    ops: AgentId,
}

const PLANNER: &[&str] = &[
    "\x1b[36m>\x1b[0m breaking down: auth refresh flow",
    "  1. token store (builder)",
    "  2. refresh endpoint (builder)",
    "  3. e2e login/refresh (tester)",
    "\x1b[2mwaiting for builder…\x1b[0m",
    "\x1b[32m✓\x1b[0m builder reported: token store done",
    "\x1b[36m>\x1b[0m asking reviewer for a pass",
];
const BUILDER: &[&str] = &[
    "\x1b[33m●\x1b[0m editing src/auth/store.rs",
    "  + fn refresh(&mut self) -> Result<Token>",
    "  + tests::refresh_rotates_token",
    "\x1b[2m$ cargo test auth::\x1b[0m",
    "   Compiling hermina-auth v0.4.1",
    "test auth::store::refresh_rotates_token ... \x1b[32mok\x1b[0m",
    "\x1b[32m✓\x1b[0m 12 passed, 0 failed",
    "sending diff to reviewer",
];
const REVIEWER: &[&str] = &[
    "\x1b[2midle · waiting for diff\x1b[0m",
    "\x1b[35m◆\x1b[0m reviewing src/auth/store.rs (+41 −7)",
    "  nit: rename `tok` → `token`",
    "  \x1b[31m!\x1b[0m refresh() ignores clock skew",
    "\x1b[33m?\x1b[0m Approve changes with 1 blocking note? (y/n)",
];
const TESTER: &[&str] = &[
    "\x1b[2midle\x1b[0m",
    "\x1b[2m$ pnpm playwright test login\x1b[0m",
    "Running 3 tests using 1 worker",
    "  ✓ login → refresh → logout (2.1s)",
    "  ✓ refresh after expiry (1.4s)",
    "  \x1b[31m✘\x1b[0m refresh with skewed clock (timeout)",
    "reporting 1 failure to planner",
];

impl Demo {
    pub fn new(ws: &mut Workspace) -> Self {
        ws.name = "demo · auth-refresh".into();
        let planner = ws.add_agent("planner", "claude");
        let builder = ws.add_agent("builder", "codex");
        let reviewer = ws.add_agent("reviewer", "opencode");
        let tester = ws.add_agent("tester", "pi");
        ws.agent_mut(planner).unwrap().role = Some("lead".into());
        ws.agent_mut(reviewer).unwrap().role = Some("review".into());
        // Spread them out so the edges have room to bend.
        let pos = [(2, 1), (48, 1), (48, 15), (2, 15)];
        for (id, (x, y)) in [planner, builder, reviewer, tester].iter().zip(pos) {
            let a = ws.agent_mut(*id).unwrap();
            a.x = x;
            a.y = y;
        }
        ws.connect(planner, builder, "task");
        ws.connect(builder, reviewer, "diff");
        ws.connect(planner, tester, "e2e");
        ws.connect(reviewer, planner, "verdict");
        let ops = ws.add_agent("ops", "claude");
        {
            let a = ws.agent_mut(ops).unwrap();
            a.x = 150;
            a.y = 30;
            a.preview.push("\x1b[2mwatching prod alerts\x1b[0m".into());
        }
        ws.connect(tester, ops, "deploy");
        ws.participants.push(hubcodes_core::model::Participant {
            name: "budi".into(),
            color_idx: 0,
            selected: None,
            is_me: false,
            read_only: true,
        });
        ws.log_human("budi", "joined via tailscale (read-only)");
        Demo {
            step: 0,
            planner,
            builder,
            reviewer,
            tester,
            ops,
        }
    }

    fn push(ws: &mut Workspace, id: AgentId, script: &[&str], idx: usize) {
        if let Some(a) = ws.agent_mut(id) {
            if let Some(line) = script.get(idx) {
                a.preview.push(line.to_string());
                if a.preview.len() > 40 {
                    a.preview.remove(0);
                }
                a.last_change = std::time::Instant::now();
            }
        }
    }

    fn set(ws: &mut Workspace, id: AgentId, s: Status) {
        if let Some(a) = ws.agent_mut(id) {
            a.status = s;
        }
    }

    /// Called every ~600 ms. One scripted beat per call, looping.
    pub fn tick(&mut self, ws: &mut Workspace) {
        let s = self.step % 40;
        match s {
            0 => {
                for id in [self.planner, self.builder, self.reviewer, self.tester] {
                    if let Some(a) = ws.agent_mut(id) {
                        a.preview.clear();
                        a.status = Status::Idle;
                    }
                }
                Self::set(ws, self.planner, Status::Working);
            }
            1..=4 => Self::push(ws, self.planner, PLANNER, (s - 1) as usize),
            5 => {
                Self::push(ws, self.planner, PLANNER, 4);
                ws.send(self.planner, self.builder, "task: token store + refresh endpoint");
                Self::set(ws, self.builder, Status::Working);
                Self::push(ws, self.ops, &["\x1b[33m?\x1b[0m prod alert: p95 latency up 40%. Roll back? (y/n)"], 0);
                Self::set(ws, self.ops, Status::Waiting);
                Self::set(ws, self.planner, Status::Idle);
            }
            6..=12 => Self::push(ws, self.builder, BUILDER, (s - 6) as usize),
            13 => {
                Self::push(ws, self.builder, BUILDER, 7);
                ws.send(self.builder, self.reviewer, "diff: src/auth/store.rs +41 −7");
                ws.send(self.builder, self.planner, "token store done");
                Self::set(ws, self.builder, Status::Idle);
                Self::set(ws, self.reviewer, Status::Working);
                Self::push(ws, self.planner, PLANNER, 5);
            }
            14..=17 => Self::push(ws, self.reviewer, REVIEWER, (s - 13) as usize),
            18 => {
                Self::set(ws, self.reviewer, Status::Waiting);
                if let Some(p) = ws.participants.iter_mut().find(|p| !p.is_me) {
                    p.selected = Some(self.reviewer);
                }
                ws.log_human("budi", "looking at reviewer");
            }
            22 => {
                Self::set(ws, self.reviewer, Status::Idle);
                ws.send(self.reviewer, self.planner, "verdict: 1 blocking note");
                Self::push(ws, self.planner, PLANNER, 6);
                ws.send(self.planner, self.tester, "run e2e login/refresh");
                Self::set(ws, self.tester, Status::Working);
            }
            23..=28 => Self::push(ws, self.tester, TESTER, (s - 22) as usize),
            29 => {
                Self::push(ws, self.tester, TESTER, 6);
                Self::set(ws, self.tester, Status::Error);
                ws.send(self.tester, self.planner, "1 failure: skewed clock");
                if let Some(p) = ws.participants.iter_mut().find(|p| !p.is_me) {
                    p.selected = Some(self.tester);
                }
            }
            33 => {
                Self::set(ws, self.tester, Status::Idle);
                Self::set(ws, self.ops, Status::Idle);
                if let Some(p) = ws.participants.iter_mut().find(|p| !p.is_me) {
                    p.selected = None;
                }
            }
            _ => {}
        }
        self.step += 1;
    }
}
