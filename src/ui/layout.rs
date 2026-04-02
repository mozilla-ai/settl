//! TUI layout — splits the terminal into board, players, and log panels.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::board_view;
use super::chat_panel;
use super::game_log;
use super::resource_bar;
use super::PlayingState;

/// Draw the full TUI game layout (used during the Playing screen).
///
/// ```text
/// +------------------------------------------+------------------+
/// |                                          |    PLAYERS       |
/// |           BOARD VIEW                     |    P0: W:2 B:3   |
/// |           (hex grid)                     |    P1: W:1 B:0   |
/// |                                          |    P2: W:0 B:1   |
/// |                                          |    P3: W:3 B:2   |
/// +------------------------------------------+------------------+
/// |                          |                                   |
/// |    GAME LOG (scrollable) |     AI REASONING (scrollable)     |
/// |                          |                                   |
/// +--------------------------------------------------------------+
/// |  Status bar: speed, paused, controls                         |
/// +--------------------------------------------------------------+
/// ```
pub fn draw_playing(f: &mut Frame, ps: &PlayingState) {
    let size = f.area();

    // Main vertical split: top (board + players) | bottom (log + chat) | status bar.
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(12),        // Board + players
            Constraint::Percentage(40), // Game log + AI chat
            Constraint::Length(1),      // Status bar
        ])
        .split(size);

    // Top horizontal split: board | players.
    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(40),    // Board
            Constraint::Length(24), // Players panel
        ])
        .split(main_chunks[0]);

    // Render board.
    if let Some(state) = &ps.state {
        board_view::render_board(state, top_chunks[0], f.buffer_mut());
        resource_bar::render_players(state, &ps.player_names, top_chunks[1], f.buffer_mut());
    } else {
        let waiting = Paragraph::new("Waiting for game to start...")
            .block(
                Block::default()
                    .title(" Board ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .alignment(Alignment::Center);
        f.render_widget(waiting, top_chunks[0]);

        let no_players = Paragraph::new("").block(
            Block::default()
                .title(" Players ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
        f.render_widget(no_players, top_chunks[1]);
    }

    // Bottom horizontal split: game log | AI reasoning.
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Game log
            Constraint::Percentage(50), // AI reasoning
        ])
        .split(main_chunks[1]);

    // Render game log and AI chat.
    game_log::render_log(
        &ps.messages,
        ps.log_scroll,
        bottom_chunks[0],
        f.buffer_mut(),
    );
    chat_panel::render_chat(
        &ps.chat_messages,
        ps.chat_scroll,
        bottom_chunks[1],
        f.buffer_mut(),
    );

    // Status bar.
    let pause_indicator = if ps.paused { " PAUSED " } else { "" };
    let status = Line::from(vec![
        Span::styled(
            format!(" Speed: {}ms ", ps.speed_ms),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            pause_indicator,
            Style::default().fg(Color::Black).bg(Color::Yellow).bold(),
        ),
        Span::styled(
            " | q:quit  Space:pause  +/-:speed  j/k:scroll ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            if ps.game_over {
                " GAME OVER — press Enter "
            } else {
                ""
            },
            Style::default().fg(Color::Black).bg(Color::Green).bold(),
        ),
    ]);
    let status_paragraph = Paragraph::new(status);
    f.render_widget(status_paragraph, main_chunks[2]);

    // Human input overlay.
    if let Some(ref prompt) = ps.pending_prompt {
        draw_human_prompt(f, prompt);
    }
}

/// Draw a centered popup overlay for human player input.
fn draw_human_prompt(f: &mut Frame, prompt: &super::PendingHumanPrompt) {
    let area = f.area();

    // Size the popup to fit the content.
    let max_option_len = prompt.options.iter().map(|o| o.len()).max().unwrap_or(10);
    let popup_width = (max_option_len as u16 + 8)
        .max(prompt.title.len() as u16 + 6)
        .min(area.width - 4);
    let popup_height = (prompt.options.len() as u16 + 4).min(area.height - 2);

    let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup.
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(format!(" {} ", prompt.title))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow).bold());
    f.render_widget(block, popup_area);

    // Render options inside the block.
    let inner = Rect::new(popup_x + 2, popup_y + 1, popup_width - 4, popup_height - 2);
    for (i, option) in prompt.options.iter().enumerate() {
        let y = inner.y + i as u16;
        if y >= inner.y + inner.height {
            break;
        }
        let is_selected = i == prompt.selected;
        let marker = if is_selected { ">" } else { " " };
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan).bold()
        } else {
            Style::default().fg(Color::White)
        };
        let line_area = Rect::new(inner.x, y, inner.width, 1);
        let text = format!("{} {}", marker, option);
        let para = Paragraph::new(text).style(style);
        f.render_widget(para, line_area);
    }

    // Hint at bottom of popup.
    let hint_y = popup_y + popup_height - 1;
    if hint_y > popup_y {
        let hint_area = Rect::new(popup_x + 1, hint_y, popup_width - 2, 1);
        let hint = Paragraph::new(" [↑↓] select  [Enter] confirm ")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hint, hint_area);
    }
}
