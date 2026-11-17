use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::config::RepoConfig;
use crate::model::*;
use worki_isolation::{Backend, LaunchRequest, Outdated, Workspace};

use crate::openers::{self, OpenerSettings};
use crate::setup::{run_captured, run_logged};
use crate::store::Store;
use crate::{agents, git, github, paths, secrets, Error, Host, Result};

const AGENT_SETTINGS_KEY: &str = "agents";
const ISOLATION_SETTINGS_KEY: &str = "isolation";
const OPENER_SETTINGS_KEY: &str = "openers";
const SSH_HOSTS_KEY: &str = "sshHosts";
const SAVED_SESSIONS_KEY: &str = "agentSessions";
/// Per-worktree isolation choices, keyed by worktree path.
const WORKTREE_ISOLATION_KEY: &str = "worktreeIsolation";
const SNAPSHOTS_KEY: &str = "snapshots";
const KEEP_AWAKE_KEY: &str = "keepAwake";
const SNAPSHOT_CLEANUP_KEY: &str = "snapshotCleanup";
/// [`ProjectEnvironment`]s by project id, where they aren't the default.
const PROJECT_ENVIRONMENTS_KEY: &str = "projectEnvironments";

type WorktreeChoices = BTreeMap<String, ProjectIsolation>;

/// Where Claude Code keeps its files for a project's sessions, and how they sign in.
#[derive(Debug, Clone)]
pub struct ClaudeHome {
    /// The account sessions sign in with; `None` when they sign in themselves.
    pub account: Option<String>,
    pub sign_in: AccountSignIn,
    /// Claude Code's config folder (`CLAUDE_CONFIG_DIR`): the project's or the worktree's.
    pub dir: PathBuf,
    /// The account's login folder (`CLAUDE_SECURESTORAGE_CONFIG_DIR`), when sessions sign in with
    /// the account's login.
    pub login_dir: Option<PathBuf>,
}

pub struct ProjectService {
    store: Store,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedWorktree {
    pub worktree: Worktree,
    pub setup_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupScripts {
    pub project: Option<String>,
    pub repo_file: Option<String>,
}

impl SetupScripts {
    /// The project's own script wins over the shared `.worki.toml`.
    pub fn effective(&self) -> Option<&str> {
        self.project
            .as_deref()
            .or(self.repo_file.as_deref())
            .filter(|s| !s.trim().is_empty())
    }
}

impl ProjectService {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    pub fn open_default() -> Result<Self> {
        Ok(Self::new(Store::open(&paths::data_dir().join("worki.db"))?))
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        self.store.list_projects()
    }

    pub fn project(&self, id: &str) -> Result<Project> {
        self.store.get_project(id)
    }

    pub async fn add_project(&self, path: &Path) -> Result<Project> {
        self.add_project_on(Host::Local, None, path).await
    }

    /// Adds a repository that lives on `remote.host`; `path` is a path on that host.
    pub async fn add_remote_project(&self, remote: Remote, path: &Path) -> Result<Project> {
        let remote = self.with_saved_host(remote)?;
        remote.target().validate().map_err(Error::isolation)?;
        self.add_project_on(Host::Ssh(remote.target()), Some(remote), path)
            .await
    }

    async fn add_project_on(
        &self,
        host: Host,
        remote: Option<Remote>,
        path: &Path,
    ) -> Result<Project> {
        let path = host.expand_tilde(path).await?;
        let repo = git::main_checkout(&host, &path).await?;
        if self
            .store
            .find_by_repo(
                &repo,
                remote.as_ref().map(|r| r.target().display()).as_deref(),
            )?
            .is_some()
        {
            return Err(Error::ProjectExists(repo));
        }
        let name = repo
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into());
        let config_base = RepoConfig::load(&host, &repo)
            .await
            .ok()
            .flatten()
            .and_then(|c| c.worktree.base);
        let default_base = match config_base {
            Some(base) => base,
            None => git::default_base(&host, &repo).await,
        };
        let worktree_root = match &host {
            Host::Local => paths::default_worktree_root(&name),
            Host::Ssh(_) => host
                .home()
                .await?
                .join("worki/worktrees")
                .join(paths::slug(&name)),
        };
        let project = Project {
            id: uuid::Uuid::new_v4().to_string(),
            worktree_root,
            name,
            repo_path: repo,
            default_base,
            isolation: ProjectIsolation::Inherit,
            setup_script: None,
            remote,
        };
        self.store.insert_project(&project)?;
        Ok(project)
    }

    pub fn update_project(&self, id: &str, patch: ProjectPatch) -> Result<Project> {
        let mut p = self.store.get_project(id)?;
        if let Some(name) = patch.name.filter(|n| !n.trim().is_empty()) {
            p.name = name.trim().into();
        }
        if let Some(root) = patch.worktree_root {
            p.worktree_root = if p.remote.is_some() {
                root
            } else {
                paths::expand_tilde(&root)
            };
        }
        if let Some(base) = patch.default_base.filter(|b| !b.trim().is_empty()) {
            p.default_base = base.trim().into();
        }
        if let Some(isolation) = patch.isolation {
            isolation
                .resolve(&Isolation::None)
                .validate()
                .map_err(Error::isolation)?;
            p.isolation = isolation;
        }
        if let Some(script) = patch.setup_script {
            p.setup_script = Some(script).filter(|s| !s.trim().is_empty());
        }
        if let (Some(remote), true) = (patch.remote, p.remote.is_some()) {
            let remote = self.with_saved_host(remote)?;
            remote.target().validate().map_err(Error::isolation)?;
            p.remote = Some(remote);
        }
        self.store.update_project(&p)?;
        Ok(p)
    }

    /// Removes the project from worki and cleans up its isolation (environments, access
    /// grants). When the isolation can't be reached (engine stopped or gone), the project is still
    /// removed and the cleanup is skipped with a warning.
    pub async fn remove_project(&self, id: &str) -> Result<Option<String>> {
        let project = self.store.get_project(id)?;
        let isolations = self.project_isolations(&project)?;
        let warnings = self.forget_environments(&project, isolations).await?;
        let mut choices = self.worktree_choices()?;
        choices.retain(|path, _| !project.contains(Path::new(path)));
        self.save_worktree_choices(&choices)?;
        let scope = secrets::project_scope(id);
        for name in self.agent_settings()?.secret_names(&scope) {
            secrets::delete(&scope, name)?;
        }
        self.update_agent_settings(|s| {
            s.project_accounts.remove(id);
            s.project_sign_in.remove(id);
            s.secrets.remove(&scope);
        })?;
        self.set_project_environment(id, ProjectEnvironment::default())?;
        let own = paths::project_dir(id);
        if own.exists() {
            std::fs::remove_dir_all(&own)?;
        }
        self.store.delete_project(id)?;
        Ok((!warnings.is_empty()).then(|| warnings.join("\n")))
    }

    /// The isolations the project's worktrees use: the project's own and worktree choices.
    fn project_isolations(&self, project: &Project) -> Result<Vec<Isolation>> {
        let global = self.isolation_settings()?.default;
        let mut isolations = vec![self.isolation_for(project)?];
        let worktree_isolations = self
            .worktree_choices()?
            .into_iter()
            .filter(|(path, choice)| {
                *choice != ProjectIsolation::Inherit && project.contains(Path::new(path))
            })
            .map(|(_, choice)| choice.resolve(&global));
        push_unique(&mut isolations, worktree_isolations);
        isolations.retain(|i| *i != Isolation::None);
        Ok(isolations)
    }

    /// Removes the project's environments in `isolations`. Isolations that can't be reached are
    /// skipped, with a warning each.
    async fn forget_environments(
        &self,
        project: &Project,
        isolations: Vec<Isolation>,
    ) -> Result<Vec<String>> {
        let mut warnings = Vec::new();
        for isolation in isolations {
            let ws = Self::workspace(project, None);
            let label = isolation.label();
            let skipped = blocking(move || {
                let backend = isolation.backend();
                if let Err(e) = backend.ready() {
                    return Ok(Some(format!("Skipped cleaning up {label}: {e:#}")));
                }
                backend.forget_project(&ws).map(|_| None)
            })
            .await?;
            warnings.extend(skipped);
        }
        Ok(warnings)
    }

    /// A new account, to sign in next; the first one becomes the default.
    pub fn add_account(&self) -> Result<ClaudeAccount> {
        let account = ClaudeAccount {
            id: uuid::Uuid::new_v4().simple().to_string()[..12].to_owned(),
            ..ClaudeAccount::default()
        };
        self.update_agent_settings(|s| {
            s.accounts.push(account.clone());
            s.default_account.get_or_insert_with(|| account.id.clone());
        })?;
        Ok(account)
    }

    pub fn account(&self, id: &str) -> Result<ClaudeAccount> {
        self.agent_settings()?
            .accounts
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| Error::AccountNotFound(id.into()))
    }

    /// Saves who the account signs in as.
    pub fn save_account(&self, account: &ClaudeAccount) -> Result<()> {
        self.update_agent_settings(|s| {
            if let Some(saved) = s.accounts.iter_mut().find(|a| a.id == account.id) {
                *saved = account.clone();
            }
        })
    }

    /// Removes the account and its login. Projects that used it fall back to the default
    /// account (another one, when it was the default), and their environments are removed so
    /// none keeps the deleted login mounted.
    pub async fn remove_account(&self, id: &str) -> Result<Vec<String>> {
        let mut users = Vec::new();
        for project in self.list_projects()? {
            if self.project_account(&project.id)?.as_deref() == Some(id) {
                users.push(project);
            }
        }
        self.update_agent_settings(|s| {
            s.accounts.retain(|a| a.id != id);
            s.project_accounts.retain(|_, account| account != id);
            if s.default_account.as_deref() == Some(id) {
                s.default_account = s.accounts.first().map(|a| a.id.clone());
            }
        })?;
        let dir = crate::paths::account_dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        if let Some(secret) = agents::claude_sign_in_secret() {
            secrets::set_account_token(id, secret, None)?;
        }
        let mut warnings = Vec::new();
        for project in &users {
            let mut isolations = self.project_isolations(project)?;
            isolations.retain(Isolation::is_container);
            warnings.extend(self.forget_environments(project, isolations).await?);
        }
        Ok(warnings)
    }

    pub fn set_default_account(&self, id: &str) -> Result<()> {
        self.account(id)?;
        self.update_agent_settings(|s| s.default_account = Some(id.to_owned()))
    }

    /// The account the project's agents use: its own choice, else the default account.
    pub fn project_account(&self, project_id: &str) -> Result<Option<String>> {
        Ok(self.agent_settings()?.account_of(project_id).cloned())
    }

    /// Picks the account the project's agents use (`None`: the default account). Its login folder
    /// is mounted when an environment is created, so each worktree's environment is replaced at
    /// a session start when nothing runs in it (see [`Self::grant_access_with`]).
    pub fn set_project_account(&self, project_id: &str, account: Option<String>) -> Result<()> {
        self.update_agent_settings(|s| match account {
            Some(id) => {
                s.project_accounts.insert(project_id.to_owned(), id);
            }
            None => {
                s.project_accounts.remove(project_id);
            }
        })
    }

    pub fn set_project_sign_in(&self, project_id: &str, sign_in: AccountSignIn) -> Result<()> {
        self.update_agent_settings(|s| match sign_in {
            AccountSignIn::Folder => {
                s.project_sign_in.remove(project_id);
            }
            other => {
                s.project_sign_in.insert(project_id.to_owned(), other);
            }
        })
    }

    pub fn project_environment(&self, project_id: &str) -> Result<ProjectEnvironment> {
        let mut all: BTreeMap<String, ProjectEnvironment> =
            self.setting(PROJECT_ENVIRONMENTS_KEY)?;
        Ok(all.remove(project_id).unwrap_or_default())
    }

    /// Takes effect in each worktree's environment at its next session start while nothing runs
    /// in it (see [`Self::grant_access_with`]).
    pub fn set_project_environment(&self, project_id: &str, env: ProjectEnvironment) -> Result<()> {
        let mut all: BTreeMap<String, ProjectEnvironment> =
            self.setting(PROJECT_ENVIRONMENTS_KEY)?;
        if env == ProjectEnvironment::default() {
            all.remove(project_id);
        } else {
            all.insert(project_id.to_owned(), env);
        }
        self.store.set_setting(PROJECT_ENVIRONMENTS_KEY, &all)
    }

    /// Where Claude Code keeps its files for `project`'s sessions in `worktree` with `isolation`,
    /// and how they sign in; `None` outside containers, where they use the environment's own. The
    /// config folder is the project's own, or the worktree's, never one other projects use; with
    /// the account's login only the login is shared. Without an account, sessions sign in
    /// themselves, and each worktree keeps its folder and that login to itself. Each folder
    /// starts from the account's (or a worktree's from the project's) on first use.
    pub fn claude_home(
        &self,
        project: &Project,
        worktree: Option<&Path>,
        isolation: &Isolation,
    ) -> Result<Option<ClaudeHome>> {
        if !isolation.is_container() {
            return Ok(None);
        }
        let settings = self.agent_settings()?;
        let sign_in = settings.sign_in_of(&project.id);
        let account = settings
            .account_of(&project.id)
            .filter(|_| sign_in != AccountSignIn::Session)
            .cloned();
        let account_dir = account.as_deref().map(paths::account_dir);
        let mut dir = paths::project_claude_dir(&project.id);
        let project_paths = [project.repo_path.as_path(), project.worktree_root.as_path()];
        agents::start_claude_dir(&dir, account_dir.as_deref(), &project_paths)
            .map_err(Error::isolation)?;
        let scope = match sign_in {
            AccountSignIn::Session => FilesScope::Worktree,
            _ => self.project_environment(&project.id)?.claude_files,
        };
        if let (FilesScope::Worktree, Some(worktree)) = (scope, worktree) {
            let own = paths::worktree_dir(&project.id, worktree).join("claude");
            agents::start_claude_dir(&own, Some(&dir), &[worktree, &project.repo_path])
                .map_err(Error::isolation)?;
            dir = own;
        }
        let login_dir = match (sign_in, &account) {
            (AccountSignIn::Folder, Some(account)) => {
                Some(self.ready_login(&settings, account, &dir)?)
            }
            _ => None,
        };
        Ok(Some(ClaudeHome {
            account,
            sign_in,
            dir,
            login_dir,
        }))
    }

    /// The Claude Code config folders sessions signing in with `account`'s login use: those of
    /// the projects on it, and of their worktrees that have their own.
    fn login_configs(&self, settings: &AgentSettings, account: &str) -> Result<Vec<PathBuf>> {
        let mut configs = Vec::new();
        for project in self.list_projects()? {
            let on_login = settings.account_of(&project.id).map(String::as_str) == Some(account)
                && settings.sign_in_of(&project.id) == AccountSignIn::Folder;
            if !on_login {
                continue;
            }
            configs.push(paths::project_claude_dir(&project.id));
            let worktrees = paths::project_dir(&project.id).join("worktrees");
            for entry in std::fs::read_dir(worktrees).into_iter().flatten().flatten() {
                configs.push(entry.path().join("claude"));
            }
        }
        Ok(configs)
    }

    /// After signing `account` in through worki with `config` as Claude Code's folder: keeps who
    /// it signed in as next to the login, and gives that to the config folders sessions on the
    /// login use, so they don't hold on to who it was before.
    pub fn keep_signed_in_identity(&self, account: &str, config: &Path) -> Result<()> {
        let login =
            agents::account_login_dir(&paths::account_dir(account)).map_err(Error::isolation)?;
        agents::keep_login_identity(&login, config).map_err(Error::isolation)?;
        let configs = self.login_configs(&self.agent_settings()?, account)?;
        agents::give_login_identity(&login, &configs).map_err(Error::isolation)
    }

    /// The account's login folder for a session using the config folder `dir`: who the login is
    /// brought in step first, in case a session signed in again itself, then given to `dir`.
    fn ready_login(&self, settings: &AgentSettings, account: &str, dir: &Path) -> Result<PathBuf> {
        let login =
            agents::account_login_dir(&paths::account_dir(account)).map_err(Error::isolation)?;
        let configs = self.login_configs(settings, account)?;
        if let Some(identity) =
            agents::sync_login_identity(&login, &configs).map_err(Error::isolation)?
        {
            self.follow_identity(account, &identity)?;
        }
        agents::copy_login_identity(&login, dir).map_err(Error::isolation)?;
        Ok(login)
    }

    /// Updates the account to who its login is after a session signed in again itself.
    fn follow_identity(&self, account: &str, identity: &agents::Identity) -> Result<()> {
        let mut saved = self.account(account)?;
        let profile = &identity["oauthAccount"];
        saved.email = profile["emailAddress"].as_str().map(str::to_owned);
        saved.organization = profile["organizationName"].as_str().map(str::to_owned);
        self.save_account(&saved)
    }

    /// Moves what projects need out of the account folders, which older versions used as every
    /// project's config folder, and empties them down to the login. Once per account; call it
    /// before any session starts.
    pub fn tidy_account_folders(&self) -> Result<()> {
        let settings = self.agent_settings()?;
        let projects = self.list_projects()?;
        for account in &settings.accounts {
            let users: Vec<(PathBuf, Vec<&Path>)> = projects
                .iter()
                .filter(|p| settings.account_of(&p.id) == Some(&account.id))
                .map(|p| {
                    let paths = vec![p.repo_path.as_path(), p.worktree_root.as_path()];
                    (paths::project_claude_dir(&p.id), paths)
                })
                .collect();
            agents::tidy_account_dir(&paths::account_dir(&account.id), &users)
                .map_err(Error::isolation)?;
        }
        Ok(())
    }

    pub fn reorder_projects(&self, ids: &[String]) -> Result<()> {
        self.store.reorder_projects(ids)
    }

    pub fn agent_settings(&self) -> Result<AgentSettings> {
        let settings: AgentSettings = self.setting(AGENT_SETTINGS_KEY)?;
        Ok(settings.with_built_in_agents().with_known_installers())
    }

    /// Moves secrets stored the ways older versions did: one unscoped list, then per isolation.
    /// Call it once at startup.
    pub fn migrate_secrets(&self) -> Result<()> {
        let mut settings: AgentSettings = self.setting(AGENT_SETTINGS_KEY)?;
        let default_scope = self.isolation_settings()?.default.scope_key();
        let mut changed = false;
        if let Some(names) = settings.secrets.remove(LEGACY_SCOPE) {
            for name in &names {
                secrets::adopt_legacy(&default_scope, name)?;
                settings.add_secret(&default_scope, name);
            }
            changed = true;
        }
        changed |= Self::move_secrets_to_global(&mut settings, &default_scope)?;
        if changed {
            self.save_agent_settings(&settings)?;
        }
        Ok(())
    }

    /// Secrets used to be kept per isolation. Those you manage move to the global ones; sign-in
    /// tokens stay with their isolation.
    fn move_secrets_to_global(settings: &mut AgentSettings, default_scope: &str) -> Result<bool> {
        let moves = settings.secrets_to_globalize(default_scope);
        for (scope, name) in &moves {
            secrets::adopt(scope, secrets::GLOBAL, name)?;
            settings.remove_secret(scope, name);
            settings.add_secret(secrets::GLOBAL, name);
        }
        settings.secrets.retain(|_, names| !names.is_empty());
        Ok(!moves.is_empty())
    }

    /// The environment `worktree`'s agents run in (call after `grant_access`).
    pub async fn agent_instance(
        &self,
        project: &Project,
        worktree: &Path,
        isolation: &Isolation,
    ) -> Result<Arc<dyn Backend>> {
        let backend = isolation.backend();
        let ws = Self::workspace(project, Some(worktree));
        blocking(move || Ok(Arc::from(backend.instance_for(&ws).unwrap_or(backend)))).await
    }

    /// Makes sure `executable` runs in `instance`: when it's missing there, calls `installing` and
    /// installs it with its known installer. Returns whether it installed it.
    pub async fn ensure_agent(
        &self,
        instance: Arc<dyn Backend>,
        isolation: &Isolation,
        executable: &str,
        log: impl Fn(LogLine) + Send + Sync + Clone + 'static,
    ) -> Result<bool> {
        let (check, exe) = (instance.clone(), executable.to_owned());
        if blocking(move || Ok(check.which(&exe).is_some())).await? {
            return Ok(false);
        }
        let label = isolation.label();
        if !instance.capabilities().install {
            return Err(Error::Isolation(format!(
                "`{executable}` isn't installed for {label}. Open Isolation and use “Use my installation” under Agents."
            )));
        }
        log(LogLine::Info(format!(
            "`{executable}` isn't installed for {label} yet. Installing it…"
        )));
        let output = Arc::new(std::sync::Mutex::new(Vec::new()));
        let lines = output.clone();
        self.install_in(isolation, instance.as_ref(), executable, move |line| {
            log(line.clone());
            let (LogLine::Info(text)
            | LogLine::Stdout(text)
            | LogLine::Stderr(text)
            | LogLine::Command(text)) = line;
            let mut lines = lines.lock().unwrap();
            lines.push(text);
            let excess = lines.len().saturating_sub(10);
            lines.drain(..excess);
        })
        .await
        .map_err(|e| {
            let tail = output.lock().unwrap().join("\n");
            Error::Isolation(format!(
                "Couldn't install `{executable}` for {label}: {e}\n{tail}"
            ))
        })?;
        Ok(true)
    }

    /// Answers `executable`'s trust question for `worktree` in `instance` (see
    /// `agents::trust_worktree`).
    pub async fn trust_worktree(
        &self,
        instance: Arc<dyn Backend>,
        executable: &str,
        worktree: &Path,
    ) -> Result<()> {
        let (exe, worktree) = (executable.to_owned(), worktree.to_owned());
        blocking(move || crate::agents::trust_worktree(instance.as_ref(), &exe, &worktree)).await
    }

    async fn install_in(
        &self,
        isolation: &Isolation,
        instance: &dyn Backend,
        command: &str,
        log: impl Fn(LogLine) + Send + Sync + Clone + 'static,
    ) -> Result<()> {
        let settings = self.agent_settings()?;
        let script = settings
            .profiles
            .iter()
            .find(|p| p.executable() == Some(command))
            .and_then(|p| p.install.clone())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| Error::NoInstaller(command.into()))?;
        log(LogLine::Info(format!("$ {script}")));
        let req = LaunchRequest {
            cwd: "/tmp".into(),
            command: Some(script),
            interactive: false,
            stdin: false,
            env: self.secret_env(None, isolation)?,
            session: None,
        };
        let launch = instance.launch(&req).map_err(Error::isolation)?;
        run_logged(launch, log).await
    }

    pub fn save_agent_settings(&self, settings: &AgentSettings) -> Result<()> {
        self.store.set_setting(AGENT_SETTINGS_KEY, settings)
    }

    pub fn update_agent_settings(&self, change: impl FnOnce(&mut AgentSettings)) -> Result<()> {
        let mut settings = self.agent_settings()?;
        change(&mut settings);
        self.save_agent_settings(&settings)
    }

    fn setting<T: DeserializeOwned + Default>(&self, key: &str) -> Result<T> {
        Ok(self.store.get_setting(key)?.unwrap_or_default())
    }

    /// Apps worktrees can be opened in. Filled with the installed well-known apps the first time.
    pub fn opener_settings(&self) -> Result<OpenerSettings> {
        if let Some(settings) = self.store.get_setting(OPENER_SETTINGS_KEY)? {
            return Ok(settings);
        }
        let apps = openers::detect_installed();
        let default_app = apps.first().map(|a| a.id.clone());
        let settings = OpenerSettings { apps, default_app };
        self.save_opener_settings(&settings)?;
        Ok(settings)
    }

    pub fn save_opener_settings(&self, settings: &OpenerSettings) -> Result<()> {
        self.store.set_setting(OPENER_SETTINGS_KEY, settings)
    }

    pub fn saved_sessions(&self) -> Result<Vec<SavedSession>> {
        self.setting(SAVED_SESSIONS_KEY)
    }

    pub fn save_session(&self, session: SavedSession) -> Result<()> {
        let mut saved = self.saved_sessions()?;
        saved.retain(|s| s.id != session.id);
        saved.push(session);
        self.store.set_setting(SAVED_SESSIONS_KEY, &saved)
    }

    pub fn forget_session(&self, id: &str) -> Result<()> {
        let mut saved = self.saved_sessions()?;
        let before = saved.len();
        saved.retain(|s| s.id != id);
        if saved.len() == before {
            return Ok(());
        }
        self.store.set_setting(SAVED_SESSIONS_KEY, &saved)
    }

    /// Forgets the sessions saved for `worktree`, so a new worktree at the same path starts fresh.
    pub fn forget_sessions_in(&self, worktree: &Path) -> Result<()> {
        let mut saved = self.saved_sessions()?;
        let before = saved.len();
        saved.retain(|s| s.worktree_path != worktree);
        if saved.len() == before {
            return Ok(());
        }
        self.store.set_setting(SAVED_SESSIONS_KEY, &saved)
    }

    pub fn ssh_hosts(&self) -> Result<Vec<SshHost>> {
        self.setting(SSH_HOSTS_KEY)
    }

    pub fn ssh_host(&self, id: &str) -> Result<SshHost> {
        self.ssh_hosts()?
            .into_iter()
            .find(|h| h.id == id)
            .ok_or_else(|| Error::SshHostNotFound(id.into()))
    }

    /// Adds or replaces a saved host (a new one when `id` is empty) and updates the projects
    /// connecting through it.
    pub fn save_ssh_host(&self, mut host: SshHost) -> Result<SshHost> {
        host.name = host.name.trim().into();
        host.host = host.host.trim().into();
        host.user = host.user.filter(|u| !u.trim().is_empty());
        if host.name.is_empty() {
            host.name = host.host.clone();
        }
        host.target().validate().map_err(Error::isolation)?;
        if host.id.is_empty() {
            host.id = uuid::Uuid::new_v4().to_string();
        }
        let mut hosts = self.ssh_hosts()?;
        match hosts.iter_mut().find(|h| h.id == host.id) {
            Some(existing) => *existing = host.clone(),
            None => hosts.push(host.clone()),
        }
        self.store.set_setting(SSH_HOSTS_KEY, &hosts)?;
        for mut p in self.store.list_projects()? {
            if let Some(remote) = p
                .remote
                .as_mut()
                .filter(|r| r.host_id.as_deref() == Some(&host.id))
            {
                host.apply(remote);
                self.store.update_project(&p)?;
            }
        }
        Ok(host)
    }

    /// Removes a saved host. Projects using it keep its connection settings as their own.
    pub fn delete_ssh_host(&self, id: &str) -> Result<()> {
        let mut hosts = self.ssh_hosts()?;
        hosts.retain(|h| h.id != id);
        self.store.set_setting(SSH_HOSTS_KEY, &hosts)?;
        for mut p in self.store.list_projects()? {
            if let Some(remote) = p
                .remote
                .as_mut()
                .filter(|r| r.host_id.as_deref() == Some(id))
            {
                remote.host_id = None;
                self.store.update_project(&p)?;
            }
        }
        Ok(())
    }

    fn with_saved_host(&self, mut remote: Remote) -> Result<Remote> {
        if let Some(id) = remote.host_id.clone() {
            self.ssh_host(&id)?.apply(&mut remote);
        }
        Ok(remote)
    }

    /// Keep the Mac awake while agent sessions are open.
    pub fn keep_awake(&self) -> Result<bool> {
        self.setting(KEEP_AWAKE_KEY)
    }

    pub fn set_keep_awake(&self, on: bool) -> Result<()> {
        self.store.set_setting(KEEP_AWAKE_KEY, &on)
    }

    /// Script run inside an environment before each snapshot of it is saved.
    pub fn snapshot_cleanup(&self) -> Result<String> {
        self.setting(SNAPSHOT_CLEANUP_KEY)
    }

    pub fn set_snapshot_cleanup(&self, script: &str) -> Result<()> {
        self.store.set_setting(SNAPSHOT_CLEANUP_KEY, &script)
    }

    pub fn isolation_settings(&self) -> Result<IsolationSettings> {
        self.setting(ISOLATION_SETTINGS_KEY)
    }

    pub fn save_isolation_settings(&self, settings: &IsolationSettings) -> Result<()> {
        self.store.set_setting(ISOLATION_SETTINGS_KEY, settings)
    }

    pub fn isolation_for(&self, project: &Project) -> Result<Isolation> {
        if let Some(remote) = &project.remote {
            return Ok(Isolation::Ssh {
                host: remote.host.clone(),
                user: remote.user.clone(),
                port: remote.port,
                keep_alive: remote.keep_alive,
            });
        }
        Ok(project
            .isolation
            .resolve(&self.isolation_settings()?.default))
    }

    fn worktree_choices(&self) -> Result<WorktreeChoices> {
        self.setting(WORKTREE_ISOLATION_KEY)
    }

    fn save_worktree_choices(&self, choices: &WorktreeChoices) -> Result<()> {
        self.store.set_setting(WORKTREE_ISOLATION_KEY, choices)
    }

    /// Stores the worktree's own choice; `Inherit` forgets it.
    fn set_worktree_choice(&self, worktree: &Path, choice: ProjectIsolation) -> Result<()> {
        let mut choices = self.worktree_choices()?;
        let key = path_key(worktree);
        let changed = if choice == ProjectIsolation::Inherit {
            choices.remove(&key).is_some()
        } else {
            choices.insert(key, choice);
            true
        };
        if changed {
            self.save_worktree_choices(&choices)?;
        }
        Ok(())
    }

    /// The worktree's own choice; `Inherit` means "project default".
    pub fn worktree_isolation_choice(&self, worktree: &Path) -> Result<ProjectIsolation> {
        Ok(self
            .worktree_choices()?
            .remove(&path_key(worktree))
            .unwrap_or_default())
    }

    /// Isolation a worktree runs in: its own choice, else the project default, else the global
    /// default. Remote projects always use their SSH connection.
    /// Saves the worktree's container as an image or sbx template named `tag`, and with
    /// `use_for_project` makes the project create new worktree environments from it.
    pub async fn save_environment_template(
        &self,
        project_id: &str,
        worktree: &Path,
        tag: &str,
        use_for_project: bool,
        log: impl Fn(LogLine) + Send + Sync + Clone + 'static,
    ) -> Result<Project> {
        let project = self.store.get_project(project_id)?;
        let isolation = self.isolation_for_worktree(&project, worktree)?;
        if !isolation.is_container() {
            return Err(Error::Isolation(
                "only container isolations can be saved as a snapshot".into(),
            ));
        }
        let cleanup = self.snapshot_cleanup()?;
        if !cleanup.trim().is_empty() {
            log(LogLine::Info("Running the cleanup script…".into()));
            self.grant_access(&project, Some(worktree)).await?;
            let script = format!("cd ~ || exit 1\n{cleanup}");
            run_setup_script(&isolation, worktree, &script, Vec::new(), log.clone())
                .await
                .map_err(|e| Error::Isolation(format!("the cleanup script failed: {e}")))?;
        }
        log(LogLine::Info(
            "Saving the worktree's environment as an image…".into(),
        ));
        let backend = isolation.backend();
        let worktree = worktree.to_path_buf();
        let tag = tag.trim().to_owned();
        let saved = blocking(move || backend.save_template(&worktree, &tag)).await?;
        if let Some(snapshot) = SnapshotEngine::snapshot_of(&saved) {
            let mut recorded: Vec<(SnapshotEngine, String)> = self.setting(SNAPSHOTS_KEY)?;
            if !recorded.contains(&snapshot) {
                recorded.push(snapshot);
                self.store.set_setting(SNAPSHOTS_KEY, &recorded)?;
            }
        }
        if !use_for_project {
            return Ok(project);
        }
        let isolation = match saved {
            Isolation::DockerSandbox { template } => ProjectIsolation::DockerSandbox { template },
            Isolation::Docker { image } => ProjectIsolation::Docker { image },
            Isolation::Podman { image } => ProjectIsolation::Podman { image },
            other => {
                return Err(Error::Isolation(format!(
                    "{} can't be a project default",
                    other.label()
                )))
            }
        };
        self.update_project(
            project_id,
            ProjectPatch {
                isolation: Some(isolation),
                ..Default::default()
            },
        )
    }

    pub async fn environment_templates(&self, isolation: Isolation) -> Result<Vec<String>> {
        blocking(move || isolation.backend().templates()).await
    }

    /// Snapshots on this machine: ones worki saved or named `worki-…`, and ones a setting starts
    /// environments from, newest first. Engines that aren't running are left out.
    pub async fn snapshots(&self) -> Result<Vec<Snapshot>> {
        let users = self.snapshot_users()?;
        let recorded: Vec<(SnapshotEngine, String)> = self.setting(SNAPSHOTS_KEY)?;
        let found = blocking(|| {
            Ok(SnapshotEngine::ALL
                .into_iter()
                .flat_map(|engine| {
                    let templates = engine
                        .isolation()
                        .backend()
                        .template_details()
                        .unwrap_or_default();
                    templates.into_iter().map(move |t| (engine, t))
                })
                .collect::<Vec<_>>())
        })
        .await?;
        let mut snapshots: Vec<Snapshot> = found
            .into_iter()
            .filter(|(engine, t)| {
                let key = (*engine, t.name.clone());
                (t.name.starts_with("worki-") && t.name != worki_isolation::DEFAULT_CONTAINER_IMAGE)
                    || recorded.contains(&key)
                    || users.iter().any(|(used, _)| *used == key)
            })
            .map(|(engine, t)| Snapshot {
                used_by: users
                    .iter()
                    .filter(|((e, n), _)| *e == engine && *n == t.name)
                    .map(|(_, by)| by.clone())
                    .collect(),
                engine,
                name: t.name,
                created: t.created,
                size: t.size,
            })
            .collect();
        snapshots
            .sort_by(|a, b| (b.created, &b.name, b.engine).cmp(&(a.created, &a.name, a.engine)));
        snapshots.dedup_by(|a, b| a.engine == b.engine && a.name == b.name);
        Ok(snapshots)
    }

    /// Removes a snapshot no setting starts environments from anymore.
    pub async fn delete_snapshot(&self, engine: SnapshotEngine, name: &str) -> Result<()> {
        let key = (engine, name.to_owned());
        let users: Vec<String> = self
            .snapshot_users()?
            .into_iter()
            .filter(|(used, _)| *used == key)
            .map(|(_, by)| by)
            .collect();
        if !users.is_empty() {
            return Err(Error::Isolation(format!(
                "{name} is still used by {}; pick another isolation there first",
                users.join(", ")
            )));
        }
        let tag = name.to_owned();
        blocking(move || engine.isolation().backend().delete_template(&tag)).await?;
        let mut recorded: Vec<(SnapshotEngine, String)> = self.setting(SNAPSHOTS_KEY)?;
        recorded.retain(|r| *r != key);
        self.store.set_setting(SNAPSHOTS_KEY, &recorded)
    }

    /// Every setting that starts new environments from a snapshot, with who it belongs to.
    fn snapshot_users(&self) -> Result<Vec<((SnapshotEngine, String), String)>> {
        let global = self.isolation_settings()?.default;
        let projects: Vec<Project> = self
            .list_projects()?
            .into_iter()
            .filter(|p| p.remote.is_none())
            .collect();
        let mut users = Vec::new();
        users.extend(
            SnapshotEngine::snapshot_of(&global).map(|s| (s, "The default isolation".to_owned())),
        );
        for p in &projects {
            if p.isolation != ProjectIsolation::Inherit {
                users.extend(
                    SnapshotEngine::snapshot_of(&p.isolation.resolve(&global))
                        .map(|s| (s, p.name.clone())),
                );
            }
        }
        for (path, choice) in self.worktree_choices()? {
            let path = Path::new(&path);
            let Some(project) = projects.iter().find(|p| p.contains(path)) else {
                continue;
            };
            if choice != ProjectIsolation::Inherit {
                let worktree = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                users.extend(
                    SnapshotEngine::snapshot_of(&choice.resolve(&global))
                        .map(|s| (s, format!("{} › {worktree}", project.name))),
                );
            }
        }
        Ok(users)
    }

    pub fn isolation_for_worktree(&self, project: &Project, worktree: &Path) -> Result<Isolation> {
        if project.remote.is_some() {
            return self.isolation_for(project);
        }
        match self.worktree_isolation_choice(worktree)? {
            ProjectIsolation::Inherit => self.isolation_for(project),
            choice => Ok(choice.resolve(&self.isolation_settings()?.default)),
        }
    }

    /// Changes a worktree's isolation. The environment of the old isolation (e.g. its container)
    /// is removed; the next session sets up the new one.
    pub async fn set_worktree_isolation(
        &self,
        project_id: &str,
        worktree: &Path,
        choice: ProjectIsolation,
    ) -> Result<Isolation> {
        let project = self.store.get_project(project_id)?;
        choice
            .resolve(&Isolation::None)
            .validate()
            .map_err(Error::isolation)?;
        let old = self.isolation_for_worktree(&project, worktree)?;
        self.set_worktree_choice(worktree, choice)?;
        let new = self.isolation_for_worktree(&project, worktree)?;
        if old != new && old != Isolation::None {
            let ws = Self::workspace(&project, Some(worktree));
            let released = blocking(move || {
                let backend = old.backend();
                backend.ready()?;
                backend.release(&ws)
            })
            .await;
            if let Err(e) = released {
                tracing::warn!(
                    "removing the old environment of {}: {e}",
                    worktree.display()
                );
            }
        }
        Ok(new)
    }

    /// The host a worktree or repository path belongs to, from the projects that contain it.
    pub fn host_for_path(&self, path: &Path) -> Result<Host> {
        Ok(self
            .list_projects()?
            .iter()
            .filter(|p| p.remote.is_some())
            .find(|p| p.contains(path))
            .map(Host::of)
            .unwrap_or(Host::Local))
    }

    /// Where [`Self::create_worktree`] puts a new worktree named `name`.
    pub fn new_worktree_path(project: &Project, name: &str) -> PathBuf {
        paths::worktree_path(&project.worktree_root, &paths::branch_name(name))
    }

    pub fn workspace(project: &Project, worktree: Option<&Path>) -> Workspace {
        Workspace {
            repo: project.repo_path.clone(),
            worktree_root: project.worktree_root.clone(),
            worktree: worktree.map(Path::to_path_buf),
            claude_dir: None,
            claude_login_dir: None,
            shared_folder: None,
            outdated: Outdated::Keep,
        }
    }

    /// Gives the project's isolated environment access to `worktree` (and the git data it
    /// needs), nothing else. Blocking work runs off the async runtime.
    pub async fn grant_access(
        &self,
        project: &Project,
        worktree: Option<&Path>,
    ) -> Result<Isolation> {
        Ok(self
            .grant_access_with(project, worktree, Outdated::Keep)
            .await?
            .0)
    }

    /// [`Self::grant_access`], with `outdated` saying what to do when the worktree's environment
    /// was made with other Claude Code folders or project folder than it gets now. Returns the
    /// Claude Code folders sessions there use.
    pub async fn grant_access_with(
        &self,
        project: &Project,
        worktree: Option<&Path>,
        outdated: Outdated,
    ) -> Result<(Isolation, Option<ClaudeHome>)> {
        let isolation = match worktree {
            Some(w) => self.isolation_for_worktree(project, w)?,
            None => self.isolation_for(project)?,
        };
        let mut home = None;
        if isolation != Isolation::None {
            let backend = isolation.backend();
            home = self.claude_home(project, worktree, &isolation)?;
            let shared_folder = (isolation.is_container()
                && self.project_environment(&project.id)?.shared_folder)
                .then(|| paths::project_shared_dir(&project.id));
            if let Some(dir) = &shared_folder {
                std::fs::create_dir_all(dir)?;
            }
            let ws = Workspace {
                outdated,
                claude_login_dir: home.as_ref().and_then(|h| h.login_dir.clone()),
                claude_dir: home.as_ref().map(|h| h.dir.clone()),
                shared_folder,
                ..Self::workspace(project, worktree)
            };
            blocking(move || backend.grant_access(&ws)).await?;
        }
        Ok((isolation, home))
    }

    /// Every distinct isolation in use: the global default, project defaults and worktree choices.
    pub fn isolations_in_use(&self) -> Result<Vec<Isolation>> {
        let global = self.isolation_settings()?.default;
        let mut all = vec![global.clone()];
        let project_defaults = self
            .list_projects()?
            .iter()
            .map(|p| self.isolation_for(p))
            .collect::<Result<Vec<_>>>()?;
        push_unique(&mut all, project_defaults);
        let choices = self.worktree_choices()?;
        push_unique(
            &mut all,
            choices
                .values()
                .filter(|c| **c != ProjectIsolation::Inherit)
                .map(|c| c.resolve(&global)),
        );
        Ok(all)
    }

    /// The secrets agents of `project` in `isolation` get: the isolation's sign-in, then the
    /// global secrets, then the project's, each over the ones before with the same name.
    pub fn secret_env(
        &self,
        project_id: Option<&str>,
        isolation: &Isolation,
    ) -> Result<Vec<(String, String)>> {
        let settings = self.agent_settings()?;
        let mut scopes = vec![isolation.scope_key(), secrets::GLOBAL.to_owned()];
        scopes.extend(project_id.map(secrets::project_scope));
        let mut env: Vec<(String, String)> = Vec::new();
        for scope in &scopes {
            for (name, value) in secrets::env(scope, settings.secret_names(scope)) {
                env.retain(|(n, _)| *n != name);
                env.push((name, value));
            }
        }
        Ok(env)
    }

    pub async fn setup_scripts(&self, id: &str) -> Result<SetupScripts> {
        let p = self.store.get_project(id)?;
        Ok(SetupScripts {
            repo_file: RepoConfig::load(&Host::of(&p), &p.repo_path)
                .await?
                .and_then(|c| c.worktree.setup),
            project: p.setup_script,
        })
    }

    pub async fn list_worktrees(&self, id: &str) -> Result<Vec<Worktree>> {
        let p = self.store.get_project(id)?;
        git::list_worktrees(&Host::of(&p), &p.repo_path).await
    }

    pub async fn list_branches(&self, id: &str) -> Result<Vec<String>> {
        let p = self.store.get_project(id)?;
        git::list_branches(&Host::of(&p), &p.repo_path).await
    }

    pub async fn worktree_status(&self, path: &Path) -> Result<WorktreeStatus> {
        git::status(&self.host_for_path(path)?, path).await
    }

    pub async fn file_diff(
        &self,
        worktree: &Path,
        file: &str,
        base: Option<&str>,
    ) -> Result<String> {
        git::file_diff(&self.host_for_path(worktree)?, worktree, file, base).await
    }

    pub async fn commit(&self, worktree: &Path, files: &[String], message: &str) -> Result<String> {
        git::commit(&self.host_for_path(worktree)?, worktree, files, message).await
    }

    pub async fn branch_log(&self, worktree: &Path, base: Option<&str>) -> Result<Vec<CommitInfo>> {
        git::branch_log(&self.host_for_path(worktree)?, worktree, base).await
    }

    pub async fn compare_base(&self, worktree: &Path, base: &str) -> Result<String> {
        Ok(git::compare_base(&self.host_for_path(worktree)?, worktree, base).await)
    }

    pub async fn fetch(&self, worktree: &Path) -> Result<()> {
        git::fetch(&self.host_for_path(worktree)?, worktree).await
    }

    pub async fn update_from_base(&self, worktree: &Path, base: &str) -> Result<BaseUpdate> {
        git::update_from_base(&self.host_for_path(worktree)?, worktree, base).await
    }

    pub async fn pull_request(&self, worktree: &Path) -> Result<Option<PullRequest>> {
        github::pull_request(&self.host_for_path(worktree)?, worktree).await
    }

    pub async fn create_pull_request(
        &self,
        worktree: &Path,
        base: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<PullRequest> {
        let host = self.host_for_path(worktree)?;
        github::create_pull_request(&host, worktree, base, title, body, draft).await
    }

    pub async fn push(&self, worktree: &Path) -> Result<()> {
        github::push(&self.host_for_path(worktree)?, worktree).await
    }

    pub async fn pr_review_comments(&self, worktree: &Path, number: u64) -> Result<Vec<PrComment>> {
        github::review_comments(&self.host_for_path(worktree)?, worktree, number).await
    }

    pub async fn github_image(&self, worktree: &Path, url: &str) -> Result<String> {
        github::attachment_image(&self.host_for_path(worktree)?, worktree, url).await
    }

    pub async fn pr_failed_checks(&self, worktree: &Path, number: u64) -> Result<Vec<FailedCheck>> {
        github::failed_checks(&self.host_for_path(worktree)?, worktree, number).await
    }

    /// Has an agent write the pull request's title and description following `instructions`.
    pub async fn draft_pull_request(
        &self,
        worktree: &Path,
        base: &str,
        instructions: &str,
        writer: &Writer,
    ) -> Result<PrDraft> {
        let context = git::pr_context(&self.host_for_path(worktree)?, worktree, base).await?;
        let (name, answer) = self
            .write_once(
                worktree,
                writer,
                crate::prompt::fill(instructions, &context, github::DRAFT_ANSWER),
            )
            .await?;
        github::parse_draft(&answer).ok_or_else(|| {
            Error::Isolation(format!("{name} didn't answer with a title and description"))
        })
    }

    /// Has an agent split the uncommitted changes to `files` into commits and write their
    /// messages.
    pub async fn draft_commits(
        &self,
        worktree: &Path,
        files: &[String],
        instructions: &str,
        writer: &Writer,
    ) -> Result<Vec<CommitDraft>> {
        let context = git::commit_context(&self.host_for_path(worktree)?, worktree, files).await?;
        let (name, answer) = self
            .write_once(
                worktree,
                writer,
                crate::prompt::fill(instructions, &context, git::COMMITS_ANSWER),
            )
            .await?;
        git::parse_commits(&answer, files)
            .ok_or_else(|| Error::Isolation(format!("{name} didn't answer with commits")))
    }

    /// Runs the writer once in the worktree's isolation on `prompt`; returns the agent's name
    /// and its answer.
    async fn write_once(
        &self,
        worktree: &Path,
        writer: &Writer,
        prompt: String,
    ) -> Result<(String, String)> {
        let project = self
            .list_projects()?
            .into_iter()
            .find(|p| p.contains(worktree))
            .ok_or_else(|| Error::ProjectNotFound(worktree.display().to_string()))?;
        let template = writer
            .command
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .unwrap_or(crate::launch::DEFAULT_WRITER_COMMAND);
        let name = template
            .split_whitespace()
            .next()
            .unwrap_or("The agent")
            .to_owned();
        let command = crate::launch::writer_command(template, writer.fork.as_deref());
        let isolation = self.grant_access(&project, Some(worktree)).await?;
        let mut env = self.secret_env(Some(&project.id), &isolation)?;
        env.push(("WORKI_PROMPT".into(), prompt));
        let req = LaunchRequest {
            cwd: worktree.to_path_buf(),
            command: Some(command),
            interactive: false,
            stdin: false,
            env,
            session: None,
        };
        let launch = blocking(move || isolation.backend().launch(&req)).await?;
        Ok((name, run_captured(launch).await?))
    }

    pub async fn branch_changes(&self, worktree: &Path, base: &str) -> Result<BranchChanges> {
        git::branch_changes(&self.host_for_path(worktree)?, worktree, base).await
    }

    pub async fn create_worktree(
        &self,
        id: &str,
        req: CreateWorktree,
        log: impl Fn(LogLine) + Send + Sync + Clone + 'static,
    ) -> Result<CreatedWorktree> {
        let project = self.store.get_project(id)?;
        let host = &Host::of(&project);
        let repo = &project.repo_path;
        let branch = &paths::branch_name(&req.name);
        let base = req.base.trim();
        if git::branch_exists(host, repo, branch).await {
            return Err(Error::BranchExists(branch.into()));
        }
        if !git::ref_exists(host, repo, base).await {
            return Err(Error::UnknownRef(base.into()));
        }

        let path = Self::new_worktree_path(&project, &req.name);
        if host.exists(&path).await? {
            return Err(Error::PathExists(path));
        }
        host.create_dir_all(&project.worktree_root).await?;

        log(LogLine::Info(format!(
            "Creating worktree {} on new branch {branch} from {base}",
            path.display()
        )));
        git::add_worktree(host, repo, &path, branch, base).await?;
        git::set_branch_base(host, repo, branch, base).await?;

        let worktree = Worktree {
            base: Some(base.to_owned()),
            ..find_worktree(host, repo, &path).await?
        };
        self.forget_sessions_in(&worktree.path)?;
        if req.isolation != ProjectIsolation::Inherit {
            self.set_worktree_choice(&worktree.path, req.isolation)?;
        }
        let isolation = self.isolation_for_worktree(&project, &worktree.path)?;
        if isolation != Isolation::None {
            log(LogLine::Info(format!(
                "Giving {} access to this worktree…",
                isolation.label()
            )));
            self.grant_access(&project, Some(&worktree.path)).await?;
        }
        let mut setup_error = None;
        if req.run_setup {
            if let Some(script) = self.setup_scripts(id).await?.effective() {
                log(LogLine::Info(format!(
                    "Running setup script as {}…",
                    isolation.label()
                )));
                let mut env = vec![
                    ("WORKI_ROOT_PATH".to_string(), path_key(repo)),
                    ("WORKI_WORKTREE_PATH".to_string(), path_key(&worktree.path)),
                    ("WORKI_BRANCH".to_string(), branch.to_string()),
                ];
                env.extend(self.secret_env(Some(id), &isolation)?);
                if let Err(e) =
                    run_setup_script(&isolation, &worktree.path, script, env, log.clone()).await
                {
                    log(LogLine::Stderr(e.to_string()));
                    setup_error = Some(e.to_string());
                }
            }
        }
        log(LogLine::Info("Done.".into()));
        Ok(CreatedWorktree {
            worktree,
            setup_error,
        })
    }

    pub async fn remove_worktree(&self, id: &str, req: RemoveWorktree) -> Result<()> {
        let project = self.store.get_project(id)?;
        let host = &Host::of(&project);
        let repo = &project.repo_path;
        let wt = find_worktree(host, repo, &req.path).await?;
        if wt.is_main {
            return Err(Error::Git {
                args: "worktree remove".into(),
                stderr: "the main checkout cannot be removed".into(),
            });
        }
        let isolation = self.isolation_for_worktree(&project, &wt.path)?;
        let ws = Self::workspace(&project, Some(&wt.path));
        let local = !host.is_remote();
        blocking(move || {
            if isolation != Isolation::None {
                isolation.backend().release(&ws)?;
            }
            if local {
                for warning in worki_isolation::Container::release_everywhere(&ws) {
                    tracing::warn!(
                        "cleaning up {}: {warning}",
                        ws.worktree.as_deref().unwrap_or(&ws.repo).display()
                    );
                }
            }
            Ok(())
        })
        .await?;
        git::remove_worktree(host, repo, &wt.path, req.force).await?;
        self.forget_sessions_in(&wt.path)?;
        self.forget_sessions_in(&req.path)?;
        self.set_worktree_choice(&wt.path, ProjectIsolation::Inherit)?;
        let own = paths::worktree_dir(id, &wt.path);
        if own.exists() {
            std::fs::remove_dir_all(&own)?;
        }
        if let (true, Some(branch)) = (req.delete_branch, wt.branch.as_deref()) {
            git::delete_branch(host, repo, branch, req.force).await?;
        }
        Ok(())
    }
}

async fn find_worktree(host: &Host, repo: &Path, path: &Path) -> Result<Worktree> {
    let wanted = host.canonical(path).await;
    let mut found = None;
    for w in git::worktrees(host, repo).await? {
        if w.path == wanted
            || w.path == path
            || (!host.is_remote() && host.canonical(&w.path).await == wanted)
        {
            found = Some(w);
            break;
        }
    }
    found.ok_or_else(|| Error::Git {
        args: "worktree list".into(),
        stderr: format!("no worktree at {}", path.display()),
    })
}

async fn run_setup_script(
    isolation: &Isolation,
    worktree: &Path,
    script: &str,
    env: Vec<(String, String)>,
    log: impl Fn(LogLine) + Send + Sync + Clone + 'static,
) -> Result<()> {
    let req = LaunchRequest {
        cwd: worktree.to_path_buf(),
        command: Some(script.to_owned()),
        interactive: false,
        stdin: false,
        env,
        session: None,
    };
    let isolation = isolation.clone();
    let launch = blocking(move || isolation.backend().launch(&req)).await?;
    run_logged(launch, log).await
}

/// Runs blocking isolation work (backend calls may shell out or talk SSH) off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(crate::trace::carry(f))
        .await
        .map_err(|e| Error::Isolation(e.to_string()))?
        .map_err(Error::isolation)
}

fn push_unique(all: &mut Vec<Isolation>, more: impl IntoIterator<Item = Isolation>) {
    for i in more {
        if !all.contains(&i) {
            all.push(i);
        }
    }
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
