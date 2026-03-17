use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::{App, EDIT_FIELD_COUNT};

pub fn render(f: &mut Frame<'_>, app: &App) {
    let area = centered_rect(58, 75, f.size());
    f.render_widget(Clear, area);

    let title = if app.edit.is_new { " New Tunnel " } else { " Edit Tunnel " };
    let footer = " [Tab/↑↓] navigate  [Enter] save  [Space] toggle bool  [Esc] cancel ";

    let block = Block::default()
        .title(title)
        .title_bottom(footer)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let s = &app.edit;

    let fields: Vec<(&str, String)> = vec![
        ("Name",          s.name.clone()),
        ("Local Port",    s.local_port.clone()),
        ("Remote Host",   s.remote_host.clone()),
        ("Remote Port",   s.remote_port.clone()),
        ("SSH Host",      s.ssh_host.clone()),
        ("SSH User",      s.ssh_user.clone()),
        ("Identity File", s.identity_file.clone()),
        ("Auto Restart",  if s.auto_restart { "yes".into() } else { "no".into() }),
    ];

    let items: Vec<ListItem> = fields
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let active = s.field == i;
            let is_bool = i == EDIT_FIELD_COUNT;

            let label_style = if active {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let value_style = if active {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let cursor = if active && !is_bool { "█" } else { "" };
            let value_col = if is_bool {
                let col = if value == "yes" { Color::Green } else { Color::DarkGray };
                Style::default().fg(col).add_modifier(Modifier::BOLD)
            } else {
                value_style
            };

            let spans = Line::from(vec![
                Span::styled(format!("  {:<15} ", label), label_style),
                Span::styled(format!("{}{}", value, cursor), value_col),
            ]);

            ListItem::new(spans)
        })
        .collect();

    // error row
    let mut all_items = items;
    if let Some(err) = &s.error {
        all_items.push(ListItem::new(Line::from(Span::styled(
            format!("  ✗ {err}"),
            Style::default().fg(Color::Red),
        ))));
    }

    let mut state = ListState::default();
    state.select(Some(s.field));

    let list = List::new(all_items)
        .highlight_style(Style::default().bg(Color::Reset)); // highlight handled per-item

    f.render_stateful_widget(list, inner, &mut state);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}
