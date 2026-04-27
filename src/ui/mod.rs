pub mod detail;
pub mod help;
pub mod tunnel_list;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Mode};

// Log pane takes 40% of the terminal height, min 8 rows, max 20 rows.
fn log_height(total: u16) -> u16 {
    ((total as f32 * 0.40) as u16).clamp(8, 20)
}

pub fn render(f: &mut Frame<'_>, app: &App) {
    let log_h = log_height(f.size().height);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(log_h)])
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
    // With word-wrap enabled, each raw log line may occupy multiple display
    // rows. We keep up to 200 entries but show only the tail that fits.
    // Use a generous look-back window so long lines still appear.
    let inner_width = area.width.saturating_sub(2) as usize; // minus borders
    let inner_height = area.height.saturating_sub(2) as usize;

    // Estimate how many raw log entries fit by counting wrapped rows from the end.
    let mut row_budget = inner_height;
    let mut start = app.logs.len();
    for msg in app.logs.iter().rev() {
        let wrapped_rows = wrapped_row_count(msg, inner_width);
        if wrapped_rows > row_budget {
            break;
        }
        row_budget -= wrapped_rows;
        start = start.saturating_sub(1);
    }

    let lines: Vec<Line> = app.logs[start..]
        .iter()
        .map(|m| render_log_line(m))
        .collect();

    let block = Block::default()
        .title(" Logs (latest) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_log_line(m: &str) -> Line<'_> {
    // colour the tunnel name bracket differently
    if let Some(rest) = m.strip_prefix('[') {
        if let Some(bracket_end) = rest.find(']') {
            let name_part = format!("[{}]", &rest[..bracket_end]);
            let rest_part = &rest[bracket_end + 1..];
            if let Some((before, marker, after, marker_color)) = split_doctor_status(rest_part) {
                return Line::from(vec![
                    Span::styled(name_part, Style::default().fg(Color::Blue)),
                    Span::styled(before.to_owned(), Style::default().fg(Color::DarkGray)),
                    Span::styled(marker.to_owned(), Style::default().fg(marker_color)),
                    Span::styled(after.to_owned(), Style::default().fg(Color::Gray)),
                ]);
            }

            let color = if m.contains("FAILED") {
                Color::Red
            } else if m.contains("UP") {
                Color::Green
            } else {
                Color::DarkGray
            };
            return Line::from(vec![
                Span::styled(name_part, Style::default().fg(Color::Blue)),
                Span::styled(rest_part.to_owned(), Style::default().fg(color)),
            ]);
        }
    }
    Line::styled(m, Style::default().fg(Color::DarkGray))
}

fn split_doctor_status(rest_part: &str) -> Option<(&str, &str, &str, Color)> {
    for (marker, color) in [
        ("[PASS]", Color::Green),
        ("[FAIL]", Color::Red),
        ("[WARN]", Color::Yellow),
        ("[INFO]", Color::Cyan),
    ] {
        if let Some(start) = rest_part.find(marker) {
            let end = start + marker.len();
            return Some((
                &rest_part[..start],
                &rest_part[start..end],
                &rest_part[end..],
                color,
            ));
        }
    }
    None
}

/// How many terminal rows a single log message occupies at the given width.
fn wrapped_row_count(msg: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    msg.lines()
        .map(|line| {
            let len = line.chars().count();
            if len == 0 {
                1
            } else {
                (len + width - 1) / width
            }
        })
        .sum::<usize>()
        .max(1)
}
