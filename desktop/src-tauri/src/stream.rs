//! Live pane output for xterm.js. `tmux pipe-pane` writes everything the
//! pane emits into a FIFO; a thread reads it and forwards chunks to the
//! webview as `pane:<id>` events. Input goes back with `send-keys -H`
//! (hex bytes), so arrow keys, Ctrl combinations and pastes arrive intact.

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone)]
struct Chunk<'a> {
    id: &'a str,
    data: String,
}

pub struct Handle {
    id: String,
    fifo: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Handle {
    pub fn stop(self) {
        eprintln!("[hubcodes] stream stop {}", self.id);
        self.stop.store(true, Ordering::SeqCst);
        let _ = Command::new("tmux").args(["pipe-pane", "-t", &self.id]).status();
        let _ = std::fs::remove_file(&self.fifo);
    }
}

fn fifo_dir() -> PathBuf {
    let d = std::env::temp_dir().join("hubcodes-desktop");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Seconds of silence after which the warm-up snapshot is taken.
const WARMUP_QUIET: Duration = Duration::from_millis(250);
/// Upper bound for the warm-up when the pane never goes quiet.
const WARMUP_MAX: Duration = Duration::from_millis(1500);

pub fn start(app: AppHandle, id: &str) -> Result<Handle> {
    let fifo = fifo_dir().join(format!("{}.pipe", id.trim_start_matches('%')));
    let _ = std::fs::remove_file(&fifo);
    let st = Command::new("mkfifo").arg(&fifo).status().context("mkfifo")?;
    if !st.success() {
        return Err(anyhow!("mkfifo failed for {}", fifo.display()));
    }
    let stop = Arc::new(AtomicBool::new(false));

    // Read+write, non-blocking: opening never waits for the writer and the
    // reader never sees EOF when tmux's `cat` goes away.
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&fifo)
        .with_context(|| format!("open fifo {}", fifo.display()))?;

    // Pipe first, snapshot second. Everything the pane prints from now on
    // reaches the FIFO; the reader discards it until the pane has been quiet
    // for a moment, then sends the screen as tmux has it (which includes the
    // discarded output) and forwards live from there. Snapshotting before the
    // pipe starts loses whatever the shell prints in between (prompt redraws
    // after SIGWINCH, async prompt segments) and xterm drifts from the pane.
    let cmd = format!("cat > '{}'", fifo.display());
    let st = Command::new("tmux")
        .args(["pipe-pane", "-O", "-t", id, &cmd])
        .stdout(Stdio::null())
        .status()
        .context("pipe-pane")?;
    if !st.success() {
        return Err(anyhow!("tmux pipe-pane failed for {id}"));
    }

    let (tid, tstop, tapp) = (id.to_string(), stop.clone(), app.clone());
    std::thread::spawn(move || {
        let mut buf = [0u8; 65536];
        let started = Instant::now();
        let mut last_data = started;
        let mut live = false;
        let mut filter = TitleFilter::default();
        loop {
            if tstop.load(Ordering::SeqCst) {
                break;
            }
            match f.read(&mut buf) {
                Ok(0) => std::thread::sleep(Duration::from_millis(10)),
                Ok(n) => {
                    if live {
                        let clean = filter.push(&buf[..n]);
                        if !clean.is_empty() {
                            emit(&tapp, &tid, &clean);
                        }
                    } else {
                        last_data = Instant::now();
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if !live && (last_data.elapsed() >= WARMUP_QUIET || started.elapsed() >= WARMUP_MAX) {
                        emit(&tapp, &tid, &snapshot(&tid));
                        live = true;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    eprintln!("[hubcodes] fifo read failed {tid}: {e}");
                    break;
                }
            }
        }
    });
    eprintln!("[hubcodes] stream start {id} → {}", fifo.display());
    Ok(Handle { id: id.to_string(), fifo, stop })
}

/// Drops `ESC k <title> ESC \\` (screen's set-window-name, which zsh and
/// friends send because `TERM` is `screen`/`tmux`). xterm.js does not know
/// the sequence and prints the title as text, which shifts the prompt by a
/// line. Keeps state across chunks.
#[derive(Default)]
struct TitleFilter {
    state: TitleState,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum TitleState {
    #[default]
    Text,
    /// Saw ESC, waiting to see whether `k` follows.
    Esc,
    InTitle,
    /// Inside a title, saw ESC: `\\` ends it.
    InTitleEsc,
}

impl TitleFilter {
    fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        for &b in bytes {
            self.state = match self.state {
                TitleState::Text => {
                    if b == 0x1b {
                        TitleState::Esc
                    } else {
                        out.push(b);
                        TitleState::Text
                    }
                }
                TitleState::Esc => match b {
                    b'k' => TitleState::InTitle,
                    0x1b => {
                        out.push(0x1b);
                        TitleState::Esc
                    }
                    _ => {
                        out.push(0x1b);
                        out.push(b);
                        TitleState::Text
                    }
                },
                TitleState::InTitle => match b {
                    0x1b => TitleState::InTitleEsc,
                    0x07 => TitleState::Text,
                    _ => TitleState::InTitle,
                },
                TitleState::InTitleEsc => match b {
                    b'\\' => TitleState::Text,
                    0x1b => TitleState::InTitleEsc,
                    _ => TitleState::InTitle,
                },
            };
        }
        out
    }
}

/// The pane's visible screen with attributes, plus the cursor position, as
/// one byte string xterm can replay: clear, draw, place cursor.
fn snapshot(id: &str) -> Vec<u8> {
    let screen = Command::new("tmux")
        .args(["capture-pane", "-p", "-e", "-t", id])
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    let cursor = Command::new("tmux")
        .args(["display-message", "-p", "-t", id, "#{cursor_x} #{cursor_y}"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let mut out: Vec<u8> = Vec::with_capacity(screen.len() + 32);
    out.extend_from_slice(b"\x1b[0m\x1b[2J\x1b[H");
    let text = String::from_utf8_lossy(&screen);
    let lines: Vec<&str> = text.lines().collect();
    out.extend_from_slice(lines.join("\r\n").as_bytes());
    if let Some((x, y)) = cursor.split_once(' ') {
        if let (Ok(x), Ok(y)) = (x.parse::<u32>(), y.parse::<u32>()) {
            out.extend_from_slice(format!("\x1b[0m\x1b[{};{}H", y + 1, x + 1).as_bytes());
        }
    }
    out
}

/// Event name for a pane. Tauri event names may not contain `%`.
pub fn event_name(id: &str) -> String {
    format!("pane:{}", id.trim_start_matches('%'))
}

fn emit(app: &AppHandle, id: &str, bytes: &[u8]) {
    // HUBCODES_STREAM_DUMP=<dir> appends every chunk to <dir>/<id>.bin (diagnostics).
    if let Some(dir) = std::env::var_os("HUBCODES_STREAM_DUMP") {
        use std::io::Write;
        let path = PathBuf::from(dir).join(format!("{}.bin", id.trim_start_matches('%')));
        if let Ok(mut f) = OpenOptions::new().append(true).create(true).open(path) {
            let _ = f.write_all(bytes);
        }
    }
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let _ = app.emit(&event_name(id), Chunk { id, data });
}

pub fn write(id: &str, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let mut args: Vec<String> = vec!["send-keys".into(), "-H".into(), "-t".into(), id.into()];
    args.extend(bytes.iter().map(|b| format!("{b:02x}")));
    let st = Command::new("tmux").args(&args).status().context("send-keys")?;
    if !st.success() {
        return Err(anyhow!("tmux send-keys failed for {id}"));
    }
    Ok(())
}

/// Match the pane's window to the xterm size. Works because the session
/// uses `window-size manual` (no attached client dictates the size).
/// Resize the pane's window to the xterm's size. Returns `true` when the
/// size changed, because the shell then redraws its prompt (SIGWINCH) and a
/// screen snapshot taken in the middle of that redraw desyncs xterm.
pub fn resize(id: &str, cols: u16, rows: u16) -> bool {
    if cols < 10 || rows < 3 {
        return false;
    }
    let current = Command::new("tmux")
        .args(["display-message", "-p", "-t", id, "#{pane_width} #{pane_height}"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if current == format!("{cols} {rows}") {
        return false;
    }
    let _ = Command::new("tmux")
        .args(["resize-window", "-t", id, "-x", &cols.to_string(), "-y", &rows.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_screen_title_and_keeps_everything_else() {
        let mut f = TitleFilter::default();
        let out = f.push(b"a\x1bk..codes/desktop\x1b\\b\x1b[31mc\x1b]2;t\x1b\\d");
        assert_eq!(out, b"ab\x1b[31mc\x1b]2;t\x1b\\d");
    }

    #[test]
    fn survives_chunk_boundaries() {
        let mut f = TitleFilter::default();
        let mut out = f.push(b"x\x1b");
        out.extend(f.push(b"ktitle\x1b"));
        out.extend(f.push(b"\\y\x1b"));
        out.extend(f.push(b"[Kz"));
        assert_eq!(out, b"xy\x1b[Kz");
    }
}
