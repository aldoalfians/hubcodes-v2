//! Persisted canvas layout: node positions and camera, one file per tmux
//! session at `~/.local/state/hubcodes/<session>.json`.
//!
//! Positions are keyed by agent *name* (the tmux window name), not by window
//! id, so they survive a tmux server restart. The demo backend never saves.

use crate::canvas::{Camera, Zoom};
use hubcodes_core::model::Workspace;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, Eq)]
pub struct SavedLayout {
    pub camera: CameraState,
    /// name → (x, y) in full-zoom canvas cells.
    pub nodes: BTreeMap<String, (i32, i32)>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, Eq)]
pub struct CameraState {
    pub ox: i32,
    pub oy: i32,
    pub compact: bool,
}

impl From<Camera> for CameraState {
    fn from(c: Camera) -> Self {
        CameraState {
            ox: c.ox,
            oy: c.oy,
            compact: c.zoom == Zoom::Compact,
        }
    }
}

impl From<&CameraState> for Camera {
    fn from(s: &CameraState) -> Self {
        Camera {
            ox: s.ox,
            oy: s.oy,
            zoom: if s.compact { Zoom::Compact } else { Zoom::Full },
        }
    }
}

/// `$XDG_STATE_HOME/hubcodes` or `~/.local/state/hubcodes`. Same rule on
/// macOS and Linux so the docs stay true on both.
pub fn state_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("XDG_STATE_HOME").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(p).join("hubcodes"));
    }
    dirs::home_dir().map(|h| h.join(".local").join("state").join("hubcodes"))
}

pub fn path_for(session: &str) -> Option<PathBuf> {
    // Session names come from tmux and may contain `/`; keep the file flat.
    let safe: String = session
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    state_dir().map(|d| d.join(format!("{safe}.json")))
}

impl SavedLayout {
    pub fn load(session: &str) -> SavedLayout {
        let Some(p) = path_for(session) else { return SavedLayout::default() };
        match std::fs::read_to_string(&p) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => SavedLayout::default(),
        }
    }

    pub fn save(&self, session: &str) -> Result<()> {
        let p = path_for(session).context("no home directory for state file")?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &p).with_context(|| format!("rename to {}", p.display()))?;
        Ok(())
    }

    /// Snapshot the live workspace and camera.
    pub fn capture(ws: &Workspace, cam: Camera) -> SavedLayout {
        SavedLayout {
            camera: cam.into(),
            nodes: ws.agents.iter().map(|a| (a.name.clone(), (a.x, a.y))).collect(),
        }
    }

    /// Saved position for an agent name, clamped to the canvas origin.
    pub fn position_of(&self, name: &str) -> Option<(i32, i32)> {
        self.nodes.get(name).map(|&(x, y)| (x.max(0), y.max(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json() {
        let mut ws = Workspace::new("t");
        let a = ws.add_agent("builder", "claude");
        let b = ws.add_agent("reviewer", "codex");
        ws.agent_mut(a).unwrap().x = 10;
        ws.agent_mut(a).unwrap().y = 20;
        ws.agent_mut(b).unwrap().x = 99;
        let cam = Camera { ox: 3, oy: -4, zoom: Zoom::Compact };
        let l = SavedLayout::capture(&ws, cam);
        let s = serde_json::to_string(&l).unwrap();
        let back: SavedLayout = serde_json::from_str(&s).unwrap();
        assert_eq!(back, l);
        let cam2: Camera = (&back.camera).into();
        assert_eq!((cam2.ox, cam2.oy, cam2.zoom), (3, -4, Zoom::Compact));
    }

    #[test]
    fn position_by_name_and_clamp() {
        let mut l = SavedLayout::default();
        l.nodes.insert("builder".into(), (40, -7));
        assert_eq!(l.position_of("builder"), Some((40, 0)));
        assert_eq!(l.position_of("other"), None);
    }

    #[test]
    fn path_is_flat_and_scoped() {
        std::env::set_var("XDG_STATE_HOME", "/tmp/xdg-test");
        let p = path_for("a/b c").unwrap();
        assert_eq!(p, PathBuf::from("/tmp/xdg-test/hubcodes/a_b_c.json"));
        std::env::remove_var("XDG_STATE_HOME");
    }
}
