//! Non-game screens: Title, MainMenu, NewGame setup, FilePicker, PostGame.
//!
//! Each screen has a state struct and a `draw_*` rendering function.
//! Input handling lives in `mod.rs` dispatch.

use std::path::PathBuf;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

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

#[derive(Debug)]
pub struct MainMenuState {
    pub selected: usize,
    pub has_save_files: bool,
    pub has_replay_files: bool,
}

impl Default for MainMenuState {
    fn default() -> Self {
        let has_save_files = find_files_with_extension("game_save", "json").is_some();
        let has_replay_files = find_files_with_extension("game_replay", "json").is_some()
            || find_files_with_extension("game_log", "jsonl").is_some();
        Self {
            selected: 0,
            has_save_files,
            has_replay_files,
        }
    }
}

impl MainMenuState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn menu_items(&self) -> Vec<&'static str> {
        let mut items = vec!["New Game"];
        if self.has_save_files {
            items.push("Continue Game");
        }
        if self.has_replay_files {
            items.push("Replay Game");
        }
        items.push("Quit");
        items
    }
}

// ── New Game Setup ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerKind {
    Random,
    Llm,
    Human,
}

impl PlayerKind {
    pub fn label(&self) -> &'static str {
        match self {
            PlayerKind::Random => "Random",
            PlayerKind::Llm => "LLM",
            PlayerKind::Human => "Human",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            PlayerKind::Random => PlayerKind::Llm,
            PlayerKind::Llm => PlayerKind::Human,
            PlayerKind::Human => PlayerKind::Random,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            PlayerKind::Random => PlayerKind::Human,
            PlayerKind::Llm => PlayerKind::Random,
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
    /// Settings: 0 = seed, 1 = max_turns.
    Setting(usize),
    /// The "Start Game" button.
    StartButton,
}

#[derive(Debug)]
pub struct NewGameState {
    pub players: Vec<PlayerConfig>,
    pub seed_input: String,
    pub max_turns_input: String,
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
                        kind: PlayerKind::Random,
                        model_index: 0,
                        personality_index: 0,
                    }
                }
            })
            .collect();

        Self {
            players,
            seed_input: String::new(),
            max_turns_input: "500".into(),
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

    pub fn seed(&self) -> Option<u64> {
        self.seed_input.parse().ok()
    }

    pub fn max_turns(&self) -> u32 {
        self.max_turns_input.parse().unwrap_or(500)
    }
}

// ── File Picker ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilePickerPurpose {
    Resume,
    Replay,
}

#[derive(Debug)]
pub struct FilePickerState {
    pub purpose: FilePickerPurpose,
    pub files: Vec<PathBuf>,
    pub selected: usize,
}

impl FilePickerState {
    pub fn new(purpose: FilePickerPurpose) -> Self {
        let files = match purpose {
            FilePickerPurpose::Resume => scan_files(&["game_save"], &["json"]),
            FilePickerPurpose::Replay => {
                scan_files(&["game_replay", "game_log"], &["json", "jsonl"])
            }
        };
        Self {
            purpose,
            files,
            selected: 0,
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

        let model_str = if player.kind == PlayerKind::Llm {
            AVAILABLE_MODELS
                .get(player.model_index)
                .copied()
                .unwrap_or("?")
        } else {
            "—"
        };
        let personality_str = if player.kind == PlayerKind::Llm {
            state
                .personality_names
                .get(player.personality_index)
                .map(|s| s.as_str())
                .unwrap_or("?")
        } else {
            "—"
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

    // Settings section.
    let settings_y = header_y + 2 + state.num_players() as u16;
    let divider_area = Rect::new(x_start, settings_y, content_width, 1);
    let divider = Paragraph::new("─".repeat(content_width as usize))
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(divider, divider_area);

    let seed_y = settings_y + 1;
    let seed_focused = matches!(state.focus, NewGameFocus::Setting(0));
    let seed_style = cell_style(seed_focused);
    let seed_display = if state.seed_input.is_empty() {
        "random"
    } else {
        &state.seed_input
    };
    let seed_line = Line::from(vec![
        Span::styled("  Seed: ", Style::default().fg(Color::White)),
        Span::styled(format!("[{}]", seed_display), seed_style),
    ]);
    let seed_area = Rect::new(x_start, seed_y, content_width, 1);
    Paragraph::new(seed_line).render(seed_area, f.buffer_mut());

    let turns_y = seed_y + 1;
    let turns_focused = matches!(state.focus, NewGameFocus::Setting(1));
    let turns_style = cell_style(turns_focused);
    let turns_line = Line::from(vec![
        Span::styled("  Max Turns: ", Style::default().fg(Color::White)),
        Span::styled(format!("[{}]", state.max_turns_input), turns_style),
    ]);
    let turns_area = Rect::new(x_start, turns_y, content_width, 1);
    Paragraph::new(turns_line).render(turns_area, f.buffer_mut());

    // Start button.
    let button_y = turns_y + 2;
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

/// Draw the file picker screen.
pub fn draw_file_picker(f: &mut Frame, state: &FilePickerState) {
    let area = f.area();
    f.render_widget(Clear, area);

    let title_text = match state.purpose {
        FilePickerPurpose::Resume => "Continue Game",
        FilePickerPurpose::Replay => "Replay Game",
    };

    let title_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let title = Paragraph::new(title_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Yellow).bold());
    f.render_widget(title, title_area);

    if state.files.is_empty() {
        let msg_area = Rect::new(area.x, area.y + 4, area.width, 1);
        let msg = Paragraph::new("No save files found.")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(msg, msg_area);
    } else {
        let items: Vec<&str> = state
            .files
            .iter()
            .map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("?"))
            .collect();
        let menu_y = area.y + 3;
        let menu_height = items.len().min(area.height.saturating_sub(6) as usize) as u16;
        let menu_area = Rect::new(area.x, menu_y, area.width, menu_height);
        render_menu(
            &items,
            state.selected,
            menu_area,
            f.buffer_mut(),
            Color::Cyan,
        );
    }

    // Hint.
    let hint_y = area.y + area.height - 1;
    let hint_area = Rect::new(area.x, hint_y, area.width, 1);
    let hint = Paragraph::new("[↑↓] navigate  [Enter] select  [Esc] back")
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
        s
    } else {
        &s[..max.saturating_sub(1)]
    }
}

/// Find a file matching `{prefix}*.{ext}` in the current directory.
fn find_files_with_extension(prefix: &str, ext: &str) -> Option<PathBuf> {
    let pattern = prefix.to_string();
    std::fs::read_dir(".")
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&pattern) && n.ends_with(ext))
                .unwrap_or(false)
        })
}

/// Scan the current directory for files matching any of the given prefixes and extensions.
fn scan_files(prefixes: &[&str], extensions: &[&str]) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(".")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| {
                    prefixes.iter().any(|prefix| n.starts_with(prefix))
                        && extensions.iter().any(|ext| n.ends_with(ext))
                })
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}
