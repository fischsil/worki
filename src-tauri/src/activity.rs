//! Operations that can take a while (creating a worktree, starting a session, saving a
//! snapshot, …), each with its log, kept for the Activity panel and sent as they change.

use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use worki_core::trace::{self, Sink};
use worki_core::LogLine;

use crate::CmdResult;

const KEPT: usize = 50;
const LINES_KEPT: usize = 500;

pub mod events {
    /// An activity started or finished; carries the whole activity.
    pub const ACTIVITY: &str = "activity";
    pub const ACTIVITY_LOG: &str = "activity-log";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: u64,
    pub title: String,
    pub project_id: Option<String>,
    pub worktree_path: Option<PathBuf>,
    pub status: Status,
    /// Milliseconds since the epoch.
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub error: Option<String>,
    pub lines: VecDeque<LogLine>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogEvent {
    id: u64,
    line: LogLine,
}

#[derive(Default)]
pub struct Activities {
    list: Mutex<VecDeque<Activity>>,
    next: AtomicU64,
}

impl Activities {
    fn update<R>(&self, id: u64, change: impl FnOnce(&mut Activity) -> R) -> Option<R> {
        let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        list.iter_mut().find(|a| a.id == id).map(change)
    }
}

/// What an activity is about, for where the UI shows it.
#[derive(Default, Clone)]
pub struct Scope {
    pub project_id: Option<String>,
    pub worktree_path: Option<PathBuf>,
}

impl Scope {
    pub fn worktree(path: impl Into<PathBuf>) -> Self {
        Self {
            project_id: None,
            worktree_path: Some(path.into()),
        }
    }

    pub fn in_project(mut self, id: &str) -> Self {
        self.project_id = Some(id.to_owned());
        self
    }
}

/// Where an activity's steps go; cheap to clone into callbacks.
#[derive(Clone)]
pub struct Log {
    app: AppHandle,
    id: u64,
}

impl Log {
    pub fn line(&self, line: LogLine) {
        let kept = self.app.state::<Activities>().update(self.id, |a| {
            if a.lines.len() >= LINES_KEPT {
                a.lines.pop_front();
            }
            a.lines.push_back(line.clone());
        });
        if kept.is_some() {
            let event = LogEvent { id: self.id, line };
            self.app.emit(events::ACTIVITY_LOG, event).ok();
        }
    }

    pub fn step(&self, text: impl Into<String>) {
        self.line(LogLine::Info(text.into()));
    }

    /// Marks the activity failed once it ends, though its work goes on and succeeds.
    pub fn fail(&self, error: impl Into<String>) {
        let error = error.into();
        self.app
            .state::<Activities>()
            .update(self.id, |a| a.error = Some(error));
    }
}

/// Runs `work` as an activity named `title`: it shows as running with the steps `work` logs,
/// then as done or failed with the error.
pub async fn track<T, E, F>(
    app: AppHandle,
    title: impl Into<String>,
    scope: Scope,
    work: impl FnOnce(Log) -> F,
) -> CmdResult<T>
where
    E: std::fmt::Display,
    F: Future<Output = Result<T, E>>,
{
    let log = start(&app, title.into(), scope);
    let mut finish = Finish {
        log: log.clone(),
        error: Some("Stopped.".into()),
    };
    let sink: Sink = Arc::new({
        let log = log.clone();
        move |line| log.line(LogLine::Command(line))
    });
    let result = trace::scope(sink, work(log))
        .await
        .map_err(|e| e.to_string());
    finish.error = result.as_ref().err().cloned();
    result
}

fn start(app: &AppHandle, title: String, scope: Scope) -> Log {
    let activities = app.state::<Activities>();
    let activity = Activity {
        id: activities.next.fetch_add(1, Ordering::SeqCst),
        title,
        project_id: scope.project_id,
        worktree_path: scope.worktree_path,
        status: Status::Running,
        started_at: now(),
        finished_at: None,
        error: None,
        lines: VecDeque::new(),
    };
    {
        let mut list = activities.list.lock().unwrap_or_else(|e| e.into_inner());
        list.push_front(activity.clone());
        while list.len() > KEPT {
            let Some(at) = list.iter().rposition(|a| a.status != Status::Running) else {
                break;
            };
            list.remove(at);
        }
    }
    app.emit(events::ACTIVITY, &activity).ok();
    Log {
        app: app.clone(),
        id: activity.id,
    }
}

/// Finishes the activity when the work ends, also when it's dropped before it does.
struct Finish {
    log: Log,
    error: Option<String>,
}

impl Drop for Finish {
    fn drop(&mut self) {
        let error = self.error.take();
        let finished = self.log.app.state::<Activities>().update(self.log.id, |a| {
            let error = error.or(a.error.take());
            a.status = if error.is_some() {
                Status::Failed
            } else {
                Status::Done
            };
            a.error = error;
            a.finished_at = Some(now());
            a.clone()
        });
        if let Some(activity) = finished {
            self.log.app.emit(events::ACTIVITY, activity).ok();
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[tauri::command]
pub fn activities(activities: State<'_, Activities>) -> Vec<Activity> {
    activities
        .list
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}
