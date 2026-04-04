//! TUI layout -- splits the terminal into board, players, context bar, and status.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::game::board::Resource;

use super::board_view;
use super::chat_panel;
use super::game_log;
use super::resource_bar;
use super::{InputMode, PlayingState, TradeSide};

// ── Shared layout ────────────────────────────────────────────────────

/// Precomputed areas for the playing screen layout.
struct PlayingLayout {
    board: Rect,
    players: Rect,
    game_log: Rect,
    context: Rect,
    status: Rect,
    full: Rect,
}

/// Compute the playing screen layout areas.
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
fn compute_layout(size: Rect) -> PlayingLayout {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(15),   // Board + players
            Constraint::Length(5), // Context bar
            Constraint::Length(1), // Status bar
        ])
        .split(size);

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1), Constraint::Length(38)])
        .split(main_chunks[0]);

    let right_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(20), Constraint::Min(5)])
        .split(top_chunks[1]);

    PlayingLayout {
        board: top_chunks[0],
        players: right_split[0],
        game_log: right_split[1],
        context: main_chunks[1],
        status: main_chunks[2],
        full: size,
    }
}

/// Render the right panel, context bar, status bar, and help overlay.
///
/// Everything except the board area -- shared between text and pixel paths.
fn draw_panels(f: &mut Frame, ps: &PlayingState, layout: &PlayingLayout) {
    if let Some(state) = &ps.state {
        resource_bar::render_players(state, &ps.player_names, layout.players, f.buffer_mut());
    } else {
        let no_players = Paragraph::new("").block(
            Block::default()
                .title(" Players ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
        f.render_widget(no_players, layout.players);
    }
    game_log::render_log(&ps.messages, u16::MAX, layout.game_log, f.buffer_mut());

    draw_context_bar(f, ps, layout.context);
    draw_status_bar(f, ps, layout.status);

    if ps.show_help {
        draw_help_overlay(f, layout.full);
    }
}

fn draw_board_placeholder(f: &mut Frame, area: Rect, msg: &str) {
    let waiting = Paragraph::new(msg.to_string())
        .block(
            Block::default()
                .title(" Board ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .alignment(Alignment::Center);
    f.render_widget(waiting, area);
}

// ── Public draw function ─────────────────────────────────────────────

/// Draw the playing screen with text-rendered board.
pub fn draw_playing(f: &mut Frame, ps: &PlayingState) {
    // Fullscreen chat mode: chat takes everything except the status bar.
    if ps.show_ai_panel {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(f.area());

        chat_panel::render_chat(&ps.chat_messages, ps.chat_scroll, chunks[0], f.buffer_mut());
        draw_status_bar(f, ps, chunks[1]);

        if ps.show_help {
            draw_help_overlay(f, f.area());
        }
        return;
    }

    let layout = compute_layout(f.area());

    if let Some(state) = &ps.state {
        if let Some(ref grid) = ps.hex_grid {
            board_view::render_board(state, grid, layout.board, f.buffer_mut(), &ps.input_mode);
        } else {
            draw_board_placeholder(f, layout.board, "Computing board layout...");
        }
    } else {
        draw_board_placeholder(f, layout.board, "Waiting for game to start...");
    }

    draw_panels(f, ps, &layout);
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
            let lines: Vec<Line> = ps
                .messages
                .iter()
                .map(|m| {
                    let color = game_log::message_color(m);
                    Line::from(Span::styled(m.as_str(), Style::default().fg(color)))
                })
                .collect();
            let visible = inner.height as usize;
            let total = lines.len();
            let max_scroll = total.saturating_sub(visible) as u16;
            let effective_scroll = ps.log_scroll.min(max_scroll);
            let para = Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .scroll((effective_scroll, 0));
            f.render_widget(para, inner);
        }

        InputMode::ActionBar { choices, selected } => {
            // Horizontal action menu with shortcuts.
            let mut spans: Vec<Span> = Vec::new();
            for (i, choice) in choices.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw("  "));
                }
                let label = choice.label();
                if i == *selected {
                    spans.push(Span::styled(
                        format!("\u{25b8} {}", label),
                        Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
                    ));
                } else {
                    spans.push(Span::styled(label, Style::default().fg(Color::White)));
                }
                if let Some(key) = choice.shortcut_key() {
                    spans.push(Span::styled(
                        format!("[{}]", key),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            let line1 = Line::from(spans);
            let line2 = Line::from(Span::styled(
                " [Arrow/Enter] select  [s]ettlement  [r]oad  [c]ity  [d]ev card  [t]rade  [e]nd turn",
                Style::default().fg(Color::DarkGray),
            ));
            let para = Paragraph::new(vec![line1, Line::from(""), line2]);
            f.render_widget(para, inner);
        }

        InputMode::BoardCursor { legal, .. } => {
            let kind_name = legal.kind_name();
            let lines = vec![
                Line::from(Span::styled(
                    format!(" Place {} -- use arrow keys to navigate", kind_name),
                    Style::default().fg(Color::Yellow).bold(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " [Arrows] move  [n/p] next/prev  [Enter] confirm",
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
            let give_str = format_resource_counts(give);
            let get_str = format_resource_counts(get);
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
                        " Remaining: W:{} B:{} S:{} H:{} O:{}  (total: {})",
                        remaining[0],
                        remaining[1],
                        remaining[2],
                        remaining[3],
                        remaining[4],
                        remaining.iter().sum::<u32>(),
                    ),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    " [w/b/s/h/o] add  [Backspace] undo  [Esc] auto-fill  [Enter] confirm",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
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
                    format!(
                        " Trade offer from {}",
                        ps.player_names
                            .get(offer.from)
                            .map(|s| s.as_str())
                            .unwrap_or("???")
                    ),
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
    let roll_span = if let Some((d1, d2, total)) = ps.last_roll {
        let is_seven = total == 7;
        let style = if is_seven {
            Style::default().fg(Color::Black).bg(Color::Red).bold()
        } else {
            Style::default().fg(Color::White).bold()
        };
        Span::styled(format!(" Rolled: {} ({}+{}) ", total, d1, d2), style)
    } else {
        Span::raw("")
    };
    let status = Line::from(vec![
        roll_span,
        Span::styled(
            pause_indicator,
            Style::default().fg(Color::Black).bg(Color::Yellow).bold(),
        ),
        Span::styled(
            mode_indicator,
            Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
        ),
        Span::styled(
            " | q:quit  ?:help  Tab:AI  Space:pause ",
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

// ── Help Overlay ──────────────────────────────────────────────────

/// Draw a centered help overlay with keyboard shortcuts and game info.
fn draw_help_overlay(f: &mut Frame, area: Rect) {
    let width = 62u16.min(area.width.saturating_sub(4));
    let height = 32u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);

    f.render_widget(Clear, overlay);

    let block = Block::default()
        .title(" Help ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    let help_text = vec![
        Line::from(Span::styled(
            "Global",
            Style::default().fg(Color::Yellow).bold(),
        )),
        Line::from("  q        Quit / back to menu"),
        Line::from("  ?        Toggle this help"),
        Line::from("  Tab      Toggle AI reasoning panel"),
        Line::from("  Space    Pause / unpause AI turns"),
        Line::from("  j / k    Scroll game log"),
        Line::from(""),
        Line::from(Span::styled(
            "Your Turn (action bar)",
            Style::default().fg(Color::Yellow).bold(),
        )),
        Line::from("  e        End Turn"),
        Line::from("  s        Build Settlement"),
        Line::from("  r        Build Road"),
        Line::from("  c        Upgrade to City"),
        Line::from("  d        Buy Development Card"),
        Line::from("  p        Play Development Card"),
        Line::from("  t        Open Trade Builder"),
        Line::from(""),
        Line::from(Span::styled(
            "Placement (board cursor)",
            Style::default().fg(Color::Yellow).bold(),
        )),
        Line::from("  Arrows   Move between legal positions"),
        Line::from("  n / p    Next / previous position"),
        Line::from("  Enter    Confirm placement"),
        Line::from(""),
        Line::from(Span::styled(
            "Resources (trade, discard, dev cards)",
            Style::default().fg(Color::Yellow).bold(),
        )),
        Line::from("  w b s h o   Wood Brick Sheep Harvest Ore"),
        Line::from("  Tab         Switch GIVE / GET (trade)"),
        Line::from("  Backspace   Undo last resource"),
        Line::from("  Enter       Confirm"),
        Line::from(""),
        Line::from(Span::styled(
            "Trade Response",
            Style::default().fg(Color::Yellow).bold(),
        )),
        Line::from("  y / Enter   Accept"),
        Line::from("  n / Esc     Reject"),
    ];

    let para = Paragraph::new(help_text).wrap(Wrap { trim: false });
    f.render_widget(para, inner);
}

// ── Helpers ─────────────────────────────────────────────────────────

fn format_resource_counts(counts: &[u32; 5]) -> String {
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
    let all = Resource::all();
    let mut counts = [0u32; 5];
    for r in resources {
        if let Some(idx) = all.iter().position(|a| a == r) {
            counts[idx] += 1;
        }
    }
    format_resource_counts(&counts)
}
