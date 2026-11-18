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
use worki_pty::{Captured, Exit, PtyInput};

use crate::activity::{track, Scope};
use crate::{blocking, err, AppState, CmdResult};

/// Sign-ins in progress, by id (an account's, or `codex:<worktree>` for Codex's in a worktree):
/// where the code the user pastes goes, and how they're cancelled. Each has a number, so a
/// sign-in that ends doesn't drop a newer one of the same id.
#[derive(Default)]
pub struct Logins {
    running: Mutex<HashMap<String, (u64, mpsc::Sender<PtyInput>)>>,
    started: AtomicU64,
}

pub struct Running<'a> {
    logins: &'a Logins,
    key: String,
    number: u64,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.logins
            .running
            .lock()
            .unwrap()
            .retain(|key, (n, _)| *key != self.key || *n != self.number);
    }
}

impl Logins {
    /// Registers a sign-in as `key` until the returned guard drops, stopping an older one of the
    /// same key. Also returns where its input arrives.
    pub fn start(&self, key: String) -> (Running<'_>, mpsc::Receiver<PtyInput>) {
        let (input, received) = mpsc::channel();
        let number = self.started.fetch_add(1, Ordering::SeqCst);
        if let Some((_, older)) = self
            .running
            .lock()
            .unwrap()
            .insert(key.clone(), (number, input))
        {
            older.send(PtyInput::Kill).ok();
        }
        let running = Running {
            logins: self,
            key,
            number,
        };
        (running, received)
    }

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
    let scope = Scope::default();
    let warnings = track(
        state.app.clone(),
        "Remove account",
        scope,
        |log| async move {
            log.step("Removing the environments of the projects that use it…");
            state.projects.remove_account(&id).await.map_err(err)
        },
    )
    .await?;
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
    let account_dir = paths::account_dir(&id);
    let login = agents::account_login_dir(&account_dir).map_err(err)?;
    let scratch = throwaway_config("sign-in")?;
    let command = "claude auth login --claudeai";
    let config = scratch.path().to_owned();
    run_sign_in(&logins, id, config, Some(login), command, on_event, |_| {
        false
    })
    .await?;
    state
        .projects
        .keep_signed_in_identity(&account.id, scratch.path())
        .map_err(err)?;
    refresh(&state, account).await
}

/// A Claude Code config folder for one sign-in or check, deleted afterwards, so an account's
/// folder never gets more than its login and who it is. Under worki's data folder, which the
/// container engines share.
fn throwaway_config(what: &str) -> CmdResult<tempfile::TempDir> {
    let dir = paths::data_dir().join("tmp");
    std::fs::create_dir_all(&dir).map_err(err)?;
    tempfile::Builder::new()
        .prefix(&format!("{what}-"))
        .tempdir_in(dir)
        .map_err(err)
}

/// Gets a sign-in token for the account with `claude setup-token`, signed in like
/// [`log_in_claude_account`] (as the sign-in `token:<id>`), and stores it. The token never
/// leaves the backend.
#[tauri::command]
pub async fn obtain_account_token(
    logins: State<'_, Logins>,
    id: String,
    on_event: Channel<LoginEvent>,
) -> CmdResult<()> {
    let scratch = throwaway_config("setup-token")?;
    let output = run_sign_in(
        &logins,
        format!("token:{id}"),
        scratch.path().to_owned(),
        None,
        "claude setup-token",
        on_event,
        |line| line.contains("sk-ant-"),
    )
    .await;
    let token =
        agents::find_claude_token(&output?).ok_or("`claude setup-token` didn't print a token.")?;
    worki_core::secrets::set_account_token(&id, token_name()?, Some(&token)).map_err(err)
}

/// Runs `command` in a throwaway container with `config` as Claude Code's folder and `login` as
/// where it keeps the login, as sign-in `key`:
/// sends the sign-in page's link, passes on output lines `hide` doesn't match, and takes the
/// code the page shows ([`enter_login_code`]). Returns all the output.
async fn run_sign_in(
    logins: &Logins,
    key: String,
    config: std::path::PathBuf,
    login: Option<std::path::PathBuf>,
    command: &'static str,
    on_event: Channel<LoginEvent>,
    hide: fn(&str) -> bool,
) -> CmdResult<String> {
    let (_running, received) = logins.start(key);
    blocking(move || {
        let run = account_launch(&config, login.as_deref(), command, true)?;
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
                if !line.contains("https://") && !hide(&line) {
                    on_event.send(LoginEvent::Output { line }).ok();
                }
            }
        });
        match code? {
            Exit::Code(0) => Ok(output),
            Exit::Code(code) => anyhow::bail!("`{command}` exited ({code}) without signing in."),
            Exit::Killed | Exit::TimedOut => anyhow::bail!("The sign-in was stopped."),
        }
    })
    .await
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
    let account_dir = paths::account_dir(&account.id);
    let login_dir = agents::account_login_dir(&account_dir).map_err(err)?;
    let scratch = throwaway_config("auth-status")?;
    agents::copy_login_identity(&login_dir, scratch.path()).map_err(err)?;
    let (config, run_login) = (scratch.path().to_owned(), login_dir.clone());
    let command = "claude auth status --json || true";
    let run = blocking(move || account_launch(&config, Some(&run_login), command, false)).await?;
    let status = worki_core::setup::run_captured(run.launch.clone())
        .await
        .map_err(err)?;
    let login = crate::agents::account_from_auth_status(&status).filter(|a| !a.token);
    (account.email, account.plan, account.organization) = login
        .map(|l| (l.email, l.plan, l.organization))
        .unwrap_or_default();
    state.projects.save_account(&account).map_err(err)?;
    crate::sessions::recheck_account(&state.app, &login_dir);
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

#[tauri::command]
pub fn set_project_sign_in(
    state: State<'_, AppState>,
    project_id: String,
    sign_in: worki_core::AccountSignIn,
) -> CmdResult<()> {
    state
        .projects
        .set_project_sign_in(&project_id, sign_in)
        .map_err(err)
}

#[tauri::command]
pub fn project_environment(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<worki_core::ProjectEnvironment> {
    state.projects.project_environment(&project_id).map_err(err)
}

#[tauri::command]
pub fn set_project_environment(
    state: State<'_, AppState>,
    project_id: String,
    environment: worki_core::ProjectEnvironment,
) -> CmdResult<()> {
    state
        .projects
        .set_project_environment(&project_id, environment)
        .map_err(err)
}

#[tauri::command]
pub fn project_shared_dir(project_id: String) -> std::path::PathBuf {
    worki_core::paths::project_shared_dir(&project_id)
}

/// Where the project's own Claude Code settings, `CLAUDE.md` and skills are.
#[tauri::command]
pub fn project_claude_dir(project_id: String) -> std::path::PathBuf {
    worki_core::paths::project_claude_dir(&project_id)
}

fn token_name() -> CmdResult<&'static str> {
    agents::claude_sign_in_secret().ok_or_else(|| "Claude Code has no sign-in token".into())
}

/// Whether the account has a sign-in token stored; never reads it.
#[tauri::command]
pub fn account_token_set(account_id: String) -> CmdResult<bool> {
    Ok(worki_core::secrets::has_account_token(
        &account_id,
        token_name()?,
    ))
}

/// Stores the account's sign-in token (`claude setup-token`), or removes it with `None`.
#[tauri::command]
pub fn set_account_token(account_id: String, token: Option<String>) -> CmdResult<()> {
    let token = token.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    worki_core::secrets::set_account_token(&account_id, token_name()?, token.as_deref())
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
