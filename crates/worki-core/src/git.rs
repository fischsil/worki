use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::host::Host;
use crate::model::{
    BaseUpdate, BranchChanges, ChangedFile, CommitDraft, CommitInfo, GitOperation, LineCounts,
    Worktree, WorktreeStatus,
};
use crate::prompt::Part;
use crate::{Error, Result};

/// Resolves any path inside a repository or one of its worktrees to the main checkout.
pub async fn main_checkout(host: &Host, path: &Path) -> Result<PathBuf> {
    let common = host
        .git(
            path,
            ["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await
        .map_err(|_| Error::NotARepo(path.into()))?;
    let common = PathBuf::from(common.trim());
    match (common.file_name(), common.parent()) {
        (Some(name), Some(parent)) if name == ".git" => Ok(parent.into()),
        _ => Err(Error::NotARepo(path.into())),
    }
}

pub async fn list_worktrees(host: &Host, repo: &Path) -> Result<Vec<Worktree>> {
    let (worktrees, bases, created) = tokio::join!(
        worktrees(host, repo),
        branch_bases(host, repo),
        worktree_creation_times(host, repo)
    );
    let mut worktrees = worktrees?;
    for wt in &mut worktrees {
        wt.base = wt.branch.as_ref().and_then(|b| bases.get(b).cloned());
    }
    sort_newest_first(&mut worktrees, &created);
    Ok(worktrees)
}

/// The repository's worktrees as git lists them, without their bases, in git's order.
pub async fn worktrees(host: &Host, repo: &Path) -> Result<Vec<Worktree>> {
    let listing = host.git(repo, ["worktree", "list", "--porcelain"]).await?;
    Ok(parse_worktree_porcelain(&listing))
}

/// Keeps the main checkout on top, then the newest worktree first.
fn sort_newest_first(worktrees: &mut [Worktree], created: &HashMap<PathBuf, i64>) {
    worktrees.sort_by_key(|wt| {
        (
            !wt.is_main,
            std::cmp::Reverse(created.get(&wt.path).copied().unwrap_or(i64::MIN)),
        )
    });
}

/// When each linked worktree was added, by path. Git writes a worktree's `commondir` file once,
/// when it's added, so its modification time is the creation time.
async fn worktree_creation_times(host: &Host, repo: &Path) -> HashMap<PathBuf, i64> {
    const SCRIPT: &str = r#"common=$(git -C "$1" rev-parse --path-format=absolute --git-common-dir) || exit 1
for d in "$common"/worktrees/*; do
  [ -f "$d/commondir" ] && [ -f "$d/gitdir" ] || continue
  t=$(stat -c %Y "$d/commondir" 2>/dev/null || stat -f %m "$d/commondir") || continue
  printf '%s\t%s\n' "$t" "$(cat "$d/gitdir")"
done"#;
    let Ok(out) = host.sh(SCRIPT, &[&repo.to_string_lossy()]).await else {
        return HashMap::new();
    };
    parse_creation_times(&String::from_utf8_lossy(&out.stdout))
}

fn parse_creation_times(text: &str) -> HashMap<PathBuf, i64> {
    text.lines()
        .filter_map(|line| {
            let (time, gitdir) = line.split_once('\t')?;
            let worktree = Path::new(gitdir.trim()).parent()?;
            Some((worktree.to_path_buf(), time.trim().parse().ok()?))
        })
        .collect()
}

/// Kept in the branch's git config, so it follows the branch when it's renamed and goes away
/// when it's deleted.
const BASE_KEY: &str = "workibase";

/// Records the branch's target: what it was started from, or picked later.
pub async fn set_branch_base(host: &Host, repo: &Path, branch: &str, base: &str) -> Result<()> {
    let key = format!("branch.{branch}.{BASE_KEY}");
    host.git(repo, ["config", "--local", &key, base])
        .await
        .map(drop)
}

/// What each branch was started from, by branch name.
async fn branch_bases(host: &Host, repo: &Path) -> HashMap<String, String> {
    let pattern = format!(r"^branch\..*\.{BASE_KEY}$");
    let out = host
        .git(repo, ["config", "--local", "--get-regexp", &pattern])
        .await
        .unwrap_or_default();
    parse_branch_bases(&out)
}

fn parse_branch_bases(config: &str) -> HashMap<String, String> {
    config
        .lines()
        .filter_map(|line| {
            let (key, base) = line.split_once(' ')?;
            let branch = key
                .strip_prefix("branch.")?
                .strip_suffix(&format!(".{BASE_KEY}"))?;
            Some((branch.to_owned(), base.trim().to_owned()))
        })
        .collect()
}

pub fn parse_worktree_porcelain(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    for block in text.split("\n\n").filter(|b| !b.trim().is_empty()) {
        let mut wt = Worktree {
            path: PathBuf::new(),
            branch: None,
            base: None,
            head: String::new(),
            is_main: out.is_empty(),
            locked: false,
            prunable: false,
        };
        let mut bare = false;
        for line in block.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "worktree" => wt.path = PathBuf::from(value),
                "HEAD" => wt.head = value.into(),
                "branch" => {
                    wt.branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value).into())
                }
                "bare" => bare = true,
                "locked" => wt.locked = true,
                "prunable" => wt.prunable = true,
                _ => {}
            }
        }
        if !bare {
            out.push(wt);
        }
    }
    out
}

pub async fn default_base(host: &Host, repo: &Path) -> String {
    if let Ok(remote_head) = host
        .git(
            repo,
            ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        )
        .await
    {
        let remote_head = remote_head.trim();
        let local = remote_head.strip_prefix("origin/").unwrap_or(remote_head);
        if branch_exists(host, repo, local).await {
            return local.into();
        }
        return remote_head.into();
    }
    for candidate in ["main", "master"] {
        if branch_exists(host, repo, candidate).await {
            return candidate.into();
        }
    }
    host.git(repo, ["branch", "--show-current"])
        .await
        .map(|s| s.trim().to_owned())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "main".into())
}

pub async fn branch_exists(host: &Host, repo: &Path, branch: &str) -> bool {
    host.git(
        repo,
        [
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .await
    .is_ok()
}

pub async fn ref_exists(host: &Host, repo: &Path, name: &str) -> bool {
    host.git(
        repo,
        [
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    )
    .await
    .is_ok()
}

pub async fn list_branches(host: &Host, repo: &Path) -> Result<Vec<String>> {
    let out = host
        .git(
            repo,
            [
                "for-each-ref",
                "--sort=-committerdate",
                "--format=%(refname:short)\t%(upstream:track)",
                "refs/heads",
                "refs/remotes",
            ],
        )
        .await?;
    Ok(out
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(name, track)| !name.ends_with("/HEAD") && *name != "origin" && *track != "[gone]")
        .map(|(name, _)| name.to_owned())
        .collect())
}

pub async fn add_worktree(
    host: &Host,
    repo: &Path,
    path: &Path,
    new_branch: &str,
    base: &str,
) -> Result<()> {
    let path = path.to_string_lossy();
    host.git(
        repo,
        [
            "worktree",
            "add",
            "--no-track",
            "-b",
            new_branch,
            &path,
            base,
        ],
    )
    .await
    .map(drop)
    .map_err(|e| match e {
        Error::Git { stderr, .. } if stderr.contains("is not a valid branch name") => {
            Error::InvalidBranch(new_branch.into())
        }
        e => e,
    })
}

pub async fn remove_worktree(host: &Host, repo: &Path, path: &Path, force: bool) -> Result<()> {
    let path = path.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path);
    host.git(repo, args).await.map(drop)
}

pub async fn delete_branch(host: &Host, repo: &Path, branch: &str, force: bool) -> Result<()> {
    host.git(repo, ["branch", if force { "-D" } else { "-d" }, branch])
        .await
        .map(drop)
}

pub async fn status(host: &Host, worktree: &Path) -> Result<WorktreeStatus> {
    let porcelain = host
        .git(
            worktree,
            [
                "status",
                "--porcelain=v2",
                "--branch",
                "--untracked-files=all",
                "--no-renames",
                "-z",
            ],
        )
        .await?;
    let mut status = parse_status_v2(&porcelain);
    if !status.conflicts.is_empty() {
        status.stopped = stopped_operation(host, worktree).await;
    }
    drop_sandbox_placeholders(host, worktree, &mut status.changed_files);
    let pushed = status_branch(&porcelain).map(|branch| format!("origin/{branch}"));
    let (totals, counts) = tokio::join!(
        count_lines(host, worktree, "HEAD", false, &mut status.changed_files),
        async {
            match &pushed {
                Some(remote) => ahead_behind(host, worktree, remote).await.ok(),
                None => None,
            }
        }
    );
    if let Ok(totals) = totals {
        (status.insertions, status.deletions) = totals;
    }
    if let Some(counts) = counts {
        (status.ahead, status.behind) = counts;
        status.upstream = pushed;
    }
    Ok(status)
}

async fn stopped_operation(host: &Host, worktree: &Path) -> Option<GitOperation> {
    const MARKERS: [(&str, GitOperation); 5] = [
        ("rebase-merge", GitOperation::Rebase),
        ("rebase-apply", GitOperation::Rebase),
        ("MERGE_HEAD", GitOperation::Merge),
        ("CHERRY_PICK_HEAD", GitOperation::CherryPick),
        ("REVERT_HEAD", GitOperation::Revert),
    ];
    let mut args = vec!["rev-parse", "--path-format=absolute"];
    for (marker, _) in MARKERS {
        args.extend(["--git-path", marker]);
    }
    let paths = host.git(worktree, args).await.ok()?;
    for (path, (_, op)) in paths.lines().zip(MARKERS) {
        if host.exists(Path::new(path)).await.unwrap_or(false) {
            return Some(op);
        }
    }
    None
}

/// Leaves out the empty, read-only files Claude Code's sandbox puts in the worktree to protect
/// paths like `.bashrc` or `.claude/skills`, and sometimes leaves behind. Only checked on this
/// machine.
fn drop_sandbox_placeholders(host: &Host, worktree: &Path, files: &mut Vec<ChangedFile>) {
    if !matches!(host, Host::Local) {
        return;
    }
    files.retain(|f| {
        f.code != "??"
            || !std::fs::metadata(worktree.join(&f.path))
                .is_ok_and(|m| m.is_file() && m.len() == 0 && m.permissions().readonly())
    });
}

fn status_branch(porcelain: &str) -> Option<&str> {
    porcelain
        .split('\0')
        .find_map(|t| t.strip_prefix("# branch.head "))
        .filter(|b| *b != "(detached)")
}

/// How many commits HEAD has that `other` doesn't, and the other way around.
async fn ahead_behind(host: &Host, worktree: &Path, other: &str) -> Result<(u32, u32)> {
    let out = host
        .git(
            worktree,
            [
                "rev-list",
                "--left-right",
                "--count",
                &format!("HEAD...{other}"),
            ],
        )
        .await?;
    let mut counts = out.split_whitespace().map(|n| n.parse().unwrap_or(0));
    Ok((counts.next().unwrap_or(0), counts.next().unwrap_or(0)))
}

pub fn parse_status_v2(text: &str) -> WorktreeStatus {
    let mut status = WorktreeStatus::default();
    let mut tokens = text.split('\0').filter(|t| !t.is_empty());
    while let Some(entry) = tokens.next() {
        let mut push = |code: &str, path: &str| {
            status.changed_files.push(ChangedFile {
                path: path.into(),
                code: code.replace('.', " "),
                ..ChangedFile::default()
            })
        };
        if let Some(rest) = entry.strip_prefix("1 ") {
            if let Some((xy, path)) = code_and_path(rest, 8) {
                push(xy, path);
            }
        } else if let Some(rest) = entry.strip_prefix("2 ") {
            if let Some((xy, path)) = code_and_path(rest, 9) {
                push(xy, path);
            }
            tokens.next();
        } else if let Some(rest) = entry.strip_prefix("u ") {
            if let Some((xy, path)) = code_and_path(rest, 10) {
                push(xy, path);
                status.conflicts.push(path.into());
            }
        } else if let Some(path) = entry.strip_prefix("? ") {
            push("??", path);
        }
    }
    status
}

/// The `XY` code and the path of a porcelain v2 entry with `fields` space-separated fields, the
/// last being the path (which may itself contain spaces).
fn code_and_path(entry: &str, fields: usize) -> Option<(&str, &str)> {
    let mut parts = entry.splitn(fields, ' ');
    let xy = parts.next()?;
    Some((xy, parts.nth(fields - 2)?))
}

/// Largest diff returned to the UI; bigger ones are cut off with a note.
const MAX_DIFF_BYTES: usize = 1024 * 1024;

/// Unified diff of one file in a worktree against `HEAD` (staged and unstaged changes), or of an
/// untracked file against nothing. The worktree may be written by an agent, so external diff
/// drivers and text conversion are disabled. The whole file is kept as context so the UI can
/// reveal unchanged lines around the changes.
pub async fn file_diff(
    host: &Host,
    worktree: &Path,
    file: &str,
    base: Option<&str>,
) -> Result<String> {
    let rel = Path::new(file);
    if rel.as_os_str().is_empty()
        || !rel
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::Git {
            args: "diff".into(),
            stderr: format!("not a path inside the worktree: {file}"),
        });
    }
    let from = match base {
        Some(base) => merge_base(host, worktree, base).await?,
        None => "HEAD".to_owned(),
    };
    let tracked = host
        .git(worktree, ["ls-files", "--error-unmatch", "--", file])
        .await
        .is_ok()
        || host
            .git(worktree, ["cat-file", "-e", &format!("{from}:{file}")])
            .await
            .is_ok();
    let args: Vec<&str> = if tracked {
        vec![
            "diff",
            &from,
            "--unified=1000000",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "-M",
            "--",
            file,
        ]
    } else {
        vec![
            "diff",
            "--no-index",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--",
            "/dev/null",
            file,
        ]
    };
    let (_, out) = host.git_output(worktree, &args).await?;
    // `--no-index` exits 1 when the files differ, which is the expected case here.
    if !out.status.success() && !(out.status.code() == Some(1) && !tracked) {
        return Err(Error::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().into(),
        });
    }
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.len() > MAX_DIFF_BYTES {
        let cut = (0..=MAX_DIFF_BYTES)
            .rev()
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(0);
        text.truncate(cut);
        text.push_str("\n\\ worki: diff truncated (larger than 1 MB)\n");
    }
    Ok(text)
}

async fn merge_base(host: &Host, worktree: &Path, base: &str) -> Result<String> {
    Ok(host
        .git(worktree, ["merge-base", "HEAD", base])
        .await?
        .trim()
        .to_owned())
}

const MAX_DIFF: usize = 30_000;

/// The branch's commits, changed files and diff since it left `base`, for an agent to describe.
pub async fn pr_context(host: &Host, worktree: &Path, base: &str) -> Result<Vec<Part>> {
    let from = merge_base(host, worktree, base).await?;
    let log = host
        .git(
            worktree,
            [
                "log",
                "--format=- %s%n%w(0,2,2)%b",
                &format!("{from}..HEAD"),
            ],
        )
        .await?;
    let stat = host
        .git(worktree, ["diff", "--stat", &from, "HEAD"])
        .await?;
    let diff = host
        .git(worktree, ["diff", "--no-ext-diff", &from, "HEAD"])
        .await?;
    Ok(vec![
        Part {
            name: "commits",
            label: "Commits",
            text: log,
        },
        Part {
            name: "files",
            label: "Changed files",
            text: stat,
        },
        Part {
            name: "diff",
            label: "Diff",
            text: cut_off(diff),
        },
    ])
}

/// The uncommitted changes to `files`, untracked ones included, and the latest commit subjects
/// for their style, for an agent to write a commit message.
pub async fn commit_context(host: &Host, worktree: &Path, files: &[String]) -> Result<Vec<Part>> {
    let log = host
        .git(worktree, ["log", "-10", "--format=- %s"])
        .await
        .unwrap_or_default();
    let mut args = vec!["diff", "HEAD", "--no-ext-diff", "--no-color", "-M", "--"];
    args.extend(files.iter().map(String::as_str));
    let mut diff = host.git(worktree, &args).await?;
    let mut args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    args.extend(files.iter().map(String::as_str));
    let untracked = host.git(worktree, &args).await?;
    for file in untracked.split('\0').filter(|f| !f.is_empty()) {
        let (_, out) = host
            .git_output(
                worktree,
                &[
                    "diff",
                    "--no-index",
                    "--no-color",
                    "--no-ext-diff",
                    "--",
                    "/dev/null",
                    file,
                ],
            )
            .await?;
        diff.push_str(&String::from_utf8_lossy(&out.stdout));
        if diff.len() > MAX_DIFF {
            break;
        }
    }
    Ok(vec![
        Part {
            name: "log",
            label: "Recent commit messages",
            text: log,
        },
        Part {
            name: "files",
            label: "Files to commit",
            text: files.iter().map(|f| format!("{f}\n")).collect(),
        },
        Part {
            name: "diff",
            label: "Changes to commit",
            text: cut_off(diff),
        },
    ])
}

/// How the agent answers with the commits to make, so worki can read them.
pub const COMMITS_ANSWER: &str = r#"Answer with only a JSON array of the commits to make, in order: [{"message": "...", "files": ["path", ...]}], and nothing else. Every file to commit belongs to exactly one commit."#;

/// The commits in an agent's answer, ignoring any text or code fence around the JSON. Only
/// `files` are kept, each in its first commit; files the agent left out join the last commit.
pub fn parse_commits(answer: &str, files: &[String]) -> Option<Vec<CommitDraft>> {
    let json = &answer[answer.find('[')?..=answer.rfind(']')?];
    let drafts: Vec<CommitDraft> = serde_json::from_str(json).ok()?;
    let mut left: Vec<&String> = files.iter().collect();
    let mut commits: Vec<CommitDraft> = drafts
        .into_iter()
        .filter_map(|d| {
            let files: Vec<String> = d
                .files
                .into_iter()
                .filter(|f| match left.iter().position(|l| *l == f) {
                    Some(i) => {
                        left.remove(i);
                        true
                    }
                    None => false,
                })
                .collect();
            let message = d.message.trim().to_owned();
            (!files.is_empty() && !message.is_empty()).then_some(CommitDraft { message, files })
        })
        .collect();
    commits.last_mut()?.files.extend(left.into_iter().cloned());
    Some(commits)
}

fn cut_off(mut diff: String) -> String {
    if diff.len() > MAX_DIFF {
        let cut = (0..=MAX_DIFF)
            .rev()
            .find(|&i| diff.is_char_boundary(i))
            .unwrap_or(0);
        diff.truncate(cut);
        diff.push_str("\n[diff cut off]\n");
    }
    diff
}

/// Everything the worktree changes compared to where it left `base`: its commits plus
/// uncommitted and untracked files.
pub async fn branch_changes(host: &Host, worktree: &Path, base: &str) -> Result<BranchChanges> {
    let from = merge_base(host, worktree, base).await?;
    let names = host
        .git(
            worktree,
            ["diff", &from, "--name-status", "-z", "-M", "--no-ext-diff"],
        )
        .await?;
    let mut files = parse_name_status(&names);
    let untracked = host
        .git(
            worktree,
            ["ls-files", "--others", "--exclude-standard", "-z"],
        )
        .await?;
    files.extend(
        untracked
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(|p| ChangedFile {
                path: p.into(),
                code: "??".into(),
                ..ChangedFile::default()
            }),
    );
    drop_sandbox_placeholders(host, worktree, &mut files);
    let (insertions, deletions) = count_lines(host, worktree, &from, true, &mut files).await?;
    let commits = host
        .git(worktree, ["rev-list", "--count", &format!("{from}..HEAD")])
        .await?
        .trim()
        .parse()
        .unwrap_or(0);
    let behind = host
        .git(worktree, ["rev-list", "--count", &format!("HEAD..{base}")])
        .await?
        .trim()
        .parse()
        .unwrap_or(0);
    Ok(BranchChanges {
        base: base.to_owned(),
        files,
        insertions,
        deletions,
        commits,
        behind,
    })
}

/// Commits `files` (adding, removing or renaming them as they are in the worktree) with
/// `message`, leaving any other changes uncommitted. Returns the new commit's id.
pub async fn commit(
    host: &Host,
    worktree: &Path,
    files: &[String],
    message: &str,
) -> Result<String> {
    if files.is_empty() || message.trim().is_empty() {
        return Err(Error::Git {
            args: "commit".into(),
            stderr: "pick at least one file and write a message".into(),
        });
    }
    // A deletion staged already (`git rm`) is in neither the index nor the worktree, so
    // `git add` rejects its path; `git commit -- <path>` still records it.
    let mut known = vec![
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
        "--",
    ];
    known.extend(files.iter().map(String::as_str));
    let known = host.git(worktree, &known).await?;
    let known: Vec<&str> = known.split('\0').filter(|f| !f.is_empty()).collect();
    if !known.is_empty() {
        let mut add = vec!["add", "-A", "--"];
        add.extend(known);
        host.git(worktree, &add).await?;
    }
    let mut commit = vec!["commit", "-q", "-m", message.trim(), "--"];
    commit.extend(files.iter().map(String::as_str));
    host.git(worktree, &commit).await?;
    Ok(host
        .git(worktree, ["rev-parse", "HEAD"])
        .await?
        .trim()
        .to_owned())
}

/// The worktree's commits since it left `base`, newest first; without a base, the latest 30.
pub async fn branch_log(
    host: &Host,
    worktree: &Path,
    base: Option<&str>,
) -> Result<Vec<CommitInfo>> {
    let range = match base {
        Some(base) => format!("{}..HEAD", merge_base(host, worktree, base).await?),
        None => "-30".to_owned(),
    };
    let text = host
        .git(
            worktree,
            ["log", "--format=%H%x1f%s%x1f%an%x1f%ct%x1e", &range],
        )
        .await?;
    Ok(parse_log(&text))
}

pub fn parse_log(text: &str) -> Vec<CommitInfo> {
    text.split('\x1e')
        .filter_map(|record| {
            let mut f = record.trim_matches(['\n', '\r']).split('\x1f');
            Some(CommitInfo {
                sha: f.next().filter(|s| !s.is_empty())?.to_owned(),
                subject: f.next()?.to_owned(),
                author: f.next()?.to_owned(),
                time: f.next()?.trim().parse().ok()?,
            })
        })
        .collect()
}

/// What a worktree is compared with and rebased onto: `base` as it was picked, local or remote.
/// When it's gone, such as a branch deleted after its pull request was merged, the default
/// branch, preferring the remote one.
pub async fn compare_base(host: &Host, worktree: &Path, base: &str) -> String {
    if usable_base(host, worktree, base).await {
        return base.to_owned();
    }
    let default = default_base(host, worktree).await;
    let local = default.strip_prefix("origin/").unwrap_or(&default);
    for candidate in [format!("origin/{local}"), local.to_owned()] {
        if usable_base(host, worktree, &candidate).await {
            return candidate;
        }
    }
    base.to_owned()
}

async fn usable_base(host: &Host, worktree: &Path, base: &str) -> bool {
    ref_exists(host, worktree, base).await
        && (base.starts_with("origin/") || !upstream_gone(host, worktree, base).await)
}

/// Whether local branch `branch` tracks a remote branch that was deleted.
async fn upstream_gone(host: &Host, worktree: &Path, branch: &str) -> bool {
    host.git(
        worktree,
        [
            "for-each-ref",
            "--format=%(upstream:track)",
            &format!("refs/heads/{branch}"),
        ],
    )
    .await
    .is_ok_and(|track| track.trim() == "[gone]")
}

/// Fetches all remotes, dropping branches deleted there.
pub async fn fetch(host: &Host, worktree: &Path) -> Result<()> {
    host.git(worktree, ["fetch", "--all", "--prune", "--quiet"])
        .await
        .map(drop)
}

/// Rebases the worktree's branch onto `base` (uncommitted changes are stashed and put back). On
/// conflicts the rebase is undone and the conflicting files are reported.
pub async fn update_from_base(host: &Host, worktree: &Path, base: &str) -> Result<BaseUpdate> {
    let before = host.git(worktree, ["rev-parse", "HEAD"]).await?;
    let (_, out) = host
        .git_output(worktree, ["rebase", "--autostash", base])
        .await?;
    if out.status.success() {
        let after = host.git(worktree, ["rev-parse", "HEAD"]).await?;
        return Ok(if after == before {
            BaseUpdate::UpToDate
        } else {
            BaseUpdate::Updated
        });
    }
    let conflicts = host
        .git(worktree, ["diff", "--name-only", "--diff-filter=U", "-z"])
        .await
        .unwrap_or_default();
    host.git(worktree, ["rebase", "--abort"]).await.ok();
    let files: Vec<String> = conflicts
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(String::from)
        .collect();
    if files.is_empty() {
        return Err(Error::Git {
            args: format!("rebase {base}"),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(BaseUpdate::Conflicts { files })
}

/// Fills in each file's added and removed lines and returns the totals of tracked files (what
/// `git diff --shortstat` reports). New files are only counted on this machine.
async fn count_lines(
    host: &Host,
    worktree: &Path,
    from: &str,
    renames: bool,
    files: &mut [ChangedFile],
) -> Result<(u32, u32)> {
    let counts = parse_numstat(
        &host
            .git(
                worktree,
                [
                    "diff",
                    from,
                    "--numstat",
                    "-z",
                    if renames { "-M" } else { "--no-renames" },
                    "--no-ext-diff",
                ],
            )
            .await?,
    );
    let totals = counts
        .values()
        .flatten()
        .fold((0, 0), |(a, r), c| (a + c.added, r + c.removed));
    let untracked: Vec<PathBuf> = files
        .iter()
        .filter(|f| f.code == "??" && matches!(host, Host::Local))
        .map(|f| worktree.join(&f.path))
        .collect();
    let mut new_lines = tokio::task::spawn_blocking(move || {
        untracked
            .iter()
            .map(|p| untracked_lines(p))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
    .into_iter();
    for file in files {
        file.lines = if file.code != "??" {
            counts.get(&file.path).copied().flatten()
        } else if matches!(host, Host::Local) {
            new_lines.next().flatten()
        } else {
            None
        };
    }
    Ok(totals)
}

/// Lines in a new file, or `None` for binary or very large files.
fn untracked_lines(path: &Path) -> Option<LineCounts> {
    const MAX_BYTES: u64 = 4 << 20;
    if std::fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    let unterminated = bytes.last().is_some_and(|&b| b != b'\n');
    Some(LineCounts {
        added: (newlines + usize::from(unterminated)) as u32,
        removed: 0,
    })
}

/// `git diff --numstat -z` output by path (the new path for renames); `None` for binary files.
pub fn parse_numstat(text: &str) -> HashMap<String, Option<LineCounts>> {
    let mut counts = HashMap::new();
    let mut tokens = text.split('\0');
    while let Some(entry) = tokens.next() {
        let mut fields = entry.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let path = if path.is_empty() {
            tokens.next();
            tokens.next().unwrap_or_default()
        } else {
            path
        };
        let lines = added.parse().ok().zip(removed.parse().ok());
        counts.insert(
            path.to_owned(),
            lines.map(|(added, removed)| LineCounts { added, removed }),
        );
    }
    counts
}

/// `git diff --name-status -z` output; renames and copies report the new path.
pub fn parse_name_status(text: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut tokens = text.split('\0').filter(|t| !t.is_empty());
    while let Some(status) = tokens.next() {
        let letter = &status[..1];
        if matches!(letter, "R" | "C") {
            tokens.next();
        }
        if let Some(path) = tokens.next() {
            files.push(ChangedFile {
                path: path.into(),
                code: format!("{letter} "),
                ..ChangedFile::default()
            });
        }
    }
    files
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_commits_from_a_fenced_answer() {
        let files = ["a.rs", "b.rs", "c.rs"].map(String::from);
        let commits = super::parse_commits(
            "```json\n[{\"message\": \" Fix a \", \"files\": [\"a.rs\", \"x.rs\"]}, {\"message\": \"Add b\\n\\nWhy.\", \"files\": [\"b.rs\", \"a.rs\"]}, {\"message\": \"Empty\", \"files\": []}]\n```",
            &files,
        )
        .unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(
            (commits[0].message.as_str(), commits[0].files.clone()),
            ("Fix a", vec!["a.rs".to_string()])
        );
        assert_eq!(commits[1].message, "Add b\n\nWhy.");
        assert_eq!(commits[1].files, ["b.rs", "c.rs"]);
        assert_eq!(super::parse_commits("no json", &files), None);
        assert_eq!(super::parse_commits("[]", &files), None);
    }

    #[test]
    fn parses_name_status_with_renames() {
        let files = parse_name_status("M\0src/a.rs\0R087\0old.rs\0new name.rs\0D\0gone.txt\0");
        let got: Vec<_> = files
            .iter()
            .map(|f| (f.code.as_str(), f.path.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("M ", "src/a.rs"),
                ("R ", "new name.rs"),
                ("D ", "gone.txt")
            ]
        );
    }

    use super::*;

    #[test]
    fn reads_what_branches_were_started_from() {
        let bases = parse_branch_bases(
            "branch.feat/login.workibase origin/develop\nbranch.fix.v1.2.workibase main\n",
        );
        assert_eq!(bases["feat/login"], "origin/develop");
        assert_eq!(bases["fix.v1.2"], "main");
        assert_eq!(bases.len(), 2);
    }

    #[test]
    fn parses_worktree_list() {
        let text = "worktree /r/app\nHEAD abc\nbranch refs/heads/main\n\n\
                    worktree /w/feat\nHEAD def\nbranch refs/heads/feat/x\nlocked\n\n\
                    worktree /w/detached\nHEAD 123\ndetached\nprunable gitdir file points to non-existent location\n\n";
        let wts = parse_worktree_porcelain(text);
        assert_eq!(wts.len(), 3);
        assert!(wts[0].is_main);
        assert_eq!(wts[1].branch.as_deref(), Some("feat/x"));
        assert!(wts[1].locked && !wts[1].is_main);
        assert_eq!(wts[2].branch, None);
        assert!(wts[2].prunable);
    }

    #[test]
    fn skips_bare_entry() {
        let text = "worktree /r/app.git\nbare\n\nworktree /w/a\nHEAD a\nbranch refs/heads/a\n";
        let wts = parse_worktree_porcelain(text);
        assert_eq!(wts.len(), 1);
        assert_eq!(wts[0].path, PathBuf::from("/w/a"));
    }

    #[test]
    fn leaves_out_the_sandbox_placeholder_files() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, content: &str, mode: u32| {
            let path = dir.path().join(name);
            std::fs::write(&path, content).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        file(".bashrc", "", 0o444);
        file("empty.txt", "", 0o644);
        file("readonly.txt", "text", 0o444);
        let changed = |path: &str, code: &str| super::ChangedFile {
            path: path.into(),
            code: code.into(),
            ..Default::default()
        };
        let mut files = vec![
            changed(".bashrc", "??"),
            changed("empty.txt", "??"),
            changed("readonly.txt", "??"),
            changed("src/main.rs", " M"),
        ];
        super::drop_sandbox_placeholders(&super::Host::Local, dir.path(), &mut files);
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["empty.txt", "readonly.txt", "src/main.rs"]);
    }

    #[test]
    fn parses_status_v2() {
        let text = "# branch.oid abc\0# branch.head feat\0# branch.upstream origin/feat\0# branch.ab +2 -1\0\
                    1 .M N... 100644 100644 100644 a a src/main.rs\0\
                    2 R. N... 100644 100644 100644 a a R100 new name.rs\0old.rs\0\
                    u UU N... 100644 100644 100644 100644 a b c both.rs\0\
                    ? notes.txt\0";
        let s = parse_status_v2(text);
        let files: Vec<_> = s
            .changed_files
            .iter()
            .map(|f| (f.code.as_str(), f.path.as_str()))
            .collect();
        assert_eq!(
            files,
            [
                (" M", "src/main.rs"),
                ("R ", "new name.rs"),
                ("UU", "both.rs"),
                ("??", "notes.txt")
            ]
        );
        assert_eq!(s.conflicts, ["both.rs"]);
    }

    #[test]
    fn parses_numstat_with_renames_and_binaries() {
        let counts = super::parse_numstat(concat!(
            "3\t1\tsrc/a.rs\0",
            "-\t-\tlogo.png\0",
            "2\t0\t\0old.rs\0new.rs\0",
        ));
        let lines = |added, removed| Some(super::LineCounts { added, removed });
        assert_eq!(counts["src/a.rs"], lines(3, 1));
        assert_eq!(counts["logo.png"], None);
        assert_eq!(counts["new.rs"], lines(2, 0));
        assert!(!counts.contains_key("old.rs"));
    }
}
