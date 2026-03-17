use std::time::Duration;

use anyhow::Result;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::config::schema::TunnelConfig;
use crate::events::AppEvent;
use crate::tunnel::TunnelState;

pub fn spawn(config: TunnelConfig, tx: mpsc::Sender<AppEvent>, token: CancellationToken) {
    tokio::spawn(run(config, tx, token));
}

async fn run(config: TunnelConfig, tx: mpsc::Sender<AppEvent>, token: CancellationToken) {
    let mut backoff = Duration::from_secs(1);

    loop {
        emit(&tx, &config.name, TunnelState::Connecting).await;

        match start_ssh(&config) {
            Err(e) => {
                emit(&tx, &config.name, TunnelState::Failed { reason: e.to_string() }).await;
            }
            Ok(mut child) => {
                let pid = child.id().unwrap_or(0);
                emit(&tx, &config.name, TunnelState::Up { pid }).await;
                backoff = Duration::from_secs(1); // reset on successful connect

                tokio::select! {
                    status = child.wait() => {
                        match status {
                            Ok(s) if s.success() => {
                                emit(&tx, &config.name, TunnelState::Stopped).await;
                                return;
                            }
                            Ok(s) => {
                                let reason = format!("ssh exited with code {:?}", s.code());
                                warn!(tunnel = %config.name, %reason);
                                emit(&tx, &config.name, TunnelState::Failed { reason }).await;
                            }
                            Err(e) => {
                                emit(&tx, &config.name, TunnelState::Failed { reason: e.to_string() }).await;
                            }
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
            return;
        }

        info!(tunnel = %config.name, secs = backoff.as_secs(), "will restart");

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

fn start_ssh(config: &TunnelConfig) -> Result<tokio::process::Child> {
    let forward = format!("{}:{}:{}", config.local_port, config.remote_host, config.remote_port);

    let mut cmd = tokio::process::Command::new("ssh");

    cmd.arg("-L").arg(&forward)
        .arg("-N")                               // no remote command
        .arg("-T")                               // no pseudo-tty
        .arg("-o").arg("ExitOnForwardFailure=yes")
        .arg("-o").arg("ServerAliveInterval=15")
        .arg("-o").arg("ServerAliveCountMax=3")
        .arg("-o").arg("BatchMode=yes");         // key-based auth only — no prompts

    if let Some(user) = &config.ssh_user {
        cmd.arg("-l").arg(user);
    }
    if let Some(key) = &config.identity_file {
        cmd.arg("-i").arg(key);
    }

    cmd.arg(&config.ssh_host)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);

    Ok(cmd.spawn()?)
}

async fn emit(tx: &mpsc::Sender<AppEvent>, name: &str, state: TunnelState) {
    let _ = tx.send(AppEvent::TunnelStateChanged { name: name.to_owned(), state }).await;
}
