//! End-to-end checks against real container engines. Each test creates and removes one `worki-*`
//! environment. Run with: `cargo test -p worki-isolation --test containers -- --ignored`
//! (or name one: `… -- --ignored podman`).

use std::path::{Path, PathBuf};
use std::process::Command;

use worki_isolation::{Container, Isolation, LaunchRequest, Outdated, Workspace};

/// A public image with bash, git and tar, so Docker and Podman tests don't need worki's image.
const TEST_IMAGE: &str = "docker.io/library/buildpack-deps:bookworm-scm";

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn run(launch: worki_isolation::Launch) -> (bool, String) {
    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.args)
        .current_dir(&launch.cwd)
        .envs(launch.env);
    let out = cmd.output().unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// Worktrees must live where the engine shares host folders at the same path: Docker Desktop
/// and Podman share `/Users`, so the test repo goes under the home folder.
fn lifecycle(isolation: Isolation) {
    let home = std::env::var("HOME").unwrap();
    let base = Path::new(&home).join("worki");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix(".e2e-")
        .tempdir_in(&base)
        .unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    let wt = root.join("wt-e2e");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "hello\n").unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "init",
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
            "main",
        ],
    );

    let backend = isolation.backend();
    backend.ready().expect("engine available and running");
    let ws = Workspace {
        repo: repo.clone(),
        worktree_root: root.clone(),
        worktree: Some(wt.clone()),
        claude_dir: None,
        claude_login_dir: None,
        shared_folder: None,
        outdated: Outdated::Keep,
    };
    backend.grant_access(&ws).expect("environment created");
    backend.grant_access(&ws).expect("second grant is a no-op");

    let req = |command: &str| LaunchRequest {
        cwd: wt.clone(),
        command: Some(command.into()),
        interactive: false,
        stdin: false,
        env: vec![("WORKI_PROBE".into(), "probe-value".into())],
        session: None,
    };
    let (ok, out) = run(backend.launch(&req("echo \"$WORKI_PROBE\"; pwd")).unwrap());
    assert!(
        ok && out.contains("probe-value") && out.contains(wt.to_str().unwrap()),
        "{out}"
    );

    let (ok, out) = run(backend
        .launch(&req(
            "git config --global --add safe.directory '*'; echo x > from-env.txt && git add from-env.txt && git -c user.name=s -c user.email=s@s commit -qm env",
        ))
        .unwrap());
    assert!(ok, "commit failed: {out}");
    assert!(git(&repo, &["log", "--oneline", "-1", "feature"]).contains("env"));

    let (ok, out) = run(backend
        .launch(&req(&format!("touch {}/.git/hooks/evil", repo.display())))
        .unwrap());
    assert!(!ok, "hooks must be read-only: {out}");
    assert!(!repo.join(".git/hooks/evil").exists());

    assert!(backend.instance_exists(&ws));
    let instance = backend.instance_for(&ws).expect("bound instance");
    assert!(instance.which("git").is_some_and(|p| p.starts_with('/')));
    assert!(instance.which("definitely-not-installed").is_none());
    instance
        .write_home_file(Path::new(".worki-test/note.txt"), b"hello")
        .unwrap();
    assert_eq!(
        instance
            .read_home_file(Path::new(".worki-test/note.txt"))
            .unwrap()
            .as_deref(),
        Some(&b"hello"[..])
    );
    assert_eq!(
        instance
            .read_home_file(Path::new(".worki-test/missing"))
            .unwrap(),
        None
    );
    let host_home = root.join("fake-home");
    std::fs::create_dir_all(host_home.join(".claude/agents")).unwrap();
    std::fs::write(host_home.join(".claude/agents/reviewer.md"), "be thorough").unwrap();
    instance
        .copy_home_files(&host_home, &[".claude/agents".into()])
        .unwrap();
    assert_eq!(
        instance
            .read_home_file(Path::new(".claude/agents/reviewer.md"))
            .unwrap()
            .as_deref(),
        Some(&b"be thorough"[..])
    );

    backend.shutdown().expect("stopped");
    backend.grant_access(&ws).expect("restarted");
    let (ok, out) = run(backend.launch(&req("true")).unwrap());
    assert!(ok, "exec after restart: {out}");

    backend.release(&ws).expect("environment removed");
    assert!(!backend.instance_exists(&ws));

    backend.grant_access(&ws).expect("environment recreated");
    assert_eq!(Container::release_everywhere(&ws), Vec::<String>::new());
    assert!(
        !backend.instance_exists(&ws),
        "the sweep must remove the worktree's environment"
    );

    backend.grant_access(&ws).expect("environment recreated");
    assert!(backend.instance_exists(&ws));
    backend
        .forget_project(&Workspace {
            worktree: None,
            ..ws.clone()
        })
        .expect("project forgotten");
    assert!(
        !backend.instance_exists(&ws),
        "project removal must delete the worktree's environment"
    );
    git(
        &repo,
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
    );
}

/// An agent in the main checkout can commit, but can't touch what your own git runs.
fn main_checkout(isolation: Isolation) {
    let home = std::env::var("HOME").unwrap();
    let base = Path::new(&home).join("worki");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix(".e2e-")
        .tempdir_in(&base)
        .unwrap();
    let repo = tmp.path().canonicalize().unwrap().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "hello\n").unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "init",
        ],
    );

    let backend = isolation.backend();
    backend.ready().expect("engine available and running");
    let ws = Workspace {
        repo: repo.clone(),
        worktree_root: tmp.path().to_path_buf(),
        worktree: Some(repo.clone()),
        claude_dir: None,
        claude_login_dir: None,
        shared_folder: None,
        outdated: Outdated::Keep,
    };
    backend.grant_access(&ws).expect("environment created");
    let run_in = |command: &str| {
        run(backend
            .launch(&LaunchRequest {
                cwd: repo.clone(),
                command: Some(command.into()),
                interactive: false,
                stdin: false,
                env: vec![],
                session: None,
            })
            .unwrap())
    };

    let (ok, out) = run_in(
        "git config --global --add safe.directory '*'; echo x > agent.txt && git add agent.txt && git -c user.name=s -c user.email=s@s commit -qm agent",
    );
    assert!(ok, "commit failed: {out}");
    assert!(git(&repo, &["log", "--oneline", "-1"]).contains("agent"));

    for attempt in [
        "touch .git/hooks/pre-commit",
        "echo '[core]' >> .git/config",
        "mkdir -p .git/modules/x && touch .git/modules/x/config",
        "touch .git/info/attributes",
    ] {
        let (ok, out) = run_in(attempt);
        assert!(!ok, "`{attempt}` must fail: {out}");
    }
    assert!(!repo.join(".git/hooks/pre-commit").exists());

    backend.release(&ws).expect("environment removed");
}

#[test]
#[ignore = "needs Docker Desktop running"]
fn docker_main_checkout() {
    pull("docker");
    main_checkout(Isolation::Docker {
        image: TEST_IMAGE.into(),
    });
}

#[test]
#[ignore = "needs Docker Sandboxes (sbx)"]
fn sbx_main_checkout() {
    main_checkout(Isolation::DockerSandbox { template: None });
}

fn pull(engine: &str) {
    let ok = Command::new(engine)
        .args(["pull", "-q", TEST_IMAGE])
        .output()
        .is_ok_and(|o| o.status.success());
    assert!(ok, "{engine} pull {TEST_IMAGE} failed");
}

#[test]
#[ignore = "needs Docker Sandboxes (sbx)"]
fn sbx_lifecycle() {
    lifecycle(Isolation::DockerSandbox { template: None });
}

#[test]
#[ignore = "needs Docker Desktop running"]
fn docker_lifecycle() {
    pull("docker");
    lifecycle(Isolation::Docker {
        image: TEST_IMAGE.into(),
    });
}

#[test]
#[ignore = "needs a running Podman machine"]
fn podman_lifecycle() {
    pull("podman");
    lifecycle(Isolation::Podman {
        image: TEST_IMAGE.into(),
    });
}

fn network_check(isolation: Isolation) {
    let checks = isolation.backend().doctor(None);
    let network = checks
        .iter()
        .find(|c| c.id == "network")
        .expect("a network check");
    println!("{}: {} ({:?})", network.ok, network.label, network.detail);
}

#[test]
#[ignore = "needs Docker Desktop running and worki's image"]
fn docker_network_check() {
    network_check(Isolation::Docker {
        image: worki_isolation::DEFAULT_CONTAINER_IMAGE.into(),
    });
}

#[test]
#[ignore = "needs a running Podman machine and worki's image"]
fn podman_network_check() {
    network_check(Isolation::Podman {
        image: worki_isolation::DEFAULT_CONTAINER_IMAGE.into(),
    });
}

/// An environment made for one account is kept while something may run in it, and replaced with
/// one for the project's new account otherwise.
fn account_switch(isolation: Isolation) {
    let home = std::env::var("HOME").unwrap();
    let base = Path::new(&home).join("worki");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix(".e2e-")
        .tempdir_in(&base)
        .unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    let (first, second) = (root.join("account a"), root.join("account b"));

    let backend = isolation.backend();
    backend.ready().expect("engine available and running");
    let ws = |account: &Path, outdated| Workspace {
        repo: repo.clone(),
        worktree_root: root.clone(),
        worktree: Some(repo.clone()),
        claude_dir: Some(account.to_path_buf()),
        claude_login_dir: None,
        shared_folder: None,
        outdated,
    };
    let config_dir = || {
        let (ok, out) = run(backend
            .launch(&LaunchRequest {
                cwd: repo.clone(),
                command: Some(r#"printf %s "$CLAUDE_CONFIG_DIR""#.into()),
                interactive: false,
                stdin: false,
                env: vec![],
                session: None,
            })
            .unwrap());
        assert!(ok, "{out}");
        out.lines().last().unwrap_or_default().to_owned()
    };

    backend
        .grant_access(&ws(&first, Outdated::Replace))
        .expect("created");
    assert_eq!(config_dir(), first.display().to_string());
    backend
        .grant_access(&ws(&second, Outdated::Keep))
        .expect("kept");
    assert_eq!(config_dir(), first.display().to_string());
    let refused = backend.grant_access(&ws(&second, Outdated::Refuse));
    assert!(
        refused.is_err(),
        "a session must not start in an outdated environment"
    );
    assert_eq!(config_dir(), first.display().to_string());
    backend
        .grant_access(&ws(&second, Outdated::Replace))
        .expect("replaced");
    assert_eq!(config_dir(), second.display().to_string());
    assert!(second.exists());

    backend
        .release(&ws(&second, Outdated::Keep))
        .expect("removed");
}

#[test]
#[ignore = "needs Docker Desktop running"]
fn docker_account_switch() {
    pull("docker");
    account_switch(Isolation::Docker {
        image: TEST_IMAGE.into(),
    });
}

#[test]
#[ignore = "needs Docker Sandboxes (sbx)"]
fn sbx_account_switch() {
    account_switch(Isolation::DockerSandbox { template: None });
}

/// Two projects on one account: each has its own Claude Code config folder, writable, and sees
/// nothing of the other's or of the account's other files; only the login is shared. The project
/// folder is shared between worktrees until it's turned off.
fn claude_files_and_project_folder(isolation: Isolation) {
    let home = std::env::var("HOME").unwrap();
    let base = Path::new(&home).join("worki");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix(".e2e-")
        .tempdir_in(&base)
        .unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "hello\n").unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "init",
        ],
    );
    let (wt_a, wt_b) = (root.join("wt-a"), root.join("wt-b"));
    for (wt, branch) in [(&wt_a, "a"), (&wt_b, "b")] {
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                wt.to_str().unwrap(),
                "main",
            ],
        );
    }

    let account = root.join("account");
    std::fs::create_dir_all(account.join("projects/-elsewhere")).unwrap();
    std::fs::write(account.join("settings.json"), r#"{"from":"account"}"#).unwrap();
    std::fs::write(account.join(".credentials.json"), "login").unwrap();
    let login = worki_isolation::account_login_dir(&account).unwrap();
    assert!(
        !account.join(".credentials.json").exists(),
        "the login moves to its folder"
    );
    let own = |name: &str| {
        let dir = root.join(format!("project {name}"));
        std::fs::create_dir_all(dir.join("skills")).unwrap();
        std::fs::write(dir.join("settings.json"), format!(r#"{{"from":"{name}"}}"#)).unwrap();
        dir
    };
    let (own_a, own_b) = (own("a"), own("b"));
    let shared = root.join("shared");

    let backend = isolation.backend();
    backend.ready().expect("engine available and running");
    let ws = |wt: &Path, dir: &Path, shared_folder: bool, outdated| Workspace {
        repo: repo.clone(),
        worktree_root: root.clone(),
        worktree: Some(wt.to_path_buf()),
        claude_dir: Some(dir.to_path_buf()),
        claude_login_dir: Some(login.clone()),
        shared_folder: shared_folder.then(|| shared.clone()),
        outdated,
    };
    let in_env = |wt: &Path, command: &str| {
        run(backend
            .launch(&LaunchRequest {
                cwd: wt.to_path_buf(),
                command: Some(command.into()),
                interactive: false,
                stdin: false,
                env: vec![],
                session: None,
            })
            .unwrap())
    };
    let read = |p: PathBuf| std::fs::read_to_string(p).unwrap();

    backend
        .grant_access(&ws(&wt_a, &own_a, true, Outdated::Keep))
        .expect("environment a");
    backend
        .grant_access(&ws(&wt_b, &own_b, true, Outdated::Keep))
        .expect("environment b");

    for (wt, dir, other) in [(&wt_a, &own_a, &own_b), (&wt_b, &own_b, &own_a)] {
        let (ok, out) = in_env(
            wt,
            &format!(
                r#"test "$CLAUDE_CONFIG_DIR" = '{}' && test "$CLAUDE_SECURESTORAGE_CONFIG_DIR" = '{}' && ! test -e '{}' && ! test -e '{}' && ! test -e '{}'"#,
                dir.display(),
                login.display(),
                account.join("settings.json").display(),
                account.join("projects").display(),
                other.display(),
            ),
        );
        assert!(
            ok,
            "{}: only its own config folder and the login: {out}",
            dir.display()
        );
    }

    let (ok, out) = in_env(
        &wt_a,
        r#"echo '{"from":"session"}' > "$CLAUDE_CONFIG_DIR/settings.json" && echo memory > "$CLAUDE_CONFIG_DIR/CLAUDE.md" && mkdir -p "$CLAUDE_CONFIG_DIR/skills/new""#,
    );
    assert!(ok, "the config folder must be writable: {out}");
    assert_eq!(
        read(own_a.join("settings.json")),
        "{\"from\":\"session\"}\n"
    );
    assert_eq!(read(own_a.join("CLAUDE.md")), "memory\n");
    assert_eq!(read(own_b.join("settings.json")), r#"{"from":"b"}"#);
    assert_eq!(read(account.join("settings.json")), r#"{"from":"account"}"#);

    let (ok, out) = in_env(
        &wt_a,
        r#"echo renewed > "$CLAUDE_SECURESTORAGE_CONFIG_DIR/.credentials.json""#,
    );
    assert!(ok, "the login must stay writable: {out}");
    assert_eq!(read(login.join(".credentials.json")), "renewed\n");
    let (ok, out) = in_env(
        &wt_b,
        r#"cat "$CLAUDE_SECURESTORAGE_CONFIG_DIR/.credentials.json""#,
    );
    assert!(
        ok && out.contains("renewed"),
        "the login must be shared: {out}"
    );

    let (ok, out) = in_env(
        &wt_a,
        r#"test "$WORKI_PROJECT_DIR" = /worki/project && echo from-a > "$WORKI_PROJECT_DIR/a""#,
    );
    assert!(ok, "project folder in a: {out}");
    let (ok, out) = in_env(
        &wt_b,
        "cat /worki/project/a && echo from-b > /worki/project/b",
    );
    assert!(ok && out.contains("from-a"), "project folder in b: {out}");
    assert_eq!(read(shared.join("a")), "from-a\n");
    assert_eq!(read(shared.join("b")), "from-b\n");

    backend
        .grant_access(&ws(&wt_a, &own_a, false, Outdated::Replace))
        .expect("replaced without the project folder");
    let (ok, out) = in_env(
        &wt_a,
        r#"test -z "$WORKI_PROJECT_DIR" && ! test -e /worki/project/a"#,
    );
    assert!(
        ok,
        "the project folder must be gone after turning it off: {out}"
    );
    assert_eq!(read(shared.join("b")), "from-b\n");

    backend
        .release(&ws(&wt_a, &own_a, false, Outdated::Keep))
        .expect("a removed");
    backend
        .release(&ws(&wt_b, &own_b, true, Outdated::Keep))
        .expect("b removed");
    for wt in [&wt_a, &wt_b] {
        git(
            &repo,
            &["worktree", "remove", "--force", wt.to_str().unwrap()],
        );
    }
}

#[test]
#[ignore = "needs Docker Desktop running"]
fn docker_claude_files_and_project_folder() {
    pull("docker");
    claude_files_and_project_folder(Isolation::Docker {
        image: TEST_IMAGE.into(),
    });
}

#[test]
#[ignore = "needs Docker Sandboxes (sbx)"]
fn sbx_claude_files_and_project_folder() {
    claude_files_and_project_folder(Isolation::DockerSandbox { template: None });
}

#[test]
#[ignore = "needs a running Podman machine"]
fn podman_claude_files_and_project_folder() {
    pull("podman");
    claude_files_and_project_folder(Isolation::Podman {
        image: TEST_IMAGE.into(),
    });
}

/// Signing an account in or checking it runs with a throwaway config folder: Claude Code's files
/// go there, and only the login reaches the account's login folder.
fn account_launch_keeps_the_account_to_its_login() {
    let home = std::env::var("HOME").unwrap();
    let base = Path::new(&home).join("worki");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix(".e2e-")
        .tempdir_in(&base)
        .unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let (scratch, account) = (root.join("scratch"), root.join("account"));
    let login = worki_isolation::account_login_dir(&account).unwrap();
    let account_run = worki_isolation::account_launch(
        &scratch,
        Some(&login),
        r#"echo config > "$CLAUDE_CONFIG_DIR/.claude.json" && echo login > "$CLAUDE_SECURESTORAGE_CONFIG_DIR/.credentials.json""#,
        false,
    )
    .expect("an engine with worki's image");
    let (ok, out) = run(account_run.launch.clone());
    assert!(ok, "{out}");
    assert_eq!(
        std::fs::read_to_string(scratch.join(".claude.json")).unwrap(),
        "config\n"
    );
    assert_eq!(
        std::fs::read_to_string(login.join(".credentials.json")).unwrap(),
        "login\n"
    );
    let mut in_account: Vec<String> = std::fs::read_dir(&account)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    in_account.sort();
    assert_eq!(in_account, ["login"]);
}

#[test]
#[ignore = "needs Docker Desktop running and worki's image"]
fn docker_account_launch_keeps_the_account_to_its_login() {
    account_launch_keeps_the_account_to_its_login();
}
