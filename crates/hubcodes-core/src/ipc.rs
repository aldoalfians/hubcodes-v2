//! The daemon's socket. `hubcodes daemon run` (hubcodesd) listens on a unix
//! socket in the state dir; the CLI, the hooks and the desktop app talk to it
//! with one JSON object per line:
//!
//!   → {"id": 1, "method": "send", "params": {"session": "hubcodes", "to": "dev", "text": "hi"}}
//!   ← {"id": 1, "result": {...}}            or {"id": 1, "error": "..."}
//!
//! A `subscribe` request keeps the connection open and the daemon pushes
//! `{"event": "status", "session": "...", "data": {...}, "at": 1700000000}`
//! lines on it (the connection may still send requests meanwhile). No
//! daemon running means every client falls back to doing the work itself
//! against tmux and SQLite, which hold the state either way.

use crate::db;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn socket_path() -> PathBuf {
    std::env::var("HUBCODES_SOCKET").map(PathBuf::from).unwrap_or_else(|_| db::state_dir().join("hubcodesd.sock"))
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Event {
    pub event: String,
    pub session: String,
    #[serde(default)]
    pub data: Value,
    pub at: i64,
}

impl Event {
    pub fn new(event: &str, session: &str, data: Value) -> Self {
        Event { event: event.into(), session: session.into(), data, at: db::now() }
    }
}

/// What a method does with its params.
pub type Handler = Arc<dyn Fn(&str, &Value) -> Result<Value> + Send + Sync>;

struct Subscriber {
    session: Option<String>,
    tx: mpsc::Sender<String>,
}

/// Fan-out of events to every subscribed connection.
#[derive(Default)]
pub struct Hub {
    subs: Mutex<Vec<Subscriber>>,
}

impl Hub {
    pub fn broadcast(&self, ev: &Event) {
        let line = match serde_json::to_string(ev) {
            Ok(l) => l,
            Err(_) => return,
        };
        let mut subs = self.subs.lock().unwrap_or_else(|e| e.into_inner());
        subs.retain(|s| {
            let wants = s.session.as_deref().map(|w| w == ev.session).unwrap_or(true);
            !wants || s.tx.send(line.clone()).is_ok()
        });
    }

    pub fn subscribers(&self) -> usize {
        self.subs.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn add(&self, session: Option<String>) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        self.subs.lock().unwrap_or_else(|e| e.into_inner()).push(Subscriber { session, tx });
        rx
    }
}

/// Bind the socket (replacing a stale file from a dead daemon) and serve
/// forever, one thread per connection.
pub fn serve(hub: Arc<Hub>, handler: Handler) -> Result<()> {
    let path = socket_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        if alive() {
            return Err(anyhow!("another hubcodesd already answers on {}", path.display()));
        }
        std::fs::remove_file(&path).with_context(|| format!("remove stale {}", path.display()))?;
    }
    let listener = UnixListener::bind(&path).with_context(|| format!("bind {}", path.display()))?;
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        let (hub, handler) = (hub.clone(), handler.clone());
        std::thread::spawn(move || {
            let _ = connection(stream, hub, handler);
        });
    }
    Ok(())
}

fn connection(stream: UnixStream, hub: Arc<Hub>, handler: Handler) -> Result<()> {
    let reader = BufReader::new(stream.try_clone()?);
    let writer = Arc::new(Mutex::new(stream));
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write_line(&writer, &serde_json::to_string(&Response { id: 0, result: None, error: Some(format!("bad request: {e}")) })?)?;
                continue;
            }
        };
        if req.method == "subscribe" {
            let session = req.params.get("session").and_then(|v| v.as_str()).map(String::from);
            let rx = hub.add(session);
            let w = writer.clone();
            std::thread::spawn(move || {
                for ev in rx {
                    if write_line(&w, &ev).is_err() {
                        break;
                    }
                }
            });
            write_line(&writer, &serde_json::to_string(&Response { id: req.id, result: Some(Value::Bool(true)), error: None })?)?;
            continue;
        }
        let resp = match handler(&req.method, &req.params) {
            Ok(v) => Response { id: req.id, result: Some(v), error: None },
            Err(e) => Response { id: req.id, result: None, error: Some(format!("{e:#}")) },
        };
        write_line(&writer, &serde_json::to_string(&resp)?)?;
    }
    Ok(())
}

fn write_line(w: &Arc<Mutex<UnixStream>>, line: &str) -> std::io::Result<()> {
    let mut s = w.lock().unwrap_or_else(|e| e.into_inner());
    s.write_all(line.as_bytes())?;
    s.write_all(b"\n")?;
    s.flush()
}

// ----- client side -----

fn connect() -> Result<UnixStream> {
    let path = socket_path();
    let s = UnixStream::connect(&path).with_context(|| format!("no hubcodesd on {}", path.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(20)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(s)
}

/// One request, one answer. Errors when no daemon runs.
pub fn call(method: &str, params: Value) -> Result<Value> {
    let mut s = connect()?;
    let req = Request { id: 1, method: method.into(), params };
    s.write_all(serde_json::to_string(&req)?.as_bytes())?;
    s.write_all(b"\n")?;
    s.flush()?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line)?;
    let resp: Response = serde_json::from_str(line.trim()).with_context(|| format!("daemon answered {}", line.trim()))?;
    match resp.error {
        Some(e) => Err(anyhow!("{e}")),
        None => Ok(resp.result.unwrap_or(Value::Null)),
    }
}

/// Is a daemon answering on the socket right now? Cheap: a `ping`.
pub fn alive() -> bool {
    if !socket_path().exists() {
        return false;
    }
    let Ok(mut s) = UnixStream::connect(socket_path()) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(500)));
    if s.write_all(b"{\"id\":0,\"method\":\"ping\"}\n").is_err() {
        return false;
    }
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line).map(|n| n > 0).unwrap_or(false)
}

/// Follow the daemon's events (blocking). `session` filters; `None` is all.
/// Returns when the daemon goes away.
pub fn subscribe(session: Option<&str>, mut on_event: impl FnMut(Event)) -> Result<()> {
    let mut s = connect()?;
    s.set_read_timeout(None)?;
    let req = Request { id: 1, method: "subscribe".into(), params: serde_json::json!({ "session": session }) };
    s.write_all(serde_json::to_string(&req)?.as_bytes())?;
    s.write_all(b"\n")?;
    s.flush()?;
    for line in BufReader::new(s).lines() {
        let line = line?;
        if let Ok(ev) = serde_json::from_str::<Event>(&line) {
            on_event(ev);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_answer_and_events_over_the_socket() {
        let dir = std::env::temp_dir().join(format!("hubcodes-ipc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("HUBCODES_SOCKET", dir.join("d.sock"));
        let hub = Arc::new(Hub::default());
        let h2 = hub.clone();
        let handler: Handler = Arc::new(|m, p| match m {
            "ping" => Ok(serde_json::json!({"pong": true})),
            "echo" => Ok(p.clone()),
            _ => Err(anyhow!("unknown method {m}")),
        });
        std::thread::spawn(move || serve(h2, handler).unwrap());
        for _ in 0..50 {
            if alive() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(alive());
        assert_eq!(call("echo", serde_json::json!({"a": 1})).unwrap(), serde_json::json!({"a": 1}));
        assert!(call("nope", Value::Null).unwrap_err().to_string().contains("unknown method"));

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = subscribe(Some("s1"), |ev| {
                let _ = tx.send(ev);
            });
        });
        for _ in 0..50 {
            if hub.subscribers() > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        hub.broadcast(&Event::new("status", "other", Value::Null));
        hub.broadcast(&Event::new("status", "s1", serde_json::json!({"agent": "dev"})));
        let got = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!((got.event.as_str(), got.session.as_str()), ("status", "s1"));
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err(), "the other session's event was filtered");
        std::env::remove_var("HUBCODES_SOCKET");
    }
}
