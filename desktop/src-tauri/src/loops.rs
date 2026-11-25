//! Agent-to-agent loops.

use super::*;

#[tauri::command]
pub fn set_loop(state: State<'_, Arc<AppState>>, a: String, b: String, on: bool, max: Option<u32>) -> Result<(), String> {
    let max = max.unwrap_or(state.reg().loop_cfg.max_rounds);
    relay::set_loop(&state.tmux, &a, &b, on, max).map_err(err)
}

#[tauri::command]
pub fn reset_loop(state: State<'_, Arc<AppState>>, a: String, b: String) -> Result<(), String> {
    relay::reset_loop(&state.tmux, &a, &b).map_err(err)
}

/// Make a↔b a loop and send `goal` to a (told to report to b).
#[tauri::command]
pub fn start_loop(state: State<'_, Arc<AppState>>, a: String, b: String, goal: String, max: Option<u32>) -> Result<u64, String> {
    let reg = state.reg();
    let max = max.unwrap_or(reg.loop_cfg.max_rounds);
    relay::kickoff(&state.tmux, &reg, &state.me(), &a, &b, &goal, max).map(|d| d.seq).map_err(|e| format!("{e:#}"))
}
