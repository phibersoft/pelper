use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use super::repo::{load_project, Project};

/// Streamed scan results.
pub enum ScanMsg {
    Found(Box<Project>),
    Done,
}

const MAX_WORKERS: usize = 8;
/// How deep to recurse under each root looking for repositories.
const MAX_DEPTH: usize = 6;

fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Recursively find git repositories under `roots`. Skips hidden directories
/// and any directory named in `ignore`, and never descends into a repository
/// once found (so nested repos/submodules aren't treated as separate projects).
pub fn candidate_repos(roots: &[PathBuf], ignore: &[String]) -> Vec<PathBuf> {
    let ignore: HashSet<&str> = ignore.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for root in roots {
        walk(root, 0, &ignore, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn walk(dir: &Path, depth: usize, ignore: &HashSet<&str>, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // Only real directories — skip files and symlinks (avoids cycles).
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => {}
            _ => continue,
        }
        let path = entry.path();
        if is_git_repo(&path) {
            out.push(path);
            continue; // a repo is a leaf — don't descend into it
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || ignore.contains(name.as_ref()) {
            continue;
        }
        if depth + 1 < MAX_DEPTH {
            walk(&path, depth + 1, ignore, out);
        }
    }
}

/// Scan `roots` on background threads, streaming each project as it is read and
/// a final [`ScanMsg::Done`].
pub fn spawn_scan(roots: Vec<PathBuf>, ignore: Vec<String>) -> Receiver<ScanMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let dirs = candidate_repos(&roots, &ignore);
        let workers = dirs.len().clamp(1, MAX_WORKERS);
        let mut chunks: Vec<Vec<PathBuf>> = (0..workers).map(|_| Vec::new()).collect();
        for (i, dir) in dirs.into_iter().enumerate() {
            chunks[i % workers].push(dir);
        }

        let mut handles = Vec::new();
        for chunk in chunks {
            let tx = tx.clone();
            handles.push(thread::spawn(move || {
                for dir in chunk {
                    let _ = tx.send(ScanMsg::Found(Box::new(load_project(&dir))));
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
        let _ = tx.send(ScanMsg::Done);
    });
    rx
}

/// Scan synchronously and return the sorted result.
pub fn scan_blocking(roots: &[PathBuf], ignore: &[String]) -> Vec<Project> {
    let rx = spawn_scan(roots.to_vec(), ignore.to_vec());
    let mut projects = Vec::new();
    for msg in rx {
        match msg {
            ScanMsg::Found(p) => projects.push(*p),
            ScanMsg::Done => break,
        }
    }
    sort_projects(&mut projects);
    projects
}

/// Sort most-recently-committed first; repos without a commit date go last,
/// ties broken by case-insensitive name.
pub fn sort_projects(projects: &mut [Project]) {
    use std::cmp::Ordering;
    projects.sort_by(|a, b| {
        let by_time = match (a.last_commit, b.last_commit) {
            (Some(at), Some(bt)) => bt.cmp(&at),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        by_time.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CTR: AtomicUsize = AtomicUsize::new(0);

    fn tmp() -> PathBuf {
        let n = CTR.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("pelper-scan-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn finds_nested_repos_and_skips_ignored() {
        let base = tmp();
        std::fs::create_dir_all(base.join("group/proj/.git")).unwrap();
        std::fs::create_dir_all(base.join("node_modules/pkg/.git")).unwrap();
        std::fs::create_dir_all(base.join(".hidden/repo/.git")).unwrap();

        let found = candidate_repos(&[base.clone()], &["node_modules".to_string()]);

        assert!(
            found.iter().any(|p| p.ends_with("group/proj")),
            "should find the nested repo, got {found:?}"
        );
        assert!(
            !found.iter().any(|p| p.to_string_lossy().contains("node_modules")),
            "should skip node_modules"
        );
        assert!(
            !found.iter().any(|p| p.to_string_lossy().contains(".hidden")),
            "should skip hidden dirs"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
