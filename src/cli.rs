use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::config::Config;
use crate::git::{self, time::relative, PruneScanMsg, UpdateMsg, UpdateStatus};

/// Resolve discovered projects to `(name, path)` targets, optionally filtered
/// to a single project by name.
fn resolve_targets(cfg: &Config, project: Option<String>) -> Result<Vec<(String, PathBuf)>> {
    let mut targets: Vec<(String, PathBuf)> = git::scan_blocking(&cfg.roots)
        .into_iter()
        .map(|p| (p.name, p.path))
        .collect();
    if let Some(name) = project {
        targets.retain(|(n, _)| n == &name);
        if targets.is_empty() {
            bail!("no project named '{name}' under the configured roots");
        }
    }
    Ok(targets)
}

/// `pelper scan` — list discovered projects as a plain table.
pub fn run_scan(cfg: &Config) -> Result<()> {
    let projects = git::scan_blocking(&cfg.roots);
    if projects.is_empty() {
        println!("No git projects found under:");
        for root in &cfg.roots {
            println!("  {}", root.display());
        }
        return Ok(());
    }

    println!("{:<27}{:<24} {:>9}  {:<10}", "PROJECT", "BRANCH", "SYNC", "LAST");
    for p in &projects {
        let sync = match p.upstream {
            Some((0, 0)) => "ok".to_string(),
            Some((a, b)) => format!("+{a} -{b}"),
            None => "-".to_string(),
        };
        let last = p.last_commit.map(relative).unwrap_or_else(|| "-".into());
        let dirty = if p.dirty { "*" } else { " " };
        println!(
            "{}{:<26}{:<24} {:>9}  {:<10}",
            dirty,
            truncate(&p.name, 26),
            truncate(&p.branch, 24),
            sync,
            last
        );
    }
    println!("\n{} project(s).", projects.len());
    Ok(())
}

/// `pelper update` — fetch and fast-forward each project's default branch.
pub fn run_update(cfg: &Config, project: Option<String>) -> Result<()> {
    let targets = resolve_targets(cfg, project)?;
    if targets.is_empty() {
        println!("No projects to update.");
        return Ok(());
    }
    println!("Updating {} project(s)…\n", targets.len());

    let rx = git::spawn_update(targets, cfg.default_branches.clone());
    let mut outcomes = Vec::new();
    for msg in rx {
        match msg {
            UpdateMsg::Done(o) => outcomes.push(*o),
            UpdateMsg::AllDone => break,
            UpdateMsg::Started(_) => {}
        }
    }
    outcomes.sort_by_key(|o| o.name.to_lowercase());

    for o in &outcomes {
        println!("{} {:<26} {}", status_icon(o.status), truncate(&o.name, 26), o.message);
    }

    let (mut updated, mut current, mut warning, mut failed, mut skipped) = (0, 0, 0, 0, 0);
    for o in &outcomes {
        match o.status {
            UpdateStatus::Updated => updated += 1,
            UpdateStatus::UpToDate => current += 1,
            UpdateStatus::Warning => warning += 1,
            UpdateStatus::Failed => failed += 1,
            UpdateStatus::Skipped => skipped += 1,
        }
    }
    println!(
        "\n{updated} updated · {current} current · {warning} warning · {failed} failed · {skipped} skipped"
    );
    Ok(())
}

/// `pelper prune` — list (dry-run) or delete (`--yes`) gone/merged branches.
pub fn run_prune(cfg: &Config, project: Option<String>, yes: bool) -> Result<()> {
    let targets = resolve_targets(cfg, project)?;
    if targets.is_empty() {
        println!("No projects to prune.");
        return Ok(());
    }
    println!("Scanning {} project(s) for merged (gone) branches…\n", targets.len());

    let rx = git::spawn_prune_scan(targets, cfg.default_branches.clone());
    let mut candidates = Vec::new();
    for msg in rx {
        match msg {
            PruneScanMsg::Found(c) => candidates.push(*c),
            PruneScanMsg::Done => break,
        }
    }
    candidates.sort_by(|a, b| {
        a.project
            .to_lowercase()
            .cmp(&b.project.to_lowercase())
            .then_with(|| a.branch.cmp(&b.branch))
    });

    if candidates.is_empty() {
        println!("No merged (gone) branches to prune. 🎉");
        return Ok(());
    }

    if !yes {
        println!(
            "Would delete {} gone branch(es) (dry-run — pass --yes to delete):",
            candidates.len()
        );
        for c in &candidates {
            let status = if c.unique_commits == 0 {
                "merged".to_string()
            } else {
                format!("{} ahead", c.unique_commits)
            };
            println!(
                "  {:<24} {:<42} {}",
                truncate(&c.project, 24),
                truncate(&c.branch, 42),
                status
            );
        }
        return Ok(());
    }

    println!("Deleting {} gone branch(es)…\n", candidates.len());
    let (mut deleted, mut failed) = (0, 0);
    for c in &candidates {
        let result = git::delete_branch(&c.path, &c.branch);
        if result.ok {
            deleted += 1;
            let sha = result.sha.unwrap_or_default();
            println!(
                "  ✓ {:<24} {:<42} deleted (was {sha})",
                truncate(&c.project, 24),
                truncate(&c.branch, 42)
            );
        } else {
            failed += 1;
            println!(
                "  ✗ {:<24} {:<42} {}",
                truncate(&c.project, 24),
                truncate(&c.branch, 42),
                result.message
            );
        }
    }
    println!("\nDeleted {deleted} · failed {failed}.");
    Ok(())
}

fn status_icon(status: UpdateStatus) -> &'static str {
    match status {
        UpdateStatus::Updated => "✓",
        UpdateStatus::UpToDate => "=",
        UpdateStatus::Warning => "⚠",
        UpdateStatus::Failed => "✗",
        UpdateStatus::Skipped => "⊘",
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}
