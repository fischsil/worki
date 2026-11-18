use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State};
use worki_core::agents;
use worki_core::{secrets, Isolation, LogLine};
use worki_isolation::{host_which, Backend, LaunchRequest};
use worki_pty::{Captured, Exit};

use crate::accounts::Logins;
use crate::sessions::events;
use crate::{blocking, err, AppState, CmdResult};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    profile_id: String,
    name: String,
    executable: String,
    /// Where the agent resolves inside the isolation.
    env_path: Option<String>,
    /// Where it resolves for the human user.
    host_path: Option<String>,
    /// The human's own installation can be reused inside.
    can_share: bool,
    auth: Option<AuthStatus>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    /// Env var the sign-in token is passed as.
    secret: String,
    /// A token is stored for this isolation.
    set: bool,
}

/// Per-agent availability inside `isolation`, with what the human has that could be reused.
#[tauri::command]
pub async fn agent_statuses(
    state: State<'_, AppState>,
    isolation: Isolation,
) -> CmdResult<Vec<AgentStatus>> {
    isolation.validate().map_err(err)?;
    let profiles = state.projects.agent_settings().map_err(err)?.profiles;
    let scope = isolation.scope_key();
    blocking(move || {
        let backend = isolation.backend();
        let caps = backend.capabilities();
        let instances = backend.instances().unwrap_or_default();
        // Where the agent resolves: the backend itself (user account, sandbox image), else in
        // every environment instance (each sandbox).
        let env_path = |executable: &str| -> Option<String> {
            if let Some(path) = backend.which(executable) {
                return Some(path);
            }
            match instances.len() {
                0 => None,
                1 => instances[0].which(executable),
                n => instances
                    .iter()
                    .all(|i| i.which(executable).is_some())
                    .then(|| format!("installed in all {n} sandboxes")),
            }
        };
        Ok(profiles
            .into_iter()
            .filter_map(|p| {
                let executable = p.executable()?.to_owned();
                let kit = agents::kit(&executable);
                Some(AgentStatus {
                    env_path: env_path(&executable),
                    host_path: host_which(&executable),
                    can_share: caps.share_host_install,
                    auth: kit.and_then(|k| k.sign_in).map(|sign_in| AuthStatus {
                        set: secrets::is_set(&scope, sign_in.secret),
                        secret: sign_in.secret.to_owned(),
                    }),
                    profile_id: p.id,
                    name: p.name,
                    executable,
                })
            })
            .collect())
    })
    .await
}

/// Reuses the human's installation of `executable` inside `isolation`.
#[tauri::command]
pub async fn share_agent(isolation: Isolation, executable: String) -> CmdResult<String> {
    isolation.validate().map_err(err)?;
    blocking(move || agents::share_install(isolation.backend().as_ref(), &executable)).await
}

/// The account in `claude auth status --json` output, or `None` when it isn't signed in.
pub fn account_from_auth_status(output: &str) -> Option<worki_pty::Account> {
    let json = output.find('{').map_or(output, |start| &output[start..]);
    let status: serde_json::Value = serde_json::from_str(json).ok()?;
    if status["loggedIn"] != true {
        return None;
    }
    let account = worki_pty::chat::account(&status, "subscriptionType", "orgName");
    Some(worki_pty::Account {
        token: status["authMethod"] != "claude.ai",
        ..account.unwrap_or_default()
    })
}

/// Creates a sign-in token for agents in `isolation` without sharing the human's own login:
/// `claude setup-token` (the user approves in the browser) bound to their subscription, stored as
/// the isolation's secret. Claude Code can't tell whose account a token is, so sessions that sign
/// in with it show `name` instead. macOS keeps interactive logins only in the Keychain, which agent
/// users lack.
#[tauri::command]
pub async fn sign_in_agent(
    state: State<'_, AppState>,
    isolation: Isolation,
    executable: String,
    name: Option<String>,
    on_log: Channel<LogLine>,
) -> CmdResult<()> {
    let scope = isolation.scope_key();
    let secret = agents::kit(&executable)
        .and_then(|k| k.sign_in)
        .map(|s| s.secret)
        .ok_or_else(|| format!("worki can't sign in {executable}"))?;
    let log = move |line: LogLine| {
        on_log.send(line).ok();
    };
    log(LogLine::Info(
        "Running `claude setup-token` — approve the request in your browser.".into(),
    ));
    let value = blocking(move || {
        let claude = host_which("claude")
            .ok_or_else(|| anyhow::anyhow!("`claude` is not installed for you"))?;
        let mut output = String::new();
        let mut pending = String::new();
        let code = worki_pty::run_captured(
            &claude,
            &["setup-token".into()],
            &std::env::temp_dir(),
            1000,
            Duration::from_secs(600),
            |bytes| {
                let text = String::from_utf8_lossy(bytes);
                output.push_str(&text);
                pending.push_str(&agents::strip_ansi(&text));
                for line in agents::take_lines(&mut pending) {
                    if !line.contains("sk-ant-") {
                        log(LogLine::Stdout(line));
                    }
                }
            },
        )?;
        match (agents::find_claude_token(&output), code) {
            (Some(token), _) => Ok(token),
            (None, Exit::Code(code)) => {
                anyhow::bail!("`claude setup-token` exited ({code}) without printing a token.")
            }
            (None, _) => anyhow::bail!("Timed out waiting for the browser sign-in."),
        }
    })
    .await?;

    secrets::set(&scope, secret, &value).map_err(err)?;
    let name = name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
    state
        .projects
        .update_agent_settings(|s| {
            s.add_secret(&scope, secret);
            if let Some(name) = name {
                s.token_names.insert(secrets::fingerprint(&value), name);
            }
        })
        .map_err(err)
}

/// Shown in the worktree while Codex waits for its device-code sign-in.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentSignIn {
    worktree_path: PathBuf,
    /// Cancels the sign-in with `cancel_login`.
    id: String,
    agent: String,
    url: String,
    code: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignInEnded {
    worktree_path: PathBuf,
}

/// Signs Codex in where `worktree`'s agents run, when it isn't yet: Codex's device-code sign-in,
/// whose link and code the worktree shows until it's approved in the browser or cancelled. The
/// login stays in that environment.
pub async fn sign_in_codex(
    state: &AppState,
    instance: Arc<dyn Backend>,
    worktree: &Path,
    agent: &str,
) -> CmdResult<()> {
    let request = |command: &str, interactive: bool| LaunchRequest {
        cwd: worktree.to_owned(),
        command: Some(command.into()),
        interactive,
        stdin: false,
        env: Vec::new(),
        session: None,
    };
    let (status, sign_in) = (
        request("codex login status", false),
        request("codex login --device-auth", true),
    );
    let app = state.app.clone();
    let (worktree_path, agent) = (worktree.to_owned(), agent.to_owned());
    let id = format!("codex:{}", worktree.display());
    blocking(move || {
        let mut status = instance.launch(&status)?.command();
        if status
            .stdin(std::process::Stdio::null())
            .output()?
            .status
            .success()
        {
            return Ok(());
        }
        let logins = app.state::<Logins>();
        let (_running, input) = logins.start(id.clone());
        let launch = instance.launch(&sign_in)?;
        let mut output = String::new();
        let mut shown = false;
        let spec = Captured {
            program: &launch.program,
            args: &launch.args,
            env: &[],
            cwd: &launch.cwd,
            cols: 120,
            timeout: Duration::from_secs(15 * 60),
        };
        let code = worki_pty::run_interactive(spec, Some(input), |bytes| {
            output.push_str(&agents::strip_ansi(&String::from_utf8_lossy(bytes)));
            if let (false, Some((url, code))) = (shown, device_code(&output)) {
                shown = true;
                let event = AgentSignIn {
                    worktree_path: worktree_path.clone(),
                    id: id.clone(),
                    agent: agent.clone(),
                    url,
                    code,
                };
                app.emit(events::AGENT_SIGN_IN, event).ok();
            }
        });
        app.emit(events::AGENT_SIGNED_IN, SignInEnded { worktree_path })
            .ok();
        match code? {
            Exit::Code(0) => Ok(()),
            Exit::Killed => anyhow::bail!("Codex sign-in was cancelled."),
            Exit::TimedOut => {
                anyhow::bail!("Codex sign-in expired; start Codex again for a new code.")
            }
            Exit::Code(_) => anyhow::bail!("Codex sign-in didn't finish: {}", output.trim()),
        }
    })
    .await
}

/// The link and one-time code in Codex's device-code sign-in output.
fn device_code(output: &str) -> Option<(String, String)> {
    let words: Vec<&str> = output.split_whitespace().collect();
    let url = words.iter().find(|w| w.starts_with("https://"))?;
    let code = words.iter().find(|w| {
        w.split_once('-').is_some_and(|(a, b)| {
            [a, b].iter().all(|part| {
                (4..=6).contains(&part.len())
                    && part
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            })
        })
    })?;
    Some((url.to_string(), code.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_device_code_from_codex_output() {
        let output = "Follow these steps to sign in with ChatGPT using device code authorization:\n\n\
1. Open this link in your browser and sign in to your account\n   https://auth.openai.com/codex/device\n\n\
2. Enter this one-time code (expires in 15 minutes)\n   CWPX-HIAOL\n";
        assert_eq!(
            super::device_code(output),
            Some((
                "https://auth.openai.com/codex/device".into(),
                "CWPX-HIAOL".into()
            ))
        );
        assert_eq!(super::device_code("1. Open this link"), None);
    }

    #[test]
    fn reads_the_account_from_claude_auth_status() {
        let login = super::account_from_auth_status(
            "note: profile loaded\n{\"loggedIn\": true, \"authMethod\": \"claude.ai\", \"email\": \"me@example.com\", \"orgName\": \"Acme\", \"subscriptionType\": \"enterprise\"}",
        )
        .unwrap();
        assert_eq!(login.email.as_deref(), Some("me@example.com"));
        assert_eq!(login.plan.as_deref(), Some("enterprise"));
        assert_eq!(login.organization.as_deref(), Some("Acme"));
        assert!(!login.token);

        let token =
            super::account_from_auth_status(r#"{"loggedIn": true, "authMethod": "oauth_token"}"#)
                .unwrap();
        assert!(token.token && token.email.is_none());
        assert_eq!(
            super::account_from_auth_status(r#"{"loggedIn": false}"#),
            None
        );
    }
}
