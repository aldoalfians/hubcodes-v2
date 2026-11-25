//! Terminal panes: streaming to xterm.js, keystrokes, resize, read.

use super::*;

/// Start streaming a pane's output as `pane:<id>` events (base64 chunks).
/// The first chunk is the current screen, so a fresh xterm shows the agent
/// immediately.
#[tauri::command]
pub fn attach_pane(app: AppHandle, state: State<'_, Arc<AppState>>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let mut streams = state.streams.lock().unwrap_or_else(|e| e.into_inner());
    // A re-attach (page reload, sheet open) restarts the stream so the new
    // listener gets a fresh screen snapshot.
    if let Some(old) = streams.remove(&id) {
        old.stop();
    }
    // A size change makes the shell redraw; the stream's warm-up waits for
    // that to settle before it snapshots the screen.
    stream::resize(&id, cols, rows);
    match stream::start(app, &id) {
        Ok(h) => {
            streams.insert(id, h);
            Ok(())
        }
        Err(e) => {
            eprintln!("[hubcodes] attach {id} failed: {e:#}");
            Err(err(e))
        }
    }
}

#[tauri::command]
pub fn detach_pane(state: State<'_, Arc<AppState>>, id: String) {
    let mut streams = state.streams.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(h) = streams.remove(&id) {
        h.stop();
    }
}

/// Raw keystrokes from xterm.js (`onData`) → the pane, byte-exact. Marks
/// this user as the pane's holder so other clients see who is typing.
#[tauri::command]
pub fn write_pane(state: State<'_, Arc<AppState>>, id: String, data: String) -> Result<(), String> {
    let me = std::env::var("USER").unwrap_or_else(|_| "me".into());
    state.tmux.check_hold(&id, &me).map_err(err)?;
    stream::write(&id, data.as_bytes()).map_err(err)?;
    let _ = state.tmux.hold(&id, &me);
    Ok(())
}

#[tauri::command]
pub fn resize_pane(id: String, cols: u16, rows: u16) {
    stream::resize(&id, cols, rows);
}

#[tauri::command]
pub fn read_pane(state: State<'_, Arc<AppState>>, id: String, lines: usize) -> Result<Vec<String>, String> {
    state.tmux.capture(&id, lines).map_err(err)
}
