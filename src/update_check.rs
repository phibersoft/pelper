use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// Parse a `vX.Y.Z` tag (ignoring any prerelease suffix) into a comparable
/// `(major, minor, patch)` triple.
fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let core = s.trim().trim_start_matches('v');
    let core = core.split('-').next().unwrap_or(core);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Spawn a silent background check for a newer release. Sends `Some(version)`
/// when the remote has a tag newer than the running build, otherwise `None`
/// (also `None` on any error — the check never surfaces failures).
pub fn spawn_check() -> Receiver<Option<String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(latest_newer());
    });
    rx
}

fn latest_newer() -> Option<String> {
    let url = option_env!("CARGO_PKG_REPOSITORY")?;
    if url.is_empty() {
        return None;
    }
    let current = parse_semver(env!("CARGO_PKG_VERSION"))?;

    // `git ls-remote` reuses the one dependency pelper already requires (git),
    // needs no auth for a public repo, and works cross-platform.
    let out = Command::new("git")
        .args(["ls-remote", "--tags", "--refs", url])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }

    let mut best: Option<(u32, u32, u32)> = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(tag) = line.split("refs/tags/").nth(1) {
            if let Some(v) = parse_semver(tag) {
                if best.is_none_or(|b| v > b) {
                    best = Some(v);
                }
            }
        }
    }

    let best = best?;
    (best > current).then(|| format!("v{}.{}.{}", best.0, best.1, best.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_versions() {
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_semver("v1.0.0-rc.1"), Some((1, 0, 0)));
        assert_eq!(parse_semver("not-a-version"), None);
        assert!((0, 3, 0) > (0, 2, 9));
        assert!((1, 0, 0) > (0, 9, 9));
    }
}
