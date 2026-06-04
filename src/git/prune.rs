use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::SystemTime;

use super::load_branches;

/// A local branch whose upstream is gone (deleted on the remote ⇒ merged),
/// and therefore a candidate for deletion.
#[derive(Debug, Clone)]
pub struct PruneCandidate {
    pub project: String,
    pub path: PathBuf,
    pub branch: String,
    pub last_commit: Option<SystemTime>,
    /// Commits on this branch not reachable from the default branch. `0` means
    /// fully contained (a normal merge); `>0` is expected for squash-merges but
    /// also flags genuinely un-merged work worth a second look.
    pub unique_commits: usize,
}

pub enum PruneScanMsg {
    Found(Box<PruneCandidate>),
    Done,
}

#[derive(Debug, Clone)]
pub struct DeleteResult {
    pub ok: bool,
    /// The tip SHA, so the branch is recoverable from the reflog.
    pub sha: Option<String>,
    pub message: String,
}

const PRUNE_WORKERS: usize = 6;

fn run(repo: &Path, args: &[&str]) -> (bool, String, String) {
    match Command::new("git").arg("-C").arg(repo).args(args).output() {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(e) => (false, String::new(), e.to_string()),
    }
}

fn default_branch(repo: &Path, defaults: &[String]) -> Option<String> {
    defaults
        .iter()
        .find(|b| run(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{b}")]).0)
        .cloned()
}

fn unique_commits(repo: &Path, default: &str, branch: &str) -> usize {
    let (ok, out, _) = run(repo, &["rev-list", "--count", &format!("{default}..{branch}")]);
    ok.then(|| out.trim().parse().ok()).flatten().unwrap_or(0)
}

/// For one repo: refresh remotes, then find gone branches (excluding the
/// current and default branches).
fn collect_for_repo(name: &str, path: &Path, defaults: &[String]) -> Vec<PruneCandidate> {
    // Refresh so `gone` is accurate. Ignore failure (offline ⇒ use known refs).
    let _ = run(path, &["fetch", "--all", "--prune"]);

    let default = default_branch(path, defaults);
    load_branches(path, defaults)
        .into_iter()
        .filter(|b| b.gone && !b.is_head && default.as_deref() != Some(b.name.as_str()))
        .map(|b| {
            let unique = match &default {
                Some(d) => unique_commits(path, d, &b.name),
                None => 0,
            };
            PruneCandidate {
                project: name.to_string(),
                path: path.to_path_buf(),
                branch: b.name,
                last_commit: b.last_commit,
                unique_commits: unique,
            }
        })
        .collect()
}

/// Collect prune candidates across all targets concurrently, streaming results.
pub fn spawn_prune_scan(
    targets: Vec<(String, PathBuf)>,
    default_branches: Vec<String>,
) -> Receiver<PruneScanMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let workers = targets.len().clamp(1, PRUNE_WORKERS);
        let mut chunks: Vec<Vec<(String, PathBuf)>> = (0..workers).map(|_| Vec::new()).collect();
        for (i, target) in targets.into_iter().enumerate() {
            chunks[i % workers].push(target);
        }

        let mut handles = Vec::new();
        for chunk in chunks {
            let tx = tx.clone();
            let defaults = default_branches.clone();
            handles.push(thread::spawn(move || {
                for (name, path) in chunk {
                    for candidate in collect_for_repo(&name, &path, &defaults) {
                        let _ = tx.send(PruneScanMsg::Found(Box::new(candidate)));
                    }
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
        let _ = tx.send(PruneScanMsg::Done);
    });
    rx
}

/// Delete a local branch with `-D`. Force is required because squash-merged
/// branches are not recognised as merged by `-d`; the reflog keeps the tip
/// recoverable, so we surface its SHA.
pub fn delete_branch(repo: &Path, branch: &str) -> DeleteResult {
    let (ok, out, err) = run(repo, &["branch", "-D", branch]);
    let sha = out
        .split_once("(was ")
        .and_then(|(_, rest)| rest.split(')').next())
        .map(|s| s.trim().to_string());
    DeleteResult {
        ok,
        sha,
        message: first_line(if ok { &out } else { &err }),
    }
}

fn first_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CTR: AtomicUsize = AtomicUsize::new(0);

    fn tmp() -> PathBuf {
        let n = CTR.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("pelper-prune-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn git(cwd: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args([
                "-c", "user.email=pelper@test",
                "-c", "user.name=pelper",
                "-c", "commit.gpgsign=false",
                "-c", "init.defaultBranch=main",
            ])
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap()
    }

    fn git_ok(cwd: &Path, args: &[&str]) {
        let o = git(cwd, args);
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    }

    #[test]
    fn finds_and_deletes_gone_branch() {
        let base = tmp();
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        git_ok(&origin, &["init"]);
        std::fs::write(origin.join("a.txt"), "1").unwrap();
        git_ok(&origin, &["add", "."]);
        git_ok(&origin, &["commit", "-m", "init"]);
        git_ok(&origin, &["branch", "-M", "main"]);

        git_ok(&base, &["clone", origin.to_str().unwrap(), "work"]);
        let work = base.join("work");

        // A feature branch with its own commit, pushed to the remote.
        git_ok(&work, &["checkout", "-b", "feature"]);
        std::fs::write(work.join("f.txt"), "x").unwrap();
        git_ok(&work, &["add", "."]);
        git_ok(&work, &["commit", "-m", "feat"]);
        git_ok(&work, &["push", "-u", "origin", "feature"]);
        git_ok(&work, &["checkout", "main"]);

        // Delete it on the remote — collect_for_repo's fetch --prune marks it gone.
        git_ok(&origin, &["branch", "-D", "feature"]);

        let defaults = vec!["main".to_string(), "master".to_string()];
        let candidates = collect_for_repo("work", &work, &defaults);
        assert_eq!(candidates.len(), 1, "expected one gone branch");
        assert_eq!(candidates[0].branch, "feature");
        assert_eq!(candidates[0].unique_commits, 1, "feature has one commit not in main");

        let result = delete_branch(&work, "feature");
        assert!(result.ok, "delete failed: {}", result.message);
        assert!(result.sha.is_some(), "deleted SHA should be reported");

        assert!(
            collect_for_repo("work", &work, &defaults).is_empty(),
            "branch should be gone after deletion"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
