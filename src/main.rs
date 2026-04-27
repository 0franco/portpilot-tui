mod app;
mod config;
mod doctor;
mod events;
mod tunnel;
mod ui;

use std::io::IsTerminal;

use anyhow::{anyhow, bail, Result};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;
use tracing_appender::rolling;
use tracing_subscriber::EnvFilter;

use crate::events::AppEvent;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("doctor") {
        return run_doctor_cli(&args[1..]).await;
    }

    init_logging()?;

    let (tx, rx) = mpsc::channel::<AppEvent>(256);

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app::run(&mut terminal, tx, rx).await;

    // always restore terminal, even on error
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_doctor_cli(args: &[String]) -> Result<()> {
    let mut project_name = None;
    let mut include_remote = true;
    let mut tunnel_name = None;
    let mut idx = 0;

    while idx < args.len() {
        match args[idx].as_str() {
            "--project" | "-p" => {
                idx += 1;
                let Some(name) = args.get(idx) else {
                    bail!("usage: portpilot doctor [--project <name>] [--no-remote] <tunnel-name>");
                };
                project_name = Some(name.as_str());
            }
            "--no-remote" => include_remote = false,
            "--help" | "-h" => {
                println!("usage: portpilot doctor [--project <name>] [--no-remote] <tunnel-name>");
                return Ok(());
            }
            name if tunnel_name.is_none() => tunnel_name = Some(name),
            extra => bail!("unexpected argument `{extra}`"),
        }
        idx += 1;
    }

    let Some(tunnel_name) = tunnel_name else {
        bail!("usage: portpilot doctor [--project <name>] [--no-remote] <tunnel-name>");
    };

    let tunnel = find_tunnel(tunnel_name, project_name)?;
    let report = doctor::diagnose(&tunnel, include_remote).await;
    let lines = if std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        report.colored_lines()
    } else {
        report.lines()
    };
    for line in lines {
        println!("{line}");
    }

    if report.has_failures() {
        bail!("doctor found failures for {}", report.tunnel);
    }

    Ok(())
}

fn find_tunnel(
    tunnel_name: &str,
    project_name: Option<&str>,
) -> Result<config::schema::TunnelConfig> {
    let paths = config::list_projects()?;
    if paths.is_empty() {
        bail!(
            "no project configs found in {}",
            config::projects_dir().display()
        );
    }

    let mut matches = Vec::new();
    for path in paths {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if project_name.map(|name| name != stem).unwrap_or(false) {
            continue;
        }
        let project = config::load_project(&path)?;
        for tunnel in project.tunnels {
            if tunnel.name == tunnel_name {
                matches.push((stem.to_owned(), tunnel));
            }
        }
    }

    match matches.len() {
        0 => Err(anyhow!(
            "tunnel `{}` not found{}",
            tunnel_name,
            project_name
                .map(|name| format!(" in project `{name}`"))
                .unwrap_or_default()
        )),
        1 => Ok(matches.remove(0).1),
        _ => Err(anyhow!(
            "tunnel `{}` exists in multiple projects: {}; rerun with --project <name>",
            tunnel_name,
            matches
                .iter()
                .map(|(project, _)| project.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn init_logging() -> Result<()> {
    let log_dir = config::config_dir().join("logs");
    std::fs::create_dir_all(&log_dir)?;

    let appender = rolling::daily(&log_dir, "portpilot.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    // leak the guard so it lives for the entire process lifetime
    std::mem::forget(guard);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_ansi(false)
        .with_writer(writer)
        .init();

    Ok(())
}
