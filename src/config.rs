use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// User configuration, loaded from `~/.config/pelper/config.toml`
/// (or `$XDG_CONFIG_HOME/pelper/config.toml`). A starter file is written
/// on first run so it is easy to discover and edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Directories whose immediate git subdirectories are treated as projects.
    #[serde(default = "default_roots")]
    pub roots: Vec<PathBuf>,
    /// Branch names treated as the project's "main" branch, in priority order.
    #[serde(default = "default_branches")]
    pub default_branches: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: default_roots(),
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
            let cfg = Config::default();
            let _ = cfg.write_to(&path); // best-effort starter file
            cfg
        };
        cfg.roots = cfg.roots.into_iter().map(expand_tilde).collect();
        Ok(cfg)
    }

    fn write_to(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

fn home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn config_path() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("pelper").join("config.toml")
    } else {
        home().join(".config").join("pelper").join("config.toml")
    }
}

fn default_roots() -> Vec<PathBuf> {
    vec![home().join("coding")]
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
