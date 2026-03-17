mod app;
mod config;
mod events;
mod tunnel;
mod ui;

use anyhow::Result;
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
