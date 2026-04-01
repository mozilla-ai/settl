//! Scrollable game log panel.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// Get the display color for a game log message.
pub fn message_color(msg: &str) -> Color {
    if msg.starts_with("GAME OVER") {
        Color::Yellow
    } else if msg.contains("wins") {
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

/// Render the game log as a scrollable panel.
#[allow(dead_code)]
pub fn render_log(messages: &[String], scroll: u16, area: Rect, buf: &mut Buffer) {
    let lines: Vec<Line> = messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let style = if msg.starts_with("GAME OVER") {
                Style::default().fg(Color::Yellow).bold()
            } else if msg.contains("wins") {
                Style::default().fg(Color::Yellow)
            } else if msg.contains("Trade") || msg.contains("trade") {
                Style::default().fg(Color::Cyan)
            } else if msg.contains("Rolled") {
                Style::default().fg(Color::White)
            } else if msg.contains("Settlement") || msg.contains("City") || msg.contains("Road") {
                Style::default().fg(Color::Green)
            } else if msg.contains("Robber") || msg.contains("Stole") {
                Style::default().fg(Color::Red)
            } else if msg.contains("Setup") {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Gray)
            };

            Line::from(Span::styled(format!("{:>4}| {}", i + 1, msg), style))
        })
        .collect();

    // Calculate scroll: keep the view near the bottom.
    let visible_height = area.height.saturating_sub(2) as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(visible_height) as u16;
    let effective_scroll = scroll.min(max_scroll);

    let block = Block::default()
        .title(format!(" Game Log ({}/{}) ", total_lines, total_lines))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll, 0));

    paragraph.render(area, buf);
}
