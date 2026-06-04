//! pelper — a terminal DX helper that brings your everyday dev tools into one
//! polished TUI. Each tool is a self-contained feature; "Projects" (a git repo
//! overview with update and prune) is the first of many.

mod cli;
mod config;
mod git;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::Config;

/// Command-line entry point. With no subcommand, launches the interactive TUI.
#[derive(Parser)]
#[command(
    name = "pelper",
    version,
    about = "A terminal DX helper — your everyday dev tools in one TUI"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Scan configured roots and print discovered git projects.
    Scan,
    /// Fetch and fast-forward each project's default branch.
    Update {
        /// Limit to a single project by name.
        #[arg(long)]
        project: Option<String>,
    },
    /// List local branches whose upstream is gone (merged); dry-run unless --yes.
    Prune {
        /// Limit to a single project by name.
        #[arg(long)]
        project: Option<String>,
        /// Actually delete the branches (default is a dry-run).
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load()?;
    match cli.command {
        Some(Command::Scan) => cli::run_scan(&cfg),
        Some(Command::Update { project }) => cli::run_update(&cfg, project),
        Some(Command::Prune { project, yes }) => cli::run_prune(&cfg, project, yes),
        None => tui::run(cfg),
    }
}
