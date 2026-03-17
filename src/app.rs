use std::collections::HashMap;
use std::io::Stdout;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::config::{self, schema::{ProjectConfig, TunnelConfig}};
use crate::events::AppEvent;
use crate::tunnel::{TunnelState, manager::TunnelManager};
use crate::ui;

// ── App state ────────────────────────────────────────────────────────────────

pub struct App {
    pub projects: Vec<PathBuf>,
    pub project_idx: usize,
    pub project: ProjectConfig,
    pub project_path: Option<PathBuf>,

    pub tunnel_states: HashMap<String, TunnelState>,
    pub selected: usize,
    pub logs: Vec<String>,

    pub mode: Mode,
    pub edit: EditState,
}

pub enum Mode {
    Normal,
    Edit,
    Help,
}

#[derive(Default)]
pub struct EditState {
    pub is_new: bool,
    pub original_name: String,
    pub name: String,
    pub local_port: String,
    pub remote_host: String,
    pub remote_port: String,
    pub ssh_host: String,
    pub ssh_user: String,
    pub identity_file: String,
    pub auto_restart: bool,
    pub field: usize,
    pub error: Option<String>,
}

pub const EDIT_FIELD_COUNT: usize = 7; // text fields before the bool

impl EditState {
    pub fn from_config(config: &TunnelConfig) -> Self {
        Self {
            is_new: false,
            original_name: config.name.clone(),
            name: config.name.clone(),
            local_port: config.local_port.to_string(),
            remote_host: config.remote_host.clone(),
            remote_port: config.remote_port.to_string(),
            ssh_host: config.ssh_host.clone(),
            ssh_user: config.ssh_user.clone().unwrap_or_default(),
            identity_file: config
                .identity_file
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            auto_restart: config.auto_restart,
            ..Default::default()
        }
    }

    pub fn new_empty() -> Self {
        Self { is_new: true, auto_restart: true, ..Default::default() }
    }

    pub fn to_config(&self) -> Option<TunnelConfig> {
        let name = self.name.trim().to_owned();
        let ssh_host = self.ssh_host.trim().to_owned();
        let remote_host = self.remote_host.trim().to_owned();
        if name.is_empty() || ssh_host.is_empty() || remote_host.is_empty() {
            return None;
        }
        Some(TunnelConfig {
            name,
            local_port: self.local_port.trim().parse().ok()?,
            remote_host,
            remote_port: self.remote_port.trim().parse().ok()?,
            ssh_host,
            ssh_user: (!self.ssh_user.trim().is_empty())
                .then(|| self.ssh_user.trim().to_owned()),
            identity_file: (!self.identity_file.trim().is_empty())
                .then(|| PathBuf::from(self.identity_file.trim())),
            auto_restart: self.auto_restart,
        })
    }

    pub fn current_text_field(&mut self) -> Option<&mut String> {
        match self.field {
            0 => Some(&mut self.name),
            1 => Some(&mut self.local_port),
            2 => Some(&mut self.remote_host),
            3 => Some(&mut self.remote_port),
            4 => Some(&mut self.ssh_host),
            5 => Some(&mut self.ssh_user),
            6 => Some(&mut self.identity_file),
            _ => None,
        }
    }
}

impl App {
    pub fn new() -> Result<Self> {
        let projects = config::list_projects()?;
        let (project, project_path) = match projects.first() {
            Some(path) => (config::load_project(path)?, Some(path.clone())),
            None => (ProjectConfig::default(), None),
        };

        Ok(Self {
            projects,
            project_idx: 0,
            project,
            project_path,
            tunnel_states: HashMap::new(),
            selected: 0,
            logs: Vec::new(),
            mode: Mode::Normal,
            edit: EditState::default(),
        })
    }

    pub fn selected_tunnel(&self) -> Option<&TunnelConfig> {
        self.project.tunnels.get(self.selected)
    }

    pub fn tunnel_state(&self, name: &str) -> &TunnelState {
        self.tunnel_states.get(name).unwrap_or(&TunnelState::Stopped)
    }

    pub fn push_log(&mut self, msg: String) {
        if self.logs.len() >= 200 {
            self.logs.remove(0);
        }
        self.logs.push(msg);
    }

    pub fn project_name(&self) -> &str {
        self.project_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
    }
}

// ── Main event loop ───────────────────────────────────────────────────────────

pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    tx: mpsc::Sender<AppEvent>,
    mut rx: mpsc::Receiver<AppEvent>,
) -> Result<()> {
    let mut app = App::new()?;
    let mut manager = TunnelManager::new(tx.clone());

    // keyboard reader — runs in its own task so it never blocks the event loop
    let input_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if input_tx.send(AppEvent::Key(key)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // tick — drives redraws at a stable rate
    let tick_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if tick_tx.send(AppEvent::Tick).await.is_err() {
                break;
            }
        }
    });

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        match rx.recv().await {
            None => break,
            Some(AppEvent::Tick) => {}

            Some(AppEvent::TunnelStateChanged { name, state }) => {
                app.push_log(format!("[{}] → {}", name, state.label()));
                app.tunnel_states.insert(name, state);
            }

            Some(AppEvent::Key(key)) => match app.mode {
                Mode::Help => app.mode = Mode::Normal,
                Mode::Edit => handle_edit(&mut app, &mut manager, key)?,
                Mode::Normal => {
                    if handle_normal(&mut app, &mut manager, key)? {
                        break;
                    }
                }
            },
        }
    }

    manager.stop_all();
    tokio::time::sleep(Duration::from_millis(300)).await;
    Ok(())
}

// ── Normal mode keys ──────────────────────────────────────────────────────────

fn handle_normal(
    app: &mut App,
    manager: &mut TunnelManager,
    key: crossterm::event::KeyEvent,
) -> Result<bool> {
    let tunnels_len = app.project.tunnels.len();

    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _)
        | (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(true),

        (KeyCode::Char('?'), _) => app.mode = Mode::Help,

        (KeyCode::Up | KeyCode::Char('k'), _) => {
            app.selected = app.selected.saturating_sub(1);
        }
        (KeyCode::Down | KeyCode::Char('j'), _) => {
            if tunnels_len > 0 {
                app.selected = (app.selected + 1).min(tunnels_len - 1);
            }
        }

        (KeyCode::Enter | KeyCode::Char(' '), _) => {
            if let Some(tunnel) = app.selected_tunnel().cloned() {
                if manager.is_running(&tunnel.name) {
                    manager.stop(&tunnel.name);
                } else {
                    manager.start(tunnel);
                }
            }
        }

        (KeyCode::Char('e'), _) => {
            if let Some(tunnel) = app.selected_tunnel() {
                app.edit = EditState::from_config(tunnel);
                app.mode = Mode::Edit;
            }
        }

        (KeyCode::Char('n'), _) => {
            app.edit = EditState::new_empty();
            app.mode = Mode::Edit;
        }

        (KeyCode::Char('d') | KeyCode::Delete, _) => {
            if tunnels_len > 0 {
                let name = app.project.tunnels[app.selected].name.clone();
                manager.stop(&name);
                app.project.tunnels.remove(app.selected);
                app.selected = app.selected.min(app.project.tunnels.len().saturating_sub(1));
                save(app)?;
            }
        }

        (KeyCode::Tab, _) => {
            if app.projects.len() > 1 {
                app.project_idx = (app.project_idx + 1) % app.projects.len();
                let path = app.projects[app.project_idx].clone();
                app.project = config::load_project(&path)?;
                app.project_path = Some(path);
                app.selected = 0;
            }
        }

        _ => {}
    }

    Ok(false)
}

// ── Edit mode keys ────────────────────────────────────────────────────────────

fn handle_edit(
    app: &mut App,
    manager: &mut TunnelManager,
    key: crossterm::event::KeyEvent,
) -> Result<()> {
    let total_fields = EDIT_FIELD_COUNT + 1; // text fields + auto_restart

    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.edit.error = None;
        }

        KeyCode::Tab | KeyCode::Down => {
            app.edit.field = (app.edit.field + 1) % total_fields;
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.edit.field =
                app.edit.field.checked_sub(1).unwrap_or(total_fields - 1);
        }

        // toggle auto_restart
        KeyCode::Char(' ') if app.edit.field == EDIT_FIELD_COUNT => {
            app.edit.auto_restart = !app.edit.auto_restart;
        }

        KeyCode::Enter => {
            match app.edit.to_config() {
                None => {
                    app.edit.error = Some(
                        "name, ssh_host, remote_host, local_port and remote_port are required"
                            .into(),
                    );
                }
                Some(new_cfg) => {
                    app.edit.error = None;
                    if app.edit.is_new {
                        app.project.tunnels.push(new_cfg);
                    } else {
                        // stop old worker if name changed or tunnel was running
                        manager.stop(&app.edit.original_name);
                        if let Some(t) = app.project.tunnels.get_mut(app.selected) {
                            *t = new_cfg;
                        }
                    }
                    save(app)?;
                    app.mode = Mode::Normal;
                }
            }
        }

        KeyCode::Backspace => {
            if let Some(field) = app.edit.current_text_field() {
                field.pop();
            }
        }

        KeyCode::Char(c) => {
            if let Some(field) = app.edit.current_text_field() {
                field.push(c);
            }
        }

        _ => {}
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn save(app: &App) -> Result<()> {
    let path = match &app.project_path {
        Some(p) => p.clone(),
        None => {
            let p = config::project_path("default");
            p
        }
    };
    config::save_project(&path, &app.project)
}
