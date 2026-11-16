use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("worki")
}

pub fn account_dir(id: &str) -> PathBuf {
    data_dir().join("accounts").join(slug(id))
}

/// What worki keeps for a project outside its repository.
pub fn project_dir(project_id: &str) -> PathBuf {
    data_dir().join("projects").join(slug(project_id))
}

/// The project's own Claude Code files: its settings, `CLAUDE.md` and skills, and with
/// [`crate::AccountSignIn::Token`] everything else Claude Code keeps too.
pub fn project_claude_dir(project_id: &str) -> PathBuf {
    project_dir(project_id).join("claude")
}

/// The folder all the project's worktrees share ([`crate::ProjectEnvironment::shared_folder`]).
pub fn project_shared_dir(project_id: &str) -> PathBuf {
    project_dir(project_id).join("shared")
}

/// What worki keeps for one of the project's worktrees.
pub fn worktree_dir(project_id: &str, worktree: &Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    let name = worktree
        .file_name()
        .map(|n| slug(&n.to_string_lossy()))
        .unwrap_or_default();
    let hash = Sha256::digest(worktree.to_string_lossy().as_bytes());
    let id: String = hash.iter().take(4).map(|b| format!("{b:02x}")).collect();
    project_dir(project_id)
        .join("worktrees")
        .join(format!("{name}-{id}"))
}

pub fn default_worktree_root(project_name: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("worki")
        .join("worktrees")
        .join(slug(project_name))
}

/// Filesystem-friendly form of a branch or project name: `feature/Login Page` → `feature-login-page`.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        "worktree".into()
    } else {
        trimmed.into()
    }
}

/// Branch name for a user-typed worktree name: whitespace runs become `-`, case and `/` are kept.
pub fn branch_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join("-")
}

pub fn worktree_path(root: &Path, branch: &str) -> PathBuf {
    root.join(slug(branch))
}

pub fn expand_tilde(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| path.into()),
        Err(_) => path.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_dirs_are_per_path() {
        let a = worktree_dir("p", Path::new("/w/feat/login"));
        let b = worktree_dir("p", Path::new("/w/feat-login"));
        assert_ne!(a, b);
        assert_eq!(a, worktree_dir("p", Path::new("/w/feat/login")));
        assert!(a.starts_with(project_dir("p").join("worktrees")));
        assert!(a
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("login-"));
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("feature/Login Page"), "feature-login-page");
        assert_eq!(slug("fix//crash__v2"), "fix-crash__v2");
        assert_eq!(slug("/../"), "worktree");
        assert_eq!(slug("release-1.2"), "release-1.2");
    }

    #[test]
    fn branch_names() {
        assert_eq!(branch_name("  Agent Login/v2 "), "Agent-Login/v2");
        assert_eq!(branch_name("fix  the\tbug"), "fix-the-bug");
    }
}
