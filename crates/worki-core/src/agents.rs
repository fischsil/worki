//! Knowledge about specific agent CLIs: where their user config lives, which parts are safe to
//! copy into an isolated environment, and which token they authenticate with. Blocking; call from
//! a worker.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;
use worki_isolation::{host_which, Backend};

use crate::model::JsonObject;

pub struct Kit {
    pub executable: &'static str,
    /// How agents sign in without the human's own login, if worki can set that up.
    pub sign_in: Option<SignIn>,
    /// Where the agent records that it trusts a folder, so worki can answer its question.
    pub trust: Option<Trust>,
}

pub struct Trust {
    /// Config file, relative to the home directory.
    pub file: &'static str,
    /// The file's new content with the folder trusted, or `None` when it already is.
    pub apply: fn(&str, &Path) -> Result<Option<String>>,
}

/// A token worki creates with `claude setup-token` (approved in the browser, bound to the
/// subscription) and passes to the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignIn {
    /// Env var the token is stored and passed as.
    pub secret: &'static str,
}

const KITS: &[Kit] = &[
    Kit {
        executable: "claude",
        sign_in: Some(SignIn {
            secret: "CLAUDE_CODE_OAUTH_TOKEN",
        }),
        trust: Some(Trust {
            file: ".claude.json",
            apply: trusted_claude_json,
        }),
    },
    Kit {
        executable: "codex",
        sign_in: None,
        trust: Some(Trust {
            file: ".codex/config.toml",
            apply: trusted_codex_config,
        }),
    },
];

pub fn kit(executable: &str) -> Option<&'static Kit> {
    KITS.iter().find(|k| k.executable == executable)
}

/// The variable Claude Code signs in with a token from.
pub fn claude_sign_in_secret() -> Option<&'static str> {
    kit("claude").and_then(|k| k.sign_in).map(|s| s.secret)
}

/// Whether `name` is the variable an agent signs in with, which belongs to the isolation it was
/// signed in for, not to the secrets you manage.
pub fn is_sign_in_secret(name: &str) -> bool {
    KITS.iter()
        .filter_map(|k| k.sign_in.as_ref())
        .any(|s| s.secret == name)
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().context("no home directory")
}

/// Makes the human's installation of `executable` usable inside the environment. Node-based
/// installs (`#!/usr/bin/env node`) also need `node`, which is shared the same way.
pub fn share_install(backend: &dyn Backend, executable: &str) -> Result<String> {
    let host = host_which(executable)
        .with_context(|| format!("`{executable}` is not installed for you"))?;
    backend.share_host_executable(executable, Path::new(&host))?;
    if needs_node(Path::new(&host)) && backend.which("node").is_none() {
        let node =
            host_which("node").context("this agent needs `node`, which isn't installed for you")?;
        backend.share_host_executable("node", Path::new(&node))?;
    }
    Ok(host)
}

fn needs_node(path: &Path) -> bool {
    let Ok(resolved) = path.canonicalize() else {
        return false;
    };
    let Ok(mut file) = std::fs::File::open(resolved) else {
        return false;
    };
    let mut head = [0u8; 128];
    let n = std::io::Read::read(&mut file, &mut head).unwrap_or(0);
    let head = String::from_utf8_lossy(&head[..n]);
    head.starts_with("#!") && head.lines().next().is_some_and(|l| l.contains("node"))
}

/// Settings left out of the host's when importing them: they run scripts from the host's home
/// folder, or restrict the agent where the isolation already confines it.
const NOT_IMPORTED: &[&str] = &[
    "hooks",
    "statusLine",
    "sandbox",
    "permissions.deny",
    "permissions.ask",
];

/// The host's own Claude Code settings (`~/.claude/settings.json`), without [`NOT_IMPORTED`].
pub fn host_claude_settings() -> Result<JsonObject> {
    let path = home()?.join(".claude/settings.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(JsonObject::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let mut settings: JsonObject =
        serde_json::from_str(&text).context("~/.claude/settings.json is not a JSON object")?;
    for path in NOT_IMPORTED {
        remove_path(&mut settings, path);
    }
    Ok(settings)
}

/// Removes a dotted key path such as `permissions.deny`, if it's there.
fn remove_path(settings: &mut JsonObject, path: &str) {
    let (parents, key) = path.rsplit_once('.').unwrap_or(("", path));
    let mut object = Some(settings);
    for part in parents.split('.').filter(|p| !p.is_empty()) {
        object = object
            .and_then(|o| o.get_mut(part))
            .and_then(Value::as_object_mut);
    }
    if let Some(object) = object {
        object.remove(key);
    }
}

/// Marks `worktree` as trusted in the agent's own config inside an isolated environment, where
/// nobody can answer the agent's trust question in a chat and the isolation already limits what
/// it reaches.
pub fn trust_worktree(backend: &dyn Backend, executable: &str, worktree: &Path) -> Result<()> {
    let Some(trust) = kit(executable).and_then(|k| k.trust.as_ref()) else {
        return Ok(());
    };
    let file = Path::new(trust.file);
    let current = backend
        .read_home_file(file)?
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    match (trust.apply)(&current, worktree)? {
        Some(updated) => backend.write_home_file(file, updated.as_bytes()),
        None => Ok(()),
    }
}

/// What a project's or worktree's Claude Code folder takes over from the one it starts from.
const STARTING_FILES: [&str; 7] = [
    "settings.json",
    "CLAUDE.md",
    "skills",
    "agents",
    "commands",
    "output-styles",
    "plugins",
];
/// Marks a Claude Code folder as started, so what's deleted from it later stays deleted.
const STARTED: &str = ".worki-started";

/// Starts `own` as a Claude Code config folder, from `from` when given (the account's, or the
/// project's for a worktree). Once; later calls leave `own` alone.
pub fn start_claude_dir(own: &Path, from: Option<&Path>, paths: &[&Path]) -> Result<()> {
    if own.join(STARTED).exists() {
        return Ok(());
    }
    match from {
        Some(from) => take_claude_files(own, from, paths)?,
        None => std::fs::create_dir_all(own)?,
    }
    std::fs::write(own.join(STARTED), "")?;
    Ok(())
}

/// Copies what `own` doesn't have yet from `from`: its settings, `CLAUDE.md`, skills, agents,
/// commands, output styles and plugins, and the history and memory Claude Code keeps for
/// `paths` and the folders under them.
fn take_claude_files(own: &Path, from: &Path, paths: &[&Path]) -> Result<()> {
    std::fs::create_dir_all(own)?;
    for name in STARTING_FILES {
        let (target, source) = (own.join(name), from.join(name));
        if target.symlink_metadata().is_err() && source.exists() {
            copy_all(&source, &target)?;
        }
    }
    let keys: Vec<String> = paths.iter().map(|p| claude_project_key(p)).collect();
    let mine = |name: &str| {
        keys.iter()
            .any(|k| name == k || name.starts_with(&format!("{k}-")))
    };
    if let Ok(entries) = std::fs::read_dir(from.join("projects")) {
        std::fs::create_dir_all(own.join("projects"))?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let target = own.join("projects").join(&name);
            if mine(&name.to_string_lossy()) && !target.exists() {
                copy_all(&entry.path(), &target)?;
            }
        }
    }
    Ok(())
}

/// Marks an account folder that holds nothing but its login.
const TIDIED: &str = ".worki-tidied";

/// Account folders used to be every project's config folder. Gives each project in `projects`
/// (its folder and the paths it keeps history for) what it doesn't have yet from the account's,
/// then empties that down to the login. Once per account.
pub fn tidy_account_dir(account: &Path, projects: &[(PathBuf, Vec<&Path>)]) -> Result<()> {
    if account.join(TIDIED).exists() || !account.exists() {
        return Ok(());
    }
    account_login_dir(account)?;
    for (own, paths) in projects {
        take_claude_files(own, account, paths)?;
        if !own.join(STARTED).exists() {
            std::fs::write(own.join(STARTED), "")?;
        }
    }
    for entry in std::fs::read_dir(account)?.flatten() {
        if entry.file_name() == "login" {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    std::fs::write(account.join(TIDIED), "")?;
    Ok(())
}

/// Who a Claude Code config folder's login is, from its `.claude.json`.
fn identity_in(config: &Path) -> Option<Identity> {
    let value = read_claude_json(config);
    let identity: Identity = ["oauthAccount", "userID"]
        .into_iter()
        .filter(|k| !value[k].is_null())
        .map(|k| (k.to_owned(), value[k].clone()))
        .collect();
    (!identity.is_empty()).then_some(identity)
}

type Identity = serde_json::Map<String, Value>;

/// Who a login is, kept beside it in its folder so the two always change together.
const IDENTITY: &str = "identity.json";

/// The account's login folder (see [`worki_isolation::account_login_dir`]), with who the login
/// is moved in from where an earlier version kept it, the account's `.claude.json`.
pub fn account_login_dir(account: &Path) -> Result<PathBuf> {
    let login = worki_isolation::account_login_dir(account)?;
    if let Some(identity) = identity_in(account) {
        if !login.join(IDENTITY).exists() {
            store_identity(&login, &identity)?;
        }
        std::fs::remove_file(account.join(".claude.json"))?;
    }
    Ok(login)
}

fn stored_identity(login: &Path) -> Option<Identity> {
    let text = std::fs::read_to_string(login.join(IDENTITY)).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()?
        .as_object()
        .filter(|m| !m.is_empty())
        .cloned()
}

fn store_identity(login: &Path, identity: &Identity) -> Result<()> {
    std::fs::create_dir_all(login)?;
    let text = serde_json::to_string_pretty(&Value::Object(identity.clone()))?;
    std::fs::write(login.join(IDENTITY), text)?;
    Ok(())
}

/// Keeps who `config` (the throwaway folder a sign-in through worki ran with) signed in as next
/// to the login it wrote.
pub fn keep_login_identity(login: &Path, config: &Path) -> Result<()> {
    match identity_in(config) {
        Some(identity) => store_identity(login, &identity),
        None => Ok(()),
    }
}

/// Puts who the login is into the config folder `own`, where Claude Code looks for it.
pub fn copy_login_identity(login: &Path, own: &Path) -> Result<()> {
    let Some(identity) = stored_identity(login) else {
        return Ok(());
    };
    let mut config = read_claude_json(own);
    if identity.iter().all(|(k, v)| config[k] == *v) {
        return Ok(());
    }
    for (k, v) in identity {
        config[k] = v;
    }
    write_claude_json(own, &config)
}

/// Prints Claude's profile of the login in `$CLAUDE_SECURESTORAGE_CONFIG_DIR`: who its token
/// signs in as, whatever `.claude.json` says. Run where the login is mounted, so the token never
/// leaves it; prints nothing when the token has expired.
pub const LOGIN_PROFILE_SCRIPT: &str = r#"token=$(tr -d '\n' < "$CLAUDE_SECURESTORAGE_CONFIG_DIR/.credentials.json" | grep -o '"claudeAiOauth"[^}]*' | sed -n 's/.*"accessToken"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p'); [ -n "$token" ] && printf 'Authorization: Bearer %s' "$token" | curl -fsS -m 10 -H @- https://api.anthropic.com/api/oauth/profile; true"#;

/// Whose a login is: the organization it signs in to, and its email when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub organization: String,
    pub email: Option<String>,
}

impl Owner {
    fn of(identity: &Identity) -> Option<Self> {
        let account = identity.get("oauthAccount")?;
        Some(Self {
            organization: account["organizationUuid"].as_str()?.to_owned(),
            email: account["emailAddress"].as_str().map(str::to_owned),
        })
    }

    pub fn name(&self) -> &str {
        self.email.as_deref().unwrap_or("another account")
    }
}

/// Who signed in with `config` as Claude Code's folder, as Claude Code wrote it there.
pub fn config_owner(config: &Path) -> Option<Owner> {
    Owner::of(&identity_in(config)?)
}

/// Who the login was signed in as through worki, kept beside it.
pub fn login_owner(login: &Path) -> Option<Owner> {
    Owner::of(&stored_identity(login)?)
}

/// Who `profile` (the output of [`LOGIN_PROFILE_SCRIPT`]) says the login signs in as, when that
/// is another organization than [`login_owner`]'s: its email, or its organization's name.
pub fn foreign_login(login: &Path, profile: &Value) -> Option<String> {
    let org = profile["organization"]["uuid"].as_str()?;
    if login_owner(login)?.organization == org {
        return None;
    }
    let who = profile["account"]["email"]
        .as_str()
        .or(profile["organization"]["name"].as_str())
        .unwrap_or("another organization");
    Some(who.to_owned())
}

/// Puts who the login is into each of `configs` that exists.
pub fn give_login_identity(login: &Path, configs: &[PathBuf]) -> Result<()> {
    for dir in configs.iter().filter(|d| d.exists()) {
        copy_login_identity(login, dir)?;
    }
    Ok(())
}

/// The folder name Claude Code keeps a path's history and memory under, in `projects/`.
fn claude_project_key(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// A config folder's `.claude.json`, or an empty object.
fn read_claude_json(dir: &Path) -> Value {
    std::fs::read_to_string(dir.join(".claude.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()))
}

fn write_claude_json(dir: &Path, config: &Value) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join(".claude.json"),
        serde_json::to_string_pretty(config)?,
    )?;
    Ok(())
}

fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = from.symlink_metadata()?;
    if meta.file_type().is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
    } else if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(drop)
    }
}

/// Answers Claude Code's trust question for `worktree` in the config folder `dir`, whose
/// `.claude.json` Claude Code reads instead of `~/.claude.json` there.
pub fn trust_in_config_dir(dir: &Path, worktree: &Path) -> Result<()> {
    let Some(trust) = kit("claude").and_then(|k| k.trust.as_ref()) else {
        return Ok(());
    };
    let file = dir.join(trust.file);
    let current = std::fs::read_to_string(&file).unwrap_or_default();
    if let Some(trusted) = (trust.apply)(&current, worktree)? {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&file, trusted)?;
    }
    Ok(())
}

/// `~/.claude.json` with `worktree`'s trust question answered, or `None` when it already is.
fn trusted_claude_json(current: &str, worktree: &Path) -> Result<Option<String>> {
    let mut json: Value = serde_json::from_str(current)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()));
    let key = worktree.to_string_lossy();
    if json["hasCompletedOnboarding"] == true
        && json["projects"][key.as_ref()]["hasTrustDialogAccepted"] == true
    {
        return Ok(None);
    }
    json["hasCompletedOnboarding"] = Value::Bool(true);
    json["projects"][key.as_ref()]["hasTrustDialogAccepted"] = Value::Bool(true);
    Ok(Some(serde_json::to_string_pretty(&json)?))
}

/// Codex's `config.toml` with `worktree` as a trusted project, written the way Codex records it,
/// or `None` when it already is.
fn trusted_codex_config(current: &str, worktree: &Path) -> Result<Option<String>> {
    let header = format!(
        "[projects.{}]",
        toml::Value::String(worktree.to_string_lossy().into_owned())
    );
    if current.contains(&header) {
        return Ok(None);
    }
    Ok(Some(format!(
        "{current}\n{header}\ntrust_level = \"trusted\"\n"
    )))
}

/// Finds the token printed by `claude setup-token` in raw terminal output.
pub fn find_claude_token(output: &str) -> Option<String> {
    let text = strip_ansi(output);
    let start = text.find("sk-ant-oat")?;
    let token: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (token.len() > 20).then_some(token)
}

/// Takes the finished lines out of terminal output collected in `pending`, trimmed, without
/// empty ones.
pub fn take_lines(pending: &mut String) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(i) = pending.find(['\n', '\r']) {
        let line: String = pending.drain(..=i).collect();
        let line = line.trim();
        if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }
    lines
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.peek() == Some(&'\\')) {
                        chars.next_if_eq(&'\\');
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_the_project_folder_from_the_account_once() {
        let root = tempfile::tempdir().unwrap();
        let (account, own) = (root.path().join("account"), root.path().join("own"));
        let write = |p: PathBuf, text: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write(account.join("skills/review/SKILL.md"), "review");
        write(account.join("settings.json"), r#"{"model":"opus"}"#);
        write(account.join(".credentials.json"), "secret");
        write(account.join("projects/-r-sam/memory/MEMORY.md"), "sam");
        write(account.join("projects/-w-sam-feat/1.jsonl"), "feat");
        write(
            account.join("projects/-r-sam2/memory/MEMORY.md"),
            "other repo",
        );
        write(account.join("projects/-r-other/1.jsonl"), "other");

        start_claude_dir(
            &own,
            Some(&account),
            &[Path::new("/r/sam"), Path::new("/w/sam")],
        )
        .unwrap();
        let read = |p: &str| std::fs::read_to_string(own.join(p)).unwrap();
        assert_eq!(read("settings.json"), r#"{"model":"opus"}"#);
        assert_eq!(read("skills/review/SKILL.md"), "review");
        assert_eq!(read("projects/-r-sam/memory/MEMORY.md"), "sam");
        assert_eq!(read("projects/-w-sam-feat/1.jsonl"), "feat");
        assert!(!own.join("projects/-r-sam2").exists());
        assert!(!own.join("projects/-r-other").exists());
        assert!(!own.join(".credentials.json").exists());

        std::fs::remove_file(own.join("settings.json")).unwrap();
        start_claude_dir(&own, Some(&account), &[Path::new("/r/sam")]).unwrap();
        assert!(!own.join("settings.json").exists());
    }

    #[test]
    fn tidying_an_account_hands_its_files_to_its_projects_first() {
        let root = tempfile::tempdir().unwrap();
        let (account, own) = (root.path().join("account"), root.path().join("own"));
        let write = |p: PathBuf, text: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write(account.join(".credentials.json"), "login");
        write(
            account.join(".claude.json"),
            r#"{"oauthAccount":{"emailAddress":"a@b.c"},"userID":"u1","projects":{"/r/sam":{}}}"#,
        );
        write(account.join("settings.json"), "account");
        write(account.join("plugins/p/plugin.json"), "plugin");
        write(account.join("projects/-r-sam/1.jsonl"), "history");
        write(account.join("projects/-r-gone/1.jsonl"), "removed project");
        write(account.join("cache/x"), "cache");
        write(own.join("settings.json"), "project");
        write(own.join(STARTED), "");

        let projects = [(own.clone(), vec![Path::new("/r/sam")])];
        tidy_account_dir(&account, &projects).unwrap();

        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap();
        assert_eq!(read(own.join("settings.json")), "project");
        assert_eq!(read(own.join("plugins/p/plugin.json")), "plugin");
        assert_eq!(read(own.join("projects/-r-sam/1.jsonl")), "history");
        assert_eq!(read(account.join("login/.credentials.json")), "login");
        let mut left: Vec<String> = std::fs::read_dir(&account)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [TIDIED, "login"]);
        let identity: Value =
            serde_json::from_str(&read(account.join("login").join(IDENTITY))).unwrap();
        assert_eq!(identity["oauthAccount"]["emailAddress"], "a@b.c");
        assert_eq!(identity["userID"], "u1");
        assert!(identity["projects"].is_null());

        write(account.join("settings.json"), "written by a later sign-in");
        tidy_account_dir(&account, &projects).unwrap();
        assert!(account.join("settings.json").exists(), "only once");
    }

    fn claude_json(dir: &Path, identity: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(".claude.json"),
            format!(r#"{{"oauthAccount":{{"emailAddress":"{identity}"}},"numStartups":1}}"#),
        )
        .unwrap();
    }

    fn email(dir: &Path) -> Value {
        read_claude_json(dir)["oauthAccount"]["emailAddress"].clone()
    }

    #[test]
    fn a_sign_in_keeps_who_it_is_beside_the_login() {
        let root = tempfile::tempdir().unwrap();
        let (login, scratch) = (root.path().join("login"), root.path().join("scratch"));
        claude_json(&scratch, "a@b.c");
        keep_login_identity(&login, &scratch).unwrap();
        assert_eq!(
            stored_identity(&login).map(Value::Object),
            Some(serde_json::json!({"oauthAccount":{"emailAddress":"a@b.c"}}))
        );
    }

    #[test]
    fn a_folder_left_from_another_account_gets_the_logins_identity_back() {
        let root = tempfile::tempdir().unwrap();
        let (login, project) = (root.path().join("login"), root.path().join("project"));
        claude_json(&root.path().join("first"), "mine@b.c");
        keep_login_identity(&login, &root.path().join("first")).unwrap();
        claude_json(&project, "other@b.c");
        copy_login_identity(&login, &project).unwrap();
        assert_eq!(email(&project), "mine@b.c");
        assert_eq!(
            stored_identity(&login).unwrap()["oauthAccount"]["emailAddress"],
            "mine@b.c"
        );
    }

    #[test]
    fn a_login_of_another_organization_is_told_apart() {
        let root = tempfile::tempdir().unwrap();
        let (login, scratch) = (root.path().join("login"), root.path().join("scratch"));
        write_claude_json(
            &scratch,
            &serde_json::json!({"oauthAccount":{"emailAddress":"me@home.org","organizationUuid":"home"}}),
        )
        .unwrap();
        keep_login_identity(&login, &scratch).unwrap();
        let profile = |org: &str| serde_json::json!({"account":{"email":"me@work.org"},"organization":{"uuid":org}});
        assert_eq!(foreign_login(&login, &profile("home")), None);
        assert_eq!(
            foreign_login(&login, &profile("work")).as_deref(),
            Some("me@work.org")
        );
        assert_eq!(foreign_login(&login, &Value::Null), None);
    }

    #[test]
    fn the_identity_moves_in_beside_the_login() {
        let root = tempfile::tempdir().unwrap();
        let account = root.path().join("account");
        claude_json(&account, "a@b.c");
        std::fs::write(account.join(TIDIED), "").unwrap();
        let login = account_login_dir(&account).unwrap();
        assert_eq!(
            stored_identity(&login).unwrap()["oauthAccount"]["emailAddress"],
            "a@b.c"
        );
        assert!(!account.join(".claude.json").exists());
    }

    #[test]
    fn a_worktree_starts_from_the_project_not_the_account() {
        let root = tempfile::tempdir().unwrap();
        let (account, project, worktree) = (
            root.path().join("account"),
            root.path().join("project"),
            root.path().join("worktree"),
        );
        std::fs::create_dir_all(&account).unwrap();
        std::fs::write(account.join("CLAUDE.md"), "account").unwrap();
        start_claude_dir(&project, Some(&account), &[]).unwrap();
        std::fs::write(project.join("CLAUDE.md"), "project").unwrap();

        start_claude_dir(&worktree, Some(&project), &[]).unwrap();
        assert_eq!(
            std::fs::read_to_string(worktree.join("CLAUDE.md")).unwrap(),
            "project"
        );
        std::fs::write(worktree.join("CLAUDE.md"), "worktree").unwrap();
        assert_eq!(
            std::fs::read_to_string(project.join("CLAUDE.md")).unwrap(),
            "project"
        );
    }

    #[test]
    fn copies_who_the_login_is_into_a_project() {
        let root = tempfile::tempdir().unwrap();
        let (login, own) = (root.path().join("login"), root.path().join("own"));
        claude_json(&root.path().join("signed-in"), "a@b.c");
        keep_login_identity(&login, &root.path().join("signed-in")).unwrap();
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(
            own.join(".claude.json"),
            r#"{"hasCompletedOnboarding":true}"#,
        )
        .unwrap();
        copy_login_identity(&login, &own).unwrap();
        assert_eq!(email(&own), "a@b.c");
        assert_eq!(read_claude_json(&own)["hasCompletedOnboarding"], true);
    }

    #[test]
    fn leaves_scripts_and_restrictions_out_of_the_import() {
        let mut settings: JsonObject = serde_json::from_str(
            r#"{"model":"opus","hooks":{"Stop":[]},"statusLine":{"command":"x"},
            "sandbox":{"allowUnsandboxedCommands":false},
            "permissions":{"defaultMode":"auto","allow":["Bash(ls)"],"deny":["Read(.env)"]}}"#,
        )
        .unwrap();
        for path in NOT_IMPORTED {
            remove_path(&mut settings, path);
        }
        assert_eq!(
            Value::from(settings),
            serde_json::json!({
                "model": "opus",
                "permissions": {"defaultMode": "auto", "allow": ["Bash(ls)"]}
            })
        );
    }

    #[test]
    fn finds_token_between_escape_codes() {
        let out = "\x1b[32m✓ Token created\x1b[0m\r\n\x1b]8;;https://x\x07link\x1b]8;;\x07\r\nsk-ant-oat01-AbC_d-123456789012345678\x1b[0m\r\n";
        assert_eq!(
            find_claude_token(out).as_deref(),
            Some("sk-ant-oat01-AbC_d-123456789012345678")
        );
        assert_eq!(find_claude_token("no token here"), None);
    }

    #[test]
    fn trusts_the_worktree_in_each_agents_config() {
        let wt = Path::new("/Users/me/worki/worktrees/app/fix");
        let claude = trusted_claude_json(r#"{"projects":{"/other":{}},"theme":"dark"}"#, wt)
            .unwrap()
            .unwrap();
        let json: Value = serde_json::from_str(&claude).unwrap();
        assert_eq!(
            json["projects"][wt.to_str().unwrap()]["hasTrustDialogAccepted"],
            true
        );
        assert_eq!(json["theme"], "dark");
        assert_eq!(trusted_claude_json(&claude, wt).unwrap(), None);

        let codex = trusted_codex_config("model = \"o3\"\n", wt)
            .unwrap()
            .unwrap();
        let config: toml::Table = toml::from_str(&codex).unwrap();
        assert_eq!(config["model"].as_str(), Some("o3"));
        assert_eq!(
            config["projects"][wt.to_str().unwrap()]["trust_level"].as_str(),
            Some("trusted")
        );
        assert_eq!(trusted_codex_config(&codex, wt).unwrap(), None);
    }

    #[test]
    fn known_kits() {
        let sign_in = |exe| kit(exe).unwrap().sign_in;
        assert_eq!(sign_in("claude").unwrap().secret, "CLAUDE_CODE_OAUTH_TOKEN");
        assert!(sign_in("codex").is_none());
        assert!(kit("aider").is_none());
    }
}
