use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::App;
use crate::tunnel::TunnelState;

pub fn render(f: &mut Frame<'_>, app: &App, area: Rect) {
    let title = format!(" PortPilot  ·  project: {} ", app.project_name());
    let hint = " [↑↓/jk] nav  [↵/ ] toggle  [D] doctor  [e] edit  [n] ssh  [N] k8s  [K] k8s+ssh  [B] k8s+bastion  [d] del  [Tab] project  [?] help  [q] quit ";

    let items: Vec<ListItem> = if app.project.tunnels.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No tunnels yet — press [n] SSH, [N] Kubernetes, [K] Kubernetes via SSH, or [B] Kubernetes via bastion",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        app.project
            .tunnels
            .iter()
            .map(|t| {
                let state = app.tunnel_state(&t.name);
                let (sym, col) = indicator(state);
                let pid_str = match state {
                    TunnelState::Up { pid } => format!("  pid:{pid}"),
                    _ => String::new(),
                };
                Line::from(vec![
                    Span::styled(format!(" {sym} "), Style::default().fg(col)),
                    Span::styled(format!("{:<20}", t.name), Style::default().fg(Color::White)),
                    Span::styled(
                        format!(" {}  ", t.connection_label()),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(
                        format!("[{}] ", t.kind_label()),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{:<12}{}", state.label(), pid_str),
                        Style::default().fg(col).add_modifier(Modifier::BOLD),
                    ),
                ])
                .into()
            })
            .collect()
    };

    let mut list_state = ListState::default();
    if !app.project.tunnels.is_empty() {
        list_state.select(Some(app.selected));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .title_bottom(hint)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut list_state);
}

fn indicator(state: &TunnelState) -> (&'static str, Color) {
    match state {
        TunnelState::Up { .. } => ("●", Color::Green),
        TunnelState::Connecting => ("◌", Color::Yellow),
        TunnelState::Failed { .. } => ("✗", Color::Red),
        TunnelState::Stopped => ("○", Color::DarkGray),
    }
}
