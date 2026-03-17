pub mod schema;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::schema::ProjectConfig;

pub fn config_dir() -> PathBuf {
    // Respect XDG_CONFIG_HOME if set, otherwise always use ~/.config.
    // Avoids ~/Library/Application Support on macOS — surprising for a CLI tool.
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("portpilot")
}

pub fn projects_dir() -> PathBuf {
    config_dir().join("projects")
}

pub fn load_project(path: &Path) -> Result<ProjectConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))
}

pub fn save_project(path: &Path, config: &ProjectConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}

pub fn list_projects() -> Result<Vec<PathBuf>> {
    let dir = projects_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Returns the path for a new project file given a stem name.
pub fn project_path(name: &str) -> PathBuf {
    projects_dir().join(format!("{name}.toml"))
}