use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;
use worki_core::{agents, paths, ClaudeAccount};
use worki_isolation::account_launch;
use worki_pty::{Captured, PtyInput};

use crate::{blocking, err, AppState, CmdResult};

/// Sign-ins in progress, by account id: where the code the user pastes goes. Each has a number,
/// so a sign-in that ends doesn't drop a newer one of the same account.
#[derive(Default)]
pub struct Logins {
    running: Mutex<HashMap<String, (u64, mpsc::Sender<PtyInput>)>>,
    started: AtomicU64,
}

impl Logins {
    fn input(&self, id: &str) -> Option<mpsc::Sender<PtyInput>> {
        self.running
            .lock()
            .unwrap()
            .get(id)
            .map(|(_, input)| input.clone())
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LoginEvent {
    /// The page to approve the sign-in on, which then shows the code to paste.
    Url {
        url: String,
    },
    Output {
        line: String,
    },
}

#[tauri::command]
pub fn add_claude_account(state: State<'_, AppState>) -> CmdResult<ClaudeAccount> {
    state.projects.add_account().map_err(err)
}

#[tauri::command]
pub fn set_default_account(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.projects.set_default_account(&id).map_err(err)
}

/// Returns warnings about environments that couldn't be cleaned up.
#[tauri::command]
pub async fn remove_claude_account(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<Option<String>> {
    let warnings = state.projects.remove_account(&id).await.map_err(err)?;
    Ok((!warnings.is_empty()).then(|| warnings.join("\n")))
}

/// Signs the account in with `claude auth login` in a throwaway container: it sends the sign-in
/// page's link, then waits for the code that page shows ([`enter_login_code`]).
#[tauri::command]
pub async fn log_in_claude_account(
    state: State<'_, AppState>,
    logins: State<'_, Logins>,
    id: String,
    on_event: Channel<LoginEvent>,
) -> CmdResult<ClaudeAccount> {
    let account = state.projects.account(&id).map_err(err)?;
    let dir = paths::account_dir(&id);
    let (input, received) = mpsc::channel();
    let number = logins.started.fetch_add(1, Ordering::SeqCst);
    if let Some((_, older)) = logins
        .running
        .lock()
        .unwrap()
        .insert(id.clone(), (number, input))
    {
        older.send(PtyInput::Kill).ok();
    }
    let result = blocking(move || {
        let run = account_launch(&dir, "claude auth login --claudeai", true)?;
        let launch = &run.launch;
        let mut output = String::new();
        let mut pending = String::new();
        let mut sent_url = false;
        let spec = Captured {
            program: &launch.program,
            args: &launch.args,
            env: &launch.env,
            cwd: &launch.cwd,
            cols: 2000,
            timeout: Duration::from_secs(15 * 60),
        };
        let code = worki_pty::run_interactive(spec, Some(received), |bytes| {
            let text = agents::strip_ansi(&String::from_utf8_lossy(bytes));
            output.push_str(&text);
            pending.push_str(&text);
            if !sent_url {
                if let Some(url) = sign_in_url(&output) {
                    sent_url = true;
                    on_event.send(LoginEvent::Url { url }).ok();
                }
            }
            for line in agents::take_lines(&mut pending) {
                if !line.contains("https://") {
                    on_event.send(LoginEvent::Output { line }).ok();
                }
            }
        });
        match code? {
            Some(0) => Ok(()),
            None => anyhow::bail!("The sign-in was stopped."),
            Some(code) => anyhow::bail!("`claude auth login` exited ({code}) without signing in."),
        }
    })
    .await;
    logins
        .running
        .lock()
        .unwrap()
        .retain(|account, (n, _)| *account != id || *n != number);
    result?;
    refresh(&state, account).await
}

/// Types the code the sign-in page showed into the waiting `claude auth login`.
#[tauri::command]
pub fn enter_login_code(logins: State<'_, Logins>, id: String, code: String) -> CmdResult<()> {
    logins
        .input(&id)
        .ok_or("This sign-in has ended.")?
        .send(PtyInput::Text(format!("{}\r", code.trim())))
        .map_err(err)
}

#[tauri::command]
pub fn cancel_login(logins: State<'_, Logins>, id: String) {
    if let Some(input) = logins.input(&id) {
        input.send(PtyInput::Kill).ok();
    }
}

/// Reads who the account signs in as again.
#[tauri::command]
pub async fn check_claude_account(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<ClaudeAccount> {
    let account = state.projects.account(&id).map_err(err)?;
    refresh(&state, account).await
}

async fn refresh(state: &AppState, mut account: ClaudeAccount) -> CmdResult<ClaudeAccount> {
    let dir = paths::account_dir(&account.id);
    let run =
        blocking(move || account_launch(&dir, "claude auth status --json || true", false)).await?;
    let status = worki_core::setup::run_captured(run.launch.clone())
        .await
        .map_err(err)?;
    let login = crate::agents::account_from_auth_status(&status).filter(|a| !a.token);
    (account.email, account.plan, account.organization) = login
        .map(|l| (l.email, l.plan, l.organization))
        .unwrap_or_default();
    state.projects.save_account(&account).map_err(err)?;
    crate::sessions::recheck_account(&state.app, &paths::account_dir(&account.id));
    Ok(account)
}

#[tauri::command]
pub fn set_project_account(
    state: State<'_, AppState>,
    project_id: String,
    account_id: Option<String>,
) -> CmdResult<()> {
    state
        .projects
        .set_project_account(&project_id, account_id)
        .map_err(err)
}

/// The authorize link in `claude auth login` output.
fn sign_in_url(output: &str) -> Option<String> {
    let complete = &output[..output.rfind(char::is_whitespace)?];
    complete
        .split_whitespace()
        .find(|w| w.starts_with("https://") && w.contains("oauth"))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_sign_in_link() {
        let output = "Browser didn't open? Use the url below to sign in (c to copy)\n\n\
https://claude.ai/oauth/authorize?code=true&client_id=abc&state=xyz\n\nPaste code here if prompted >";
        assert_eq!(
            super::sign_in_url(output).as_deref(),
            Some("https://claude.ai/oauth/authorize?code=true&client_id=abc&state=xyz")
        );
        assert_eq!(super::sign_in_url("Opening https://claude.ai\n"), None);
        assert_eq!(super::sign_in_url("\nhttps://claude.ai/oauth/auth"), None);
    }
}
