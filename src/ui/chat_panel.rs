//! AI reasoning / chat panel — shows LLM reasoning traces and trade negotiations.
//!
//! Renders a scrollable panel of AI "thoughts" that were sent via UiEvent messages
//! containing reasoning text. Visually distinct from the game log which shows
//! mechanical game events.

use super::PLAYER_COLORS;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// A single chat message from an AI player.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// Player name.
    pub player: String,
    /// Player index (for coloring).
    pub player_id: usize,
    /// The reasoning or trade message.
    pub text: String,
}

/// Render the AI chat / reasoning panel.
pub fn render_chat(messages: &[ChatMessage], scroll: u16, area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line> = Vec::new();

    for msg in messages {
        let color = PLAYER_COLORS
            .get(msg.player_id)
            .copied()
            .unwrap_or(Color::White);

        // Player name header.
        let mut spans: Vec<Span> = vec![Span::styled(
            format!("{}: ", msg.player),
            Style::default().fg(color).bold(),
        )];

        // Truncate long reasoning to keep the panel readable (char-safe).
        let text = if msg.text.chars().count() > 200 {
            let truncated: String = msg.text.chars().take(197).collect();
            format!("{}...", truncated)
        } else {
            msg.text.clone()
        };

        spans.push(Span::styled(text, Style::default().fg(Color::Gray)));
        lines.push(Line::from(spans));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Waiting for AI reasoning...",
            Style::default().fg(Color::DarkGray).italic(),
        )));
    }

    let visible_height = area.height.saturating_sub(2) as usize;
    let total_lines = lines.len();
    let max_scroll = total_lines.saturating_sub(visible_height) as u16;
    let effective_scroll = scroll.min(max_scroll);

    let block = Block::default()
        .title(format!(" AI Reasoning ({}) ", total_lines))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll, 0));

    paragraph.render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_messages_at_char_boundary() {
        let long_text = "a".repeat(300);
        let messages = vec![ChatMessage {
            player: "Alice".into(),
            player_id: 0,
            text: long_text,
        }];
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        render_chat(&messages, 0, area, &mut buf);
        // Should not panic — the truncation is char-safe.
    }

    #[test]
    fn truncates_multibyte_chars_without_panic() {
        // 100 two-byte chars = 200 bytes but 100 chars — should NOT truncate.
        let short_emoji = "\u{00e9}".repeat(100);
        let messages = vec![ChatMessage {
            player: "Bob".into(),
            player_id: 1,
            text: short_emoji,
        }];
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        render_chat(&messages, 0, area, &mut buf);

        // 201 multi-byte chars — should truncate safely.
        let long_emoji = "\u{00e9}".repeat(250);
        let messages2 = vec![ChatMessage {
            player: "Bob".into(),
            player_id: 1,
            text: long_emoji,
        }];
        render_chat(&messages2, 0, area, &mut buf);
    }

    #[test]
    fn empty_messages_show_placeholder() {
        let messages: Vec<ChatMessage> = vec![];
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        render_chat(&messages, 0, area, &mut buf);
        // Should render without panic.
    }

    #[test]
    fn player_color_out_of_bounds_defaults_to_white() {
        let messages = vec![ChatMessage {
            player: "Player5".into(),
            player_id: 99, // out of bounds
            text: "test".into(),
        }];
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        render_chat(&messages, 0, area, &mut buf);
    }
}
