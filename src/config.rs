use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// User configuration, read from `~/.config/pelper/config.toml`
/// (or `$XDG_CONFIG_HOME/pelper/config.toml`) when present. Config is optional:
/// with no file, pelper scans the current working directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Directories whose immediate git subdirectories are treated as projects.
    /// When empty, the current working directory is scanned.
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    /// Branch names treated as a project's "main", in priority order.
    #[serde(default = "default_branches")]
    pub default_branches: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            default_branches: default_branches(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let mut cfg = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading config at {}", path.display()))?;
            toml::from_str(&text).with_context(|| format!("parsing config at {}", path.display()))?
        } else {
            Config::default()
        };
        cfg.roots = cfg.roots.into_iter().map(expand_tilde).collect();
        // No roots configured ⇒ scan wherever pelper was launched from.
        if cfg.roots.is_empty() {
            cfg.roots = vec![current_dir()];
        }
        Ok(cfg)
    }
}

fn home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn config_path() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("pelper").join("config.toml")
    } else {
        home().join(".config").join("pelper").join("config.toml")
    }
}

fn default_branches() -> Vec<String> {
    vec!["main".into(), "master".into()]
}

fn expand_tilde(p: PathBuf) -> PathBuf {
    match p.strip_prefix("~") {
        Ok(rest) => home().join(rest),
        Err(_) => p,
    }
}
