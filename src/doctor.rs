use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::process::Command;

use crate::config::schema::{kind, TunnelConfig};
use crate::tunnel::worker;

const CHECK_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Info,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoctorCheck {
    pub status: CheckStatus,
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoctorReport {
    pub tunnel: String,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.status == CheckStatus::Fail)
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Doctor report for {}", self.tunnel)];
        for check in &self.checks {
            if check.detail.is_empty() {
                lines.push(format!("[{}] {}", check.status.label(), check.title));
            } else {
                lines.push(format!(
                    "[{}] {}: {}",
                    check.status.label(),
                    check.title,
                    check.detail
                ));
            }
        }
        lines
    }
}

impl CheckStatus {
    fn label(self) -> &'static str {
        match self {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Info => "INFO",
        }
    }
}

pub async fn diagnose(tunnel: &TunnelConfig, include_remote: bool) -> DoctorReport {
    let mut checks = Vec::new();

    add_command_preview(tunnel, &mut checks);
    add_config_checks(tunnel, &mut checks);
    add_local_port_check(tunnel, &mut checks);
    add_identity_checks(tunnel, &mut checks);
    add_binary_checks(tunnel, &mut checks).await;

    if include_remote {
        add_remote_checks(tunnel, &mut checks).await;
    } else {
        checks.push(info(
            "remote checks skipped",
            "run `portpilot doctor <name>` for SSH/kubectl reachability checks",
        ));
    }

    DoctorReport {
        tunnel: tunnel.name.clone(),
        checks,
    }
}

fn add_command_preview(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    match worker::command_line(tunnel) {
        Ok(command) => checks.push(info("generated command", command)),
        Err(e) => checks.push(fail("generated command", e.to_string())),
    }
}

fn add_config_checks(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    match tunnel.validate() {
        Ok(()) => checks.push(pass("config", "required fields match tunnel kind")),
        Err(e) => checks.push(fail("config", e.to_string())),
    }

    match tunnel.kind.as_str() {
        kind::SSH => {
            if tunnel.ssh_user.is_none() {
                checks.push(warn(
                    "ssh_user",
                    "not set; OpenSSH will use your local OS user or ssh config",
                ));
            }
        }
        kind::K8S_VIA_SSH => {
            checks.push(info(
                "ssh_user",
                "SSH login for ssh_host; remote_user only controls optional sudo for kubectl",
            ));
        }
        kind::K8S_VIA_BASTION => {
            checks.push(info(
                "bastion/target users",
                "bastion_user logs into bastion_host; target_user logs into target_host; target_remote_user only controls optional sudo for kubectl",
            ));
        }
        _ => {}
    }
}

fn add_local_port_check(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    let addr = format!("127.0.0.1:{}", tunnel.local_port);
    match TcpListener::bind(&addr) {
        Ok(listener) => {
            drop(listener);
            checks.push(pass("local port", format!("{addr} is available")));
        }
        Err(e) => checks.push(fail("local port", format!("{addr} cannot be bound: {e}"))),
    }
}

fn add_identity_checks(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    match tunnel.kind.as_str() {
        kind::SSH | kind::K8S_VIA_SSH => {
            check_identity("identity_file", tunnel.identity_file.as_deref(), checks);
        }
        kind::K8S_VIA_BASTION => {
            check_identity(
                "bastion_identity_file",
                tunnel.bastion_identity_file.as_deref(),
                checks,
            );
            check_identity(
                "target_identity_file",
                tunnel.target_identity_file.as_deref(),
                checks,
            );
        }
        _ => {}
    }
}

fn check_identity(label: &str, path: Option<&Path>, checks: &mut Vec<DoctorCheck>) {
    let Some(path) = path else {
        checks.push(warn(
            label,
            "not set; ssh will use ssh-agent, default keys, or ~/.ssh/config",
        ));
        return;
    };

    let expanded = PathBuf::from(worker::expand_identity_file(path));
    match std::fs::metadata(&expanded) {
        Ok(meta) if meta.is_file() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    checks.push(warn(
                        label,
                        format!(
                            "{} exists but permissions are {:03o}; ssh may reject keys readable by group/others",
                            expanded.display(),
                            mode
                        ),
                    ));
                    return;
                }
            }
            checks.push(pass(label, format!("{} exists", expanded.display())));
        }
        Ok(_) => checks.push(fail(
            label,
            format!("{} exists but is not a file", expanded.display()),
        )),
        Err(e) => checks.push(fail(
            label,
            format!("{} cannot be read: {e}", expanded.display()),
        )),
    }
}

async fn add_binary_checks(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    check_program("ssh", &["-V"], checks).await;
    if matches!(tunnel.kind.as_str(), kind::KUBERNETES) {
        check_program("kubectl", &["version", "--client"], checks).await;
    }
}

async fn check_program(program: &str, args: &[&str], checks: &mut Vec<DoctorCheck>) {
    match run_command(program, args.iter().copied().map(str::to_owned).collect()).await {
        Ok(output) if output.status_success => checks.push(pass(program, "available")),
        Ok(output) => checks.push(fail(program, output.summary())),
        Err(e) => checks.push(fail(program, e.to_string())),
    }
}

async fn add_remote_checks(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    match tunnel.kind.as_str() {
        kind::SSH => check_plain_ssh(tunnel, checks).await,
        kind::KUBERNETES => check_local_kubectl_target(tunnel, checks).await,
        kind::K8S_VIA_SSH => check_kubectl_via_ssh(tunnel, checks).await,
        kind::K8S_VIA_BASTION => check_kubectl_via_bastion(tunnel, checks).await,
        _ => checks.push(fail("remote checks", "unknown tunnel kind")),
    }
}

async fn check_plain_ssh(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    let Some(host) = tunnel.ssh_host.as_deref() else {
        checks.push(fail("ssh reachability", "ssh_host is required"));
        return;
    };

    let mut args = ssh_check_base_args();
    push_ssh_credentials(
        &mut args,
        tunnel.ssh_user.as_deref(),
        tunnel.identity_file.as_deref(),
    );
    args.extend([host.to_owned(), "true".to_owned()]);

    add_command_result(
        "ssh reachability",
        "can run `true` on ssh_host",
        "ssh",
        args,
        checks,
    )
    .await;
}

async fn check_local_kubectl_target(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    let args = kubectl_get_args(tunnel);
    add_command_result(
        "kubectl target",
        "target exists and kubectl can reach the configured cluster",
        "kubectl",
        args,
        checks,
    )
    .await;
}

async fn check_kubectl_via_ssh(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    let Some(host) = tunnel.ssh_host.as_deref() else {
        checks.push(fail("ssh reachability", "ssh_host is required"));
        return;
    };

    let remote_args = remote_kubectl_get_args(tunnel, tunnel.remote_user.as_deref());
    let mut args = ssh_check_base_args();
    push_ssh_credentials(
        &mut args,
        tunnel.ssh_user.as_deref(),
        tunnel.identity_file.as_deref(),
    );
    args.push(host.to_owned());
    args.extend(remote_args);

    add_command_result(
        "ssh kubectl target",
        "can connect to ssh_host and run kubectl get for target",
        "ssh",
        args,
        checks,
    )
    .await;
}

async fn check_kubectl_via_bastion(tunnel: &TunnelConfig, checks: &mut Vec<DoctorCheck>) {
    let Some(bastion) = tunnel.bastion_host.as_deref() else {
        checks.push(fail("bastion reachability", "bastion_host is required"));
        return;
    };
    let Some(target_host) = tunnel.target_host.as_deref() else {
        checks.push(fail("target reachability", "target_host is required"));
        return;
    };

    let mut args = ssh_check_base_args();
    args.extend([
        "-o".to_owned(),
        format!(
            "ProxyCommand={}",
            worker::proxy_command(
                bastion,
                tunnel.bastion_user.as_deref(),
                tunnel.bastion_identity_file.as_deref()
            )
        ),
    ]);
    push_ssh_credentials(
        &mut args,
        tunnel.target_user.as_deref(),
        tunnel.target_identity_file.as_deref(),
    );
    args.push(target_host.to_owned());
    args.extend(remote_kubectl_get_args(
        tunnel,
        tunnel.target_remote_user.as_deref(),
    ));

    add_command_result(
        "bastion kubectl target",
        "can connect through bastion and run kubectl get for target",
        "ssh",
        args,
        checks,
    )
    .await;
}

fn ssh_check_base_args() -> Vec<String> {
    vec![
        "-T".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=5".to_owned(),
        "-o".to_owned(),
        "StrictHostKeyChecking=accept-new".to_owned(),
    ]
}

fn push_ssh_credentials(args: &mut Vec<String>, user: Option<&str>, identity: Option<&Path>) {
    if let Some(user) = user {
        args.extend(["-l".to_owned(), user.to_owned()]);
    }
    if let Some(identity) = identity {
        args.extend(["-i".to_owned(), worker::expand_identity_file(identity)]);
    }
}

fn kubectl_get_args(tunnel: &TunnelConfig) -> Vec<String> {
    let mut args = vec!["get".to_owned(), tunnel.target.clone().unwrap_or_default()];
    if let Some(ns) = &tunnel.namespace {
        args.extend(["-n".to_owned(), ns.clone()]);
    }
    if let Some(ctx) = &tunnel.context {
        args.extend(["--context".to_owned(), ctx.clone()]);
    }
    args
}

fn remote_kubectl_get_args(tunnel: &TunnelConfig, sudo_user: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(user) = sudo_user {
        args.extend(["sudo".to_owned(), "-u".to_owned(), user.to_owned()]);
    }
    args.push("kubectl".to_owned());
    args.extend(kubectl_get_args(tunnel));
    args
}

async fn add_command_result(
    title: &'static str,
    success_detail: &'static str,
    program: &str,
    args: Vec<String>,
    checks: &mut Vec<DoctorCheck>,
) {
    match run_command(program, args).await {
        Ok(output) if output.status_success => checks.push(pass(title, success_detail)),
        Ok(output) => checks.push(fail(title, output.summary())),
        Err(e) => checks.push(fail(title, e.to_string())),
    }
}

struct CommandOutput {
    status_success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl CommandOutput {
    fn summary(&self) -> String {
        let mut detail = match self.code {
            Some(code) => format!("exited with code {code}"),
            None => "terminated by signal".to_owned(),
        };
        if let Some(line) =
            first_nonempty_line(&self.stderr).or_else(|| first_nonempty_line(&self.stdout))
        {
            detail.push_str(": ");
            detail.push_str(line);
        }
        detail
    }
}

async fn run_command(program: &str, args: Vec<String>) -> Result<CommandOutput> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| anyhow!("failed to spawn {program}: {e}"))?;
    let output = tokio::time::timeout(CHECK_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| anyhow!("{program} timed out after {}s", CHECK_TIMEOUT.as_secs()))??;

    Ok(CommandOutput {
        status_success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

fn first_nonempty_line(s: &str) -> Option<&str> {
    s.lines().map(str::trim).find(|line| !line.is_empty())
}

fn pass(title: impl Into<String>, detail: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        status: CheckStatus::Pass,
        title: title.into(),
        detail: detail.into(),
    }
}

fn warn(title: impl Into<String>, detail: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        status: CheckStatus::Warn,
        title: title.into(),
        detail: detail.into(),
    }
}

fn fail(title: impl Into<String>, detail: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        status: CheckStatus::Fail,
        title: title.into(),
        detail: detail.into(),
    }
}

fn info(title: impl Into<String>, detail: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        status: CheckStatus::Info,
        title: title.into(),
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{diagnose, CheckStatus};
    use crate::config::schema::{kind, TunnelConfig};

    #[tokio::test]
    async fn doctor_warns_when_plain_ssh_would_use_os_user() {
        let report = diagnose(
            &TunnelConfig {
                name: "db".to_owned(),
                kind: kind::SSH.to_owned(),
                local_port: 5432,
                remote_host: Some("db.internal".to_owned()),
                remote_port: 5432,
                ssh_host: Some("bastion.example.com".to_owned()),
                ..Default::default()
            },
            false,
        )
        .await;

        assert!(report.checks.iter().any(|check| {
            check.status == CheckStatus::Warn
                && check.title == "ssh_user"
                && check.detail.contains("local OS user")
        }));
    }

    #[tokio::test]
    async fn doctor_reports_missing_identity_files() {
        let report = diagnose(
            &TunnelConfig {
                name: "db".to_owned(),
                kind: kind::SSH.to_owned(),
                local_port: 5432,
                remote_host: Some("db.internal".to_owned()),
                remote_port: 5432,
                ssh_host: Some("bastion.example.com".to_owned()),
                identity_file: Some(PathBuf::from("/definitely/missing/key.pem")),
                ..Default::default()
            },
            false,
        )
        .await;

        assert!(report.checks.iter().any(|check| {
            check.status == CheckStatus::Fail
                && check.title == "identity_file"
                && check.detail.contains("/definitely/missing/key.pem")
        }));
    }
}
