//! Rooms that live on this device: list, transcript, members, posts.

use super::*;
use hubcodes_core::rooms;

#[tauri::command]
pub fn room_list(state: State<'_, Arc<AppState>>) -> Result<Vec<rooms::RoomInfo>, String> {
    rooms::list(&state.tmux).map_err(err)
}

#[tauri::command]
pub fn room_log(state: State<'_, Arc<AppState>>, name: String, limit: Option<usize>) -> Result<Vec<rooms::RoomMessage>, String> {
    rooms::log(&state.tmux, &name, limit.unwrap_or(200)).map_err(err)
}

#[tauri::command]
pub fn room_create(state: State<'_, Arc<AppState>>, name: String, members: Vec<String>) -> Result<(), String> {
    rooms::create(&state.tmux, name.trim(), &members, &state.me()).map_err(err)
}

#[tauri::command]
pub fn room_members(state: State<'_, Arc<AppState>>, name: String, add: Vec<String>, remove: Vec<String>) -> Result<(), String> {
    if !add.is_empty() {
        rooms::add(&state.tmux, &name, &add).map_err(err)?;
    }
    if !remove.is_empty() {
        rooms::remove(&state.tmux, &name, &remove).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub fn room_delete(state: State<'_, Arc<AppState>>, name: String) -> Result<(), String> {
    rooms::delete(&state.tmux, &name).map_err(err)
}

/// Post as the person at this Mac. Delivery to members on other devices
/// can take a moment (SSH or the listener), so it runs off the UI thread.
#[tauri::command]
pub async fn room_post(state: State<'_, Arc<AppState>>, name: String, text: String) -> Result<usize, String> {
    let st: Arc<AppState> = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || rooms::post(&st.tmux, &st.reg(), &name, &st.me(), &text).map_err(err)).await.map_err(err)?
}
