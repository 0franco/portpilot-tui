use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub name: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub ssh_host: String,
    pub ssh_user: Option<String>,
    pub identity_file: Option<PathBuf>,
    #[serde(default = "default_true")]
    pub auto_restart: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    #[serde(default)]
    pub tunnels: Vec<TunnelConfig>,
}
