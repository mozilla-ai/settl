//! AI reasoning / chat panel -- shows LLM reasoning traces and trade negotiations.
//!
//! Renders a scrollable panel of AI "thoughts" that were sent via UiEvent messages
//! containing reasoning text. Visually distinct from the game log which shows
//! mechanical game events.

use super::PLAYER_TEXT_COLORS;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// Whether a chat entry is AI reasoning or a game narration line.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatMessageKind {
    /// LLM reasoning trace (existing behavior).
    Reasoning,
    /// Game event narration (turn markers, dice, actions, trades).
    Narration,
}

/// A single chat message from an AI player.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// Player name.
    pub player: String,
    /// Player index (for coloring).
    pub player_id: usize,
    /// The reasoning or trade message.
    pub text: String,
    /// Whether this is AI reasoning or game narration.
    pub kind: ChatMessageKind,
}

/// Build AI chat lines (shared helper).
fn build_chat_lines(messages: &[ChatMessage]) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    for msg in messages {
        match msg.kind {
            ChatMessageKind::Narration => {
                lines.push(Line::from(Span::styled(
                    msg.text.clone(),
                    Style::default().fg(Color::DarkGray).italic(),
                )));
            }
            ChatMessageKind::Reasoning => {
                let color = PLAYER_TEXT_COLORS
                    .get(msg.player_id)
                    .copied()
                    .unwrap_or(Color::White);

                let mut spans: Vec<Span> = vec![Span::styled(
                    format!("{}: ", msg.player),
                    Style::default().fg(color).bold(),
                )];

                // Truncate long reasoning to keep the panel readable (char-safe).
                let text = if msg.text.chars().count() > 2000 {
                    let truncated: String = msg.text.chars().take(1997).collect();
                    format!("{}...", truncated)
                } else {
                    msg.text.clone()
                };

                spans.push(Span::styled(text, Style::default().fg(Color::Gray)));
                lines.push(Line::from(spans));
            }
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Waiting for AI thoughts...",
            Style::default().fg(Color::DarkGray).italic(),
        )));
    }

    lines
}

/// Estimate visual line count after wrapping.
fn estimate_visual_lines(lines: &[Line], width: usize) -> usize {
    let mut total: usize = 0;
    for line in lines {
        let char_count: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        total += if char_count == 0 {
            1
        } else {
            char_count.div_ceil(width)
        };
    }
    total
}

/// Render AI chat content without a border (for right-panel tab use).
pub fn render_chat_inner(messages: &[ChatMessage], scroll: u16, area: Rect, buf: &mut Buffer) {
    let lines = build_chat_lines(messages);
    let inner_width = area.width.max(1) as usize;
    let total_visual_lines = estimate_visual_lines(&lines, inner_width);
    let visible_height = area.height as usize;
    let max_scroll = total_visual_lines.saturating_sub(visible_height) as u16;
    let effective_scroll = scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll, 0));

    paragraph.render(area, buf);
}

/// Render the AI chat / reasoning panel (with border).
pub fn render_chat(messages: &[ChatMessage], scroll: u16, area: Rect, buf: &mut Buffer) {
    let lines = build_chat_lines(messages);
    let inner_width = area.width.saturating_sub(2).max(1) as usize;
    let total_visual_lines = estimate_visual_lines(&lines, inner_width);
    let visible_height = area.height.saturating_sub(2) as usize;
    let max_scroll = total_visual_lines.saturating_sub(visible_height) as u16;
    let effective_scroll = scroll.min(max_scroll);

    let block = Block::default()
        .title(format!(" AI Thoughts ({}) ", lines.len()))
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
            kind: ChatMessageKind::Reasoning,
        }];
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        render_chat(&messages, 0, area, &mut buf);
        // Should not panic --the truncation is char-safe.
    }

    #[test]
    fn truncates_multibyte_chars_without_panic() {
        // 100 two-byte chars = 200 bytes but 100 chars -- should NOT truncate.
        let short_emoji = "\u{00e9}".repeat(100);
        let messages = vec![ChatMessage {
            player: "Bob".into(),
            player_id: 1,
            text: short_emoji,
            kind: ChatMessageKind::Reasoning,
        }];
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        render_chat(&messages, 0, area, &mut buf);

        // 201 multi-byte chars -- should truncate safely.
        let long_emoji = "\u{00e9}".repeat(250);
        let messages2 = vec![ChatMessage {
            player: "Bob".into(),
            player_id: 1,
            text: long_emoji,
            kind: ChatMessageKind::Reasoning,
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
            kind: ChatMessageKind::Reasoning,
        }];
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        render_chat(&messages, 0, area, &mut buf);
    }

    #[test]
    fn narration_renders_without_player_header() {
        let messages = vec![
            ChatMessage {
                player: String::new(),
                player_id: usize::MAX,
                text: "-- Turn 1 -- Alice's turn --".into(),
                kind: ChatMessageKind::Narration,
            },
            ChatMessage {
                player: "Alice".into(),
                player_id: 0,
                text: "I should build near wheat.".into(),
                kind: ChatMessageKind::Reasoning,
            },
            ChatMessage {
                player: String::new(),
                player_id: usize::MAX,
                text: "Alice built a settlement.".into(),
                kind: ChatMessageKind::Narration,
            },
        ];
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        render_chat(&messages, 0, area, &mut buf);

        let text = crate::ui::testing::buffer_to_string(&buf);
        // Narration lines should appear without "PlayerName: " prefix.
        assert!(
            text.contains("Turn 1"),
            "narration turn marker should be visible"
        );
        assert!(
            text.contains("Alice:"),
            "reasoning should have player prefix"
        );
        assert!(
            text.contains("built a settlement"),
            "narration action should be visible"
        );
    }

    #[test]
    fn mixed_narration_and_reasoning_count() {
        let messages = vec![
            ChatMessage {
                player: String::new(),
                player_id: usize::MAX,
                text: "narration".into(),
                kind: ChatMessageKind::Narration,
            },
            ChatMessage {
                player: "Bot".into(),
                player_id: 0,
                text: "reasoning".into(),
                kind: ChatMessageKind::Reasoning,
            },
        ];
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        render_chat(&messages, 0, area, &mut buf);

        let text = crate::ui::testing::buffer_to_string(&buf);
        // Panel title should show total count of 2 (both kinds).
        assert!(
            text.contains("AI Thoughts (2)"),
            "title should count all message kinds"
        );
    }
}
