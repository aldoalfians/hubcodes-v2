//! Rooms: a named group of agents, possibly on several devices, that hear
//! each other's posts.
//!
//! A room lives on the device that created it (its home). The home keeps
//! the member list and the only transcript; a post from anywhere goes to
//! the home first (`hubcodes room post <home>:<room> …`, over the device
//! link like `task done`), which writes it down and delivers it to every
//! other member through the ordinary relay, so a member on another device
//! gets it on that device and an unreachable one gets it from the remote
//! queue later. Nothing else is shared: no screens, no files. Each device
//! keeps its own record of what it sent and received (messages, events).
//!
//! Only the owner of the home creates a room or changes its members: those
//! commands are not on the federation verb list. A post from someone
//! else's device waits for approval in the Inbox like any message.

use crate::db::Db;
use crate::providers::Registry;
use crate::relay;
use crate::tmux::Tmux;
use anyhow::{anyhow, Result};

/// `design`, `auth-team`: lower case, digits, `-` and `_`, up to 32.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Where a room lives: `home:room` names a device, a bare name is here.
pub fn split(addr: &str, peers: &[crate::remote::Peer]) -> (Option<String>, String) {
    match addr.split_once(':') {
        Some((home, room)) if peers.iter().any(|p| p.name == home) => (Some(home.to_string()), room.to_string()),
        _ => (None, addr.to_string()),
    }
}

fn check(name: &str) -> Result<()> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(anyhow!("room names are lower case letters, digits, - and _ (up to 32): '{name}'"))
    }
}

pub fn create(t: &Tmux, name: &str, members: &[String], by: &str) -> Result<()> {
    check(name)?;
    let db = Db::open(&t.session)?;
    if !db.room_create(name, by)? {
        return Err(anyhow!("room '{name}' already exists"));
    }
    for m in members {
        db.room_add_member(name, m.trim())?;
    }
    let _ = db.add_event(by, "room", &format!("created {name} with {}", members.join(", ")));
    Ok(())
}

fn existing(db: &Db, name: &str) -> Result<()> {
    if db.room_exists(name)? {
        Ok(())
    } else {
        Err(anyhow!("no room '{name}' on this device (`hubcodes room list`)"))
    }
}

/// Post to a room that lives here: write it down, then deliver it to every
/// member but the sender. Returns how many members it went to.
pub fn post(t: &Tmux, reg: &Registry, room: &str, from: &str, text: &str) -> Result<usize> {
    let db = Db::open(&t.session)?;
    existing(&db, room)?;
    let text = crate::redact::redact(text.trim());
    if text.is_empty() {
        return Err(anyhow!("empty message"));
    }
    let members = db.room_members(room)?;
    if !from.starts_with("human:") && !members.iter().any(|m| m == from) {
        return Err(anyhow!("{from} is not in room {room}; `hubcodes room add {room} {from}` on {}", reg.sharing.name));
    }
    db.room_post(room, from, &text)?;
    let _ = db.add_event(from, "room", &format!("posted to {room}"));
    let body = format!("[room {room}] {from}: {text}");
    let sender = format!("room:{room}");
    let mut sent = 0;
    let mut failed = Vec::new();
    for m in members.iter().filter(|m| m.as_str() != from) {
        match relay::deliver(t, reg, &sender, m, &body) {
            Ok(_) => sent += 1,
            Err(e) => failed.push(format!("{m}: {e:#}")),
        }
    }
    if !failed.is_empty() {
        let _ = db.add_event(from, "room", &format!("{room}: not delivered to {}", failed.join("; ")));
    }
    Ok(sent)
}

pub fn add(t: &Tmux, room: &str, members: &[String]) -> Result<()> {
    let db = Db::open(&t.session)?;
    existing(&db, room)?;
    for m in members {
        db.room_add_member(room, m.trim())?;
    }
    Ok(())
}

pub fn remove(t: &Tmux, room: &str, members: &[String]) -> Result<()> {
    let db = Db::open(&t.session)?;
    existing(&db, room)?;
    for m in members {
        if !db.room_remove_member(room, m.trim())? {
            return Err(anyhow!("{m} is not in room {room}"));
        }
    }
    Ok(())
}

pub fn delete(t: &Tmux, room: &str) -> Result<()> {
    let db = Db::open(&t.session)?;
    if !db.room_delete(room)? {
        return Err(anyhow!("no room '{room}' on this device"));
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct RoomInfo {
    pub name: String,
    pub created_at: i64,
    pub created_by: String,
    pub members: Vec<String>,
}

pub fn list(t: &Tmux) -> Result<Vec<RoomInfo>> {
    let db = Db::open(&t.session)?;
    db.rooms()?
        .into_iter()
        .map(|(name, created_at, created_by)| Ok(RoomInfo { members: db.room_members(&name)?, name, created_at, created_by }))
        .collect()
}

#[derive(serde::Serialize)]
pub struct RoomMessage {
    pub id: i64,
    pub at: i64,
    pub from: String,
    pub text: String,
}

pub fn log(t: &Tmux, room: &str, limit: usize) -> Result<Vec<RoomMessage>> {
    let db = Db::open(&t.session)?;
    existing(&db, room)?;
    Ok(db.room_log(room, limit)?.into_iter().map(|(id, at, from, text)| RoomMessage { id, at, from, text }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_names() {
        assert!(valid_name("design"));
        assert!(valid_name("auth-team_2"));
        assert!(!valid_name("Design"));
        assert!(!valid_name("-x"));
        assert!(!valid_name(""));
        assert!(!valid_name("a b"));
        assert!(!valid_name(&"a".repeat(33)));
    }

    #[test]
    fn a_room_address_names_its_home_only_when_that_is_a_device() {
        let peers = vec![crate::remote::Peer { name: "a".into(), host: "mac-a".into(), user: None, session: "hubcodes".into(), hubcodes: "hubcodes".into(), max_agents: 0, allow: vec![], transport: String::new(), port: None }];
        assert_eq!(split("a:design", &peers), (Some("a".into()), "design".into()));
        assert_eq!(split("design", &peers), (None, "design".into()));
        assert_eq!(split("x:design", &peers), (None, "x:design".into()));
    }
}
