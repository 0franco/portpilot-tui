use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};
use ratatui::Frame;

pub fn render(f: &mut Frame<'_>) {
    let area = centered_rect(52, 75, f.size());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Help — any key to close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows: &[(&str, &str)] = &[
        ("↑ / k", "move up"),
        ("↓ / j", "move down"),
        ("Enter / ␣", "toggle tunnel on/off"),
        ("D", "doctor selected tunnel"),
        ("n", "new SSH tunnel"),
        ("N", "new Kubernetes tunnel"),
        ("K", "new Kubernetes via SSH tunnel"),
        ("B", "new Kubernetes via bastion tunnel"),
        ("e", "edit selected tunnel"),
        ("d / Del", "delete selected tunnel"),
        ("Tab", "switch project"),
        ("?", "show this help"),
        ("q / Ctrl-c", "quit"),
        ("", ""),
        ("── Edit mode ──", ""),
        ("Tab / ↓", "next field"),
        ("Shift+Tab", "previous field"),
        ("Space", "toggle auto-restart"),
        ("Enter", "save"),
        ("Esc", "cancel"),
    ];

    let items: Vec<ListItem> = rows
        .iter()
        .map(|(key, desc)| {
            if key.is_empty() {
                ListItem::new(Line::from(""))
            } else if desc.is_empty() {
                ListItem::new(Line::from(Span::styled(
                    format!("  {key}"),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )))
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {:<14}", key), Style::default().fg(Color::Cyan)),
                    Span::styled(*desc, Style::default().fg(Color::Gray)),
                ]))
            }
        })
        .collect();

    f.render_widget(List::new(items), inner);
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
