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

const SUBTITLE: &str = "terminal catan";

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
        vec!["New Game", "Quit"]
    }
}

// ── New Game Setup ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerKind {
    Random,
    Llamafile,
    Llm,
    Human,
}

impl PlayerKind {
    pub fn label(&self) -> &'static str {
        match self {
            PlayerKind::Random => "Random",
            PlayerKind::Llamafile => "Llamafile",
            PlayerKind::Llm => "LLM",
            PlayerKind::Human => "Human",
        }
    }

    /// Cycle to the next AI player kind (Human is excluded for AI slots).
    pub fn next_ai(&self) -> Self {
        match self {
            PlayerKind::Random => PlayerKind::Llamafile,
            PlayerKind::Llamafile => PlayerKind::Llm,
            PlayerKind::Llm => PlayerKind::Random,
            PlayerKind::Human => PlayerKind::Random,
        }
    }

    /// Cycle to the previous AI player kind (Human is excluded for AI slots).
    pub fn prev_ai(&self) -> Self {
        match self {
            PlayerKind::Random => PlayerKind::Llm,
            PlayerKind::Llamafile => PlayerKind::Random,
            PlayerKind::Llm => PlayerKind::Llamafile,
            PlayerKind::Human => PlayerKind::Llm,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerConfig {
    pub name: String,
    pub kind: PlayerKind,
    pub model_index: usize,
    pub personality_index: usize,
}

const DEFAULT_NAMES: &[&str] = &["Alice", "Bob", "Charlie", "Diana"];

pub const AVAILABLE_MODELS: &[&str] = &[
    "claude-sonnet-4-6",
    "claude-haiku-4-5-20251001",
    "gpt-4o-mini",
    "gpt-4o",
    "gemini-2.0-flash",
];

/// Which column is focused in the player table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NewGameCol {
    Name,
    Kind,
    Model,
    Personality,
}

impl NewGameCol {
    pub fn next(self) -> Self {
        match self {
            NewGameCol::Name => NewGameCol::Kind,
            NewGameCol::Kind => NewGameCol::Model,
            NewGameCol::Model => NewGameCol::Personality,
            NewGameCol::Personality => NewGameCol::Name,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            NewGameCol::Name => NewGameCol::Personality,
            NewGameCol::Kind => NewGameCol::Name,
            NewGameCol::Model => NewGameCol::Kind,
            NewGameCol::Personality => NewGameCol::Model,
        }
    }
}

/// Which section of the new game screen has focus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NewGameFocus {
    /// Player table row + column.
    Player { row: usize, col: NewGameCol },
    /// The "Start Game" button.
    StartButton,
}

#[derive(Debug)]
pub struct NewGameState {
    pub players: Vec<PlayerConfig>,
    pub focus: NewGameFocus,
    /// Personality names: built-ins + discovered from TOML.
    pub personality_names: Vec<String>,
    /// Whether we are currently editing a text field.
    pub editing: bool,
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

        // Default AI players to Llamafile when no provider API keys are set.
        let has_api_key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some()
            || std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
                .is_some()
            || std::env::var("GOOGLE_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
                .is_some();
        let default_ai_kind = if has_api_key {
            PlayerKind::Llm
        } else {
            PlayerKind::Llamafile
        };

        let players = (0..4)
            .map(|i| {
                if i == 0 {
                    PlayerConfig {
                        name: username.clone(),
                        kind: PlayerKind::Human,
                        model_index: 0,
                        personality_index: 0,
                    }
                } else {
                    PlayerConfig {
                        name: DEFAULT_NAMES[i].into(),
                        kind: default_ai_kind.clone(),
                        model_index: 0,
                        personality_index: i.min(personality_names.len().saturating_sub(1)),
                    }
                }
            })
            .collect();

        Self {
            players,
            focus: NewGameFocus::Player {
                row: 0,
                col: NewGameCol::Kind,
            },
            personality_names,
            editing: false,
        }
    }

    pub fn num_players(&self) -> usize {
        self.players.len()
    }

    pub fn add_player(&mut self) {
        if self.players.len() < 4 {
            let i = self.players.len();
            self.players.push(PlayerConfig {
                name: DEFAULT_NAMES[i].into(),
                kind: PlayerKind::Random,
                model_index: 0,
                personality_index: 0,
            });
        }
    }

    pub fn remove_player(&mut self) {
        if self.players.len() > 2 {
            self.players.pop();
            // Adjust focus if it was on a removed row.
            if let NewGameFocus::Player { row, .. } = &mut self.focus {
                if *row >= self.players.len() {
                    *row = self.players.len() - 1;
                }
            }
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

// ── Llamafile Setup ───────────────────────────────────────────────────

/// Status for the llamafile download/start screen.
#[derive(Debug)]
pub struct LlamafileSetupState {
    pub status: crate::llamafile::LlamafileStatus,
    pub status_rx: tokio::sync::mpsc::UnboundedReceiver<crate::llamafile::LlamafileStatus>,
    /// Saved NewGame config so we can launch the game when ready.
    pub saved_config: NewGameState,
    /// Handle to the background setup task so we can abort it on cancel.
    pub task_handle: Option<tokio::task::JoinHandle<()>>,
}

// ── Drawing Functions ──────────────────────────────────────────────────

/// Draw the title screen.
pub fn draw_title(f: &mut Frame, frame_count: u64) {
    let area = f.area();
    f.render_widget(Clear, area);

    // Calculate vertical centering.
    let art_lines = TITLE_ART.lines().count() as u16;
    let total_height = art_lines + 4; // art + gap + subtitle + gap + prompt
    let y_start = area.y + area.height.saturating_sub(total_height) / 2;

    // Title art -- center the block as a whole (not per-line) so that
    // lines of different widths stay aligned with each other.
    render_title_art(f, area, y_start, art_lines);

    // Subtitle.
    let sub_y = y_start + art_lines + 1;
    if sub_y < area.y + area.height {
        let sub_area = Rect::new(area.x, sub_y, area.width, 1);
        let sub = Paragraph::new(SUBTITLE)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(sub, sub_area);
    }

    // Blinking "Press any key" prompt.
    let prompt_y = sub_y + 2;
    if prompt_y < area.y + area.height {
        let show = (frame_count / 15) % 2 == 0; // blink every ~15 frames
        if show {
            let prompt_area = Rect::new(area.x, prompt_y, area.width, 1);
            let prompt = Paragraph::new("Press any key to start")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Cyan));
            f.render_widget(prompt, prompt_area);
        }
    }
}

/// Draw the main menu.
pub fn draw_main_menu(f: &mut Frame, state: &MainMenuState) {
    let area = f.area();
    f.render_widget(Clear, area);

    // Compact title at top.
    let art_lines = TITLE_ART.lines().count() as u16;
    let items = state.menu_items();
    let menu_height = items.len() as u16;
    let total_height = art_lines + 3 + menu_height + 2; // art + gaps + menu + hint
    let y_start = area.y + area.height.saturating_sub(total_height) / 2;

    render_title_art(f, area, y_start, art_lines);

    // Menu.
    let menu_y = y_start + art_lines + 2;
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

    // Player count.
    let count_area = Rect::new(x_start, area.y + 3, content_width, 1);
    let count_text = format!("Players: {}  (press +/- to change)", state.num_players());
    let count = Paragraph::new(count_text).style(Style::default().fg(Color::Cyan));
    f.render_widget(count, count_area);

    // Player table header.
    let header_y = area.y + 5;
    let header_area = Rect::new(x_start, header_y, content_width, 1);
    let header = Paragraph::new(" #  Name         Type     Model              Personality")
        .style(Style::default().fg(Color::DarkGray).bold());
    f.render_widget(header, header_area);

    // Player rows.
    for (i, player) in state.players.iter().enumerate() {
        let row_y = header_y + 1 + i as u16;
        if row_y >= area.y + area.height - 6 {
            break;
        }
        let row_area = Rect::new(x_start, row_y, content_width, 1);

        let model_str = match player.kind {
            PlayerKind::Llm => AVAILABLE_MODELS
                .get(player.model_index)
                .copied()
                .unwrap_or("?"),
            PlayerKind::Llamafile => "Bonsai-1.7B",
            _ => "\u{2014}",
        };
        let personality_str = match player.kind {
            PlayerKind::Llm | PlayerKind::Llamafile => state
                .personality_names
                .get(player.personality_index)
                .map(|s| s.as_str())
                .unwrap_or("?"),
            _ => "\u{2014}",
        };

        // Build columns with highlights.
        let is_focused_row = matches!(state.focus, NewGameFocus::Player { row, .. } if row == i);

        let name_style = cell_style(
            is_focused_row
                && matches!(
                    state.focus,
                    NewGameFocus::Player {
                        col: NewGameCol::Name,
                        ..
                    }
                ),
        );
        let kind_style = cell_style(
            is_focused_row
                && matches!(
                    state.focus,
                    NewGameFocus::Player {
                        col: NewGameCol::Kind,
                        ..
                    }
                ),
        );
        let model_style = cell_style(
            is_focused_row
                && matches!(
                    state.focus,
                    NewGameFocus::Player {
                        col: NewGameCol::Model,
                        ..
                    }
                ),
        );
        let pers_style = cell_style(
            is_focused_row
                && matches!(
                    state.focus,
                    NewGameFocus::Player {
                        col: NewGameCol::Personality,
                        ..
                    }
                ),
        );

        let marker = if is_focused_row { ">" } else { " " };

        let line = Line::from(vec![
            Span::styled(
                format!("{} {}  ", marker, i + 1),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(format!("{:<12} ", player.name), name_style),
            Span::styled(format!("{:<8} ", player.kind.label()), kind_style),
            Span::styled(format!("{:<18} ", truncate_str(model_str, 18)), model_style),
            Span::styled(truncate_str(personality_str, 14).to_string(), pers_style),
        ]);
        let row_widget = Paragraph::new(line);
        row_widget.render(row_area, f.buffer_mut());
    }

    // Start button.
    let button_y = header_y + 2 + state.num_players() as u16 + 1;
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
    let hint_text = if state.editing {
        "Type to edit  |  Enter: confirm  |  Esc: cancel"
    } else {
        "↑↓: move  |  ←→/Tab: change  |  Enter: start  |  +/-: players  |  Esc: back"
    };
    let hint = Paragraph::new(hint_text)
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
        LlamafileStatus::Checking => "Checking for Bonsai-1.7B...".to_string(),
        LlamafileStatus::Downloading { bytes, total } => {
            if *total > 0 {
                let pct = (*bytes as f64 / *total as f64 * 100.0) as u16;
                format!(
                    "Downloading Bonsai-1.7B... {} / {} ({}%)",
                    format_bytes(*bytes),
                    format_bytes(*total),
                    pct
                )
            } else {
                format!("Downloading Bonsai-1.7B... {}", format_bytes(*bytes))
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

fn cell_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Black).bg(Color::Cyan).bold()
    } else {
        Style::default().fg(Color::White)
    }
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
