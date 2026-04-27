use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::{App, EditKind};

pub fn render(f: &mut Frame<'_>, app: &App) {
    let area = centered_rect(60, 80, f.size());
    f.render_widget(Clear, area);

    let kind_str = match app.edit.kind {
        Some(EditKind::Ssh) => "SSH",
        Some(EditKind::Kubernetes) => "Kubernetes",
        Some(EditKind::KubernetesSsh) => "Kubernetes via SSH",
        Some(EditKind::KubernetesBastionSsh) => "Kubernetes via Bastion+SSH",
        None => "?",
    };
    let title = if app.edit.is_new {
        format!(" New Tunnel ({kind_str}) ")
    } else {
        format!(" Edit Tunnel ({kind_str}) ")
    };
    let block = Block::default()
        .title(title.as_str())
        .title_bottom(" [Tab/↑↓] field  [Enter] save  [Space] toggle  [Esc] cancel ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let labels = app.edit.field_labels();
    let last = labels.len().saturating_sub(1);

    let mut items: Vec<ListItem> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let active = app.edit.field == i;
            let is_bool = i == last;
            let value = app.edit.field_value(i);

            let label_sty = if active {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let value_sty = if is_bool {
                Style::default()
                    .fg(if value == "yes" {
                        Color::Green
                    } else {
                        Color::DarkGray
                    })
                    .add_modifier(Modifier::BOLD)
            } else if active {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let cursor = if active && !is_bool { "█" } else { "" };

            ListItem::new(Line::from(vec![
                Span::styled(format!("  {:<16} ", label), label_sty),
                Span::styled(format!("{}{}", value, cursor), value_sty),
            ]))
        })
        .collect();

    if let Some(err) = &app.edit.error {
        items.push(ListItem::new(Line::from(Span::styled(
            format!("  ✗ {err}"),
            Style::default().fg(Color::Red),
        ))));
    }

    let mut state = ListState::default();
    state.select(Some(app.edit.field));
    f.render_stateful_widget(
        List::new(items).highlight_style(Style::default()),
        inner,
        &mut state,
    );
}

fn centered_rect(pct_x: u16, pct_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(vert[1])[1]
}
