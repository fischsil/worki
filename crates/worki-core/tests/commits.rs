use std::path::Path;
use std::process::Command;

use worki_core::{git, github};
use worki_core::{BaseUpdate, GitOperation, Host};

fn sh(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    sh(dir, &["init", "-q", "-b", "main"]);
    sh(dir, &["config", "user.name", "t"]);
    sh(dir, &["config", "user.email", "t@t"]);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    sh(dir, &["add", "."]);
    sh(dir, &["commit", "-q", "-m", "init"]);
    sh(dir, &["checkout", "-q", "-b", "work"]);
    tmp
}

#[tokio::test]
async fn commits_only_the_picked_files() {
    let tmp = repo();
    let dir = tmp.path();
    let host = Host::Local;
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();
    std::fs::write(dir.join("new.txt"), "new\n").unwrap();
    std::fs::write(dir.join("keep.txt"), "later\n").unwrap();

    git::commit(
        &host,
        dir,
        &["a.txt".into(), "new.txt".into()],
        "Change a, add new",
    )
    .await
    .unwrap();

    let log = git::branch_log(&host, dir, Some("main")).await.unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].subject, "Change a, add new");
    let recent = git::branch_log(&host, dir, None).await.unwrap();
    assert_eq!(
        recent
            .iter()
            .map(|c| c.subject.as_str())
            .collect::<Vec<_>>(),
        ["Change a, add new", "init"]
    );
    let left = host.git(dir, ["status", "--porcelain"]).await.unwrap();
    assert_eq!(left.trim(), "?? keep.txt");
    assert!(git::commit(&host, dir, &[], "nothing").await.is_err());
    assert_eq!(git::compare_base(&host, dir, "main").await, "main");
}

#[tokio::test]
async fn commits_a_deletion_that_is_already_staged() {
    let tmp = repo();
    let dir = tmp.path();
    let host = Host::Local;
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    git::commit(&host, dir, &["b.txt".into()], "Add b")
        .await
        .unwrap();
    sh(dir, &["rm", "-q", "a.txt"]);
    std::fs::remove_file(dir.join("b.txt")).unwrap();

    git::commit(
        &host,
        dir,
        &["a.txt".into(), "b.txt".into()],
        "Remove a and b",
    )
    .await
    .unwrap();

    let left = host.git(dir, ["ls-files"]).await.unwrap();
    assert_eq!(left.trim(), "");
    let status = host.git(dir, ["status", "--porcelain"]).await.unwrap();
    assert_eq!(status.trim(), "");
}

#[tokio::test]
async fn a_staged_rename_lists_and_commits_both_paths() {
    let tmp = repo();
    let dir = tmp.path();
    let host = Host::Local;
    sh(dir, &["mv", "a.txt", "b.txt"]);

    let status = git::status(&host, dir).await.unwrap();
    let files: Vec<_> = status
        .changed_files
        .iter()
        .map(|f| (f.code.as_str(), f.path.as_str()))
        .collect();
    assert_eq!(files, [("D ", "a.txt"), ("A ", "b.txt")]);

    git::commit(&host, dir, &["a.txt".into(), "b.txt".into()], "Rename a")
        .await
        .unwrap();
    let left = host.git(dir, ["status", "--porcelain"]).await.unwrap();
    assert_eq!(left.trim(), "");
}

#[tokio::test]
async fn compares_with_the_default_branch_once_the_base_is_gone() {
    let tmp = repo();
    let dir = tmp.path();
    let host = Host::Local;
    sh(
        dir,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    sh(dir, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    sh(
        dir,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    assert_eq!(
        git::compare_base(&host, dir, "origin/feature").await,
        "origin/main"
    );
    assert_eq!(
        git::compare_base(&host, dir, "feature").await,
        "origin/main"
    );

    assert_eq!(git::compare_base(&host, dir, "main").await, "main");
    assert_eq!(
        git::compare_base(&host, dir, "origin/main").await,
        "origin/main"
    );

    sh(dir, &["branch", "merged"]);
    assert_eq!(git::compare_base(&host, dir, "merged").await, "merged");
    sh(dir, &["config", "branch.merged.remote", "origin"]);
    sh(dir, &["config", "branch.merged.merge", "refs/heads/merged"]);
    assert_eq!(git::compare_base(&host, dir, "merged").await, "origin/main");
    let branches = git::list_branches(&host, dir).await.unwrap();
    assert!(branches.contains(&"origin/main".to_owned()));
    assert!(!branches.contains(&"merged".to_owned()));

    sh(dir, &["tag", "v1"]);
    assert_eq!(git::compare_base(&host, dir, "v1").await, "v1");
}

#[tokio::test]
async fn update_from_base_rebases_or_reports_conflicts() {
    let tmp = repo();
    let dir = tmp.path();
    let host = Host::Local;
    assert_eq!(
        git::update_from_base(&host, dir, "main").await.unwrap(),
        BaseUpdate::UpToDate
    );

    std::fs::write(dir.join("b.txt"), "mine\n").unwrap();
    git::commit(&host, dir, &["b.txt".into()], "Add b")
        .await
        .unwrap();
    sh(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("c.txt"), "theirs\n").unwrap();
    sh(dir, &["add", "c.txt"]);
    sh(dir, &["commit", "-q", "-m", "Add c"]);
    sh(dir, &["checkout", "-q", "work"]);
    assert_eq!(
        git::update_from_base(&host, dir, "main").await.unwrap(),
        BaseUpdate::Updated
    );
    assert!(dir.join("c.txt").exists());

    sh(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("a.txt"), "main\n").unwrap();
    sh(dir, &["commit", "-q", "-am", "main edits a"]);
    sh(dir, &["checkout", "-q", "work"]);
    std::fs::write(dir.join("a.txt"), "work\n").unwrap();
    git::commit(&host, dir, &["a.txt".into()], "work edits a")
        .await
        .unwrap();
    let before = host.git(dir, ["rev-parse", "HEAD"]).await.unwrap();
    assert_eq!(
        git::update_from_base(&host, dir, "main").await.unwrap(),
        BaseUpdate::Conflicts {
            files: vec!["a.txt".into()]
        }
    );
    assert_eq!(
        host.git(dir, ["rev-parse", "HEAD"]).await.unwrap(),
        before,
        "rebase undone"
    );
}

#[tokio::test]
async fn a_new_target_shows_and_rebases_everything_it_lacks() {
    let tmp = repo();
    let dir = tmp.path();
    let host = Host::Local;
    sh(dir, &["checkout", "-q", "-b", "release", "main"]);
    sh(dir, &["commit", "-q", "--allow-empty", "-m", "release fix"]);
    sh(dir, &["checkout", "-q", "main"]);
    sh(dir, &["commit", "-q", "--allow-empty", "-m", "main only"]);
    sh(dir, &["checkout", "-q", "work"]);
    sh(dir, &["reset", "-q", "--hard", "main"]);
    sh(dir, &["commit", "-q", "--allow-empty", "-m", "mine"]);

    git::set_branch_base(&host, dir, "work", "release")
        .await
        .unwrap();
    let ahead = git::branch_log(&host, dir, Some("release")).await.unwrap();
    assert_eq!(
        ahead.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
        ["mine", "main only"]
    );
    assert_eq!(
        git::update_from_base(&host, dir, "release").await.unwrap(),
        BaseUpdate::Updated
    );
    let log = Command::new("git")
        .args(["log", "--format=%s", "release..HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "mine\nmain only"
    );
}

fn diverge(dir: &Path) {
    std::fs::write(dir.join("a.txt"), "ours\n").unwrap();
    sh(dir, &["commit", "-q", "-am", "ours"]);
    sh(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("a.txt"), "theirs\n").unwrap();
    sh(dir, &["commit", "-q", "-am", "theirs"]);
    sh(dir, &["checkout", "-q", "work"]);
}

#[tokio::test]
async fn status_reports_what_stopped_on_conflicts() {
    for (command, operation) in [
        ("merge", GitOperation::Merge),
        ("rebase", GitOperation::Rebase),
    ] {
        let tmp = repo();
        let dir = tmp.path();
        diverge(dir);
        let _ = Command::new("git")
            .args([command, "-q", "main"])
            .current_dir(dir)
            .output();

        let status = git::status(&Host::Local, dir).await.unwrap();
        assert_eq!(status.conflicts, ["a.txt"], "{command}");
        assert_eq!(status.stopped, Some(operation), "{command}");
    }
}

#[tokio::test]
async fn status_has_no_conflicts_in_a_clean_worktree() {
    let tmp = repo();
    let status = git::status(&Host::Local, tmp.path()).await.unwrap();
    assert!(status.conflicts.is_empty());
    assert_eq!(status.stopped, None);
}

#[tokio::test]
async fn status_counts_against_where_a_push_goes() {
    let (tmp, _remote) = with_remote();
    let dir = tmp.path();
    sh(dir, &["fetch", "-q", "origin"]);
    sh(dir, &["branch", "-q", "--set-upstream-to", "origin/main"]);
    sh(dir, &["commit", "-q", "--allow-empty", "-m", "pushed"]);
    sh(dir, &["push", "-q", "origin", "work"]);
    sh(dir, &["commit", "-q", "--allow-empty", "-m", "not yet"]);

    let status = git::status(&Host::Local, dir).await.unwrap();
    assert_eq!(status.upstream.as_deref(), Some("origin/work"));
    assert_eq!((status.ahead, status.behind), (1, 0));
}

fn with_remote() -> (tempfile::TempDir, tempfile::TempDir) {
    let tmp = repo();
    let remote = tempfile::tempdir().unwrap();
    sh(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    let url = remote.path().to_str().unwrap();
    sh(tmp.path(), &["remote", "add", "origin", url]);
    sh(tmp.path(), &["push", "-q", "origin", "main"]);
    (tmp, remote)
}

#[tokio::test]
async fn push_replaces_the_branch_after_a_rebase() {
    let (tmp, _remote) = with_remote();
    let dir = tmp.path();
    let host = Host::Local;
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    sh(dir, &["add", "."]);
    sh(dir, &["commit", "-q", "-m", "feature"]);
    github::push(&host, dir).await.unwrap();

    sh(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("c.txt"), "c\n").unwrap();
    sh(dir, &["add", "."]);
    sh(dir, &["commit", "-q", "-m", "on main"]);
    sh(dir, &["checkout", "-q", "work"]);
    sh(dir, &["rebase", "-q", "main"]);

    github::push(&host, dir).await.unwrap();
    let local = host.git(dir, ["rev-parse", "HEAD"]).await.unwrap();
    let remote = host.git(dir, ["rev-parse", "origin/work"]).await.unwrap();
    assert_eq!(local, remote);
}

#[tokio::test]
async fn push_keeps_commits_someone_else_pushed() {
    let (tmp, remote) = with_remote();
    let dir = tmp.path();
    let host = Host::Local;
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    sh(dir, &["add", "."]);
    sh(dir, &["commit", "-q", "-m", "feature"]);
    github::push(&host, dir).await.unwrap();

    let other = tempfile::tempdir().unwrap();
    let clone = other.path().join("clone");
    sh(
        other.path(),
        &[
            "clone",
            "-q",
            "-b",
            "work",
            remote.path().to_str().unwrap(),
            "clone",
        ],
    );
    sh(&clone, &["config", "user.name", "t"]);
    sh(&clone, &["config", "user.email", "t@t"]);
    std::fs::write(clone.join("d.txt"), "d\n").unwrap();
    sh(&clone, &["add", "."]);
    sh(&clone, &["commit", "-q", "-m", "someone else"]);
    sh(&clone, &["push", "-q", "origin", "work"]);

    sh(dir, &["fetch", "-q", "origin"]);
    sh(dir, &["commit", "-q", "--amend", "-m", "feature, reworded"]);

    assert!(github::push(&host, dir).await.is_err());
    let theirs = host.git(&clone, ["rev-parse", "HEAD"]).await.unwrap();
    let remote_tip = host.git(dir, ["rev-parse", "origin/work"]).await.unwrap();
    assert_eq!(theirs, remote_tip);
}
