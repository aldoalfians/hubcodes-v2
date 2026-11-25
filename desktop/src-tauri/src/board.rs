//! Task board, columns, notes, fan-out, review, playbooks and trackers.

use super::*;

#[tauri::command]
pub fn task_add(state: State<'_, Arc<AppState>>, title: String, detail: String, role: String, owner: String) -> Result<Task, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    let task = tasks::add(&db, &title, &detail, &role, &state.me(), &owner).map_err(err)?;
    if !owner.is_empty() {
        let reg = state.reg();
        relay::deliver(&state.tmux, &reg, "hubcodes", &owner, &tasks::assignment_text(&task, &state.me())).map_err(|e| format!("{e:#}"))?;
    }
    Ok(task)
}

#[tauri::command]
pub fn task_set(state: State<'_, Arc<AppState>>, id: i64, status: String, result: Option<String>) -> Result<Task, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    tasks::move_to(&db, id, &status, result.as_deref(), &state.reg().board).map_err(err)
}

#[tauri::command]
pub fn task_update(state: State<'_, Arc<AppState>>, id: i64, title: String, detail: String, role: String) -> Result<Task, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    tasks::update(&db, id, &title, &detail, &role).map_err(err)
}

/// Put a task in a column before another task of that column (or last).
#[tauri::command]
pub fn task_place(state: State<'_, Arc<AppState>>, id: i64, column: String, before: Option<i64>) -> Result<Task, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    tasks::place(&db, id, &column, before, &state.reg().board).map_err(err)
}

#[tauri::command]
pub fn task_history(state: State<'_, Arc<AppState>>, id: i64) -> Result<Vec<tasks::HistoryItem>, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    tasks::history(&db, id).map_err(err)
}

/// The board's columns, in order.
#[tauri::command]
pub fn board_columns(state: State<'_, Arc<AppState>>) -> Vec<hubcodes_core::board::BoardColumn> {
    state.reg().board.columns.clone()
}

/// Save the columns as given (built-ins come back if left out); tasks in a
/// removed column go to To do. Returns the columns as saved.
#[tauri::command]
pub fn board_columns_set(state: State<'_, Arc<AppState>>, columns: Vec<hubcodes_core::board::BoardColumn>) -> Result<Vec<hubcodes_core::board::BoardColumn>, String> {
    let board = hubcodes_core::board::BoardConfig { columns }.normalized().map_err(|e| format!("{e:#}"))?;
    let mut next = state.reg().clone();
    let gone: Vec<String> = next.board.columns.iter().map(|c| c.id.clone()).filter(|id| !board.allows(id)).collect();
    next.board = board.clone();
    let path = next.save().map_err(|e| format!("{e:#}"))?;
    next.source = Some(path);
    *state.reg.write().unwrap_or_else(|e| e.into_inner()) = next;
    if !gone.is_empty() {
        let db = Db::open(&state.tmux.session).map_err(err)?;
        for id in gone {
            let _ = tasks::fold_column(&db, &id, "todo");
        }
    }
    Ok(board.columns)
}

#[tauri::command]
pub fn task_assign(state: State<'_, Arc<AppState>>, id: i64, agent: String) -> Result<Task, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    let task = tasks::assign(&db, id, &agent).map_err(err)?;
    let reg = state.reg();
    relay::deliver(&state.tmux, &reg, "hubcodes", &agent, &tasks::assignment_text(&task, &task.created_by)).map_err(|e| format!("{e:#}"))?;
    Ok(task)
}

#[tauri::command]
pub fn task_remove(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    tasks::remove(&db, id).map_err(err)
}

/// A feedback round paused after rounds without a commit: let it go on.
#[tauri::command]
pub fn task_resume(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    hubcodes_core::feedback::resume(&db, id).map_err(err)
}

/// Approve a pull request on GitHub after the reviewer recommended it: a person pressed the button.
#[tauri::command]
pub fn pr_approve(state: State<'_, Arc<AppState>>, id: i64) -> Result<String, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    hubcodes_core::tracker::approve(&db, id, &state.me()).map_err(err)
}

/// Stop handling a pull request: no more reviews or feedback rounds for it.
#[tauri::command]
pub fn pr_ignore(state: State<'_, Arc<AppState>>, id: i64) -> Result<String, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    hubcodes_core::tracker::ignore(&db, id, &state.me()).map_err(err)
}

#[tauri::command]
pub fn tasks_all(state: State<'_, Arc<AppState>>, include_done: bool) -> Result<Vec<Task>, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    tasks::list(&db, include_done).map_err(err)
}

#[tauri::command]
pub fn notes_write(state: State<'_, Arc<AppState>>, text: String) -> Result<(), String> {
    notes::write(&state.tmux.session, &text).map_err(err)
}

#[tauri::command]
pub fn notes_add(state: State<'_, Arc<AppState>>, text: String) -> Result<(), String> {
    notes::add(&state.tmux.session, &state.me(), &text).map_err(err)
}

#[tauri::command]
pub fn fanout_start(state: State<'_, Arc<AppState>>, targets: Vec<String>, collector: Option<String>, prompt: String) -> Result<Fanout, String> {
    let reg = state.reg();
    fanout::start(&state.tmux, &reg, &state.me(), &targets, collector.as_deref().filter(|c| !c.is_empty()), &prompt).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn fanouts(state: State<'_, Arc<AppState>>) -> Result<Vec<Fanout>, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    fanout::list(&db, 20).map_err(err)
}

/// Approve a task in Review (the human always may).
#[tauri::command]
pub fn task_approve(state: State<'_, Arc<AppState>>, id: i64) -> Result<Task, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    let task = tasks::set_status(&db, id, "done", None).map_err(err)?;
    if !task.owner.is_empty() {
        let reg = state.reg();
        let _ = relay::deliver(&state.tmux, &reg, &state.me(), &task.owner, &format!("Task #{id} “{}” approved.", task.title));
    }
    Ok(task)
}

/// Send a task back to its owner with a note.
#[tauri::command]
pub fn task_request_changes(state: State<'_, Arc<AppState>>, id: i64, note: String) -> Result<Task, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    let task = tasks::set_status(&db, id, "doing", Some(&note)).map_err(err)?;
    if !task.owner.is_empty() {
        let reg = state.reg();
        let _ = relay::deliver(&state.tmux, &reg, &state.me(), &task.owner, &format!("Task #{id} “{}” needs changes: {note}\nWhen done, run: hubcodes task done {id} \"<what changed>\"", task.title));
    }
    Ok(task)
}

#[tauri::command]
pub fn playbook_start(state: State<'_, Arc<AppState>>, name: String, goal: String, cwd: Option<String>) -> Result<hubcodes_core::playbook::Run, String> {
    let reg = state.reg();
    let cwd = cwd.filter(|c| !c.trim().is_empty()).or_else(|| reg.desktop.default_folder.clone());
    hubcodes_core::playbook::start(&state.tmux, &reg, &name, &goal, &state.me(), cwd.as_deref()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn playbook_approve(state: State<'_, Arc<AppState>>, id: i64) -> Result<hubcodes_core::playbook::Run, String> {
    let reg = state.reg();
    hubcodes_core::playbook::approve(&state.tmux, &reg, id, &state.me()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn playbook_abort(state: State<'_, Arc<AppState>>, id: i64) -> Result<hubcodes_core::playbook::Run, String> {
    hubcodes_core::playbook::abort(&state.tmux, id, &state.me()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn playbook_runs(state: State<'_, Arc<AppState>>, all: bool) -> Result<Vec<hubcodes_core::playbook::Run>, String> {
    let db = Db::open(&state.tmux.session).map_err(err)?;
    hubcodes_core::playbook::list(&db, !all).map_err(err)
}

/// Playbooks for a folder: the repo's first, then the global ones.
#[tauri::command]
pub fn playbooks(state: State<'_, Arc<AppState>>, cwd: Option<String>) -> Vec<hubcodes_core::playbook::Playbook> {
    let reg = state.reg();
    match cwd.filter(|c| !c.trim().is_empty()) {
        Some(c) => reg.for_dir(Path::new(&c)).playbooks,
        None => reg.playbooks,
    }
}

#[tauri::command]
pub fn tracker_check(tracker: hubcodes_core::tracker::Tracker) -> hubcodes_core::tracker::Check {
    hubcodes_core::tracker::check(&tracker)
}

#[tauri::command]
pub fn tracker_pull(state: State<'_, Arc<AppState>>, name: String) -> Result<Vec<String>, String> {
    let reg = state.reg();
    let tr = reg.trackers.iter().find(|t| t.name == name).ok_or_else(|| format!("no tracker {name}"))?;
    if !tr.enabled {
        return Err(format!("{name} is off; turn it on to pull"));
    }
    let db = Db::open(&state.tmux.session).map_err(err)?;
    Ok(hubcodes_core::tracker::pull(&db, tr).into_iter().map(|e| e.text).collect())
}

/// Skills on this Mac (`device` empty) or on another device, with the
/// providers that have each.
#[tauri::command]
pub async fn skills_list(state: State<'_, Arc<AppState>>, device: Option<String>) -> Result<Vec<hubcodes_core::skills::Skill>, String> {
    let peer = match device.filter(|d| !d.is_empty()) {
        Some(d) => Some(state.reg().peers.into_iter().find(|p| p.name == d).ok_or_else(|| format!("no device {d}"))?),
        None => None,
    };
    tauri::async_runtime::spawn_blocking(move || match peer {
        Some(p) => hubcodes_core::remote::skills(&p).map_err(err),
        None => Ok(hubcodes_core::skills::list()),
    })
    .await
    .map_err(err)?
}

/// Link a skill into every tool on this Mac; with `from`, fetch it from that
/// device into the shared folder first. Returns what changed.
#[tauri::command]
pub async fn skill_copy(state: State<'_, Arc<AppState>>, name: String, from: Option<String>) -> Result<Vec<String>, String> {
    let reg = state.reg();
    let peer = match from.filter(|d| !d.is_empty()) {
        Some(d) => Some(reg.peers.iter().find(|p| p.name == d).cloned().ok_or_else(|| format!("no device {d}"))?),
        None => None,
    };
    let cfg = reg.skills.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use hubcodes_core::skills;
        if let Some(p) = peer {
            skills::unpack(&name, &hubcodes_core::remote::skill_pack(&p, &name).map_err(err)?, &["shared".to_string()]).map_err(err)?;
        }
        Ok(skills::sync_only(&cfg, Some(&name)))
    })
    .await
    .map_err(err)?
}

/// Link every skill into every tool now (`[skills] sync` does it each minute).
#[tauri::command]
pub async fn skills_sync(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let cfg = state.reg().skills.clone();
    tauri::async_runtime::spawn_blocking(move || hubcodes_core::skills::ensure_builtin().into_iter().chain(hubcodes_core::skills::sync(&cfg)).collect()).await.map_err(err)
}

/// Add a skill from a folder, a .zip/.skill file or a git URL, then link it everywhere.
#[tauri::command]
pub async fn skill_add(state: State<'_, Arc<AppState>>, source: String) -> Result<Vec<String>, String> {
    let cfg = state.reg().skills.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let names = hubcodes_core::skills::add(&source).map_err(err)?;
        let _ = hubcodes_core::skills::sync(&cfg);
        Ok(names)
    })
    .await
    .map_err(err)?
}

/// Pull every skill source that is a git checkout, then link what is new.
#[tauri::command]
pub async fn skills_pull(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let cfg = state.reg().skills.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = hubcodes_core::skills::pull(&cfg);
        out.extend(hubcodes_core::skills::sync(&cfg));
        out
    })
    .await
    .map_err(err)
}

/// Which of these trackers another device already polls (same kind, place
/// and query), so the same ticket does not land on two boards. Devices that
/// do not answer are left out.
#[tauri::command]
pub async fn tracker_overlaps(state: State<'_, Arc<AppState>>, trackers: Vec<hubcodes_core::tracker::Tracker>) -> Result<Vec<hubcodes_core::tracker::Overlap>, String> {
    let peers = state.reg().peers;
    tauri::async_runtime::spawn_blocking(move || {
        let handles: Vec<_> = peers.into_iter().map(|p| std::thread::spawn(move || hubcodes_core::remote::trackers(&p).ok().map(|list| (p.name.clone(), list)))).collect();
        let theirs: Vec<_> = handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect();
        hubcodes_core::tracker::overlaps(&trackers, &theirs)
    })
    .await
    .map_err(err)
}
