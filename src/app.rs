use std::collections::HashMap;
use std::io::Stdout;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::config::{
    self,
    schema::{kind, ProjectConfig, TunnelConfig},
};
use crate::events::AppEvent;
use crate::tunnel::{manager::TunnelManager, TunnelState};
use crate::ui;

// ── EditState ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum EditKind {
    Ssh,
    Kubernetes,
    KubernetesSsh,
    KubernetesBastionSsh,
}

pub const FIELDS_SSH: &[&str] = &[
    "Name",
    "Local Port",
    "Remote Host",
    "Remote Port",
    "SSH Host",
    "SSH User",
    "Identity File",
    "Auto Restart",
];

pub const FIELDS_K8S: &[&str] = &[
    "Name",
    "Local Port",
    "Remote Port",
    "Target",
    "Namespace",
    "Context",
    "Auto Restart",
];
pub const FIELDS_K8S_SSH: &[&str] = &[
    "Name",
    "Local Port",
    "Remote Port",
    "Target",
    "Namespace",
    "Context",
    "SSH Host",
    "SSH User",
    "Identity File",
    "Remote User (sudo)",
    "Auto Restart",
];

pub const FIELDS_K8S_BASTION: &[&str] = &[
    "Name",
    "Local Port",
    "Remote Port",
    "Target",
    "Namespace",
    "Context",
    "Bastion Host",
    "Bastion User",
    "Bastion Identity File",
    "Target Host",
    "Target User",
    "Target Identity File",
    "Target Remote User (sudo)",
    "Auto Restart",
];

#[derive(Debug, Default)]
pub struct EditState {
    pub is_new: bool,
    pub original_name: String,
    pub kind: Option<EditKind>,
    // common
    pub name: String,
    pub local_port: String,
    pub auto_restart: bool,
    // ssh
    pub remote_host: String,
    pub remote_port: String,
    pub ssh_host: String,
    pub ssh_user: String,
    pub identity_file: String,
    // k8s
    pub k8s_remote_port: String,
    pub k8s_target: String,
    pub k8s_namespace: String,
    pub k8s_context: String,
    pub k8s_remote_user: String,
    // bastion-ssh specific
    pub bastion_host: String,
    pub bastion_user: String,
    pub bastion_identity_file: String,
    pub target_host: String,
    pub target_user: String,
    pub target_identity_file: String,
    pub target_remote_user: String,

    pub field: usize,
    pub error: Option<String>,
}

impl EditState {
    pub fn new_ssh() -> Self {
        Self {
            is_new: true,
            kind: Some(EditKind::Ssh),
            auto_restart: true,
            ..Default::default()
        }
    }

    pub fn new_k8s() -> Self {
        Self {
            is_new: true,
            kind: Some(EditKind::Kubernetes),
            auto_restart: true,
            ..Default::default()
        }
    }

    pub fn new_k8s_ssh() -> Self {
        Self {
            is_new: true,
            kind: Some(EditKind::KubernetesSsh),
            auto_restart: true,
            ..Default::default()
        }
    }

    pub fn new_k8s_bastion() -> Self {
        Self {
            is_new: true,
            kind: Some(EditKind::KubernetesBastionSsh),
            auto_restart: true,
            ..Default::default()
        }
    }

    pub fn from_config(c: &TunnelConfig) -> Self {
        let ek = match c.kind.as_str() {
            kind::SSH => EditKind::Ssh,
            kind::KUBERNETES => EditKind::Kubernetes,
            kind::K8S_VIA_SSH => EditKind::KubernetesSsh,
            kind::K8S_VIA_BASTION => EditKind::KubernetesBastionSsh,
            _ => EditKind::Ssh,
        };
        Self {
            is_new: false,
            original_name: c.name.clone(),
            kind: Some(ek),
            name: c.name.clone(),
            local_port: c.local_port.to_string(),
            remote_host: c.remote_host.clone().unwrap_or_default(),
            remote_port: c.remote_port.to_string(),
            ssh_host: c.ssh_host.clone().unwrap_or_default(),
            ssh_user: c.ssh_user.clone().unwrap_or_default(),
            identity_file: c
                .identity_file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            k8s_remote_port: c.remote_port.to_string(),
            k8s_target: c.target.clone().unwrap_or_default(),
            k8s_namespace: c.namespace.clone().unwrap_or_default(),
            k8s_context: c.context.clone().unwrap_or_default(),
            k8s_remote_user: c.remote_user.clone().unwrap_or_default(),
            bastion_host: c.bastion_host.clone().unwrap_or_default(),
            bastion_user: c.bastion_user.clone().unwrap_or_default(),
            bastion_identity_file: c
                .bastion_identity_file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            target_host: c.target_host.clone().unwrap_or_default(),
            target_user: c.target_user.clone().unwrap_or_default(),
            target_identity_file: c
                .target_identity_file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            target_remote_user: c.target_remote_user.clone().unwrap_or_default(),
            auto_restart: c.auto_restart,
            ..Default::default()
        }
    }

    pub fn total_fields(&self) -> usize {
        match self.kind {
            Some(EditKind::Ssh) => FIELDS_SSH.len(),
            Some(EditKind::Kubernetes) => FIELDS_K8S.len(),
            Some(EditKind::KubernetesSsh) => FIELDS_K8S_SSH.len(),
            Some(EditKind::KubernetesBastionSsh) => FIELDS_K8S_BASTION.len(),
            None => 1,
        }
    }

    pub fn is_bool_field(&self) -> bool {
        self.field + 1 == self.total_fields()
    }

    pub fn field_labels(&self) -> &[&'static str] {
        match self.kind {
            Some(EditKind::Ssh) => FIELDS_SSH,
            Some(EditKind::Kubernetes) => FIELDS_K8S,
            Some(EditKind::KubernetesSsh) => FIELDS_K8S_SSH,
            Some(EditKind::KubernetesBastionSsh) => FIELDS_K8S_BASTION,
            None => &[],
        }
    }

    pub fn field_value(&self, idx: usize) -> String {
        match &self.kind {
            Some(EditKind::Ssh) => match idx {
                0 => self.name.clone(),
                1 => self.local_port.clone(),
                2 => self.remote_host.clone(),
                3 => self.remote_port.clone(),
                4 => self.ssh_host.clone(),
                5 => self.ssh_user.clone(),
                6 => self.identity_file.clone(),
                _ => bool_str(self.auto_restart),
            },
            Some(EditKind::Kubernetes) => match idx {
                0 => self.name.clone(),
                1 => self.local_port.clone(),
                2 => self.k8s_remote_port.clone(),
                3 => self.k8s_target.clone(),
                4 => self.k8s_namespace.clone(),
                5 => self.k8s_context.clone(),
                _ => bool_str(self.auto_restart),
            },
            Some(EditKind::KubernetesSsh) => match idx {
                0 => self.name.clone(),
                1 => self.local_port.clone(),
                2 => self.k8s_remote_port.clone(),
                3 => self.k8s_target.clone(),
                4 => self.k8s_namespace.clone(),
                5 => self.k8s_context.clone(),
                6 => self.ssh_host.clone(),
                7 => self.ssh_user.clone(),
                8 => self.identity_file.clone(),
                9 => self.k8s_remote_user.clone(),
                _ => bool_str(self.auto_restart),
            },
            Some(EditKind::KubernetesBastionSsh) => match idx {
                0 => self.name.clone(),
                1 => self.local_port.clone(),
                2 => self.k8s_remote_port.clone(),
                3 => self.k8s_target.clone(),
                4 => self.k8s_namespace.clone(),
                5 => self.k8s_context.clone(),
                6 => self.bastion_host.clone(),
                7 => self.bastion_user.clone(),
                8 => self.bastion_identity_file.clone(),
                9 => self.target_host.clone(),
                10 => self.target_user.clone(),
                11 => self.target_identity_file.clone(),
                12 => self.target_remote_user.clone(),
                _ => bool_str(self.auto_restart),
            },
            None => String::new(),
        }
    }

    pub fn toggle_bool(&mut self) {
        if self.is_bool_field() {
            self.auto_restart = !self.auto_restart;
        }
    }

    pub fn backspace(&mut self) {
        if let Some(f) = self.current_text_field() {
            f.pop();
        }
    }

    pub fn push_char(&mut self, c: char) {
        if let Some(f) = self.current_text_field() {
            f.push(c);
        }
    }

    fn current_text_field(&mut self) -> Option<&mut String> {
        if self.is_bool_field() {
            return None;
        }
        match &self.kind {
            Some(EditKind::Ssh) => match self.field {
                0 => Some(&mut self.name),
                1 => Some(&mut self.local_port),
                2 => Some(&mut self.remote_host),
                3 => Some(&mut self.remote_port),
                4 => Some(&mut self.ssh_host),
                5 => Some(&mut self.ssh_user),
                6 => Some(&mut self.identity_file),
                _ => None,
            },
            Some(EditKind::Kubernetes) => match self.field {
                0 => Some(&mut self.name),
                1 => Some(&mut self.local_port),
                2 => Some(&mut self.k8s_remote_port),
                3 => Some(&mut self.k8s_target),
                4 => Some(&mut self.k8s_namespace),
                5 => Some(&mut self.k8s_context),
                _ => None,
            },
            Some(EditKind::KubernetesSsh) => match self.field {
                0 => Some(&mut self.name),
                1 => Some(&mut self.local_port),
                2 => Some(&mut self.k8s_remote_port),
                3 => Some(&mut self.k8s_target),
                4 => Some(&mut self.k8s_namespace),
                5 => Some(&mut self.k8s_context),
                6 => Some(&mut self.ssh_host),
                7 => Some(&mut self.ssh_user),
                8 => Some(&mut self.identity_file),
                9 => Some(&mut self.k8s_remote_user),
                _ => None,
            },
            Some(EditKind::KubernetesBastionSsh) => match self.field {
                0 => Some(&mut self.name),
                1 => Some(&mut self.local_port),
                2 => Some(&mut self.k8s_remote_port),
                3 => Some(&mut self.k8s_target),
                4 => Some(&mut self.k8s_namespace),
                5 => Some(&mut self.k8s_context),
                6 => Some(&mut self.bastion_host),
                7 => Some(&mut self.bastion_user),
                8 => Some(&mut self.bastion_identity_file),
                9 => Some(&mut self.target_host),
                10 => Some(&mut self.target_user),
                11 => Some(&mut self.target_identity_file),
                12 => Some(&mut self.target_remote_user),
                _ => None,
            },
            None => None,
        }
    }

    pub fn to_config(&self) -> Result<TunnelConfig, String> {
        let name = self.name.trim().to_owned();
        if name.is_empty() {
            return Err("name is required".into());
        }
        let local_port = parse_port(&self.local_port, "local port")?;

        match &self.kind {
            Some(EditKind::Ssh) => {
                let ssh_host = self.ssh_host.trim().to_owned();
                if ssh_host.is_empty() {
                    return Err("SSH host is required".into());
                }
                let remote_host = self.remote_host.trim().to_owned();
                if remote_host.is_empty() {
                    return Err("remote host is required".into());
                }
                let remote_port = parse_port(&self.remote_port, "remote port")?;
                Ok(TunnelConfig {
                    name,
                    local_port,
                    remote_port,
                    kind: kind::SSH.to_owned(),
                    remote_host: Some(remote_host),
                    ssh_host: Some(ssh_host),
                    ssh_user: nonempty(&self.ssh_user),
                    identity_file: nonempty(&self.identity_file).map(PathBuf::from),
                    auto_restart: self.auto_restart,
                    ..Default::default()
                })
            }
            Some(EditKind::Kubernetes) => {
                let target = self.k8s_target.trim().to_owned();
                if target.is_empty() {
                    return Err("target is required (e.g. svc/my-service)".into());
                }
                let remote_port = parse_port(&self.k8s_remote_port, "remote port")?;
                Ok(TunnelConfig {
                    name,
                    local_port,
                    remote_port,
                    kind: kind::KUBERNETES.to_owned(),
                    target: Some(target),
                    namespace: nonempty(&self.k8s_namespace),
                    context: nonempty(&self.k8s_context),
                    auto_restart: self.auto_restart,
                    ..Default::default()
                })
            }
            Some(EditKind::KubernetesSsh) => {
                let ssh_host = self.ssh_host.trim().to_owned();
                if ssh_host.is_empty() {
                    return Err("SSH host is required".into());
                }
                let ssh_user = self.ssh_user.trim().to_owned();
                if ssh_user.is_empty() {
                    return Err("SSH user is required".into());
                }
                let target = self.k8s_target.trim().to_owned();
                if target.is_empty() {
                    return Err("target is required (e.g. svc/my-service)".into());
                }
                let remote_port = parse_port(&self.k8s_remote_port, "remote port")?;
                Ok(TunnelConfig {
                    name,
                    local_port,
                    remote_port,
                    kind: kind::K8S_VIA_SSH.to_owned(),
                    ssh_host: Some(ssh_host),
                    ssh_user: Some(ssh_user),
                    identity_file: nonempty(&self.identity_file).map(PathBuf::from),
                    target: Some(target),
                    namespace: nonempty(&self.k8s_namespace),
                    context: nonempty(&self.k8s_context),
                    remote_user: nonempty(&self.k8s_remote_user),
                    auto_restart: self.auto_restart,
                    ..Default::default()
                })
            }
            Some(EditKind::KubernetesBastionSsh) => {
                let bastion_host = self.bastion_host.trim().to_owned();
                if bastion_host.is_empty() {
                    return Err("bastion host is required".into());
                }
                let bastion_user = self.bastion_user.trim().to_owned();
                if bastion_user.is_empty() {
                    return Err("bastion user is required".into());
                }
                let target_host = self.target_host.trim().to_owned();
                if target_host.is_empty() {
                    return Err("target host is required".into());
                }
                let target_user = self.target_user.trim().to_owned();
                if target_user.is_empty() {
                    return Err("target user is required".into());
                }
                let target = self.k8s_target.trim().to_owned();
                if target.is_empty() {
                    return Err("target is required (e.g. svc/my-service)".into());
                }
                let remote_port = parse_port(&self.k8s_remote_port, "remote port")?;
                Ok(TunnelConfig {
                    name,
                    local_port,
                    remote_port,
                    kind: kind::K8S_VIA_BASTION.to_owned(),
                    bastion_host: Some(bastion_host),
                    bastion_user: Some(bastion_user),
                    bastion_identity_file: nonempty(&self.bastion_identity_file).map(PathBuf::from),
                    target_host: Some(target_host),
                    target_user: Some(target_user),
                    target_identity_file: nonempty(&self.target_identity_file).map(PathBuf::from),
                    target: Some(target),
                    namespace: nonempty(&self.k8s_namespace),
                    context: nonempty(&self.k8s_context),
                    target_remote_user: nonempty(&self.target_remote_user),
                    auto_restart: self.auto_restart,
                    ..Default::default()
                })
            }
            None => Err("no tunnel kind selected".into()),
        }
    }
}

fn nonempty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_owned())
    }
}

fn parse_port(value: &str, label: &str) -> Result<u16, String> {
    match value.trim().parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(format!("{label} must be a number 1-65535")),
    }
}

fn bool_str(b: bool) -> String {
    if b {
        "yes".into()
    } else {
        "no".into()
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

pub enum Mode {
    Normal,
    Edit,
    Help,
}

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
        self.tunnel_states
            .get(name)
            .unwrap_or(&TunnelState::Stopped)
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

// ── Event loop ────────────────────────────────────────────────────────────────

pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    tx: mpsc::Sender<AppEvent>,
    mut rx: mpsc::Receiver<AppEvent>,
) -> Result<()> {
    let mut app = App::new()?;
    let mut manager = TunnelManager::new(tx.clone());

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

            Some(AppEvent::Log { tunnel, line }) => {
                app.push_log(format!("[{}] {}", tunnel, line));
            }

            Some(AppEvent::TunnelStateChanged { name, state }) => {
                let log_line = match &state {
                    TunnelState::Failed { reason } => format!("[{}] → FAILED: {}", name, reason),
                    _ => format!("[{}] → {}", name, state.label()),
                };
                app.push_log(log_line);
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

    manager.stop_all().await;
    Ok(())
}

// ── Normal mode ───────────────────────────────────────────────────────────────

fn handle_normal(
    app: &mut App,
    manager: &mut TunnelManager,
    key: crossterm::event::KeyEvent,
) -> Result<bool> {
    let len = app.project.tunnels.len();
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(true),
        (KeyCode::Char('?'), _) => app.mode = Mode::Help,
        (KeyCode::Up | KeyCode::Char('k'), _) => {
            app.selected = app.selected.saturating_sub(1);
        }
        (KeyCode::Down | KeyCode::Char('j'), _) => {
            if len > 0 {
                app.selected = (app.selected + 1).min(len - 1);
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
            if let Some(t) = app.selected_tunnel() {
                app.edit = EditState::from_config(t);
                app.mode = Mode::Edit;
            }
        }
        (KeyCode::Char('n'), _) => {
            app.edit = EditState::new_ssh();
            app.mode = Mode::Edit;
        }
        (KeyCode::Char('N'), _) => {
            app.edit = EditState::new_k8s();
            app.mode = Mode::Edit;
        }
        (KeyCode::Char('K'), _) => {
            app.edit = EditState::new_k8s_ssh();
            app.mode = Mode::Edit;
        }
        (KeyCode::Char('B'), _) => {
            app.edit = EditState::new_k8s_bastion();
            app.mode = Mode::Edit;
        }
        (KeyCode::Char('d') | KeyCode::Delete, _) => {
            if len > 0 {
                let name = app.project.tunnels[app.selected].name.clone();
                manager.stop(&name);
                app.project.tunnels.remove(app.selected);
                app.selected = app
                    .selected
                    .min(app.project.tunnels.len().saturating_sub(1));
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

// ── Edit mode ─────────────────────────────────────────────────────────────────

fn handle_edit(
    app: &mut App,
    manager: &mut TunnelManager,
    key: crossterm::event::KeyEvent,
) -> Result<()> {
    let total = app.edit.total_fields();
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.edit.error = None;
        }
        KeyCode::Tab | KeyCode::Down => {
            app.edit.field = (app.edit.field + 1) % total;
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.edit.field = app.edit.field.checked_sub(1).unwrap_or(total - 1);
        }
        KeyCode::Char(' ') if app.edit.is_bool_field() => {
            app.edit.toggle_bool();
        }
        KeyCode::Enter => match app.edit.to_config() {
            Err(msg) => app.edit.error = Some(msg),
            Ok(new_cfg) => {
                app.edit.error = None;
                if app.edit.is_new {
                    app.project.tunnels.push(new_cfg);
                } else {
                    manager.stop(&app.edit.original_name);
                    if let Some(t) = app.project.tunnels.get_mut(app.selected) {
                        *t = new_cfg;
                    }
                }
                save(app)?;
                app.mode = Mode::Normal;
            }
        },
        KeyCode::Backspace => {
            app.edit.backspace();
        }
        KeyCode::Char(c) => {
            app.edit.push_char(c);
        }
        _ => {}
    }
    Ok(())
}

fn save(app: &App) -> Result<()> {
    let path = app
        .project_path
        .clone()
        .unwrap_or_else(|| config::project_path("default"));
    config::save_project(&path, &app.project)
}
