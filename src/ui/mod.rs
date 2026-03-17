pub mod detail;
pub mod help;
pub mod tunnel_list;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode};

pub fn render(f: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(6)])
        .split(f.size());

    tunnel_list::render(f, app, chunks[0]);
    render_logs(f, app, chunks[1]);

    match app.mode {
        Mode::Help => help::render(f),
        Mode::Edit => detail::render(f, app),
        Mode::Normal => {}
    }
}

fn render_logs(f: &mut Frame<'_>, app: &App, area: ratatui::layout::Rect) {
    let visible = area.height.saturating_sub(2) as usize;
    let start = app.logs.len().saturating_sub(visible);

    let lines: Vec<Line> = app.logs[start..]
        .iter()
        .map(|m| Line::from(ratatui::text::Span::styled(m.as_str(), Style::default().fg(Color::DarkGray))))
        .collect();

    let block = Block::default()
        .title(" Logs ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    f.render_widget(Paragraph::new(lines).block(block), area);
}
