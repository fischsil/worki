//! Keeps the Mac from idle sleep while the user wants it and an agent session is open, through
//! `caffeinate`, which also ends with worki.

use std::collections::HashSet;
use std::process::{Child, Command};
use std::sync::{Mutex, MutexGuard};

use tauri::State;
use worki_pty::{SessionInfo, SessionState};

use crate::{err, AppState, CmdResult};

#[derive(Default)]
pub struct KeepAwake(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    enabled: bool,
    open_agents: HashSet<String>,
    caffeinate: Option<Child>,
}

impl KeepAwake {
    pub fn new(enabled: bool) -> Self {
        Self(Mutex::new(Inner {
            enabled,
            ..Inner::default()
        }))
    }

    pub fn set_enabled(&self, on: bool) {
        let mut inner = self.lock();
        inner.enabled = on;
        inner.apply();
    }

    pub fn session_changed(&self, session: &SessionInfo) {
        let open_agent = session.profile_id.is_some() && session.state == SessionState::Running;
        self.track(&session.id, open_agent);
    }

    fn track(&self, id: &str, open_agent: bool) {
        let mut inner = self.lock();
        if open_agent {
            inner.open_agents.insert(id.to_owned());
        } else {
            inner.open_agents.remove(id);
        }
        inner.apply();
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Inner {
    fn apply(&mut self) {
        let wanted = self.enabled && !self.open_agents.is_empty();
        let running = self
            .caffeinate
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        if wanted == running {
            return;
        }
        if let Some(mut child) = self.caffeinate.take() {
            child.kill().ok();
            child.wait().ok();
        }
        if wanted {
            self.caffeinate = Command::new("/usr/bin/caffeinate")
                .args(["-i", "-w", &std::process::id().to_string()])
                .spawn()
                .inspect_err(|e| tracing::warn!("couldn't start caffeinate: {e}"))
                .ok();
        }
    }
}

#[tauri::command]
pub fn keep_awake(state: State<'_, AppState>) -> CmdResult<bool> {
    state.projects.keep_awake().map_err(err)
}

#[tauri::command]
pub fn set_keep_awake(state: State<'_, AppState>, on: bool) -> CmdResult<()> {
    state.projects.set_keep_awake(on).map_err(err)?;
    state.awake.set_enabled(on);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holding(awake: &KeepAwake) -> bool {
        awake.lock().caffeinate.is_some()
    }

    #[test]
    fn holds_while_enabled_and_an_agent_is_open() {
        let awake = KeepAwake::new(false);
        awake.track("a", true);
        assert!(!holding(&awake));
        awake.set_enabled(true);
        assert!(holding(&awake));
        awake.track("b", true);
        awake.track("a", false);
        assert!(holding(&awake));
        awake.track("b", false);
        assert!(!holding(&awake));
    }
}
