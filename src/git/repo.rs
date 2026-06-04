use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A single discovered git project and a snapshot of its local state.
#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    /// Current branch name, a short SHA when detached, or a status label.
    pub branch: String,
    pub detached: bool,
    pub dirty: bool,
    /// `(ahead, behind)` relative to the configured upstream, if any.
    pub upstream: Option<(usize, usize)>,
    /// Commit time of HEAD.
    pub last_commit: Option<SystemTime>,
    pub error: Option<String>,
}

impl Project {
    fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            branch: "?".into(),
            detached: false,
            dirty: false,
            upstream: None,
            last_commit: None,
            error: None,
        }
    }
}

/// Run `git -C <repo> <args>`, returning stdout on success.
fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(repo).args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    }
}

/// Read a snapshot of `path`'s git state. Never panics: on failure the
/// returned [`Project`] carries an `error` and best-effort fields.
pub fn load_project(path: &Path) -> Project {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let mut p = Project::new(name, path.to_path_buf());

    // One call gives branch, upstream, ahead/behind and dirtiness.
    match git(path, &["status", "--porcelain=v2", "--branch"]) {
        Some(s) => parse_status(&mut p, &s),
        None => p.error = Some("git status failed".into()),
    }

    if let Some(out) = git(path, &["log", "-1", "--format=%ct"]) {
        if let Ok(secs) = out.trim().parse::<u64>() {
            p.last_commit = Some(UNIX_EPOCH + Duration::from_secs(secs));
        }
    }

    p
}

fn parse_status(p: &mut Project, s: &str) {
    let mut oid: Option<String> = None;
    let mut dirty = false;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            oid = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.head ") {
            if rest == "(detached)" {
                p.detached = true;
            } else {
                p.branch = rest.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let mut ahead = 0;
            let mut behind = 0;
            for tok in rest.split_whitespace() {
                if let Some(a) = tok.strip_prefix('+') {
                    ahead = a.parse().unwrap_or(0);
                } else if let Some(b) = tok.strip_prefix('-') {
                    behind = b.parse().unwrap_or(0);
                }
            }
            p.upstream = Some((ahead, behind));
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            dirty = true;
        }
    }
    p.dirty = dirty;

    if p.detached {
        p.branch = match oid.as_deref().filter(|o| *o != "(initial)") {
            Some(sha) => format!("@{}", &sha.chars().take(7).collect::<String>()),
            None => "(detached)".into(),
        };
    } else if oid.as_deref() == Some("(initial)") {
        p.branch = format!("{} (no commits)", p.branch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_ahead_behind_and_dirty() {
        let mut p = Project::new("x".into(), PathBuf::from("/x"));
        let s = "# branch.oid abc123\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n1 .M N... 100644 100644 100644 aaa bbb file.rs\n";
        parse_status(&mut p, s);
        assert_eq!(p.branch, "main");
        assert!(!p.detached);
        assert_eq!(p.upstream, Some((2, 1)));
        assert!(p.dirty);
    }

    #[test]
    fn parses_detached_clean() {
        let mut p = Project::new("x".into(), PathBuf::from("/x"));
        let s = "# branch.oid deadbeefcafe1234\n# branch.head (detached)\n";
        parse_status(&mut p, s);
        assert!(p.detached);
        assert_eq!(p.branch, "@deadbee");
        assert!(!p.dirty);
        assert_eq!(p.upstream, None);
    }

    #[test]
    fn clean_repo_no_upstream() {
        let mut p = Project::new("x".into(), PathBuf::from("/x"));
        let s = "# branch.oid abc\n# branch.head main\n";
        parse_status(&mut p, s);
        assert!(!p.dirty);
        assert_eq!(p.upstream, None);
        assert_eq!(p.branch, "main");
    }
}
