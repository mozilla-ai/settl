//! Non-game screens: Title, MainMenu, NewGame setup, PostGame.
//!
//! Each screen has a state struct and a `draw_*` rendering function.
//! Input handling lives in `mod.rs` dispatch.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::player::personality::Personality;

use super::menu::render_menu;

// ── ASCII Art ──────────────────────────────────────────────────────────

const TITLE_ART: &str = r#"
 ███████╗███████╗████████╗████████╗██╗
 ██╔════╝██╔════╝╚══██╔══╝╚══██╔══╝██║
 ███████╗█████╗     ██║      ██║   ██║
 ╚════██║██╔══╝     ██║      ██║   ██║
 ███████║███████╗   ██║      ██║   ███████╗
 ╚══════╝╚══════╝   ╚═╝      ╚═╝   ╚══════╝"#;

// ── Main Menu ──────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct MainMenuState {
    pub selected: usize,
}

impl MainMenuState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn menu_items(&self) -> Vec<&'static str> {
        vec!["New Game", "About", "Quit"]
    }
}

// ── New Game Setup ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerKind {
    Llamafile,
    Human,
}

impl PlayerKind {
    pub fn label(&self) -> &'static str {
        match self {
            PlayerKind::Llamafile => "Llamafile",
            PlayerKind::Human => "Human",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerConfig {
    pub name: String,
    pub kind: PlayerKind,
    pub personality_index: usize,
}

const DEFAULT_NAMES: &[&str] = &["Marco", "Leif", "Vasco"];

/// Which row has focus on the new game screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NewGameFocus {
    /// Player count radio toggle (3 or 4).
    PlayerCount,
    /// An AI player row (indices 1-3 in the players vec).
    Player { row: usize },
    /// Friendly Robber toggle.
    FriendlyRobber,
    /// Board Layout toggle.
    BoardLayout,
    /// AI Model Size toggle.
    ModelSize,
    /// The "Start Game" button.
    StartButton,
}

#[derive(Debug)]
pub struct NewGameState {
    pub players: Vec<PlayerConfig>,
    pub focus: NewGameFocus,
    /// Personality names: built-ins + discovered from TOML.
    pub personality_names: Vec<String>,
    /// Whether the game uses 3 or 4 players.
    pub four_players: bool,
    /// Friendly robber rule: robber cannot target players with 2 or fewer VP.
    pub friendly_robber: bool,
    /// Whether to randomize the board layout.
    pub random_board: bool,
    /// Which llamafile model to use for AI players.
    pub llamafile_model: crate::llamafile::LlamafileModel,
}

impl NewGameState {
    pub fn new(personalities: &[Personality]) -> Self {
        let personality_names: Vec<String> = vec![
            "Balanced".into(),
            "Aggressive".into(),
            "Grudge Holder".into(),
            "Builder".into(),
            "Chaos Agent".into(),
        ]
        .into_iter()
        .chain(personalities.iter().map(|p| p.name.clone()))
        .collect();

        let username = std::env::var("USER")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Player".into());

        let players = (0..4)
            .map(|i| {
                if i == 0 {
                    PlayerConfig {
                        name: username.clone(),
                        kind: PlayerKind::Human,
                        personality_index: 0,
                    }
                } else {
                    PlayerConfig {
                        name: DEFAULT_NAMES[i - 1].into(),
                        kind: PlayerKind::Llamafile,
                        personality_index: i.min(personality_names.len().saturating_sub(1)),
                    }
                }
            })
            .collect();

        Self {
            players,
            focus: NewGameFocus::PlayerCount,
            personality_names,
            four_players: true,
            friendly_robber: false,
            random_board: false,
            llamafile_model: crate::llamafile::LlamafileModel::default(),
        }
    }

    pub fn num_players(&self) -> usize {
        if self.four_players {
            4
        } else {
            3
        }
    }
}

// ── Post-Game ──────────────────────────────────────────────────────────

pub const POST_GAME_ITEMS: &[&str] = &["Play Again", "Main Menu", "Quit"];

#[derive(Debug)]
pub struct PostGameState {
    pub winner_name: String,
    pub winner_index: usize,
    pub scores: Vec<(String, u8)>, // (name, VP)
    pub selected: usize,
}

// ── About ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct AboutState;

// ── Llamafile Setup ───────────────────────────────────────────────────

/// Status for the llamafile download/start screen.
pub struct LlamafileSetupState {
    pub status: crate::llamafile::LlamafileStatus,
    pub status_rx: tokio::sync::mpsc::UnboundedReceiver<crate::llamafile::LlamafileStatus>,
    /// Saved NewGame config so we can launch the game when ready.
    pub saved_config: NewGameState,
    /// Handle to the background setup task so we can abort it on cancel.
    pub task_handle: Option<tokio::task::JoinHandle<()>>,
    /// Oneshot receiver for the llamafile process once it's ready.
    pub process_rx: Option<tokio::sync::oneshot::Receiver<crate::llamafile::LlamafileProcess>>,
}

impl std::fmt::Debug for LlamafileSetupState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamafileSetupState")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

// ── Drawing Functions ──────────────────────────────────────────────────

/// Draw the main menu.
pub fn draw_main_menu(f: &mut Frame, state: &MainMenuState) {
    let area = f.area();
    f.render_widget(Clear, area);

    // Title art + subtitle at top.
    let art_lines = TITLE_ART.lines().count() as u16;
    let items = state.menu_items();
    let menu_height = items.len() as u16;
    let total_height = art_lines + 4 + menu_height + 2; // art + subtitle + gaps + menu + hint
    let y_start = area.y + area.height.saturating_sub(total_height) / 2;

    render_title_art(f, area, y_start, art_lines);

    // Subtitle.
    let sub_y = y_start + art_lines + 1;
    if sub_y < area.y + area.height {
        let sub_area = Rect::new(area.x, sub_y, area.width, 1);
        let sub = Paragraph::new("terminal settlers")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(sub, sub_area);
    }

    // Menu.
    let menu_y = sub_y + 2;
    let menu_area = Rect::new(area.x, menu_y, area.width, menu_height);
    render_menu(
        &items,
        state.selected,
        menu_area,
        f.buffer_mut(),
        Color::Yellow,
    );

    // Hint bar.
    let hint_y = menu_y + menu_height + 1;
    if hint_y < area.y + area.height {
        let hint_area = Rect::new(area.x, hint_y, area.width, 1);
        let hint = Paragraph::new("[↑↓] navigate  [Enter] select  [q] quit")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hint, hint_area);
    }
}

/// Draw the about screen.
pub fn draw_about(f: &mut Frame) {
    let area = f.area();
    f.render_widget(Clear, area);

    let content_width = 60u16.min(area.width.saturating_sub(4));
    let x_start = area.x + (area.width.saturating_sub(content_width)) / 2;

    // Title.
    let title_y = area.y + 2;
    let title_area = Rect::new(x_start, title_y, content_width, 1);
    let title = Paragraph::new("About settl")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Yellow).bold());
    f.render_widget(title, title_area);

    // Content lines.
    let lines = vec![
        Line::from(""),
        Line::styled(
            "A terminal hex settlement game powered by local AI.",
            Style::default().fg(Color::White),
        ),
        Line::from(""),
        Line::styled("Built by", Style::default().fg(Color::DarkGray)),
        Line::styled("  mozilla.ai", Style::default().fg(Color::Cyan).bold()),
        Line::styled("  https://mozilla.ai", Style::default().fg(Color::DarkGray)),
        Line::from(""),
        Line::styled("AI backend", Style::default().fg(Color::DarkGray)),
        Line::styled(
            "  llamafile by Mozilla",
            Style::default().fg(Color::Cyan).bold(),
        ),
        Line::styled(
            "  Run LLMs locally with a single file.",
            Style::default().fg(Color::White),
        ),
        Line::styled(
            "  https://github.com/mozilla-ai/llamafile",
            Style::default().fg(Color::DarkGray),
        ),
        Line::from(""),
        Line::styled("Game", Style::default().fg(Color::DarkGray)),
        Line::styled(
            "  A hex-based resource trading and building game.",
            Style::default().fg(Color::White),
        ),
    ];

    let content_y = title_y + 2;
    let content_height = lines.len() as u16;
    let content_area = Rect::new(x_start, content_y, content_width, content_height);
    let content = Paragraph::new(lines);
    f.render_widget(content, content_area);

    // Hint bar.
    let hint_y = area.y + area.height - 1;
    let hint_area = Rect::new(area.x, hint_y, area.width, 1);
    let hint = Paragraph::new("[Esc] back")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, hint_area);
}

/// Draw the new game setup screen.
pub fn draw_new_game(f: &mut Frame, state: &NewGameState) {
    let area = f.area();
    f.render_widget(Clear, area);

    let content_width = 64u16.min(area.width.saturating_sub(4));
    let x_start = area.x + (area.width.saturating_sub(content_width)) / 2;

    // Title.
    let title_area = Rect::new(x_start, area.y + 1, content_width, 1);
    let title = Paragraph::new("New Game")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Yellow).bold());
    f.render_widget(title, title_area);

    // -- PLAYERS section --
    let section_y = area.y + 3;
    let section_area = Rect::new(x_start, section_y, content_width, 1);
    let section = Paragraph::new("PLAYERS").style(Style::default().fg(Color::DarkGray).bold());
    f.render_widget(section, section_area);

    // Player count radio toggle.
    let count_y = section_y + 2;
    let count_focused = matches!(state.focus, NewGameFocus::PlayerCount);
    let (three_marker, four_marker) = if state.four_players {
        ("\u{25cb}", "\u{25cf}")
    } else {
        ("\u{25cf}", "\u{25cb}")
    };
    let count_line = Line::from(vec![
        Span::styled("    Players:  ", Style::default().fg(Color::White)),
        Span::styled(
            format!("{} 3", three_marker),
            toggle_style(count_focused && !state.four_players),
        ),
        Span::styled("   ", Style::default()),
        Span::styled(
            format!("{} 4", four_marker),
            toggle_style(count_focused && state.four_players),
        ),
    ]);
    let count_area = Rect::new(x_start, count_y, content_width, 1);
    let count_style = if count_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let count_widget = Paragraph::new(count_line).style(count_style);
    f.render_widget(count_widget, count_area);

    // Player rows.
    let first_row_y = count_y + 2;
    for (i, player) in state.players.iter().enumerate() {
        let row_y = first_row_y + i as u16;
        if row_y >= area.y + area.height - 6 {
            break;
        }
        let row_area = Rect::new(x_start, row_y, content_width, 1);

        let is_human = player.kind == PlayerKind::Human;
        let is_dimmed = !state.four_players && i == 3;
        let is_focused = matches!(state.focus, NewGameFocus::Player { row } if row == i);

        let role_str = player.kind.label();
        let personality_str = if is_human {
            ""
        } else {
            state
                .personality_names
                .get(player.personality_index)
                .map(|s| s.as_str())
                .unwrap_or("?")
        };

        let marker = if is_focused { ">" } else { " " };

        let base_fg = if is_dimmed {
            Color::DarkGray
        } else {
            Color::White
        };

        let pers_style = if is_focused && !is_human {
            Style::default().fg(Color::Black).bg(Color::Cyan).bold()
        } else {
            Style::default().fg(base_fg)
        };

        let line = Line::from(vec![
            Span::styled(
                format!("  {} {}  ", marker, i + 1),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(format!("{:<14}", player.name), Style::default().fg(base_fg)),
            Span::styled(
                format!("{:<12}", role_str),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(truncate_str(personality_str, 16).to_string(), pers_style),
        ]);
        let row_widget = Paragraph::new(line);
        row_widget.render(row_area, f.buffer_mut());
    }

    // -- RULES section --
    let rules_y = first_row_y + 4 + 1;
    let rules_area = Rect::new(x_start, rules_y, content_width, 1);
    let rules = Paragraph::new("RULES").style(Style::default().fg(Color::DarkGray).bold());
    f.render_widget(rules, rules_area);

    // Friendly Robber.
    let fr_y = rules_y + 2;
    let fr_focused = matches!(state.focus, NewGameFocus::FriendlyRobber);
    let fr_value = if state.friendly_robber { "On" } else { "Off" };
    draw_toggle_row(
        f,
        x_start,
        fr_y,
        content_width,
        "Friendly Robber",
        fr_value,
        fr_focused,
    );

    // Board Layout.
    let bl_y = fr_y + 1;
    let bl_focused = matches!(state.focus, NewGameFocus::BoardLayout);
    let bl_value = if state.random_board {
        "Random"
    } else {
        "Beginner"
    };
    draw_toggle_row(
        f,
        x_start,
        bl_y,
        content_width,
        "Board Layout",
        bl_value,
        bl_focused,
    );

    // AI Model.
    let ms_y = bl_y + 1;
    let ms_focused = matches!(state.focus, NewGameFocus::ModelSize);
    draw_toggle_row(
        f,
        x_start,
        ms_y,
        content_width,
        "AI Model",
        state.llamafile_model.display_name(),
        ms_focused,
    );

    // Start button.
    let button_y = ms_y + 2;
    let button_focused = matches!(state.focus, NewGameFocus::StartButton);
    let button_style = if button_focused {
        Style::default().fg(Color::Black).bg(Color::Green).bold()
    } else {
        Style::default().fg(Color::Green).bold()
    };
    let button_area = Rect::new(x_start, button_y, content_width, 1);
    let button = Paragraph::new("  [ Start Game ]").style(button_style);
    f.render_widget(button, button_area);

    // Hint bar at bottom.
    let hint_y = area.y + area.height - 1;
    let hint_area = Rect::new(area.x, hint_y, area.width, 1);
    let hint = Paragraph::new(
        "\u{2191}\u{2193}: move  |  \u{2190}\u{2192}: change  |  Enter: select  |  Esc: back",
    )
    .alignment(Alignment::Center)
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, hint_area);
}

/// Draw the post-game screen.
pub fn draw_post_game(f: &mut Frame, state: &PostGameState) {
    let area = f.area();
    f.render_widget(Clear, area);

    let content_width = 50u16.min(area.width.saturating_sub(4));
    let x_start = area.x + (area.width.saturating_sub(content_width)) / 2;

    // Winner announcement.
    let winner_y = area.y + 2;
    let winner_block = Block::default()
        .title(" Game Over ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let winner_area = Rect::new(
        x_start,
        winner_y,
        content_width,
        3 + state.scores.len() as u16 + 1,
    );
    f.render_widget(winner_block, winner_area);

    // Winner line.
    let winner_text = format!("{} wins!", state.winner_name);
    let winner_line_area = Rect::new(x_start + 2, winner_y + 1, content_width - 4, 1);
    let winner_line = Paragraph::new(winner_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Green).bold());
    f.render_widget(winner_line, winner_line_area);

    // Scores.
    for (i, (name, vp)) in state.scores.iter().enumerate() {
        let score_y = winner_y + 3 + i as u16;
        if score_y >= winner_area.y + winner_area.height - 1 {
            break;
        }
        let score_area = Rect::new(x_start + 2, score_y, content_width - 4, 1);
        let marker = if i == state.winner_index { "★" } else { " " };
        let score_text = format!("{} {:<16} {:>2} VP", marker, name, vp);
        let style = if i == state.winner_index {
            Style::default().fg(Color::Yellow).bold()
        } else {
            Style::default().fg(Color::White)
        };
        let score_line = Paragraph::new(score_text).style(style);
        f.render_widget(score_line, score_area);
    }

    // Menu below scores.
    let menu_y = winner_area.y + winner_area.height + 1;
    let menu_height = POST_GAME_ITEMS.len() as u16;
    if menu_y + menu_height < area.y + area.height {
        let menu_area = Rect::new(area.x, menu_y, area.width, menu_height);
        render_menu(
            POST_GAME_ITEMS,
            state.selected,
            menu_area,
            f.buffer_mut(),
            Color::Yellow,
        );
    }

    // Hint.
    let hint_y = area.y + area.height - 1;
    let hint_area = Rect::new(area.x, hint_y, area.width, 1);
    let hint = Paragraph::new("[↑↓] navigate  [Enter] select")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, hint_area);
}

/// Draw the llamafile download/setup screen.
pub fn draw_llamafile_setup(f: &mut Frame, state: &LlamafileSetupState) {
    use crate::llamafile::{format_bytes, LlamafileStatus};

    let area = f.area();
    f.render_widget(Clear, area);

    let content_width = 60u16.min(area.width.saturating_sub(4));
    let x_start = area.x + (area.width.saturating_sub(content_width)) / 2;
    let y_center = area.y + area.height / 2;

    // Title.
    let title_y = y_center.saturating_sub(4);
    let title_area = Rect::new(x_start, title_y, content_width, 1);
    let title = Paragraph::new("Setting up local AI")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Yellow).bold());
    f.render_widget(title, title_area);

    // Status text.
    let status_text = match &state.status {
        LlamafileStatus::Checking => "Checking for Bonsai-8B...".to_string(),
        LlamafileStatus::Downloading { bytes, total } => {
            if *total > 0 {
                let pct = (*bytes as f64 / *total as f64 * 100.0) as u16;
                format!(
                    "Downloading Bonsai-8B... {} / {} ({}%)",
                    format_bytes(*bytes),
                    format_bytes(*total),
                    pct
                )
            } else {
                format!("Downloading Bonsai-8B... {}", format_bytes(*bytes))
            }
        }
        LlamafileStatus::Preparing => "Making executable...".to_string(),
        LlamafileStatus::Starting => "Starting local AI server...".to_string(),
        LlamafileStatus::WaitingForReady => "Waiting for server to be ready...".to_string(),
        LlamafileStatus::Ready(port) => format!("Ready on port {}!", port),
        LlamafileStatus::Error(msg) => format!("Error: {}", msg),
    };

    let status_y = title_y + 2;
    let status_height = if matches!(&state.status, LlamafileStatus::Error(_)) {
        5
    } else {
        1
    };
    let status_area = Rect::new(x_start, status_y, content_width, status_height);
    let status_style = match &state.status {
        LlamafileStatus::Error(_) => Style::default().fg(Color::Red),
        LlamafileStatus::Ready(_) => Style::default().fg(Color::Green),
        _ => Style::default().fg(Color::Cyan),
    };
    let status = Paragraph::new(status_text)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .style(status_style);
    f.render_widget(status, status_area);

    // Progress bar for download.
    if let LlamafileStatus::Downloading { bytes, total } = &state.status {
        if *total > 0 {
            let bar_y = status_y + 2;
            let bar_width = 40u16.min(content_width);
            let bar_x = x_start + (content_width.saturating_sub(bar_width)) / 2;
            let bar_area = Rect::new(bar_x, bar_y, bar_width, 1);
            let filled = (*bytes as f64 / *total as f64 * bar_width as f64) as u16;
            let bar_str: String = (0..bar_width)
                .map(|i| if i < filled { '\u{2588}' } else { '\u{2591}' })
                .collect();
            let bar = Paragraph::new(bar_str).style(Style::default().fg(Color::Cyan));
            f.render_widget(bar, bar_area);
        }
    }

    // Hint bar.
    let hint_y = area.y + area.height - 1;
    let hint_area = Rect::new(area.x, hint_y, area.width, 1);
    let hint_text = match &state.status {
        LlamafileStatus::Error(_) => "Esc: go back",
        _ => "Esc: cancel",
    };
    let hint = Paragraph::new(hint_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, hint_area);
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Render the title ASCII art centered as a block (not per-line).
///
/// `Paragraph` with `Alignment::Center` centers each line independently,
/// which misaligns lines of different widths. This helper computes the
/// centering offset from the widest line and left-aligns within that rect.
fn render_title_art(f: &mut Frame, area: Rect, y_start: u16, art_lines: u16) {
    let art_text = TITLE_ART.trim_start_matches('\n');
    let max_width = art_text
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let x_offset = area.width.saturating_sub(max_width) / 2;
    let art_area = Rect::new(
        area.x + x_offset,
        y_start,
        max_width.min(area.width.saturating_sub(x_offset)),
        art_lines,
    );
    let art = Paragraph::new(art_text)
        .alignment(Alignment::Left)
        .style(Style::default().fg(Color::Yellow).bold());
    f.render_widget(art, art_area);
}

fn toggle_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Black).bg(Color::Cyan).bold()
    } else {
        Style::default().fg(Color::White)
    }
}

fn draw_toggle_row(
    f: &mut Frame,
    x: u16,
    y: u16,
    width: u16,
    label: &str,
    value: &str,
    focused: bool,
) {
    let marker = if focused { ">" } else { " " };
    let value_style = if focused {
        Style::default().fg(Color::Black).bg(Color::Cyan).bold()
    } else {
        Style::default().fg(Color::Cyan)
    };
    let line = Line::from(vec![
        Span::styled(
            format!("  {}  {:<20}", marker, label),
            Style::default().fg(Color::White),
        ),
        Span::styled(value.to_string(), value_style),
    ]);
    let area = Rect::new(x, y, width, 1);
    Paragraph::new(line).render(area, f.buffer_mut());
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    // Find the last char boundary at or before `max` to avoid panicking on multi-byte strings.
    let end = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max)
        .last()
        .unwrap_or(0);
    &s[..end]
}
