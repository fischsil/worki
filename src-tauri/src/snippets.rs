use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::Notify;
use worki_core::LogLine;

use crate::{err, AppState, CmdResult};

#[derive(Default)]
pub struct Snippets(Mutex<HashMap<String, Arc<Notify>>>);

#[tauri::command]
pub async fn run_snippet(
    state: State<'_, AppState>,
    snippets: State<'_, Snippets>,
    project_id: String,
    worktree_path: PathBuf,
    run_id: String,
    command: String,
    on_output: Channel<LogLine>,
) -> CmdResult<Option<i32>> {
    let stop = Arc::new(Notify::new());
    snippets
        .0
        .lock()
        .unwrap()
        .insert(run_id.clone(), stop.clone());
    let result = state
        .projects
        .run_snippet(
            &project_id,
            &worktree_path,
            &command,
            move |line| {
                on_output.send(line).ok();
            },
            &stop,
        )
        .await
        .map_err(err);
    snippets.0.lock().unwrap().remove(&run_id);
    result
}

#[tauri::command]
pub fn stop_snippet(snippets: State<'_, Snippets>, run_id: String) {
    if let Some(stop) = snippets.0.lock().unwrap().get(&run_id) {
        stop.notify_one();
    }
}
