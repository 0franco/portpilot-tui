use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Discriminator values for the `kind` field in TOML.
pub mod kind {
    pub const SSH: &str = "ssh";
    pub const KUBERNETES: &str = "kubernetes";
    pub const K8S_VIA_SSH: &str = "kubernetes-via-ssh";
    pub const K8S_VIA_BASTION: &str = "kubernetes-via-bastion-ssh";
}

/// Flat config struct — all kind-specific fields are Option<>.
/// The `kind` string drives which fields are required at runtime.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TunnelConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub local_port: u16,
    #[serde(default)]
    pub remote_port: u16,

    #[serde(default = "default_true")]
    pub auto_restart: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,

    // ── ssh / kubernetes-via-ssh ──────────────────────────────────────────
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<PathBuf>,
    /// sudo -u <remote_user> kubectl … on the ssh host
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_user: Option<String>,

    // ── kubernetes (local kubectl) / kubernetes-via-ssh ───────────────────
    /// e.g. "svc/my-svc", "pod/my-pod-0", "deployment/api"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,

    // ── kubernetes-via-bastion-ssh ────────────────────────────────────────
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bastion_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bastion_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bastion_identity_file: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_identity_file: Option<PathBuf>,
    /// sudo -u <target_remote_user> kubectl … on the target host
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_remote_user: Option<String>,
}

fn default_true() -> bool {
    true
}

impl TunnelConfig {
    pub fn normalize_and_validate(&mut self) -> Result<()> {
        if self.kind.trim().is_empty() {
            self.kind = kind::SSH.to_owned();
        }

        self.validate()
    }

    pub fn validate(&self) -> Result<()> {
        require_nonempty("name", Some(&self.name))?;
        require_port("local_port", self.local_port)?;
        require_port("remote_port", self.remote_port)?;

        match self.kind.as_str() {
            kind::SSH => {
                require_present("remote_host", &self.remote_host)?;
                require_present("ssh_host", &self.ssh_host)?;
                reject_present(kind::SSH, "remote_user", &self.remote_user)?;
                reject_kubernetes_fields(kind::SSH, self)?;
                reject_bastion_fields(kind::SSH, self)?;
            }
            kind::KUBERNETES => {
                require_present("target", &self.target)?;
                reject_present(kind::KUBERNETES, "remote_host", &self.remote_host)?;
                reject_ssh_fields(kind::KUBERNETES, self)?;
                reject_present(kind::KUBERNETES, "remote_user", &self.remote_user)?;
                reject_bastion_fields(kind::KUBERNETES, self)?;
            }
            kind::K8S_VIA_SSH => {
                require_present("ssh_host", &self.ssh_host)?;
                require_present("ssh_user", &self.ssh_user)?;
                require_present("target", &self.target)?;
                reject_present(kind::K8S_VIA_SSH, "remote_host", &self.remote_host)?;
                reject_bastion_fields(kind::K8S_VIA_SSH, self)?;
            }
            kind::K8S_VIA_BASTION => {
                require_present("bastion_host", &self.bastion_host)?;
                require_present("bastion_user", &self.bastion_user)?;
                require_present("target_host", &self.target_host)?;
                require_present("target_user", &self.target_user)?;
                require_present("target", &self.target)?;
                reject_present(kind::K8S_VIA_BASTION, "remote_host", &self.remote_host)?;
                reject_ssh_fields(kind::K8S_VIA_BASTION, self)?;
                reject_present(kind::K8S_VIA_BASTION, "remote_user", &self.remote_user)?;
            }
            other => bail!("unknown tunnel kind: {other}"),
        }

        Ok(())
    }

    pub fn connection_label(&self) -> String {
        match self.kind.as_str() {
            kind::SSH => {
                let host = self.remote_host.as_deref().unwrap_or("localhost");
                let via = self.ssh_host.as_deref().unwrap_or("?");
                format!(
                    "{} → {}:{} via {}",
                    self.local_port, host, self.remote_port, via
                )
            }
            kind::KUBERNETES => {
                let ns = self.namespace.as_deref().unwrap_or("default");
                let tgt = self.target.as_deref().unwrap_or("?");
                format!(
                    "{} → k8s {}/{} :{}",
                    self.local_port, ns, tgt, self.remote_port
                )
            }
            kind::K8S_VIA_SSH => {
                let ns = self.namespace.as_deref().unwrap_or("default");
                let tgt = self.target.as_deref().unwrap_or("?");
                let via = self.ssh_host.as_deref().unwrap_or("?");
                format!(
                    "{} → k8s {}/{} via {} :{}",
                    self.local_port, ns, tgt, via, self.remote_port
                )
            }
            kind::K8S_VIA_BASTION => {
                let ns = self.namespace.as_deref().unwrap_or("default");
                let tgt = self.target.as_deref().unwrap_or("?");
                let b = self.bastion_host.as_deref().unwrap_or("?");
                let th = self.target_host.as_deref().unwrap_or("?");
                format!(
                    "{} → k8s {}/{} via {}→{} :{}",
                    self.local_port, ns, tgt, b, th, self.remote_port
                )
            }
            _ => format!("{} → :{}", self.local_port, self.remote_port),
        }
    }

    pub fn kind_label(&self) -> &str {
        match self.kind.as_str() {
            kind::SSH => "ssh",
            kind::KUBERNETES => "k8s",
            kind::K8S_VIA_SSH => "k8s+ssh",
            kind::K8S_VIA_BASTION => "k8s+bastion",
            other => other,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default)]
    pub tunnels: Vec<TunnelConfig>,
}

impl ProjectConfig {
    pub fn normalize_and_validate(&mut self) -> Result<()> {
        for (idx, tunnel) in self.tunnels.iter_mut().enumerate() {
            tunnel.normalize_and_validate().map_err(|e| {
                anyhow!("invalid tunnel #{} ({}): {e}", idx + 1, tunnel_name(tunnel))
            })?;
        }
        Ok(())
    }
}

fn tunnel_name(tunnel: &TunnelConfig) -> &str {
    if tunnel.name.trim().is_empty() {
        "<unnamed>"
    } else {
        tunnel.name.trim()
    }
}

fn require_port(field: &str, port: u16) -> Result<()> {
    if port == 0 {
        bail!("{field} must be a number 1-65535");
    }
    Ok(())
}

fn require_present(field: &str, value: &Option<String>) -> Result<()> {
    require_nonempty(field, value.as_ref())
}

fn require_nonempty(field: &str, value: Option<&String>) -> Result<()> {
    if value.map(|v| v.trim().is_empty()).unwrap_or(true) {
        bail!("{field} is required");
    }
    Ok(())
}

fn reject_ssh_fields(current_kind: &str, tunnel: &TunnelConfig) -> Result<()> {
    reject_present(current_kind, "ssh_host", &tunnel.ssh_host)?;
    reject_present(current_kind, "ssh_user", &tunnel.ssh_user)?;
    reject_path_present(current_kind, "identity_file", &tunnel.identity_file)?;
    Ok(())
}

fn reject_kubernetes_fields(current_kind: &str, tunnel: &TunnelConfig) -> Result<()> {
    reject_present(current_kind, "target", &tunnel.target)?;
    reject_present(current_kind, "namespace", &tunnel.namespace)?;
    reject_present(current_kind, "context", &tunnel.context)?;
    Ok(())
}

fn reject_bastion_fields(current_kind: &str, tunnel: &TunnelConfig) -> Result<()> {
    reject_present(current_kind, "bastion_host", &tunnel.bastion_host)?;
    reject_present(current_kind, "bastion_user", &tunnel.bastion_user)?;
    reject_path_present(
        current_kind,
        "bastion_identity_file",
        &tunnel.bastion_identity_file,
    )?;
    reject_present(current_kind, "target_host", &tunnel.target_host)?;
    reject_present(current_kind, "target_user", &tunnel.target_user)?;
    reject_path_present(
        current_kind,
        "target_identity_file",
        &tunnel.target_identity_file,
    )?;
    reject_present(
        current_kind,
        "target_remote_user",
        &tunnel.target_remote_user,
    )?;
    Ok(())
}

fn reject_present(current_kind: &str, field: &str, value: &Option<String>) -> Result<()> {
    if value
        .as_ref()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        bail!("{field} is not valid for kind={current_kind}");
    }
    Ok(())
}

fn reject_path_present(current_kind: &str, field: &str, value: &Option<PathBuf>) -> Result<()> {
    if value
        .as_ref()
        .map(|v| !v.as_os_str().is_empty())
        .unwrap_or(false)
    {
        bail!("{field} is not valid for kind={current_kind}");
    }
    Ok(())
}
