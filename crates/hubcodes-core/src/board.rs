//! The board's columns. Four are built in and carry the engine's meaning:
//! `todo` (unowned tasks go to idle agents from here), `doing` (an agent
//! holds it), `review` (waits for an approval), `done` (closed, hidden from
//! the open list). They can be renamed and reordered but not removed. The
//! user adds columns of their own (`backlog`, `blocked`, `qa`…): a task in
//! one of those waits there, counted as open, until someone moves it.
//!
//! ```toml
//! [board]
//! columns = [
//!   { id = "backlog", label = "Backlog" },
//!   { id = "todo", label = "To do" },
//!   { id = "doing", label = "Doing" },
//!   { id = "review", label = "Review" },
//!   { id = "done", label = "Done" },
//! ]
//! ```

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoardColumn {
    pub id: String,
    pub label: String,
    /// Most tasks the column should hold (0 = no limit). Dispatch stops
    /// handing out tasks while Doing is full; a person can still drop more.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wip: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoardConfig {
    #[serde(default = "default_columns")]
    pub columns: Vec<BoardColumn>,
}

impl Default for BoardConfig {
    fn default() -> Self {
        BoardConfig { columns: default_columns() }
    }
}

/// The built-in ids, in their default order, with their labels.
pub const BUILTIN: [(&str, &str); 4] = [("todo", "To do"), ("doing", "Doing"), ("review", "Review"), ("done", "Done")];

pub fn default_columns() -> Vec<BoardColumn> {
    BUILTIN.iter().map(|(id, label)| BoardColumn { id: (*id).into(), label: (*label).into(), wip: 0 }).collect()
}

pub fn is_builtin(id: &str) -> bool {
    BUILTIN.iter().any(|(b, _)| *b == id)
}

/// A column id from a label: lowercase, `-` between words, ascii only.
pub fn slug(label: &str) -> String {
    let mut out = String::new();
    for ch in label.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if (ch == ' ' || ch == '-' || ch == '_' || ch == '/') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

impl BoardConfig {
    /// `columns` cleaned: labels trimmed, ids slugs, no duplicates, every
    /// built-in present (a missing one is put back at its default place).
    pub fn normalized(&self) -> Result<BoardConfig> {
        let mut columns: Vec<BoardColumn> = Vec::new();
        for c in &self.columns {
            let label = c.label.trim().to_string();
            let id = if c.id.trim().is_empty() { slug(&label) } else { slug(&c.id) };
            if id.is_empty() {
                return Err(anyhow!("a column needs a name (got “{label}”)"));
            }
            let label = if label.is_empty() { id.clone() } else { label };
            if columns.iter().any(|x| x.id == id) {
                return Err(anyhow!("two columns would both be called “{id}”"));
            }
            columns.push(BoardColumn { id, label, wip: c.wip });
        }
        // A missing built-in goes right before the next built-in that is
        // there, else after the previous one, else at the end: the user's
        // own columns keep their places around them.
        for (n, (id, label)) in BUILTIN.iter().enumerate() {
            if columns.iter().any(|c| c.id == *id) {
                continue;
            }
            let next = BUILTIN[n + 1..].iter().find_map(|(b, _)| columns.iter().position(|c| c.id == *b));
            let prev = BUILTIN[..n].iter().rev().find_map(|(b, _)| columns.iter().position(|c| c.id == *b)).map(|i| i + 1);
            let at = next.or(prev).unwrap_or(columns.len());
            columns.insert(at, BoardColumn { id: (*id).into(), label: (*label).into(), wip: 0 });
        }
        Ok(BoardConfig { columns })
    }

    pub fn allows(&self, status: &str) -> bool {
        is_builtin(status) || self.columns.iter().any(|c| c.id == status)
    }

    /// The column's WIP limit, 0 when none.
    pub fn wip_of(&self, id: &str) -> u32 {
        self.columns.iter().find(|c| c.id == id).map(|c| c.wip).unwrap_or(0)
    }

    /// Add a column after the last one (or before `before`).
    pub fn add_column(&self, label: &str, before: Option<&str>) -> Result<BoardConfig> {
        let id = slug(label);
        if id.is_empty() {
            return Err(anyhow!("a column needs a name"));
        }
        if self.columns.iter().any(|c| c.id == id) {
            return Err(anyhow!("there is already a column “{id}”"));
        }
        let mut columns = self.columns.clone();
        let at = before.and_then(|b| columns.iter().position(|c| c.id == b)).unwrap_or(columns.len());
        columns.insert(at, BoardColumn { id, label: label.trim().to_string(), wip: 0 });
        BoardConfig { columns }.normalized()
    }

    /// Remove one of the user's columns (built-ins stay).
    pub fn remove_column(&self, id: &str) -> Result<BoardConfig> {
        if is_builtin(id) {
            return Err(anyhow!("“{id}” is built in: rename or move it, but it stays"));
        }
        if !self.columns.iter().any(|c| c.id == id) {
            return Err(anyhow!("no column “{id}”; the board has {}", self.ids().join(", ")));
        }
        BoardConfig { columns: self.columns.iter().filter(|c| c.id != id).cloned().collect() }.normalized()
    }

    /// Rename a column's label and/or set its WIP limit.
    pub fn edit_column(&self, id: &str, label: Option<&str>, wip: Option<u32>) -> Result<BoardConfig> {
        if !self.columns.iter().any(|c| c.id == id) {
            return Err(anyhow!("no column “{id}”; the board has {}", self.ids().join(", ")));
        }
        let columns = self.columns.iter().map(|c| if c.id == id { BoardColumn { id: c.id.clone(), label: label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).unwrap_or_else(|| c.label.clone()), wip: wip.unwrap_or(c.wip) } } else { c.clone() }).collect();
        BoardConfig { columns }.normalized()
    }

    pub fn ids(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_come_back_and_custom_columns_keep_their_place() {
        let cfg = BoardConfig { columns: vec![BoardColumn { id: "".into(), label: "Backlog".into(), wip: 0 }, BoardColumn { id: "doing".into(), label: "In progress".into(), wip: 2 }, BoardColumn { id: "blocked".into(), label: "Blocked".into(), wip: 0 }] };
        let n = cfg.normalized().unwrap();
        assert_eq!(n.ids(), ["backlog", "todo", "doing", "review", "done", "blocked"].map(String::from));
        assert_eq!(n.columns[2].label, "In progress");
        assert_eq!(n.wip_of("doing"), 2);
        assert!(n.allows("blocked") && n.allows("todo") && !n.allows("qa"));
        assert_eq!(slug("  QA / Staging  "), "qa-staging");
        assert!(BoardConfig { columns: vec![BoardColumn { id: "a".into(), label: "A".into(), wip: 0 }, BoardColumn { id: "A".into(), label: "B".into(), wip: 0 }] }.normalized().is_err());
        let b = BoardConfig::default().add_column("QA", Some("done")).unwrap();
        assert_eq!(b.ids(), ["todo", "doing", "review", "qa", "done"].map(String::from));
        assert!(b.add_column("qa", None).is_err());
        assert!(b.remove_column("todo").is_err());
        assert_eq!(b.remove_column("qa").unwrap().ids().len(), 4);
        let e = b.edit_column("doing", Some("In progress"), Some(3)).unwrap();
        assert_eq!((e.columns[1].label.as_str(), e.columns[1].wip), ("In progress", 3));
    }
}
