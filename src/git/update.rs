use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    /// The default branch was fast-forwarded.
    Updated,
    /// Already current — nothing to do.
    UpToDate,
    /// Intentionally not touched (detached HEAD, no default branch, …).
    Skipped,
    /// Partially done — needs the user's attention (not-ff, stash left behind).
    Warning,
    /// The update failed outright.
    Failed,
}

#[derive(Debug, Clone)]
pub struct UpdateOutcome {
    pub name: String,
    pub status: UpdateStatus,
    pub message: String,
}

pub enum UpdateMsg {
    Started(String),
    Done(Box<UpdateOutcome>),
    AllDone,
}

/// Network-bound, so a modest pool keeps things lively without hammering.
const UPDATE_WORKERS: usize = 6;

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

fn first_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn is_dirty(repo: &Path) -> bool {
    !run(repo, &["status", "--porcelain"]).1.trim().is_empty()
}

fn local_branch_exists(repo: &Path, name: &str) -> bool {
    run(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{name}")]).0
}

fn current_branch(repo: &Path) -> Option<String> {
    let (ok, out, _) = run(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    ok.then(|| out.trim().to_string())
}

/// Classify the result of `git pull --ff-only`.
fn classify_pull(ok: bool, stdout: &str, stderr: &str) -> (UpdateStatus, String) {
    if !ok {
        return (
            UpdateStatus::Warning,
            format!("not fast-forwardable: {}", first_line(stderr)),
        );
    }
    if stdout.to_lowercase().contains("already up to date") {
        (UpdateStatus::UpToDate, "already up to date".to_string())
    } else {
        (UpdateStatus::Updated, "fast-forwarded".to_string())
    }
}

/// Update one repository: fetch+prune, then fast-forward its default branch,
/// stashing and restoring local changes and returning to the original branch.
pub fn update_project(name: &str, repo: &Path, default_branches: &[String]) -> UpdateOutcome {
    let mk = |status, message: String| UpdateOutcome {
        name: name.to_string(),
        status,
        message,
    };

    let (fetched, _, ferr) = run(repo, &["fetch", "--all", "--prune"]);
    if !fetched {
        return mk(UpdateStatus::Failed, format!("fetch failed: {}", first_line(&ferr)));
    }

    let Some(default) = default_branches
        .iter()
        .find(|b| local_branch_exists(repo, b))
        .cloned()
    else {
        return mk(UpdateStatus::Skipped, "no main/master branch (fetched only)".into());
    };

    let Some(current) = current_branch(repo) else {
        return mk(UpdateStatus::Skipped, "detached HEAD (fetched only)".into());
    };

    let switched = current != default;
    let mut stashed = false;

    if is_dirty(repo) {
        let (ok, _, err) = run(
            repo,
            &["stash", "push", "--include-untracked", "-m", "pelper-autostash"],
        );
        if !ok {
            return mk(UpdateStatus::Failed, format!("could not stash: {}", first_line(&err)));
        }
        stashed = true;
    }

    if switched {
        let (ok, _, err) = run(repo, &["checkout", &default]);
        if !ok {
            if stashed {
                let _ = run(repo, &["stash", "pop"]);
            }
            return mk(UpdateStatus::Failed, format!("checkout {default} failed: {}", first_line(&err)));
        }
    }

    let (pok, pout, perr) = run(repo, &["pull", "--ff-only"]);
    let (status, message) = classify_pull(pok, &pout, &perr);

    if switched {
        let (ok, _, err) = run(repo, &["checkout", &current]);
        if !ok {
            return mk(
                UpdateStatus::Warning,
                format!("{default} {message}, but failed to return to {current}: {}", first_line(&err)),
            );
        }
    }

    if stashed {
        let (ok, _, err) = run(repo, &["stash", "pop"]);
        if !ok {
            return mk(
                UpdateStatus::Warning,
                format!(
                    "{default} {message}; stashed changes NOT reapplied (conflict) — safe in `git stash`: {}",
                    first_line(&err)
                ),
            );
        }
    }

    mk(status, format!("{default}: {message}"))
}

/// Update many repositories concurrently, streaming progress.
pub fn spawn_update(
    targets: Vec<(String, PathBuf)>,
    default_branches: Vec<String>,
) -> Receiver<UpdateMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let workers = targets.len().clamp(1, UPDATE_WORKERS);
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
                    let _ = tx.send(UpdateMsg::Started(name.clone()));
                    let outcome = update_project(&name, &path, &defaults);
                    let _ = tx.send(UpdateMsg::Done(Box::new(outcome)));
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
        let _ = tx.send(UpdateMsg::AllDone);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn classify_pull_cases() {
        assert_eq!(classify_pull(true, "Already up to date.\n", "").0, UpdateStatus::UpToDate);
        assert_eq!(classify_pull(true, "Updating a..b\nFast-forward\n", "").0, UpdateStatus::Updated);
        assert_eq!(
            classify_pull(false, "", "fatal: Not possible to fast-forward, aborting.").0,
            UpdateStatus::Warning
        );
    }

    static CTR: AtomicUsize = AtomicUsize::new(0);

    fn tmp() -> PathBuf {
        let n = CTR.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("pelper-it-{}-{}", std::process::id(), n));
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

    fn head(repo: &Path) -> String {
        String::from_utf8_lossy(&git(repo, &["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string()
    }

    #[test]
    fn update_fast_forwards_default_branch() {
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

        // Advance origin's main.
        std::fs::write(origin.join("b.txt"), "2").unwrap();
        git_ok(&origin, &["add", "."]);
        git_ok(&origin, &["commit", "-m", "second"]);

        let before = head(&work);
        let outcome = update_project("work", &work, &["main".to_string(), "master".to_string()]);
        assert_eq!(outcome.status, UpdateStatus::Updated, "message: {}", outcome.message);
        assert_ne!(before, head(&work), "local main should have advanced");

        let _ = std::fs::remove_dir_all(&base);
    }
}
