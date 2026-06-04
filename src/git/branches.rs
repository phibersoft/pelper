use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A local branch and a snapshot of its state, for the project detail view.
#[derive(Debug, Clone)]
pub struct Branch {
    pub name: String,
    pub last_commit: Option<SystemTime>,
    pub author: String,
    pub subject: String,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    /// Upstream is configured but gone from the remote — a prune candidate.
    pub gone: bool,
    /// This is the currently checked-out branch.
    pub is_head: bool,
}

/// Tab-separated so author names (which may contain spaces) stay in one field.
const FORMAT: &str = "%(refname:short)%09%(committerdate:unix)%09%(authorname)%09%(upstream:short)%09%(upstream:track,nobracket)%09%(HEAD)%09%(contents:subject)";

/// List local branches of `repo`, primary branches (per `default_branches`,
/// in priority order) first, then most-recently-committed first.
pub fn load_branches(repo: &Path, default_branches: &[String]) -> Vec<Branch> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format",
            FORMAT,
            "refs/heads",
        ])
        .output();

    let mut branches: Vec<Branch> = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter_map(parse_branch)
            .collect(),
        _ => Vec::new(),
    };

    branches.sort_by(|a, b| branch_order(a, b, default_branches));
    branches
}

fn parse_branch(line: &str) -> Option<Branch> {
    let mut fields = line.splitn(7, '\t');
    let name = fields.next()?.to_string();
    if name.is_empty() {
        return None;
    }
    let date = fields.next().unwrap_or("");
    let author = fields.next().unwrap_or("").to_string();
    let upstream = fields.next().unwrap_or("");
    let track = fields.next().unwrap_or("");
    let head = fields.next().unwrap_or("");
    let subject = fields.next().unwrap_or("").to_string();

    let last_commit = date
        .trim()
        .parse::<u64>()
        .ok()
        .map(|s| UNIX_EPOCH + Duration::from_secs(s));

    let (ahead, behind, gone) = parse_track(track);

    Some(Branch {
        name,
        last_commit,
        author,
        subject,
        upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
        ahead,
        behind,
        gone,
        is_head: head.trim() == "*",
    })
}

/// Parse git's `upstream:track,nobracket`: "", "gone", "ahead 2",
/// "behind 1", or "ahead 2, behind 1".
fn parse_track(track: &str) -> (usize, usize, bool) {
    if track.contains("gone") {
        return (0, 0, true);
    }
    let (mut ahead, mut behind) = (0, 0);
    let tokens: Vec<&str> = track.split_whitespace().collect();
    for pair in tokens.windows(2) {
        let n = pair[1].trim_end_matches(',').parse().unwrap_or(0);
        match pair[0] {
            "ahead" => ahead = n,
            "behind" => behind = n,
            _ => {}
        }
    }
    (ahead, behind, false)
}

fn branch_order(a: &Branch, b: &Branch, defaults: &[String]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let rank_a = defaults.iter().position(|d| *d == a.name);
    let rank_b = defaults.iter().position(|d| *d == b.name);
    match (rank_a, rank_b) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => match (a.last_commit, b.last_commit) {
            (Some(at), Some(bt)) => bt.cmp(&at),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_track_variants() {
        assert_eq!(parse_track(""), (0, 0, false));
        assert_eq!(parse_track("gone"), (0, 0, true));
        assert_eq!(parse_track("ahead 2"), (2, 0, false));
        assert_eq!(parse_track("behind 3"), (0, 3, false));
        assert_eq!(parse_track("ahead 2, behind 1"), (2, 1, false));
    }

    #[test]
    fn parses_branch_line() {
        let line = "feature\t1700000000\tAda Lovelace\torigin/feature\tahead 1\t \tDo the thing";
        let b = parse_branch(line).unwrap();
        assert_eq!(b.name, "feature");
        assert_eq!(b.author, "Ada Lovelace");
        assert_eq!(b.subject, "Do the thing");
        assert_eq!(b.upstream.as_deref(), Some("origin/feature"));
        assert_eq!((b.ahead, b.behind, b.gone), (1, 0, false));
        assert!(!b.is_head);
    }

    #[test]
    fn parses_head_and_gone() {
        let head = parse_branch("main\t1700000000\tAda\t\t\t*\tInit").unwrap();
        assert!(head.is_head);
        assert_eq!(head.upstream, None);

        let stale = parse_branch("old\t1\tAda\torigin/old\tgone\t \tx").unwrap();
        assert!(stale.gone);
    }

    #[test]
    fn primary_branches_sort_first() {
        let defaults = vec!["main".to_string(), "master".to_string()];
        let mk = |name: &str, t: u64| Branch {
            name: name.into(),
            last_commit: Some(UNIX_EPOCH + Duration::from_secs(t)),
            author: String::new(),
            subject: String::new(),
            upstream: None,
            ahead: 0,
            behind: 0,
            gone: false,
            is_head: false,
        };
        let mut v = vec![mk("feature", 100), mk("main", 1), mk("other", 200)];
        v.sort_by(|a, b| branch_order(a, b, &defaults));
        let order: Vec<&str> = v.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(order, vec!["main", "other", "feature"]);
    }
}
