//! TUI layout -- splits the terminal into board, players, context bar, and status.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::game::board::Resource;

use super::board_view;
use super::chat_panel;
use super::game_log;
use super::resource_bar;
use super::{InputMode, PlayingState, TradeSide};

/// Draw the full TUI game layout (used during the Playing screen).
///
/// ```text
/// +------------------------------------------+------------------+
/// |                                          |    PLAYERS       |
/// |           BOARD VIEW                     |    P0: W:2 B:3   |
/// |           (hex grid)                     |    P1: W:1 B:0   |
/// |                                          |                  |
/// +------------------------------------------+------------------+
/// |  CONTEXT BAR (mode-dependent)                                |
/// +--------------------------------------------------------------+
/// |  Status bar                                                   |
/// +--------------------------------------------------------------+
/// ```
pub fn draw_playing(f: &mut Frame, ps: &PlayingState) {
    let size = f.area();

    // Main vertical split: top (board + players) | context bar | status bar.
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(15),   // Board + players
            Constraint::Length(5), // Context bar
            Constraint::Length(1), // Status bar
        ])
        .split(size);

    // Top horizontal split: board | players (or board | AI panel if toggled).
    let right_panel_width = if ps.show_ai_panel { 30 } else { 22 };
    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(50),                   // Board
            Constraint::Length(right_panel_width), // Players or AI panel
        ])
        .split(main_chunks[0]);

    // Render board.
    if let Some(state) = &ps.state {
        if let Some(ref grid) = ps.hex_grid {
            board_view::render_board(state, grid, top_chunks[0], f.buffer_mut(), &ps.input_mode);
        } else {
            let waiting = Paragraph::new("Computing board layout...")
                .block(
                    Block::default()
                        .title(" Board ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                )
                .alignment(Alignment::Center);
            f.render_widget(waiting, top_chunks[0]);
        }

        // Right panel: players or AI reasoning.
        if ps.show_ai_panel {
            chat_panel::render_chat(
                &ps.chat_messages,
                ps.chat_scroll,
                top_chunks[1],
                f.buffer_mut(),
            );
        } else {
            resource_bar::render_players(state, &ps.player_names, top_chunks[1], f.buffer_mut());
        }
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

    // Context bar (mode-dependent content).
    draw_context_bar(f, ps, main_chunks[1]);

    // Status bar.
    draw_status_bar(f, ps, main_chunks[2]);
}

/// Draw the context bar based on the current input mode.
fn draw_context_bar(f: &mut Frame, ps: &PlayingState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    match &ps.input_mode {
        InputMode::Spectating => {
            // Show last few game log messages.
            let n = inner.height as usize;
            let start = ps.messages.len().saturating_sub(n);
            let recent: Vec<Line> = ps.messages[start..]
                .iter()
                .map(|m| {
                    let color = game_log::message_color(m);
                    Line::from(Span::styled(m.as_str(), Style::default().fg(color)))
                })
                .collect();
            let para = Paragraph::new(recent).wrap(Wrap { trim: true });
            f.render_widget(para, inner);
        }

        InputMode::ActionBar { choices, selected } => {
            // Horizontal action menu with shortcuts.
            let mut spans: Vec<Span> = Vec::new();
            for (i, choice) in choices.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw("  "));
                }
                let shortcut = action_shortcut(choice);
                if i == *selected {
                    spans.push(Span::styled(
                        format!("\u{25b8} {}", choice),
                        Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
                    ));
                } else {
                    spans.push(Span::styled(
                        choice.clone(),
                        Style::default().fg(Color::White),
                    ));
                }
                if let Some(key) = shortcut {
                    spans.push(Span::styled(
                        format!("[{}]", key),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            let line1 = Line::from(spans);
            let line2 = Line::from(Span::styled(
                " [Arrow/Enter] select  [s]ettlement  [r]oad  [d]ev card  [t]rade  [e]nd turn",
                Style::default().fg(Color::DarkGray),
            ));
            let para = Paragraph::new(vec![line1, Line::from(""), line2]);
            f.render_widget(para, inner);
        }

        InputMode::BoardCursor { kind, .. } => {
            let kind_name = match kind {
                super::CursorKind::Settlement => "settlement",
                super::CursorKind::Road => "road",
                super::CursorKind::Robber => "robber",
            };
            let lines = vec![
                Line::from(Span::styled(
                    format!(" Place {} -- use arrow keys to navigate", kind_name),
                    Style::default().fg(Color::Yellow).bold(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " [Arrows] move  [n/p] next/prev  [Enter] confirm  [Esc] cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let para = Paragraph::new(lines);
            f.render_widget(para, inner);
        }

        InputMode::TradeBuilder {
            give,
            get,
            side,
            available,
            ..
        } => {
            let res_names = ["Wood", "Brick", "Sheep", "Wheat", "Ore"];
            let give_str = format_resource_counts(give, &res_names);
            let get_str = format_resource_counts(get, &res_names);
            let side_indicator = match side {
                TradeSide::Give => "\u{25b8}GIVE",
                TradeSide::Get => "\u{25b8}GET",
            };
            let lines = vec![
                Line::from(vec![
                    Span::styled(" GIVE: ", Style::default().fg(if *side == TradeSide::Give { Color::Yellow } else { Color::White }).bold()),
                    Span::styled(&give_str, Style::default().fg(Color::White)),
                    Span::raw("     "),
                    Span::styled("GET: ", Style::default().fg(if *side == TradeSide::Get { Color::Yellow } else { Color::White }).bold()),
                    Span::styled(&get_str, Style::default().fg(Color::White)),
                    Span::raw("  "),
                    Span::styled(side_indicator, Style::default().fg(Color::DarkGray)),
                ]),
                Line::from(Span::styled(
                    format!(" Have: W:{} B:{} S:{} H:{} O:{}", available[0], available[1], available[2], available[3], available[4]),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    " [w/b/s/h/o] add  [Tab] switch give/get  [Backspace] undo  [Enter] send  [Esc] cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let para = Paragraph::new(lines);
            f.render_widget(para, inner);
        }

        InputMode::Discard {
            selected: sel,
            count,
            remaining,
            ..
        } => {
            let res_names = ["Wood", "Brick", "Sheep", "Wheat", "Ore"];
            let sel_str = format_resource_list(sel);
            let lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!(" Discard {}/{}: ", sel.len(), count),
                        Style::default().fg(Color::Yellow).bold(),
                    ),
                    Span::styled(&sel_str, Style::default().fg(Color::White)),
                ]),
                Line::from(Span::styled(
                    format!(
                        " Remaining: W:{} B:{} S:{} H:{} O:{}",
                        remaining[0], remaining[1], remaining[2], remaining[3], remaining[4]
                    ),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    " [w/b/s/h/o] add  [Backspace] undo  [Enter] confirm",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let _ = res_names;
            let para = Paragraph::new(lines);
            f.render_widget(para, inner);
        }

        InputMode::ResourcePicker { context } => {
            let lines = vec![
                Line::from(Span::styled(
                    format!(" {}", context),
                    Style::default().fg(Color::Yellow).bold(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " [w]ood  [b]rick  [s]heep  [h]arvest(wheat)  [o]re",
                    Style::default().fg(Color::White),
                )),
            ];
            let para = Paragraph::new(lines);
            f.render_widget(para, inner);
        }

        InputMode::StealTarget { targets, selected } => {
            let mut spans: Vec<Span> = vec![Span::styled(
                " Steal from: ",
                Style::default().fg(Color::Yellow).bold(),
            )];
            for (i, (_, label)) in targets.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw("  "));
                }
                if i == *selected {
                    spans.push(Span::styled(
                        format!("\u{25b8} {}", label),
                        Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
                    ));
                } else {
                    spans.push(Span::styled(
                        label.clone(),
                        Style::default().fg(Color::White),
                    ));
                }
            }
            let lines = vec![
                Line::from(spans),
                Line::from(""),
                Line::from(Span::styled(
                    " [Up/Down] select  [Enter] confirm  [1-4] pick player",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let para = Paragraph::new(lines);
            f.render_widget(para, inner);
        }

        InputMode::TradeResponse { offer } => {
            let offering: String = offer
                .offering
                .iter()
                .map(|(r, n)| format!("{} {}", n, r))
                .collect::<Vec<_>>()
                .join(", ");
            let requesting: String = offer
                .requesting
                .iter()
                .map(|(r, n)| format!("{} {}", n, r))
                .collect::<Vec<_>>()
                .join(", ");
            let lines = vec![
                Line::from(Span::styled(
                    format!(" Trade offer from Player {}", offer.from),
                    Style::default().fg(Color::Yellow).bold(),
                )),
                Line::from(vec![
                    Span::styled(" Offering: ", Style::default().fg(Color::White)),
                    Span::styled(&offering, Style::default().fg(Color::Green)),
                    Span::styled("  Wanting: ", Style::default().fg(Color::White)),
                    Span::styled(&requesting, Style::default().fg(Color::Red)),
                ]),
                Line::from(Span::styled(
                    " [y]es accept  [n]o reject",
                    Style::default().fg(Color::White),
                )),
            ];
            let para = Paragraph::new(lines);
            f.render_widget(para, inner);
        }
    }
}

/// Draw the status bar.
fn draw_status_bar(f: &mut Frame, ps: &PlayingState, area: Rect) {
    let pause_indicator = if ps.paused { " PAUSED " } else { "" };
    let mode_indicator = match &ps.input_mode {
        InputMode::Spectating => "",
        InputMode::ActionBar { .. } => " YOUR TURN ",
        InputMode::BoardCursor { .. } => " PLACING ",
        InputMode::TradeBuilder { .. } => " TRADING ",
        InputMode::Discard { .. } => " DISCARD ",
        InputMode::ResourcePicker { .. } => " PICK RESOURCE ",
        InputMode::StealTarget { .. } => " STEAL ",
        InputMode::TradeResponse { .. } => " TRADE OFFER ",
    };
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
            mode_indicator,
            Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
        ),
        Span::styled(
            " | q:quit  Tab:AI panel  Space:pause  +/-:speed ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            if ps.game_over {
                " GAME OVER -- press Enter "
            } else {
                ""
            },
            Style::default().fg(Color::Black).bg(Color::Green).bold(),
        ),
    ]);
    let status_paragraph = Paragraph::new(status);
    f.render_widget(status_paragraph, area);
}

// ── Helpers ─────────────────────────────────────────────────────────

fn action_shortcut(choice: &str) -> Option<char> {
    if choice.contains("End Turn") {
        Some('e')
    } else if choice.contains("Build Settlement") {
        Some('s')
    } else if choice.contains("Build Road") {
        Some('r')
    } else if choice.contains("Build City") {
        Some('c')
    } else if choice.contains("Buy Development") {
        Some('d')
    } else if choice.contains("Propose Trade") {
        Some('t')
    } else if choice.starts_with("Play ") {
        Some('p')
    } else {
        None
    }
}

fn format_resource_counts(counts: &[u32; 5], _names: &[&str; 5]) -> String {
    let mut parts = Vec::new();
    let labels = ["W", "B", "S", "H", "O"];
    for (i, &c) in counts.iter().enumerate() {
        if c > 0 {
            parts.push(format!("{}x{}", c, labels[i]));
        }
    }
    if parts.is_empty() {
        "(none)".to_string()
    } else {
        parts.join(" ")
    }
}

fn format_resource_list(resources: &[Resource]) -> String {
    if resources.is_empty() {
        return "(none)".to_string();
    }
    let mut counts = [0u32; 5];
    for r in resources {
        let idx = match r {
            Resource::Wood => 0,
            Resource::Brick => 1,
            Resource::Sheep => 2,
            Resource::Wheat => 3,
            Resource::Ore => 4,
        };
        counts[idx] += 1;
    }
    format_resource_counts(&counts, &["Wood", "Brick", "Sheep", "Wheat", "Ore"])
}
