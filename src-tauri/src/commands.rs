use std::path::PathBuf;

use tauri::State;
use worki_core::service::{CreatedWorktree, SetupScripts};
use worki_core::*;
use worki_isolation::ssh;

use crate::activity::{track, Scope};
use crate::{blocking, err, err_chain, AppState, CmdResult};

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> CmdResult<Vec<Project>> {
    state.projects.list_projects().map_err(err)
}

#[tauri::command]
pub async fn add_project(state: State<'_, AppState>, path: PathBuf) -> CmdResult<Project> {
    track(
        state.app.clone(),
        "Add project",
        Scope::default(),
        |log| async move {
            log.step(format!("Reading {}…", path.display()));
            state.projects.add_project(&path).await.map_err(err)
        },
    )
    .await
}

/// `password`, when given, is stored in the Keychain vault before connecting.
#[tauri::command]
pub async fn add_remote_project(
    state: State<'_, AppState>,
    remote: Remote,
    path: PathBuf,
    password: Option<String>,
) -> CmdResult<Project> {
    let remote = match &remote.host_id {
        Some(id) => {
            let mut r = remote.clone();
            state.projects.ssh_host(id).map_err(err)?.apply(&mut r);
            r
        }
        None => remote,
    };
    remote.target().validate().map_err(err_chain)?;
    if let Some(password) = password.as_deref().filter(|p| !p.is_empty()) {
        ssh::set_password(&remote.target(), Some(password)).map_err(err_chain)?;
    }
    track(
        state.app.clone(),
        "Add remote project",
        Scope::default(),
        |log| async move {
            log.step(format!("Connecting to {}…", remote.host));
            state
                .projects
                .add_remote_project(remote, &path)
                .await
                .map_err(err)
        },
    )
    .await
}

/// Keeps a remote project's stored password when its host, user or port change.
#[tauri::command]
pub fn update_project(
    state: State<'_, AppState>,
    id: String,
    patch: ProjectPatch,
) -> CmdResult<Project> {
    let before = state
        .projects
        .project(&id)
        .map_err(err)?
        .remote
        .map(|r| r.target());
    let project = state.projects.update_project(&id, patch).map_err(err)?;
    let after = project.remote.as_ref().map(|r| r.target());
    if let (Some(old), Some(new)) = (before, after) {
        if old != new && ssh::has_password(&old) && !ssh::has_password(&new) {
            let password = ssh::password(&old);
            ssh::set_password(&new, password.as_deref()).map_err(err_chain)?;
            ssh::set_password(&old, None).map_err(err_chain)?;
        }
    }
    Ok(project)
}

#[tauri::command]
pub fn ssh_hosts(state: State<'_, AppState>) -> CmdResult<Vec<SshHost>> {
    state.projects.ssh_hosts().map_err(err)
}

/// Saves a host profile; its stored password moves along when host, user or port change, and
/// `password`, when given, replaces it.
#[tauri::command]
pub fn save_ssh_host(
    state: State<'_, AppState>,
    host: SshHost,
    password: Option<String>,
) -> CmdResult<SshHost> {
    let before = (!host.id.is_empty())
        .then(|| state.projects.ssh_host(&host.id).ok())
        .flatten()
        .map(|h| h.target());
    let saved = state.projects.save_ssh_host(host).map_err(err)?;
    let after = saved.target();
    if let Some(old) = before.filter(|old| *old != after) {
        if ssh::has_password(&old) && !ssh::has_password(&after) {
            let moved = ssh::password(&old);
            ssh::set_password(&after, moved.as_deref()).map_err(err_chain)?;
            ssh::set_password(&old, None).map_err(err_chain)?;
        }
    }
    if let Some(password) = password.as_deref().filter(|p| !p.is_empty()) {
        ssh::set_password(&after, Some(password)).map_err(err_chain)?;
    }
    Ok(saved)
}

#[tauri::command]
pub fn delete_ssh_host(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.projects.delete_ssh_host(&id).map_err(err)
}

#[tauri::command]
pub fn ssh_password_set(host: String, user: Option<String>, port: Option<u16>) -> bool {
    ssh::has_password(&ssh::SshTarget::new(host, user, port))
}

/// Stores the login password for `remote`, or removes it with `None`.
#[tauri::command]
pub fn set_ssh_password(
    host: String,
    user: Option<String>,
    port: Option<u16>,
    password: Option<String>,
) -> CmdResult<()> {
    let target = ssh::SshTarget::new(host, user, port);
    target.validate().map_err(err_chain)?;
    ssh::set_password(&target, password.as_deref()).map_err(err_chain)
}

#[tauri::command]
pub async fn remove_project(state: State<'_, AppState>, id: String) -> CmdResult<Option<String>> {
    let project = state.projects.project(&id).map_err(err)?;
    state.close_sessions(|s| project.contains(&s.worktree_path));
    let title = format!("Remove project {}", project.name);
    track(
        state.app.clone(),
        title,
        Scope::default(),
        |log| async move {
            log.step("Removing the worktrees' environments…");
            state.projects.remove_project(&id).await.map_err(err)
        },
    )
    .await
}

#[tauri::command]
pub fn reorder_projects(state: State<'_, AppState>, ids: Vec<String>) -> CmdResult<()> {
    state.projects.reorder_projects(&ids).map_err(err)
}

#[tauri::command]
pub async fn setup_scripts(state: State<'_, AppState>, id: String) -> CmdResult<SetupScripts> {
    state.projects.setup_scripts(&id).await.map_err(err)
}

#[tauri::command]
pub async fn list_worktrees(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<Vec<Worktree>> {
    state
        .projects
        .list_worktrees(&project_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn list_branches(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<Vec<String>> {
    state.projects.list_branches(&project_id).await.map_err(err)
}

#[tauri::command]
pub async fn worktree_status(
    state: State<'_, AppState>,
    path: PathBuf,
) -> CmdResult<WorktreeStatus> {
    state.projects.worktree_status(&path).await.map_err(err)
}

#[tauri::command]
pub async fn file_diff(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    file: String,
    base: Option<String>,
) -> CmdResult<String> {
    state
        .projects
        .file_diff(&worktree_path, &file, base.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn environment_templates(
    state: State<'_, AppState>,
    isolation: Isolation,
) -> CmdResult<Vec<String>> {
    state
        .projects
        .environment_templates(isolation)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn snapshots(state: State<'_, AppState>) -> CmdResult<Vec<Snapshot>> {
    state.projects.snapshots().await.map_err(err)
}

#[tauri::command]
pub async fn delete_snapshot(
    state: State<'_, AppState>,
    engine: SnapshotEngine,
    name: String,
) -> CmdResult<()> {
    let title = format!("Delete snapshot {name}");
    track(state.app.clone(), title, Scope::default(), |_| async move {
        state.projects.delete_snapshot(engine, &name).await
    })
    .await
}

#[tauri::command]
pub fn snapshot_cleanup(state: State<'_, AppState>) -> CmdResult<String> {
    state.projects.snapshot_cleanup().map_err(err)
}

#[tauri::command]
pub fn set_snapshot_cleanup(state: State<'_, AppState>, script: String) -> CmdResult<()> {
    state.projects.set_snapshot_cleanup(&script).map_err(err)
}

#[tauri::command]
pub async fn save_environment_template(
    state: State<'_, AppState>,
    project_id: String,
    worktree_path: PathBuf,
    tag: String,
    use_for_project: bool,
) -> CmdResult<Project> {
    let scope = Scope::worktree(&worktree_path).in_project(&project_id);
    track(
        state.app.clone(),
        format!("Save snapshot {tag}"),
        scope,
        |log| async move {
            state
                .projects
                .save_environment_template(&project_id, &worktree_path, &tag, use_for_project, {
                    let log = log.clone();
                    move |line| log.line(line)
                })
                .await
                .map_err(err)
        },
    )
    .await
}

#[tauri::command]
pub async fn commit_changes(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    files: Vec<String>,
    message: String,
) -> CmdResult<String> {
    let scope = Scope::worktree(&worktree_path);
    track(state.app.clone(), "Commit", scope, |_| async move {
        state
            .projects
            .commit(&worktree_path, &files, &message)
            .await
    })
    .await
}

#[tauri::command]
pub async fn branch_log(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    base: Option<String>,
) -> CmdResult<Vec<CommitInfo>> {
    state
        .projects
        .branch_log(&worktree_path, base.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn update_from_base(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    base: String,
) -> CmdResult<BaseUpdate> {
    let scope = Scope::worktree(&worktree_path);
    track(
        state.app.clone(),
        format!("Update from {base}"),
        scope,
        |_| async move {
            state
                .projects
                .update_from_base(&worktree_path, &base)
                .await
                .map_err(err)
        },
    )
    .await
}

#[tauri::command]
pub async fn pull_request(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
) -> CmdResult<Option<PullRequest>> {
    state
        .projects
        .pull_request(&worktree_path)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn set_pull_request_base(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    number: u64,
    base: String,
) -> CmdResult<()> {
    state
        .projects
        .set_pull_request_base(&worktree_path, number, &base)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn draft_pull_request(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    base: String,
    instructions: String,
    writer: Writer,
) -> CmdResult<PrDraft> {
    let scope = Scope::worktree(&worktree_path);
    track(
        state.app.clone(),
        "Draft pull request",
        scope,
        |_| async move {
            state
                .projects
                .draft_pull_request(&worktree_path, &base, &instructions, &writer)
                .await
                .map_err(err)
        },
    )
    .await
}

#[tauri::command]
pub async fn draft_commits(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    files: Vec<String>,
    instructions: String,
    writer: Writer,
) -> CmdResult<Vec<CommitDraft>> {
    let scope = Scope::worktree(&worktree_path);
    track(state.app.clone(), "Draft commits", scope, |_| async move {
        state
            .projects
            .draft_commits(&worktree_path, &files, &instructions, &writer)
            .await
    })
    .await
}

#[tauri::command]
pub async fn create_pull_request(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    base: String,
    title: String,
    body: String,
    draft: bool,
) -> CmdResult<PullRequest> {
    let scope = Scope::worktree(&worktree_path);
    track(
        state.app.clone(),
        "Create pull request",
        scope,
        |_| async move {
            state
                .projects
                .create_pull_request(&worktree_path, &base, &title, &body, draft)
                .await
                .map_err(err)
        },
    )
    .await
}

#[tauri::command]
pub async fn compare_base(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    base: String,
) -> CmdResult<String> {
    state
        .projects
        .compare_base(&worktree_path, &base)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn retarget_branch(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    branch: String,
    base: String,
) -> CmdResult<()> {
    state
        .projects
        .retarget_branch(&worktree_path, &branch, &base)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn fetch_remote(state: State<'_, AppState>, worktree_path: PathBuf) -> CmdResult<()> {
    let scope = Scope::worktree(&worktree_path);
    track(state.app.clone(), "Fetch", scope, |_| async move {
        state.projects.fetch(&worktree_path).await
    })
    .await
}

#[tauri::command]
pub async fn push_branch(state: State<'_, AppState>, worktree_path: PathBuf) -> CmdResult<()> {
    let scope = Scope::worktree(&worktree_path);
    track(state.app.clone(), "Push", scope, |_| async move {
        state.projects.push(&worktree_path).await
    })
    .await
}

/// A GitHub attachment from a review comment, as a `data:` URL.
#[tauri::command]
pub async fn github_image(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    url: String,
) -> CmdResult<String> {
    state
        .projects
        .github_image(&worktree_path, &url)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn pr_failed_checks(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    number: u64,
) -> CmdResult<Vec<FailedCheck>> {
    state
        .projects
        .pr_failed_checks(&worktree_path, number)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn pr_review_comments(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    number: u64,
) -> CmdResult<Vec<PrComment>> {
    state
        .projects
        .pr_review_comments(&worktree_path, number)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn branch_changes(
    state: State<'_, AppState>,
    worktree_path: PathBuf,
    base: String,
) -> CmdResult<BranchChanges> {
    state
        .projects
        .branch_changes(&worktree_path, &base)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn create_worktree(
    state: State<'_, AppState>,
    project_id: String,
    req: CreateWorktree,
) -> CmdResult<CreatedWorktree> {
    let title = format!("Create worktree {}", req.name);
    let project = state.projects.project(&project_id).map_err(err)?;
    let path = ProjectService::new_worktree_path(&project, &req.name);
    let scope = Scope::worktree(path).in_project(&project_id);
    track(state.app.clone(), title, scope, |log| async move {
        let created = state
            .projects
            .create_worktree(&project_id, req, {
                let log = log.clone();
                move |line| log.line(line)
            })
            .await
            .map_err(err)?;
        if let Some(e) = &created.setup_error {
            log.fail(format!("The setup script failed: {e}"));
        }
        Ok::<_, String>(created)
    })
    .await
}

#[tauri::command]
pub async fn open_pull_requests(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<Vec<OpenPullRequest>> {
    state
        .projects
        .open_pull_requests(&project_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn create_pr_worktree(
    state: State<'_, AppState>,
    project_id: String,
    req: CreatePrWorktree,
) -> CmdResult<CreatedWorktree> {
    let title = format!("Create worktree for #{}", req.number);
    let project = state.projects.project(&project_id).map_err(err)?;
    let path = ProjectService::new_worktree_path(&project, &req.branch);
    let scope = Scope::worktree(path).in_project(&project_id);
    track(state.app.clone(), title, scope, |log| async move {
        let created = state
            .projects
            .create_pr_worktree(&project_id, req, {
                let log = log.clone();
                move |line| log.line(line)
            })
            .await
            .map_err(err)?;
        if let Some(e) = &created.setup_error {
            log.fail(format!("The setup script failed: {e}"));
        }
        Ok::<_, String>(created)
    })
    .await
}

#[tauri::command]
pub async fn remove_worktree(
    state: State<'_, AppState>,
    project_id: String,
    req: RemoveWorktree,
) -> CmdResult<()> {
    state.close_sessions(|s| s.worktree_path == req.path);
    let scope = Scope::worktree(&req.path).in_project(&project_id);
    track(
        state.app.clone(),
        "Remove worktree",
        scope,
        |log| async move {
            log.step("Removing the worktree and its environment…");
            state
                .projects
                .remove_worktree(&project_id, req)
                .await
                .map_err(err)
        },
    )
    .await
}

#[tauri::command]
pub fn agent_settings(state: State<'_, AppState>) -> CmdResult<AgentSettings> {
    state.projects.agent_settings().map_err(err)
}

/// Saves what the agents dialog edits; secrets and what new environments get set up with stay.
#[tauri::command]
pub fn save_agent_profiles(
    state: State<'_, AppState>,
    profiles: Vec<AgentProfile>,
    default_profile: Option<String>,
) -> CmdResult<()> {
    state
        .projects
        .update_agent_settings(|s| {
            s.profiles = profiles;
            s.default_profile = default_profile;
        })
        .map_err(err)
}

/// Starts new Claude Code sessions with Remote Control, or without.
#[tauri::command]
pub fn set_remote_control(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    state
        .projects
        .update_agent_settings(|s| s.remote_control = enabled)
        .map_err(err)
}

/// The Claude Code settings passed to sessions in isolated environments.
#[tauri::command]
pub fn set_claude_settings(state: State<'_, AppState>, settings: JsonObject) -> CmdResult<()> {
    state
        .projects
        .update_agent_settings(|s| s.claude_settings = settings)
        .map_err(err)
}

/// What isolated Claude sessions get until the user changes it.
#[tauri::command]
pub fn default_claude_settings() -> JsonObject {
    worki_core::default_claude_settings()
}

/// The human's own Claude Code settings, to start isolated sessions' settings from.
#[tauri::command]
pub async fn host_claude_settings() -> CmdResult<JsonObject> {
    blocking(worki_core::agents::host_claude_settings).await
}

/// Puts text on the clipboard natively, for writes that don't come from a click or key press
/// (the web clipboard refuses those), such as a terminal program copying through OSC 52.
#[tauri::command]
pub async fn write_clipboard(text: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write;
        let mut child = std::process::Command::new("/usr/bin/pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(text.as_bytes())?;
        child.wait()?;
        Ok(())
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// Writes a file the user picked a location for in the save dialog, such as a chat diagram.
#[tauri::command]
pub async fn save_file(path: PathBuf, contents: Vec<u8>) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || std::fs::write(&path, contents))
        .await
        .map_err(err)?
        .map_err(err)
}
