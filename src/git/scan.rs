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

fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Immediate subdirectories of `roots` that look like git repositories.
pub fn candidate_repos(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && is_git_repo(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Scan `roots` on a background thread, streaming each project as it is read
/// and a final [`ScanMsg::Done`]. Work is spread across a small worker pool.
pub fn spawn_scan(roots: Vec<PathBuf>) -> Receiver<ScanMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let dirs = candidate_repos(&roots);
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
pub fn scan_blocking(roots: &[PathBuf]) -> Vec<Project> {
    let rx = spawn_scan(roots.to_vec());
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
