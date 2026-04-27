use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::config::schema::{kind, TunnelConfig};
use crate::events::AppEvent;
use crate::tunnel::TunnelState;

const DEFAULT_MAX_RETRIES: u32 = 5;

pub fn spawn(
    config: TunnelConfig,
    tx: mpsc::Sender<AppEvent>,
    token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run(config, tx, token))
}

async fn run(config: TunnelConfig, tx: mpsc::Sender<AppEvent>, token: CancellationToken) {
    let mut backoff = Duration::from_secs(1);
    let max_retries = config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES);
    let mut attempts: u32 = 0;

    loop {
        emit(&tx, &config.name, TunnelState::Connecting).await;

        match start_process(&config) {
            Err(e) => {
                let reason = format!("failed to spawn process: {e}");
                warn!(tunnel = %config.name, %reason);
                emit(&tx, &config.name, TunnelState::Failed { reason }).await;
            }
            Ok(mut child) => {
                let pid = child.id().unwrap_or(0);
                emit(&tx, &config.name, TunnelState::Up { pid }).await;
                let started_at = std::time::Instant::now();

                tokio::select! {
                    status = child.wait() => {
                        let (reason_line, full_stderr, is_fatal) = read_stderr(&mut child).await;

                        for line in full_stderr.lines().filter(|l| !l.trim().is_empty()) {
                            emit_log(&tx, &config.name, line).await;
                        }

                        let reason = match status {
                            Ok(s) if s.success() => {
                                emit(&tx, &config.name, TunnelState::Stopped).await;
                                return;
                            }
                            Ok(s) => {
                                let code = s.code().map(|c| c.to_string()).unwrap_or_else(|| "?".into());
                                if reason_line.is_empty() {
                                    format!("exited (code {code})")
                                } else {
                                    format!("exited (code {code}): {reason_line}")
                                }
                            }
                            Err(e) => e.to_string(),
                        };
                        warn!(tunnel = %config.name, %reason);
                        emit(&tx, &config.name, TunnelState::Failed { reason }).await;

                        // Only reset if the tunnel was stable for >5s.
                        if started_at.elapsed() > Duration::from_secs(5) {
                            backoff = Duration::from_secs(1);
                            attempts = 0;
                        }

                        if is_fatal {
                            return;
                        }
                    }
                    _ = token.cancelled() => {
                        let _ = child.kill().await;
                        emit(&tx, &config.name, TunnelState::Stopped).await;
                        return;
                    }
                }
            }
        }

        if !config.auto_restart {
            emit(&tx, &config.name, TunnelState::Stopped).await;
            return;
        }

        attempts += 1;
        if attempts >= max_retries {
            let reason = format!("gave up after {max_retries} attempts");
            warn!(tunnel = %config.name, %reason);
            emit(&tx, &config.name, TunnelState::Failed { reason }).await;
            return;
        }

        info!(tunnel = %config.name, attempt = attempts, max_retries, secs = backoff.as_secs(), "restarting");

        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = token.cancelled() => {
                emit(&tx, &config.name, TunnelState::Stopped).await;
                return;
            }
        }

        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

// ── Process spawners ─────────────────────────────────────────────────────────

fn start_process(c: &TunnelConfig) -> anyhow::Result<tokio::process::Child> {
    let spec = command_spec(c)?;
    let mut cmd = spec.into_command();
    attach_io(&mut cmd);
    Ok(cmd.spawn()?)
}

#[derive(Debug, PartialEq, Eq)]
struct CommandSpec {
    program: String,
    args: Vec<String>,
}

impl CommandSpec {
    fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    fn arg(&mut self, arg: impl Into<String>) -> &mut Self {
        self.args.push(arg.into());
        self
    }

    fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    fn into_command(self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(self.program);
        cmd.args(self.args);
        cmd
    }
}

fn command_spec(c: &TunnelConfig) -> anyhow::Result<CommandSpec> {
    match c.kind.as_str() {
        kind::SSH => ssh_spec(c),
        kind::KUBERNETES => kubectl_spec(c),
        kind::K8S_VIA_SSH => kubectl_via_ssh_spec(c),
        kind::K8S_VIA_BASTION => kubectl_via_bastion_spec(c),
        other => anyhow::bail!("unknown tunnel kind: {other}"),
    }
}

fn ssh_base_spec() -> CommandSpec {
    let mut cmd = CommandSpec::new("ssh");
    cmd.args([
        "-T",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "BatchMode=yes",
    ]);
    cmd
}

fn apply_ssh_credentials(cmd: &mut CommandSpec, user: Option<&str>, identity: Option<&Path>) {
    if let Some(u) = user {
        cmd.arg("-l").arg(u);
    }
    if let Some(k) = identity {
        cmd.arg("-i").arg(expand_identity_file(k));
    }
}

fn attach_io(cmd: &mut tokio::process::Command) {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
}

fn kubectl_remote_args(c: &TunnelConfig) -> Vec<String> {
    let tgt = c.target.as_deref().unwrap_or("");
    let mut args = vec![
        "kubectl".into(),
        "port-forward".into(),
        tgt.to_owned(),
        format!("{}:{}", c.remote_port, c.remote_port),
    ];
    if let Some(ns) = &c.namespace {
        args.extend(["-n".into(), ns.clone()]);
    }
    if let Some(ctx) = &c.context {
        args.extend(["--context".into(), ctx.clone()]);
    }
    args
}

/// ssh -L local:remote_host:remote -N [creds] ssh_host
fn ssh_spec(c: &TunnelConfig) -> anyhow::Result<CommandSpec> {
    let remote = c.remote_host.as_deref().unwrap_or("localhost");
    let forward = format!("{}:{}:{}", c.local_port, remote, c.remote_port);
    let host = c
        .ssh_host
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("ssh_host is required for kind=ssh"))?;

    let mut cmd = ssh_base_spec();
    cmd.arg("-L").arg(&forward).arg("-N");
    apply_ssh_credentials(&mut cmd, c.ssh_user.as_deref(), c.identity_file.as_deref());
    cmd.arg(host);
    Ok(cmd)
}

/// kubectl port-forward locally
fn kubectl_spec(c: &TunnelConfig) -> anyhow::Result<CommandSpec> {
    let tgt = c
        .target
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("target is required for kind=kubernetes"))?;

    let mut cmd = CommandSpec::new("kubectl");
    cmd.arg("port-forward")
        .arg(tgt)
        .arg(format!("{}:{}", c.local_port, c.remote_port));
    if let Some(ns) = &c.namespace {
        cmd.arg("-n").arg(ns);
    }
    if let Some(ctx) = &c.context {
        cmd.arg("--context").arg(ctx);
    }
    Ok(cmd)
}

/// ssh -L local:localhost:remote [creds] host [sudo -u remote_user] kubectl port-forward …
fn kubectl_via_ssh_spec(c: &TunnelConfig) -> anyhow::Result<CommandSpec> {
    let host = c
        .ssh_host
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("ssh_host is required for kind=kubernetes-via-ssh"))?;
    if c.target.is_none() {
        anyhow::bail!("target is required for kind=kubernetes-via-ssh");
    }

    let forward = format!("{}:localhost:{}", c.local_port, c.remote_port);
    let mut cmd = ssh_base_spec();
    cmd.arg("-L").arg(&forward);
    apply_ssh_credentials(&mut cmd, c.ssh_user.as_deref(), c.identity_file.as_deref());
    cmd.arg(host);

    if let Some(ru) = &c.remote_user {
        cmd.arg("sudo").arg("-u").arg(ru);
    }
    for arg in kubectl_remote_args(c) {
        cmd.arg(&arg);
    }

    Ok(cmd)
}

/// Two-hop: ssh -L … -o ProxyCommand=... target_host kubectl port-forward …
fn kubectl_via_bastion_spec(c: &TunnelConfig) -> anyhow::Result<CommandSpec> {
    let bastion = c
        .bastion_host
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bastion_host is required"))?;
    let target_h = c
        .target_host
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("target_host is required"))?;
    if c.target.is_none() {
        anyhow::bail!("target is required for kind=kubernetes-via-bastion-ssh");
    }

    let forward = format!("{}:localhost:{}", c.local_port, c.remote_port);
    let mut cmd = ssh_base_spec();
    cmd.arg("-L")
        .arg(&forward)
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new");

    cmd.arg("-o").arg(format!(
        "ProxyCommand={}",
        proxy_command(
            bastion,
            c.bastion_user.as_deref(),
            c.bastion_identity_file.as_deref()
        )
    ));
    apply_ssh_credentials(
        &mut cmd,
        c.target_user.as_deref(),
        c.target_identity_file.as_deref(),
    );

    cmd.arg(target_h);
    if let Some(ru) = &c.target_remote_user {
        cmd.arg("sudo").arg("-u").arg(ru);
    }
    for arg in kubectl_remote_args(c) {
        cmd.arg(&arg);
    }

    Ok(cmd)
}

fn proxy_command(bastion: &str, user: Option<&str>, identity: Option<&Path>) -> String {
    let mut args = vec![
        "ssh".to_owned(),
        "-W".to_owned(),
        "%h:%p".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "StrictHostKeyChecking=accept-new".to_owned(),
    ];
    if let Some(u) = user {
        args.extend(["-l".to_owned(), u.to_owned()]);
    }
    if let Some(k) = identity {
        args.extend(["-i".to_owned(), expand_identity_file(k)]);
    }
    args.push(bastion.to_owned());
    args.into_iter()
        .map(|arg| shell_quote(&arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn expand_identity_file(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .to_string_lossy()
            .into_owned();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    raw.into_owned()
}

fn shell_quote(arg: &str) -> String {
    if arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:%=".contains(c))
    {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{command_spec, expand_identity_file};
    use crate::config::schema::{kind, TunnelConfig};

    #[test]
    fn ssh_spec_maps_configured_user_and_identity_to_ssh_args() {
        let spec = command_spec(&TunnelConfig {
            kind: kind::SSH.to_owned(),
            local_port: 5432,
            remote_host: Some("db.internal".to_owned()),
            remote_port: 5432,
            ssh_host: Some("bastion.example.com".to_owned()),
            ssh_user: Some("alice".to_owned()),
            identity_file: Some(PathBuf::from("~/.ssh/id_prod")),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(spec.program, "ssh");
        assert!(
            contains_seq(&spec.args, &["-l", "alice"]),
            "{:?}",
            spec.args
        );
        assert!(
            contains_seq(
                &spec.args,
                &[
                    "-i",
                    &expand_identity_file(PathBuf::from("~/.ssh/id_prod").as_path())
                ]
            ),
            "{:?}",
            spec.args
        );
        assert!(
            contains_seq(&spec.args, &["-L", "5432:db.internal:5432"]),
            "{:?}",
            spec.args
        );
    }

    #[test]
    fn ssh_spec_omits_user_and_identity_when_unconfigured() {
        let spec = command_spec(&TunnelConfig {
            kind: kind::SSH.to_owned(),
            local_port: 5432,
            remote_host: Some("db.internal".to_owned()),
            remote_port: 5432,
            ssh_host: Some("bastion.example.com".to_owned()),
            ..Default::default()
        })
        .unwrap();

        assert!(!spec.args.iter().any(|arg| arg == "-l"), "{:?}", spec.args);
        assert!(!spec.args.iter().any(|arg| arg == "-i"), "{:?}", spec.args);
    }

    #[test]
    fn ssh_spec_handles_all_user_identity_combinations() {
        for ssh_user in [None, Some("alice")] {
            for identity in [None, Some("/keys/id_prod")] {
                let spec = command_spec(&TunnelConfig {
                    kind: kind::SSH.to_owned(),
                    local_port: 5432,
                    remote_host: Some("db.internal".to_owned()),
                    remote_port: 5432,
                    ssh_host: Some("bastion.example.com".to_owned()),
                    ssh_user: ssh_user.map(str::to_owned),
                    identity_file: identity.map(PathBuf::from),
                    ..Default::default()
                })
                .unwrap();

                assert_eq!(
                    contains_seq(&spec.args, &["-l", "alice"]),
                    ssh_user.is_some(),
                    "{:?}",
                    spec.args
                );
                assert_eq!(
                    contains_seq(&spec.args, &["-i", "/keys/id_prod"]),
                    identity.is_some(),
                    "{:?}",
                    spec.args
                );
            }
        }
    }

    #[test]
    fn kubernetes_spec_includes_namespace_and_context() {
        let spec = command_spec(&TunnelConfig {
            kind: kind::KUBERNETES.to_owned(),
            local_port: 8080,
            remote_port: 80,
            target: Some("svc/api".to_owned()),
            namespace: Some("staging".to_owned()),
            context: Some("cluster-a".to_owned()),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(spec.program, "kubectl");
        assert_eq!(
            spec.args,
            vec![
                "port-forward",
                "svc/api",
                "8080:80",
                "-n",
                "staging",
                "--context",
                "cluster-a",
            ]
        );
    }

    #[test]
    fn kubernetes_via_ssh_spec_maps_ssh_user_and_remote_user_correctly() {
        let spec = command_spec(&TunnelConfig {
            kind: kind::K8S_VIA_SSH.to_owned(),
            local_port: 3306,
            remote_port: 3306,
            ssh_host: Some("k8s-admin.example.com".to_owned()),
            ssh_user: Some("ec2-user".to_owned()),
            identity_file: Some(PathBuf::from("/keys/k8s-admin.pem")),
            target: Some("svc/mysql".to_owned()),
            namespace: Some("data".to_owned()),
            context: Some("cluster-a".to_owned()),
            remote_user: Some("deploy".to_owned()),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(spec.program, "ssh");
        assert!(
            contains_seq(&spec.args, &["-l", "ec2-user"]),
            "{:?}",
            spec.args
        );
        assert!(
            contains_seq(&spec.args, &["-i", "/keys/k8s-admin.pem"]),
            "{:?}",
            spec.args
        );
        assert!(
            contains_seq(&spec.args, &["-L", "3306:localhost:3306"]),
            "{:?}",
            spec.args
        );
        assert!(
            contains_seq(
                &spec.args,
                &["k8s-admin.example.com", "sudo", "-u", "deploy", "kubectl"]
            ),
            "{:?}",
            spec.args
        );
        assert!(
            contains_seq(&spec.args, &["-n", "data", "--context", "cluster-a"]),
            "{:?}",
            spec.args
        );
    }

    #[test]
    fn kubernetes_via_ssh_spec_omits_optional_user_identity_and_sudo() {
        let spec = command_spec(&TunnelConfig {
            kind: kind::K8S_VIA_SSH.to_owned(),
            local_port: 3306,
            remote_port: 3306,
            ssh_host: Some("k8s-admin.example.com".to_owned()),
            target: Some("svc/mysql".to_owned()),
            ..Default::default()
        })
        .unwrap();

        assert!(!spec.args.iter().any(|arg| arg == "-l"), "{:?}", spec.args);
        assert!(!spec.args.iter().any(|arg| arg == "-i"), "{:?}", spec.args);
        assert!(
            !contains_seq(&spec.args, &["sudo", "-u"]),
            "{:?}",
            spec.args
        );
        assert!(
            contains_seq(&spec.args, &["k8s-admin.example.com", "kubectl"]),
            "{:?}",
            spec.args
        );
    }

    #[test]
    fn kubernetes_via_ssh_spec_handles_all_user_identity_sudo_combinations() {
        for ssh_user in [None, Some("ec2-user")] {
            for identity in [None, Some("/keys/k8s-admin.pem")] {
                for remote_user in [None, Some("deploy")] {
                    let spec = command_spec(&TunnelConfig {
                        kind: kind::K8S_VIA_SSH.to_owned(),
                        local_port: 3306,
                        remote_port: 3306,
                        ssh_host: Some("k8s-admin.example.com".to_owned()),
                        ssh_user: ssh_user.map(str::to_owned),
                        identity_file: identity.map(PathBuf::from),
                        target: Some("svc/mysql".to_owned()),
                        remote_user: remote_user.map(str::to_owned),
                        ..Default::default()
                    })
                    .unwrap();

                    assert_eq!(
                        contains_seq(&spec.args, &["-l", "ec2-user"]),
                        ssh_user.is_some(),
                        "{:?}",
                        spec.args
                    );
                    assert_eq!(
                        contains_seq(&spec.args, &["-i", "/keys/k8s-admin.pem"]),
                        identity.is_some(),
                        "{:?}",
                        spec.args
                    );
                    assert_eq!(
                        contains_seq(&spec.args, &["sudo", "-u", "deploy"]),
                        remote_user.is_some(),
                        "{:?}",
                        spec.args
                    );
                }
            }
        }
    }

    #[test]
    fn bastion_spec_keeps_bastion_and_target_credentials_separate_for_all_combinations() {
        for bastion_user in [None, Some("bastion-user")] {
            for target_user in [None, Some("target-user")] {
                for bastion_identity in [None, Some("/keys/bastion.pem")] {
                    for target_identity in [None, Some("/keys/target.pem")] {
                        let spec = command_spec(&TunnelConfig {
                            kind: kind::K8S_VIA_BASTION.to_owned(),
                            local_port: 3306,
                            remote_port: 3306,
                            bastion_host: Some("bastion.example.com".to_owned()),
                            bastion_user: bastion_user.map(str::to_owned),
                            bastion_identity_file: bastion_identity.map(PathBuf::from),
                            target_host: Some("10.0.10.25".to_owned()),
                            target_user: target_user.map(str::to_owned),
                            target_identity_file: target_identity.map(PathBuf::from),
                            target: Some("svc/mysql".to_owned()),
                            namespace: Some("data".to_owned()),
                            target_remote_user: Some("deploy".to_owned()),
                            ..Default::default()
                        })
                        .unwrap();

                        assert_eq!(spec.program, "ssh");
                        assert!(
                            contains_seq(&spec.args, &["-L", "3306:localhost:3306"]),
                            "{:?}",
                            spec.args
                        );
                        assert!(!spec.args.iter().any(|arg| arg == "-J"), "{:?}", spec.args);

                        let proxy = proxy_arg(&spec.args);
                        assert!(proxy.contains("bastion.example.com"), "{proxy}");
                        assert_eq!(
                            proxy.contains("-l bastion-user"),
                            bastion_user.is_some(),
                            "{proxy}"
                        );
                        assert_eq!(
                            proxy.contains("-i /keys/bastion.pem"),
                            bastion_identity.is_some(),
                            "{proxy}"
                        );

                        assert_eq!(
                            contains_seq(&spec.args, &["-l", "target-user"]),
                            target_user.is_some(),
                            "{:?}",
                            spec.args
                        );
                        assert_eq!(
                            contains_seq(&spec.args, &["-i", "/keys/target.pem"]),
                            target_identity.is_some(),
                            "{:?}",
                            spec.args
                        );
                        assert!(
                            contains_seq(
                                &spec.args,
                                &["10.0.10.25", "sudo", "-u", "deploy", "kubectl"]
                            ),
                            "{:?}",
                            spec.args
                        );
                    }
                }
            }
        }
    }

    fn contains_seq(args: &[String], expected: &[&str]) -> bool {
        args.windows(expected.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        })
    }

    fn proxy_arg(args: &[String]) -> &str {
        args.iter()
            .find_map(|arg| arg.strip_prefix("ProxyCommand="))
            .expect("ProxyCommand arg")
    }
}

// ── stderr helpers ───────────────────────────────────────────────────────────

/// Returns `(reason, full_stderr, is_fatal)`.
async fn read_stderr(child: &mut tokio::process::Child) -> (String, String, bool) {
    let Some(mut stderr) = child.stderr.take() else {
        return (String::new(), String::new(), false);
    };
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_millis(300), stderr.read(&mut buf))
        .await
        .unwrap_or(Ok(0))
        .unwrap_or(0);

    let full = String::from_utf8_lossy(&buf[..n]).trim().to_owned();

    let reason = full
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !is_ssh_noise(l))
        .next()
        .unwrap_or("")
        .to_owned();

    let is_fatal =
        full.contains("Address already in use") || full.contains("cannot listen to port");

    (reason, full, is_fatal)
}

fn is_ssh_noise(line: &str) -> bool {
    let lo = line.to_ascii_lowercase();
    let tlo = line.trim_start_matches('*').trim().to_ascii_lowercase();
    lo.contains("warning:")
        || lo.starts_with("debug")
        || lo.starts_with("pledge:")
        || lo.starts_with("notice:")
        || tlo.contains("post-quantum")
        || tlo.contains("store now, decrypt later")
        || tlo.contains("session may be vulnerable")
        || tlo.contains("server may need to be upgraded")
        || line.starts_with("**")
}

async fn emit(tx: &mpsc::Sender<AppEvent>, name: &str, state: TunnelState) {
    let _ = tx
        .send(AppEvent::TunnelStateChanged {
            name: name.to_owned(),
            state,
        })
        .await;
}

async fn emit_log(tx: &mpsc::Sender<AppEvent>, name: &str, line: &str) {
    let _ = tx
        .send(AppEvent::Log {
            tunnel: name.to_owned(),
            line: line.to_owned(),
        })
        .await;
}
