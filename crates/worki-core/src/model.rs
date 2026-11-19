use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
pub use worki_isolation::{Isolation, ProjectIsolation};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub repo_path: PathBuf,
    pub worktree_root: PathBuf,
    pub default_base: String,
    pub isolation: ProjectIsolation,
    pub setup_script: Option<String>,
    /// Set when the repository lives on another machine; paths are then paths on that host.
    #[serde(default)]
    pub remote: Option<Remote>,
}

impl Project {
    /// Whether `path` is in the main checkout or under the worktree folder.
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.repo_path) || path.starts_with(&self.worktree_root)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    /// Host name or `~/.ssh/config` alias.
    pub host: String,
    /// Login name; unset uses the SSH config or the local user name.
    #[serde(default)]
    pub user: Option<String>,
    /// Unset uses the SSH config or 22.
    #[serde(default)]
    pub port: Option<u16>,
    /// Run sessions in tmux on the host so they outlive worki and the connection.
    pub keep_alive: bool,
    /// The saved host these connection settings come from; they follow its changes.
    #[serde(default)]
    pub host_id: Option<String>,
}

impl Remote {
    pub fn target(&self) -> worki_isolation::ssh::SshTarget {
        worki_isolation::ssh::SshTarget::new(self.host.clone(), self.user.clone(), self.port)
    }
}

/// Connection settings saved once and shared by remote projects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshHost {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

impl SshHost {
    pub fn target(&self) -> worki_isolation::ssh::SshTarget {
        worki_isolation::ssh::SshTarget::new(self.host.clone(), self.user.clone(), self.port)
    }

    pub fn apply(&self, remote: &mut Remote) {
        remote.host = self.host.clone();
        remote.user = self.user.clone();
        remote.port = self.port;
        remote.host_id = Some(self.id.clone());
    }
}

/// An agent session to start again, resuming its conversation, when worki restarts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub id: String,
    pub project_id: String,
    pub worktree_path: PathBuf,
    pub profile_id: String,
    /// Shown as a chat instead of in a terminal.
    #[serde(default)]
    pub chat: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPatch {
    pub name: Option<String>,
    pub worktree_root: Option<PathBuf>,
    pub default_base: Option<String>,
    pub isolation: Option<ProjectIsolation>,
    /// An empty string clears the script.
    pub setup_script: Option<String>,
    /// Only for remote projects: new connection settings.
    pub remote: Option<Remote>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    /// What the branch was started from, when worki created it.
    #[serde(default)]
    pub base: Option<String>,
    pub head: String,
    pub is_main: bool,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatus {
    pub changed_files: Vec<ChangedFile>,
    pub insertions: u32,
    pub deletions: u32,
    pub ahead: u32,
    pub behind: u32,
    pub upstream: Option<String>,
    /// Files git couldn't merge, waiting to be resolved.
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// What stopped on `conflicts`; `None` when there are none, or for a `git stash pop`.
    #[serde(default)]
    pub stopped: Option<GitOperation>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GitOperation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BranchChanges {
    pub base: String,
    pub files: Vec<ChangedFile>,
    pub insertions: u32,
    pub deletions: u32,
    /// Commits on the worktree's branch since it left `base`.
    pub commits: u32,
    /// Commits on `base` the branch doesn't have yet.
    #[serde(default)]
    pub behind: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    pub sha: String,
    pub subject: String,
    pub author: String,
    /// Seconds since the epoch.
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum BaseUpdate {
    UpToDate,
    Updated,
    /// The rebase stopped on conflicts in these files and was undone.
    Conflicts {
        files: Vec<String>,
    },
}

/// Where a snapshot lives: an sbx template, or a Docker, Podman or Apple container image.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotEngine {
    DockerSandbox,
    Docker,
    Podman,
    AppleContainer,
}

impl SnapshotEngine {
    pub const ALL: [Self; 4] = [
        Self::DockerSandbox,
        Self::Docker,
        Self::Podman,
        Self::AppleContainer,
    ];

    /// An isolation on this engine, to reach its snapshots.
    pub fn isolation(self) -> Isolation {
        match self {
            Self::DockerSandbox => Isolation::DockerSandbox { template: None },
            Self::Docker => Isolation::Docker {
                image: worki_isolation::DEFAULT_CONTAINER_IMAGE.into(),
            },
            Self::Podman => Isolation::Podman {
                image: worki_isolation::DEFAULT_CONTAINER_IMAGE.into(),
            },
            Self::AppleContainer => Isolation::AppleContainer {
                image: worki_isolation::DEFAULT_CONTAINER_IMAGE.into(),
            },
        }
    }

    /// The snapshot new environments of `isolation` start from, if any.
    pub fn snapshot_of(isolation: &Isolation) -> Option<(Self, String)> {
        let custom = |image: &str| image != worki_isolation::DEFAULT_CONTAINER_IMAGE;
        match isolation {
            Isolation::DockerSandbox { template: Some(t) } => {
                Some((Self::DockerSandbox, t.clone()))
            }
            Isolation::Docker { image } if custom(image) => Some((Self::Docker, image.clone())),
            Isolation::Podman { image } if custom(image) => Some((Self::Podman, image.clone())),
            Isolation::AppleContainer { image } if custom(image) => {
                Some((Self::AppleContainer, image.clone()))
            }
            _ => None,
        }
    }
}

/// A saved environment snapshot, with the settings that start new environments from it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub engine: SnapshotEngine,
    pub name: String,
    /// Unix seconds, when the engine says.
    pub created: Option<i64>,
    /// Bytes, when the engine says.
    pub size: Option<u64>,
    /// "The default isolation", project names, or "project › worktree".
    pub used_by: Vec<String>,
}

/// How worki runs an agent once in the background to write a text: `command`, or
/// [`crate::launch::DEFAULT_WRITER_COMMAND`], where `{fork}` copies the conversation of the
/// Claude session `fork` and `{prompt}` is the task.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Writer {
    pub fork: Option<String>,
    pub command: Option<String>,
}

/// A commit an agent proposes: its message and which of the files to commit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitDraft {
    pub message: String,
    pub files: Vec<String>,
}

/// A pull request title and description an agent wrote.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrDraft {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// OPEN, CLOSED or MERGED.
    pub state: String,
    pub is_draft: bool,
    /// The branch it merges into.
    pub base: String,
    pub checks_passed: u32,
    pub checks_failed: u32,
    pub checks_pending: u32,
}

/// A check that failed on a pull request, with the end of its log where GitHub Actions ran it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedCheck {
    pub name: String,
    pub url: String,
    pub description: String,
    pub log: Option<String>,
}

/// A review comment on a pull request's diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PrComment {
    pub path: String,
    /// `None` when the comment no longer maps onto the current diff.
    pub line: Option<u32>,
    /// "new" for the changed side, "old" for removed lines.
    pub side: String,
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    /// Two-letter porcelain v1 code, e.g. " M", "??", "A ".
    pub code: String,
    /// Lines added and removed; `None` for binary files or when they couldn't be counted.
    #[serde(default)]
    pub lines: Option<LineCounts>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineCounts {
    pub added: u32,
    pub removed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorktree {
    /// Names both the worktree folder (slugified) and the new branch.
    pub name: String,
    /// Existing branch or ref the new branch starts from.
    pub base: String,
    pub run_setup: bool,
    /// The new worktree's isolation; `Inherit` (the default) follows the project default.
    #[serde(default)]
    pub isolation: ProjectIsolation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveWorktree {
    pub path: PathBuf,
    pub force: bool,
    pub delete_branch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "stream", content = "line")]
pub enum LogLine {
    Info(String),
    Stdout(String),
    Stderr(String),
    /// A command worki ran, as a shell line.
    Command(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    /// Shell command line, run through the user's login shell.
    pub command: String,
    /// Shell code that installs the agent inside an isolated environment.
    #[serde(default)]
    pub install: Option<String>,
}

impl AgentProfile {
    pub fn executable(&self) -> Option<&str> {
        self.command.split_whitespace().next()
    }
}

/// The Codex install command worki 0.2.8 and earlier saved into profiles.
const OLD_CODEX_INSTALLER: &str = r#"npm install -g --prefix "$HOME/.local" @openai/codex"#;

/// Install commands for agents worki knows. They run as the isolated user, so they must not need
/// root and should install into that user's home (`~/.local/bin` is on its PATH).
fn known_installer(executable: &str) -> Option<&'static str> {
    match executable {
        "claude" => Some("curl -fsSL https://claude.ai/install.sh | bash"),
        "codex" => {
            Some("curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_NON_INTERACTIVE=1 sh")
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettings {
    pub profiles: Vec<AgentProfile>,
    pub default_profile: Option<String>,
    /// Per isolation scope (`Isolation::scope_key`), names of environment variables whose
    /// values live in the Keychain and are passed to agents running there. Earlier versions
    /// stored one global list; it is read into [`LEGACY_SCOPE`] and moved on load.
    #[serde(default, deserialize_with = "scoped_secrets")]
    pub secrets: BTreeMap<String, Vec<String>>,
    /// Starts Claude Code sessions with Remote Control, to continue them from claude.ai/code
    /// or the Claude app.
    #[serde(default)]
    pub remote_control: bool,
    /// Claude Code settings passed to sessions in isolated environments, on top of the
    /// environment's own (`--settings`).
    #[serde(default = "default_claude_settings")]
    pub claude_settings: JsonObject,
    /// Names the user gave sign-in tokens, by the token's fingerprint (`secrets::fingerprint`),
    /// shown when a session signs in with one, since Claude Code can't tell whose account it is.
    #[serde(default)]
    pub token_names: BTreeMap<String, String>,
    /// Claude Code logins of worki's own, each in its folder (`paths::account_dir`).
    #[serde(default)]
    pub accounts: Vec<ClaudeAccount>,
    /// The account a project's agents use, by project id, where it isn't the default account.
    #[serde(default)]
    pub project_accounts: BTreeMap<String, String>,
    #[serde(default)]
    pub default_account: Option<String>,
    /// How a project's sessions sign in to its account, by project id, where it isn't
    /// [`AccountSignIn::Folder`].
    #[serde(default)]
    pub project_sign_in: BTreeMap<String, AccountSignIn>,
}

/// What a project's containers and sandboxes get besides its worktree.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEnvironment {
    /// A folder all the project's worktrees share, for whatever they keep there (a build
    /// cache, say).
    #[serde(default)]
    pub shared_folder: bool,
    #[serde(default)]
    pub claude_files: FilesScope,
    /// Over the global default, field by field.
    #[serde(default)]
    pub resources: worki_isolation::Resources,
}

/// Who shares a set of files: the whole project, or each worktree its own.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FilesScope {
    #[default]
    Project,
    Worktree,
}

/// How a project's sessions sign in to its account in containers and sandboxes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccountSignIn {
    /// With the account's login, which projects on the account share; nothing else is.
    #[default]
    Folder,
    /// With the account's long-lived token (`claude setup-token`). The token can only run the
    /// model: no Remote Control.
    Token,
    /// Without an account: sessions sign in themselves, and each worktree keeps its Claude Code
    /// folder and that login to itself. Also what projects get when no account is set up.
    Session,
}

/// A Claude Code login projects can pick. Who it signs in as is read from the folder with
/// `claude auth status` after signing in; it has no email until then.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccount {
    pub id: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
}

pub type JsonObject = serde_json::Map<String, serde_json::Value>;

pub fn default_claude_settings() -> JsonObject {
    JsonObject::from_iter([(
        "sandbox".to_owned(),
        serde_json::json!({
            "enabled": false,
            "allowUnsandboxedCommands": true,
            "network": { "allowAllUnixSockets": true },
            "filesystem": { "allowWrite": ["/"] },
        }),
    )])
}

pub const LEGACY_SCOPE: &str = "legacy";

fn scoped_secrets<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, Vec<String>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Scoped(BTreeMap<String, Vec<String>>),
        Global(Vec<String>),
    }
    Ok(match Stored::deserialize(d)? {
        Stored::Scoped(map) => map,
        Stored::Global(names) if names.is_empty() => BTreeMap::new(),
        Stored::Global(names) => BTreeMap::from([(LEGACY_SCOPE.to_owned(), names)]),
    })
}

impl Default for AgentSettings {
    fn default() -> Self {
        let profile = |id: &str, name: &str, command: &str| AgentProfile {
            id: id.into(),
            name: name.into(),
            command: command.into(),
            install: known_installer(command).map(Into::into),
        };
        Self {
            profiles: vec![
                profile("claude", "Claude Code", "claude"),
                profile("codex", "Codex", "codex"),
            ],
            default_profile: Some("claude".into()),
            secrets: BTreeMap::new(),
            remote_control: false,
            claude_settings: default_claude_settings(),
            token_names: BTreeMap::new(),
            accounts: Vec::new(),
            project_accounts: BTreeMap::new(),
            default_account: None,
            project_sign_in: BTreeMap::new(),
        }
    }
}

impl AgentSettings {
    pub fn secret_names(&self, scope: &str) -> &[String] {
        self.secrets
            .get(scope)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The account a project's agents use: its own choice, else the default account.
    pub fn account_of(&self, project_id: &str) -> Option<&String> {
        self.project_accounts
            .get(project_id)
            .or(self.default_account.as_ref())
    }

    /// How a project's sessions sign in: its own choice, signing in themselves without an account.
    pub fn sign_in_of(&self, project_id: &str) -> AccountSignIn {
        match self.account_of(project_id) {
            None => AccountSignIn::Session,
            Some(_) => self
                .project_sign_in
                .get(project_id)
                .copied()
                .unwrap_or_default(),
        }
    }

    pub fn add_secret(&mut self, scope: &str, name: &str) {
        let names = self.secrets.entry(scope.to_owned()).or_default();
        if !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
        }
    }

    pub fn remove_secret(&mut self, scope: &str, name: &str) {
        if let Some(names) = self.secrets.get_mut(scope) {
            names.retain(|n| n != name);
        }
    }

    /// `(scope, name)` of the secrets kept per isolation that aren't sign-in tokens, the
    /// default isolation's first, so its value wins where several have one.
    pub fn secrets_to_globalize(&self, default_scope: &str) -> Vec<(String, String)> {
        let mut scopes: Vec<&String> = self
            .secrets
            .keys()
            .filter(|s| !crate::secrets::is_managed_scope(s))
            .collect();
        scopes.sort_by_key(|s| *s != default_scope);
        scopes
            .into_iter()
            .flat_map(|scope| {
                self.secret_names(scope)
                    .iter()
                    .filter(|n| !crate::agents::is_sign_in_secret(n))
                    .map(move |n| (scope.clone(), n.clone()))
            })
            .collect()
    }

    /// Fills in install commands for known agents saved before installs existed, and replaces
    /// worki's earlier Codex install through npm, which images without Node can't run.
    pub fn with_known_installers(mut self) -> Self {
        for p in &mut self.profiles {
            if p.install.is_none() || p.install.as_deref() == Some(OLD_CODEX_INSTALLER) {
                p.install = p.executable().and_then(known_installer).map(Into::into);
            }
        }
        self
    }

    /// Claude Code and Codex, which are always offered: added back in front when missing.
    pub fn with_built_in_agents(mut self) -> Self {
        let built_ins = Self::default().profiles;
        let missing: Vec<AgentProfile> = built_ins
            .into_iter()
            .filter(|b| !self.profiles.iter().any(|p| p.id == b.id))
            .collect();
        self.profiles.splice(0..0, missing);
        self
    }

    pub fn profile(&self, id: &str) -> Option<&AgentProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn default_profile(&self) -> Option<&AgentProfile> {
        self.default_profile
            .as_deref()
            .and_then(|id| self.profile(id))
            .or(self.profiles.first())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IsolationSettings {
    /// Used by projects whose isolation is `inherit`.
    pub default: Isolation,
    /// Stop worktree containers and sandboxes when worki quits instead of leaving them running.
    #[serde(default)]
    pub stop_on_quit: bool,
    /// CPUs and memory for new containers and sandboxes; projects can override them.
    #[serde(default)]
    pub resources: worki_isolation::Resources,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_old_codex_installer_only() {
        let saved = |install: &str| AgentSettings {
            profiles: vec![AgentProfile {
                id: "x".into(),
                name: "Codex".into(),
                command: "codex".into(),
                install: Some(install.into()),
            }],
            ..AgentSettings::default()
        };
        let old = saved(OLD_CODEX_INSTALLER).with_known_installers();
        assert_eq!(old.profiles[0].install.as_deref(), known_installer("codex"));
        let own = saved("brew install codex").with_known_installers();
        assert_eq!(
            own.profiles[0].install.as_deref(),
            Some("brew install codex")
        );
    }

    #[test]
    fn adds_missing_built_in_agents_in_front() {
        let saved: AgentSettings = serde_json::from_str(
            r#"{"profiles":[{"id":"codex","name":"Codex","command":"codex --yolo"},{"id":"a","name":"Aider","command":"aider"}],"defaultProfile":"a"}"#,
        )
        .unwrap();
        let agents: Vec<_> = saved
            .with_built_in_agents()
            .profiles
            .into_iter()
            .map(|p| (p.id, p.command))
            .collect();
        assert_eq!(
            agents,
            [
                ("claude".into(), "claude".into()),
                ("codex".into(), "codex --yolo".into()),
                ("a".into(), "aider".into())
            ]
        );
    }

    #[test]
    fn backfills_installers_for_known_agents() {
        let old: AgentSettings = serde_json::from_str(
            r#"{"profiles":[{"id":"c","name":"C","command":"claude --model opus"},{"id":"x","name":"X","command":"aider"}],"defaultProfile":"c"}"#,
        )
        .unwrap();
        let s = old.with_known_installers();
        assert!(s.profiles[0]
            .install
            .as_deref()
            .unwrap()
            .contains("claude.ai/install.sh"));
        assert_eq!(s.profiles[1].install, None);
        assert_eq!(s.profiles[0].executable(), Some("claude"));
    }

    #[test]
    fn isolation_secrets_go_global_but_sign_in_tokens_stay() {
        let settings: AgentSettings = serde_json::from_str(
            r#"{"profiles":[],"defaultProfile":null,"secrets":{
                "docker":["GH_TOKEN","CLAUDE_CODE_OAUTH_TOKEN"],
                "dockerSandbox":["GH_TOKEN","NPM_TOKEN"],
                "global":["A"],
                "project:p1":["B"]}}"#,
        )
        .unwrap();
        assert_eq!(
            settings.secrets_to_globalize("dockerSandbox"),
            [
                ("dockerSandbox".into(), "GH_TOKEN".into()),
                ("dockerSandbox".into(), "NPM_TOKEN".into()),
                ("docker".into(), "GH_TOKEN".into()),
            ] as [(String, String); 3]
        );
    }

    #[test]
    fn reads_old_global_secret_list() {
        let old: AgentSettings =
            serde_json::from_str(r#"{"profiles":[],"defaultProfile":null,"secrets":["GH_TOKEN"]}"#)
                .unwrap();
        assert_eq!(old.secret_names(LEGACY_SCOPE), ["GH_TOKEN"]);
        let new: AgentSettings = serde_json::from_str(
            r#"{"profiles":[],"defaultProfile":null,"secrets":{"none":["A"]}}"#,
        )
        .unwrap();
        assert_eq!(new.secret_names("none"), ["A"]);
        assert!(new.secret_names("macUser:_worki").is_empty());
    }
}
