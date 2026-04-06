//! Scrollable game log panel.

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Wrap};

/// Get the display color for a game log message.
pub fn message_color(msg: &str) -> Color {
    if msg.starts_with("GAME OVER") || msg.contains("wins") {
        Color::Yellow
    } else if msg.contains("Trade") || msg.contains("trade") {
        Color::Cyan
    } else if msg.contains("Rolled") {
        Color::White
    } else if msg.contains("Settlement") || msg.contains("City") || msg.contains("Road") {
        Color::Green
    } else if msg.contains("Robber") || msg.contains("Stole") {
        Color::Red
    } else if msg.contains("Setup") {
        Color::DarkGray
    } else {
        Color::Gray
    }
}

/// Build game log lines (shared helper).
fn build_log_lines(messages: &[String]) -> Vec<Line<'static>> {
    messages
        .iter()
        .map(|msg| {
            let mut style = Style::default().fg(message_color(msg));
            if msg.starts_with("GAME OVER") {
                style = style.bold();
            }
            Line::from(Span::styled(msg.clone(), style))
        })
        .collect()
}

/// Render the game log without a border (for use inside a shared panel).
pub fn render_log_inner(messages: &[String], scroll: u16, area: Rect, buf: &mut Buffer) {
    let lines = build_log_lines(messages);
    let visible_height = area.height as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(visible_height) as u16;
    let effective_scroll = scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll, 0));

    paragraph.render(area, buf);
}
