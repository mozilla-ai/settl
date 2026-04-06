//! Non-game screens: Title, MainMenu, NewGame setup, Settings, PostGame.
//!
//! Each screen has a state struct and a `draw_*` rendering function.
//! Input handling lives in `mod.rs` dispatch.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::config::{Config, ModelBackend, ModelEntry};
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
    pub has_save: bool,
}

impl MainMenuState {
    pub fn new() -> Self {
        Self {
            selected: 0,
            has_save: crate::game::save::has_autosave(),
        }
    }

    pub fn menu_items(&self) -> Vec<&'static str> {
        if self.has_save {
            vec![
                "Continue",
                "New Game",
                "Personalities",
                "Settings",
                "Docs",
                "About",
                "Quit",
            ]
        } else {
            vec![
                "New Game",
                "Personalities",
                "Settings",
                "Docs",
                "About",
                "Quit",
            ]
        }
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
    pub effort_index: usize,
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
    AiModel,
    /// Reasoning effort level toggle.
    ReasoningEffort,
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
    /// Selected model index into the Config.models registry.
    pub model_index: usize,
    /// Cached model display names from the config.
    pub model_names: Vec<String>,
    /// Selected reasoning effort index into EFFORT_LEVELS.
    pub effort_index: usize,
    /// If set, shows a RAM warning popup: (required_gb, available_gb).
    pub ram_warning: Option<(u32, u32)>,
}

impl NewGameState {
    pub fn new(personalities: &[Personality], config: &Config) -> Self {
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

        let model_names: Vec<String> = config.models.iter().map(|m| m.name.clone()).collect();

        let effort_index = crate::config::EFFORT_LEVELS
            .iter()
            .position(|&l| l == config.default_effort)
            .unwrap_or(crate::config::DEFAULT_EFFORT_INDEX);

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
                        effort_index,
                    }
                } else {
                    PlayerConfig {
                        name: DEFAULT_NAMES[i - 1].into(),
                        kind: PlayerKind::Llamafile,
                        personality_index: i.min(personality_names.len().saturating_sub(1)),
                        effort_index,
                    }
                }
            })
            .collect();

        Self {
            players,
            focus: NewGameFocus::StartButton,
            personality_names,
            four_players: true,
            friendly_robber: false,
            random_board: false,
            model_index: 0,
            model_names,
            effort_index,
            ram_warning: None,
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

// ── Docs ──────────────────────────────────────────────────────────────

/// A single documentation page embedded at compile time.
pub struct DocsPage {
    pub title: &'static str,
    pub body: &'static str,
}

/// Parse a docs markdown file: strip YAML frontmatter, extract title, return body lines.
fn parse_doc(raw: &'static str) -> DocsPage {
    let stripped = raw.strip_prefix("---\n").unwrap_or(raw);
    let (frontmatter, body) = stripped.split_once("\n---\n").unwrap_or(("", stripped));

    let title = frontmatter
        .lines()
        .find_map(|l| l.strip_prefix("title:"))
        .map(|t| t.trim())
        .unwrap_or("Docs");

    // Skip the leading `# Title` line if present (we render our own header).
    let body = body.trim_start_matches('\n');
    let body = if let Some(rest) = body.strip_prefix("# ") {
        rest.split_once('\n').map(|(_, b)| b).unwrap_or("")
    } else {
        body
    };

    DocsPage { title, body }
}

/// All embedded documentation pages, ordered for sidebar display.
pub fn docs_pages() -> Vec<DocsPage> {
    vec![
        parse_doc(include_str!("../../docs/getting-started.md")),
        parse_doc(include_str!("../../docs/controls.md")),
    ]
}

#[derive(Debug, Default)]
pub struct DocsState {
    /// Which page is selected in the sidebar.
    pub page_index: usize,
    /// Scroll offset within the content panel.
    pub scroll: u16,
    /// Cached page count (avoids re-parsing on every frame).
    pub page_count: usize,
}

impl DocsState {
    pub fn new() -> Self {
        Self {
            page_index: 0,
            scroll: 0,
            page_count: 5,
        }
    }
}

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
    /// If set, we are resuming a saved game instead of starting a new one.
    pub resume_save: Option<crate::game::save::SaveFile>,
}

impl std::fmt::Debug for LlamafileSetupState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamafileSetupState")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

// ── Settings ──────────────────────────────────────────────────────────

/// Which field is being edited in a model entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelField {
    Name,
    /// Llamafile download URL.
    Url,
    /// Llamafile filename on disk.
    Filename,
    /// API base URL.
    BaseUrl,
    /// API key.
    ApiKey,
    /// API model identifier.
    Model,
}

impl ModelField {
    /// Label shown in the edit form.
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Url => "URL",
            Self::Filename => "Filename",
            Self::BaseUrl => "Base URL",
            Self::ApiKey => "API Key",
            Self::Model => "Model",
        }
    }

    /// Return the fields applicable to a given backend, in order.
    pub fn fields_for(backend: &ModelBackend) -> &'static [ModelField] {
        match backend {
            ModelBackend::Llamafile { .. } => {
                &[ModelField::Name, ModelField::Url, ModelField::Filename]
            }
            ModelBackend::Api { .. } => &[
                ModelField::Name,
                ModelField::BaseUrl,
                ModelField::ApiKey,
                ModelField::Model,
            ],
        }
    }

    /// Return the next field after this one for the given backend, or None if last.
    pub fn next(self, backend: &ModelBackend) -> Option<ModelField> {
        let fields = Self::fields_for(backend);
        let pos = fields.iter().position(|f| *f == self)?;
        fields.get(pos + 1).copied()
    }
}

// ── Shared Text Input Helpers ──────────────────────────────────────────

/// Insert a character at the cursor position in a text buffer.
fn text_insert(buf: &mut String, cursor: &mut usize, ch: char) {
    buf.insert(*cursor, ch);
    *cursor += ch.len_utf8();
}

/// Delete the character before the cursor.
fn text_backspace(buf: &mut String, cursor: &mut usize) {
    if *cursor > 0 {
        let prev = buf[..*cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        buf.drain(prev..*cursor);
        *cursor = prev;
    }
}

/// Delete the character at the cursor.
fn text_delete(buf: &mut String, cursor: &mut usize) {
    if *cursor < buf.len() {
        let next = buf[*cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| *cursor + i)
            .unwrap_or(buf.len());
        buf.drain(*cursor..next);
    }
}

/// Move cursor left by one character.
fn text_left(buf: &str, cursor: &mut usize) {
    if *cursor > 0 {
        *cursor = buf[..*cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }
}

/// Move cursor right by one character.
fn text_right(buf: &str, cursor: &mut usize) {
    if *cursor < buf.len() {
        *cursor = buf[*cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| *cursor + i)
            .unwrap_or(buf.len());
    }
}

/// Sub-focus within the Settings screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsFocus {
    /// Browsing the model list.
    ModelList,
    /// Editing a field of the selected model.
    EditField(ModelField),
    /// Confirming deletion of the selected model.
    ConfirmDelete,
}

/// State for the Settings screen.
#[derive(Debug)]
pub struct SettingsState {
    /// Working copy of the model list (cloned from Config on entry).
    pub models: Vec<ModelEntry>,
    /// Which model row is focused.
    pub selected: usize,
    /// Current sub-focus.
    pub focus: SettingsFocus,
    /// Text input buffer (used when editing a field).
    pub input_buf: String,
    /// Cursor position within input_buf (byte offset).
    pub input_cursor: usize,
    /// Whether changes have been made since entering.
    pub dirty: bool,
}

impl SettingsState {
    pub fn from_config(config: &Config) -> Self {
        Self {
            models: config.models.clone(),
            selected: 0,
            focus: SettingsFocus::ModelList,
            input_buf: String::new(),
            input_cursor: 0,
            dirty: false,
        }
    }

    /// Start editing a field: populate the input buffer with the current value.
    pub fn begin_edit(&mut self, field: ModelField) {
        if let Some(entry) = self.models.get(self.selected) {
            self.input_buf = Self::field_value(entry, field);
            self.input_cursor = self.input_buf.len();
            self.focus = SettingsFocus::EditField(field);
        }
    }

    /// Apply the current input buffer to the selected model's field.
    pub fn commit_edit(&mut self, field: ModelField) {
        if let Some(entry) = self.models.get_mut(self.selected) {
            let val = self.input_buf.clone();
            match (&mut entry.backend, field) {
                (_, ModelField::Name) => entry.name = val,
                (ModelBackend::Llamafile { ref mut url, .. }, ModelField::Url) => *url = val,
                (
                    ModelBackend::Llamafile {
                        ref mut filename, ..
                    },
                    ModelField::Filename,
                ) => {
                    *filename = val;
                }
                (
                    ModelBackend::Api {
                        ref mut base_url, ..
                    },
                    ModelField::BaseUrl,
                ) => {
                    *base_url = val;
                }
                (
                    ModelBackend::Api {
                        ref mut api_key, ..
                    },
                    ModelField::ApiKey,
                ) => {
                    *api_key = val;
                }
                (ModelBackend::Api { ref mut model, .. }, ModelField::Model) => *model = val,
                _ => {}
            }
            self.dirty = true;
        }
    }

    /// Read the current value of a field from a model entry.
    fn field_value(entry: &ModelEntry, field: ModelField) -> String {
        match (&entry.backend, field) {
            (_, ModelField::Name) => entry.name.clone(),
            (ModelBackend::Llamafile { url, .. }, ModelField::Url) => url.clone(),
            (ModelBackend::Llamafile { filename, .. }, ModelField::Filename) => filename.clone(),
            (ModelBackend::Api { base_url, .. }, ModelField::BaseUrl) => base_url.clone(),
            (ModelBackend::Api { api_key, .. }, ModelField::ApiKey) => api_key.clone(),
            (ModelBackend::Api { model, .. }, ModelField::Model) => model.clone(),
            _ => String::new(),
        }
    }

    /// Save the current model list to the config and disk.
    pub fn save(&self) -> Config {
        let config = Config {
            models: self.models.clone(),
            hooks: Vec::new(),
            default_effort: crate::config::default_effort(),
        };
        let _ = crate::config::save_config(&config);
        config
    }

    pub fn input_insert(&mut self, ch: char) {
        text_insert(&mut self.input_buf, &mut self.input_cursor, ch);
    }

    pub fn input_backspace(&mut self) {
        text_backspace(&mut self.input_buf, &mut self.input_cursor);
    }

    pub fn input_delete(&mut self) {
        text_delete(&mut self.input_buf, &mut self.input_cursor);
    }

    pub fn input_left(&mut self) {
        text_left(&self.input_buf, &mut self.input_cursor);
    }

    pub fn input_right(&mut self) {
        text_right(&self.input_buf, &mut self.input_cursor);
    }
}

// ── Personalities Screen ───────────────────────────────────────────────

/// Which field is being edited in a personality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonalityField {
    Name,
    Style,
    Aggression,
    Cooperation,
    Catchphrases,
}

/// Sub-focus within the Personalities screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonalitiesFocus {
    /// Browsing the personality list (left panel).
    List,
    /// Viewing detail in the right panel (scrollable).
    Detail,
    /// Editing a text field (Name or Style) with the input buffer.
    EditText(PersonalityField),
    /// Adjusting a slider field (Aggression or Cooperation).
    EditSlider(PersonalityField),
    /// Editing the catchphrases list.
    EditCatchphrases,
    /// Adding or editing a single catchphrase with text input.
    EditCatchphraseText,
    /// Confirming deletion of a custom personality.
    ConfirmDelete,
}

/// Source tracking: built-in or from a TOML file.
#[derive(Debug, Clone)]
pub enum PersonalitySource {
    BuiltIn,
    /// Loaded from a TOML file. Stores the filename stem (no path/ext).
    Custom(String),
}

/// State for the Personalities screen.
pub struct PersonalitiesState {
    /// All personalities: built-in first, then custom.
    pub entries: Vec<(Personality, PersonalitySource)>,
    /// Which entry is selected in the list.
    pub selected: usize,
    /// Current sub-focus.
    pub focus: PersonalitiesFocus,
    /// Text input buffer (used when editing a field).
    pub input_buf: String,
    /// Cursor position within input_buf (byte offset).
    pub input_cursor: usize,
    /// Scroll offset for the detail panel.
    pub detail_scroll: u16,
    /// Index of the catchphrase being edited/selected.
    pub catchphrase_selected: usize,
    /// Whether any changes were made.
    pub dirty: bool,
    /// Base directory for personality TOML files.
    pub base_dir: String,
}

impl PersonalitiesState {
    const DEFAULT_DIR: &'static str = "./personalities";

    pub fn new(discovered: &[Personality]) -> Self {
        let base_dir = Self::DEFAULT_DIR.to_string();
        let mut entries: Vec<(Personality, PersonalitySource)> = Personality::built_in_all()
            .into_iter()
            .map(|p| (p, PersonalitySource::BuiltIn))
            .collect();

        // Discover custom personalities from disk to get filenames.
        if let Ok(dir) = std::fs::read_dir(&base_dir) {
            let mut custom: Vec<(Personality, String)> = dir
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                        let stem = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        Personality::from_toml_file(&path).ok().map(|p| (p, stem))
                    } else {
                        None
                    }
                })
                .collect();
            custom.sort_by(|a, b| a.0.name.cmp(&b.0.name));
            for (p, stem) in custom {
                entries.push((p, PersonalitySource::Custom(stem)));
            }
        }

        // If no custom found but discovered personalities were passed, use those.
        let has_custom = entries
            .iter()
            .any(|(_, s)| matches!(s, PersonalitySource::Custom(_)));
        if !has_custom {
            for p in discovered {
                let stem = Personality::filename_from_name(&p.name);
                entries.push((p.clone(), PersonalitySource::Custom(stem)));
            }
        }

        Self {
            entries,
            selected: 0,
            focus: PersonalitiesFocus::List,
            input_buf: String::new(),
            input_cursor: 0,
            detail_scroll: 0,
            catchphrase_selected: 0,
            dirty: false,
            base_dir,
        }
    }

    /// Whether the currently selected personality is a custom (editable) one.
    pub fn selected_is_custom(&self) -> bool {
        self.entries
            .get(self.selected)
            .map(|(_, s)| matches!(s, PersonalitySource::Custom(_)))
            .unwrap_or(false)
    }

    /// The number of built-in personalities in the list.
    pub fn builtin_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, s)| matches!(s, PersonalitySource::BuiltIn))
            .count()
    }

    /// Start editing a text field: populate the input buffer with the current value.
    pub fn begin_edit_text(&mut self, field: PersonalityField) {
        if let Some((p, _)) = self.entries.get(self.selected) {
            self.input_buf = match field {
                PersonalityField::Name => p.name.clone(),
                PersonalityField::Style => p.style.clone(),
                _ => String::new(),
            };
            self.input_cursor = self.input_buf.len();
            self.focus = PersonalitiesFocus::EditText(field);
        }
    }

    /// Apply the current input buffer to the selected personality's text field.
    pub fn commit_edit_text(&mut self, field: PersonalityField) {
        if let Some((p, _)) = self.entries.get_mut(self.selected) {
            match field {
                PersonalityField::Name => p.name = self.input_buf.clone(),
                PersonalityField::Style => p.style = self.input_buf.clone(),
                _ => {}
            }
            self.dirty = true;
        }
    }

    /// Begin editing a slider field.
    pub fn begin_edit_slider(&mut self, field: PersonalityField) {
        self.focus = PersonalitiesFocus::EditSlider(field);
    }

    /// Advance to the next field in the sequential edit flow.
    pub fn next_field(&mut self, current: PersonalityField) {
        match current {
            PersonalityField::Name => self.begin_edit_text(PersonalityField::Style),
            PersonalityField::Style => {
                self.begin_edit_slider(PersonalityField::Aggression);
            }
            PersonalityField::Aggression => {
                self.begin_edit_slider(PersonalityField::Cooperation);
            }
            PersonalityField::Cooperation => {
                self.catchphrase_selected = 0;
                self.focus = PersonalitiesFocus::EditCatchphrases;
            }
            PersonalityField::Catchphrases => {
                self.focus = PersonalitiesFocus::List;
            }
        }
    }

    /// Save the currently selected custom personality to its TOML file.
    pub fn save_current(&self) {
        if let Some((p, PersonalitySource::Custom(stem))) = self.entries.get(self.selected) {
            let path = format!("{}/{}.toml", self.base_dir, stem);
            let _ = std::fs::create_dir_all(&self.base_dir);
            let _ = p.to_toml_file(std::path::Path::new(&path));
        }
    }

    /// Delete the currently selected custom personality's TOML file.
    pub fn delete_current(&mut self) {
        if let Some((_, PersonalitySource::Custom(stem))) = self.entries.get(self.selected) {
            let path = format!("{}/{}.toml", self.base_dir, stem);
            let _ = std::fs::remove_file(&path);
            self.entries.remove(self.selected);
            if self.selected >= self.entries.len() && self.selected > 0 {
                self.selected -= 1;
            }
            self.dirty = true;
        }
    }

    pub fn input_insert(&mut self, ch: char) {
        text_insert(&mut self.input_buf, &mut self.input_cursor, ch);
    }

    pub fn input_backspace(&mut self) {
        text_backspace(&mut self.input_buf, &mut self.input_cursor);
    }

    pub fn input_delete(&mut self) {
        text_delete(&mut self.input_buf, &mut self.input_cursor);
    }

    pub fn input_left(&mut self) {
        text_left(&self.input_buf, &mut self.input_cursor);
    }

    pub fn input_right(&mut self) {
        text_right(&self.input_buf, &mut self.input_cursor);
    }
}

// ── Drawing Functions ──────────────────────────────────────────────────

/// Draw the main menu.
pub fn draw_main_menu(
    f: &mut Frame,
    state: &MainMenuState,
    update_info: Option<&crate::update_check::UpdateInfo>,
) {
    let area = f.area();
    f.render_widget(Clear, area);

    // Title art + subtitle at top.
    let art_lines = TITLE_ART.lines().count() as u16;
    let items = state.menu_items();
    let menu_height = items.len() as u16;
    let has_update = update_info.is_some();
    // Reserve an extra line for the update badge below the hint bar.
    let update_height = if has_update { 2 } else { 0 };
    let total_height = art_lines + 4 + menu_height + 2 + update_height;
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

    // Update badge.
    if let Some(info) = update_info {
        let badge_y = hint_y + 2;
        if badge_y < area.y + area.height {
            let badge_area = Rect::new(area.x, badge_y, area.width, 1);
            let text = format!(
                "update available: v{} -> v{}",
                info.current_version, info.latest_version
            );
            let badge = Paragraph::new(text)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(badge, badge_area);
        }
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

/// Render markdown body text into styled ratatui Lines.
///
/// Handles: `## headings`, `### subheadings`, `| tables |`, `` ```code blocks``` ``,
/// `- list items`, `**bold**`, `` `inline code` ``, and plain paragraphs.
fn render_markdown_lines(body: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;

    for raw_line in body.lines() {
        if raw_line.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            lines.push(Line::styled(
                format!("  {}", raw_line),
                Style::default().fg(Color::Cyan),
            ));
            continue;
        }

        let trimmed = raw_line.trim();

        if trimmed.is_empty() {
            lines.push(Line::from(""));
        } else if let Some(heading) = trimmed.strip_prefix("### ") {
            lines.push(Line::from(""));
            lines.push(Line::styled(
                heading.to_string(),
                Style::default().fg(Color::White).bold(),
            ));
        } else if let Some(heading) = trimmed.strip_prefix("## ") {
            lines.push(Line::from(""));
            lines.push(Line::styled(
                heading.to_string(),
                Style::default().fg(Color::Yellow).bold(),
            ));
            // Underline.
            let underline = "-".repeat(heading.len());
            lines.push(Line::styled(
                underline,
                Style::default().fg(Color::DarkGray),
            ));
        } else if trimmed.starts_with('|') {
            // Table row: render in dim white.
            if trimmed
                .chars()
                .all(|c| c == '|' || c == '-' || c == ' ' || c == ':')
            {
                // Separator row: skip.
            } else {
                lines.push(Line::styled(
                    format!("  {}", trimmed),
                    Style::default().fg(Color::White),
                ));
            }
        } else if let Some(item) = trimmed.strip_prefix("- ") {
            lines.push(Line::from(vec![
                Span::styled("  - ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    render_inline_markup(item),
                    Style::default().fg(Color::White),
                ),
            ]));
        } else {
            lines.push(Line::styled(
                render_inline_markup(trimmed),
                Style::default().fg(Color::White),
            ));
        }
    }
    lines
}

/// Strip markdown inline formatting (`**bold**`, `` `code` ``) for plain text display.
fn render_inline_markup(text: &str) -> String {
    let mut out = text.replace("**", "");
    // Remove inline backticks.
    out = out.replace('`', "");
    out
}

/// Draw the documentation viewer with sidebar and scrollable content.
pub fn draw_docs(f: &mut Frame, state: &DocsState) {
    let area = f.area();
    f.render_widget(Clear, area);

    let pages = docs_pages();

    // Two-column layout: sidebar (22 chars) | content (rest).
    let sidebar_width = 24u16.min(area.width / 3);
    let content_width = area.width.saturating_sub(sidebar_width);

    // Sidebar.
    let sidebar_area = Rect::new(area.x, area.y, sidebar_width, area.height.saturating_sub(1));
    let sidebar_block = Block::default()
        .title(" Docs ")
        .title_alignment(Alignment::Left)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let sidebar_inner = sidebar_block.inner(sidebar_area);
    f.render_widget(sidebar_block, sidebar_area);

    for (i, page) in pages.iter().enumerate() {
        if i as u16 >= sidebar_inner.height {
            break;
        }
        let row = Rect::new(
            sidebar_inner.x,
            sidebar_inner.y + i as u16,
            sidebar_inner.width,
            1,
        );
        let style = if i == state.page_index {
            Style::default().fg(Color::Black).bg(Color::Cyan).bold()
        } else {
            Style::default().fg(Color::White)
        };
        let label = if i == state.page_index {
            format!(" > {}", page.title)
        } else {
            format!("   {}", page.title)
        };
        let item = Paragraph::new(label).style(style);
        f.render_widget(item, row);
    }

    // Content panel.
    let content_area = Rect::new(
        area.x + sidebar_width,
        area.y,
        content_width,
        area.height.saturating_sub(1),
    );
    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let content_inner = content_block.inner(content_area);
    f.render_widget(content_block, content_area);

    if let Some(page) = pages.get(state.page_index) {
        // Page title.
        let title_area = Rect::new(
            content_inner.x + 1,
            content_inner.y,
            content_inner.width.saturating_sub(2),
            1,
        );
        let title = Paragraph::new(page.title).style(Style::default().fg(Color::Yellow).bold());
        f.render_widget(title, title_area);

        // Rendered markdown content.
        let body_lines = render_markdown_lines(page.body);
        let body_area = Rect::new(
            content_inner.x + 1,
            content_inner.y + 2,
            content_inner.width.saturating_sub(2),
            content_inner.height.saturating_sub(2),
        );

        let content = Paragraph::new(body_lines)
            .scroll((state.scroll, 0))
            .wrap(Wrap { trim: false });
        f.render_widget(content, body_area);
    }

    // Hint bar.
    let hint_y = area.y + area.height - 1;
    let hint_area = Rect::new(area.x, hint_y, area.width, 1);
    let hint = Paragraph::new("[↑↓] page  [j/k] scroll  [Esc] back")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, hint_area);
}

/// Draw the new game setup screen.
pub fn draw_new_game(f: &mut Frame, state: &NewGameState) {
    let area = f.area();
    f.render_widget(Clear, area);

    // Content block height: title(1) + gap(1) + button(1) + gap(1) + PLAYERS(1) + gap(1)
    // + count(1) + gap(1) + 4 player rows(4) + gap(1) + RULES(1) + gap(1)
    // + 4 toggle rows(4) + gap(1) + hint(1) = 21 rows.
    let content_height: u16 = 21;
    let content_width = 64u16.min(area.width.saturating_sub(4));
    let x_start = area.x + (area.width.saturating_sub(content_width)) / 2;
    let top = area.y + (area.height.saturating_sub(content_height)) / 2;

    // Title.
    let title_area = Rect::new(x_start, top, content_width, 1);
    let title = Paragraph::new("New Game")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Yellow).bold());
    f.render_widget(title, title_area);

    // Start button.
    let button_y = top + 2;
    let button_focused = matches!(state.focus, NewGameFocus::StartButton);
    let button_style = if button_focused {
        Style::default().fg(Color::Black).bg(Color::Green).bold()
    } else {
        Style::default().fg(Color::Green).bold()
    };
    let button_area = Rect::new(x_start, button_y, content_width, 1);
    let button = Paragraph::new("  [ Start Game ]").style(button_style);
    f.render_widget(button, button_area);

    // -- PLAYERS section --
    let section_y = button_y + 2;
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
    let ms_focused = matches!(state.focus, NewGameFocus::AiModel);
    draw_toggle_row(
        f,
        x_start,
        ms_y,
        content_width,
        "AI Model",
        state
            .model_names
            .get(state.model_index)
            .map(|s| s.as_str())
            .unwrap_or("(none)"),
        ms_focused,
    );

    // Reasoning Effort.
    let re_y = ms_y + 1;
    let re_focused = matches!(state.focus, NewGameFocus::ReasoningEffort);
    draw_toggle_row(
        f,
        x_start,
        re_y,
        content_width,
        "Reasoning Effort",
        crate::config::EFFORT_LEVELS
            .get(state.effort_index)
            .unwrap_or(&"low"),
        re_focused,
    );

    // Hint bar right below the last rule row.
    let hint_y = re_y + 2;
    let hint_area = Rect::new(x_start, hint_y, content_width, 1);
    let hint_text = if matches!(state.focus, NewGameFocus::StartButton) {
        "j/k/\u{2191}\u{2193}: move  |  Enter: start game  |  Esc: back"
    } else {
        "j/k/\u{2191}\u{2193}: move  |  h/l/\u{2190}\u{2192}/Tab/Enter: change  |  Esc: back"
    };
    let hint = Paragraph::new(hint_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, hint_area);

    // RAM warning popup overlay.
    if let Some((required, available)) = state.ram_warning {
        let width = 52u16.min(area.width.saturating_sub(4));
        let height = 7u16.min(area.height.saturating_sub(4));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup_area = Rect::new(x, y, width, height);

        f.render_widget(Clear, popup_area);
        let block = Block::default()
            .title(" Low Memory Warning ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(popup_area);
        f.render_widget(block, popup_area);

        let text = vec![
            Line::from(Span::styled(
                format!("Model needs ~{required} GB RAM, system has {available} GB."),
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Enter: start anyway  |  Any key: cancel",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let para = Paragraph::new(text).alignment(Alignment::Center);
        f.render_widget(para, inner);
    }
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

/// Draw the settings screen (model registry).
pub fn draw_settings(f: &mut Frame, state: &SettingsState) {
    let area = f.area();
    f.render_widget(Clear, area);

    let content_width = 70u16.min(area.width.saturating_sub(4));
    let x_start = area.x + (area.width.saturating_sub(content_width)) / 2;

    // Title.
    let title_area = Rect::new(x_start, area.y + 1, content_width, 1);
    let title = Paragraph::new("Settings")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Yellow).bold());
    f.render_widget(title, title_area);

    // Section header.
    let section_y = area.y + 3;
    let section_area = Rect::new(x_start, section_y, content_width, 1);
    let section = Paragraph::new("MODELS").style(Style::default().fg(Color::DarkGray).bold());
    f.render_widget(section, section_area);

    // Model list.
    let list_y = section_y + 2;
    if state.models.is_empty() {
        let empty_area = Rect::new(x_start, list_y, content_width, 1);
        let empty = Paragraph::new(
            "    No models configured. Press [a] to add a llamafile or [A] to add an API model.",
        )
        .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, empty_area);
    } else {
        let max_visible = (area.height.saturating_sub(list_y + 8)) as usize;
        for (i, entry) in state.models.iter().enumerate() {
            if i >= max_visible {
                break;
            }
            let row_y = list_y + i as u16;
            let row_area = Rect::new(x_start, row_y, content_width, 1);
            let is_focused = state.selected == i && matches!(state.focus, SettingsFocus::ModelList);

            let marker = if state.selected == i { ">" } else { " " };
            let backend_label = match &entry.backend {
                ModelBackend::Llamafile { .. } => "Llamafile",
                ModelBackend::Api { .. } => "API",
            };

            let name_style = if is_focused {
                Style::default().fg(Color::Black).bg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::White)
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("  {} {}  ", marker, i + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{:<40}", truncate_str(&entry.name, 40)), name_style),
                Span::styled(
                    backend_label.to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            Paragraph::new(line).render(row_area, f.buffer_mut());
        }
    }

    // Edit form (when editing a field).
    if let SettingsFocus::EditField(active_field) = state.focus {
        if let Some(entry) = state.models.get(state.selected) {
            let form_y = list_y + state.models.len().min(10) as u16 + 1;
            let fields = ModelField::fields_for(&entry.backend);

            // Section divider.
            let div_area = Rect::new(x_start, form_y, content_width, 1);
            let div_text = format!("  Edit: {} ", truncate_str(&entry.name, 40));
            let div = Paragraph::new(div_text).style(Style::default().fg(Color::Yellow).bold());
            f.render_widget(div, div_area);

            for (fi, &field) in fields.iter().enumerate() {
                let field_y = form_y + 1 + fi as u16;
                if field_y >= area.y + area.height - 2 {
                    break;
                }
                let field_area = Rect::new(x_start, field_y, content_width, 1);
                let is_active = field == active_field;

                let value_str = if is_active {
                    // Show input buffer with cursor.
                    let before = &state.input_buf[..state.input_cursor];
                    let after = &state.input_buf[state.input_cursor..];
                    format!("{}|{}", before, after)
                } else {
                    let val = SettingsState::field_value(entry, field);
                    if field == ModelField::ApiKey && !val.is_empty() {
                        // Mask API key when not editing.
                        let visible = val.len().min(4);
                        format!(
                            "{}{}",
                            &val[..visible],
                            "*".repeat(val.len().saturating_sub(visible))
                        )
                    } else {
                        val
                    }
                };

                let value_style = if is_active {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };

                let line = Line::from(vec![
                    Span::styled(
                        format!("    {:<12}", field.label()),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(truncate_str(&value_str, 50).to_string(), value_style),
                ]);
                Paragraph::new(line).render(field_area, f.buffer_mut());
            }
        }
    }

    // Delete confirmation.
    if matches!(state.focus, SettingsFocus::ConfirmDelete) {
        if let Some(entry) = state.models.get(state.selected) {
            let confirm_y = list_y + state.models.len().min(10) as u16 + 1;
            let confirm_area = Rect::new(x_start, confirm_y, content_width, 1);
            let confirm = Paragraph::new(format!(
                "  Delete \"{}\"? [y] yes  [n] no",
                truncate_str(&entry.name, 30)
            ))
            .style(Style::default().fg(Color::Red).bold());
            f.render_widget(confirm, confirm_area);
        }
    }

    // Hint bar.
    let hint_y = area.y + area.height - 1;
    let hint_area = Rect::new(area.x, hint_y, area.width, 1);
    let hint_text = match state.focus {
        SettingsFocus::ModelList => {
            "\u{2191}\u{2193}: move  |  Enter: edit  |  [a] add llamafile  [A] add API  [d] delete  |  Esc: back"
        }
        SettingsFocus::EditField(_) => {
            "Type to edit  |  Enter/Tab: next field  |  Esc: cancel"
        }
        SettingsFocus::ConfirmDelete => "[y] confirm  [n] cancel",
    };
    let hint = Paragraph::new(hint_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, hint_area);
}

/// Draw the Personalities management screen.
pub fn draw_personalities(f: &mut Frame, state: &PersonalitiesState) {
    let area = f.area();
    f.render_widget(Clear, area);

    // Two-panel layout: sidebar (26 chars) | detail (rest).
    let sidebar_width = 26u16.min(area.width / 3);
    let detail_width = area.width.saturating_sub(sidebar_width);

    // -- Left panel: personality list --
    let sidebar_area = Rect::new(area.x, area.y, sidebar_width, area.height.saturating_sub(1));
    let sidebar_border_color = if matches!(state.focus, PersonalitiesFocus::List) {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let sidebar_block = Block::default()
        .title(" Personalities ")
        .title_alignment(Alignment::Left)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(sidebar_border_color));
    let sidebar_inner = sidebar_block.inner(sidebar_area);
    f.render_widget(sidebar_block, sidebar_area);

    let builtin_count = state.builtin_count();
    let mut row_y = 0u16;

    for (i, (p, source)) in state.entries.iter().enumerate() {
        // Draw separator between built-in and custom.
        if i == builtin_count && builtin_count > 0 && row_y < sidebar_inner.height {
            let sep_area = Rect::new(
                sidebar_inner.x,
                sidebar_inner.y + row_y,
                sidebar_inner.width,
                1,
            );
            let sep_line = "\u{2500}".repeat(sidebar_inner.width as usize);
            f.render_widget(
                Paragraph::new(sep_line).style(Style::default().fg(Color::DarkGray)),
                sep_area,
            );
            row_y += 1;
        }
        if row_y >= sidebar_inner.height {
            break;
        }

        let is_selected = i == state.selected;
        let row_area = Rect::new(
            sidebar_inner.x,
            sidebar_inner.y + row_y,
            sidebar_inner.width,
            1,
        );

        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan).bold()
        } else {
            Style::default().fg(Color::White)
        };

        let marker = if is_selected { ">" } else { " " };
        let tag = if matches!(source, PersonalitySource::Custom(_)) {
            " *"
        } else {
            ""
        };
        let max_name = (sidebar_inner.width as usize).saturating_sub(4 + tag.len());
        let label = format!(" {} {}{}", marker, truncate_str(&p.name, max_name), tag);

        f.render_widget(Paragraph::new(label).style(style), row_area);
        row_y += 1;
    }

    // -- Right panel: detail view --
    let detail_area = Rect::new(
        area.x + sidebar_width,
        area.y,
        detail_width,
        area.height.saturating_sub(1),
    );
    let detail_border_color = if !matches!(state.focus, PersonalitiesFocus::List) {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let detail_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(detail_border_color));
    let detail_inner = detail_block.inner(detail_area);
    f.render_widget(detail_block, detail_area);

    if let Some((p, source)) = state.entries.get(state.selected) {
        let w = detail_inner.width.saturating_sub(2) as usize;
        let mut lines: Vec<Line> = Vec::new();

        // Name header.
        lines.push(Line::from(Span::styled(
            &p.name,
            Style::default().fg(Color::Yellow).bold(),
        )));

        let is_builtin = matches!(source, PersonalitySource::BuiltIn);
        if is_builtin {
            lines.push(Line::from(Span::styled(
                "(built-in, read-only)",
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.push(Line::from(""));

        // Style field.
        if matches!(
            state.focus,
            PersonalitiesFocus::EditText(PersonalityField::Style)
        ) {
            lines.push(Line::from(Span::styled(
                "Style:",
                Style::default().fg(Color::DarkGray),
            )));
            let before = &state.input_buf[..state.input_cursor];
            let after = &state.input_buf[state.input_cursor..];
            lines.push(Line::from(vec![
                Span::styled(before, Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::styled("|", Style::default().fg(Color::Yellow).bg(Color::Cyan)),
                Span::styled(after, Style::default().fg(Color::Black).bg(Color::Cyan)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                "Style:",
                Style::default().fg(Color::DarkGray),
            )));
            for chunk in wrap_text(&p.style, w) {
                lines.push(Line::from(Span::styled(
                    chunk,
                    Style::default().fg(Color::White),
                )));
            }
        }
        lines.push(Line::from(""));

        // Aggression slider.
        let agg_editing = matches!(
            state.focus,
            PersonalitiesFocus::EditSlider(PersonalityField::Aggression)
        );
        draw_slider_line(
            &mut lines,
            "Aggression",
            p.aggression,
            Color::Red,
            agg_editing,
        );

        // Cooperation slider.
        let coop_editing = matches!(
            state.focus,
            PersonalitiesFocus::EditSlider(PersonalityField::Cooperation)
        );
        draw_slider_line(
            &mut lines,
            "Cooperation",
            p.cooperation,
            Color::Green,
            coop_editing,
        );
        lines.push(Line::from(""));

        // Catchphrases.
        let editing_catchphrases = matches!(
            state.focus,
            PersonalitiesFocus::EditCatchphrases | PersonalitiesFocus::EditCatchphraseText
        );
        lines.push(Line::from(Span::styled(
            "Catchphrases:",
            Style::default().fg(Color::DarkGray),
        )));
        if p.catchphrases.is_empty() && !editing_catchphrases {
            lines.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(Color::DarkGray),
            )));
        }
        for (ci, phrase) in p.catchphrases.iter().enumerate() {
            let is_cp_selected = editing_catchphrases && ci == state.catchphrase_selected;
            if matches!(state.focus, PersonalitiesFocus::EditCatchphraseText)
                && ci == state.catchphrase_selected
            {
                let before = &state.input_buf[..state.input_cursor];
                let after = &state.input_buf[state.input_cursor..];
                lines.push(Line::from(vec![
                    Span::styled("  \"", Style::default().fg(Color::Cyan)),
                    Span::styled(before, Style::default().fg(Color::Black).bg(Color::Cyan)),
                    Span::styled("|", Style::default().fg(Color::Yellow).bg(Color::Cyan)),
                    Span::styled(after, Style::default().fg(Color::Black).bg(Color::Cyan)),
                    Span::styled("\"", Style::default().fg(Color::Cyan)),
                ]));
            } else if is_cp_selected {
                lines.push(Line::from(Span::styled(
                    format!("  > \"{}\"", truncate_str(phrase, w.saturating_sub(6))),
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("    \"{}\"", truncate_str(phrase, w.saturating_sub(6))),
                    Style::default().fg(Color::Cyan),
                )));
            }
        }
        // Show text input for new catchphrase being added.
        if matches!(state.focus, PersonalitiesFocus::EditCatchphraseText)
            && state.catchphrase_selected >= p.catchphrases.len()
        {
            let before = &state.input_buf[..state.input_cursor];
            let after = &state.input_buf[state.input_cursor..];
            lines.push(Line::from(vec![
                Span::styled("  \"", Style::default().fg(Color::Cyan)),
                Span::styled(before, Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::styled("|", Style::default().fg(Color::Yellow).bg(Color::Cyan)),
                Span::styled(after, Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::styled("\"", Style::default().fg(Color::Cyan)),
            ]));
        }
        if editing_catchphrases && !matches!(state.focus, PersonalitiesFocus::EditCatchphraseText) {
            lines.push(Line::from(Span::styled(
                "  [a] add  [d] delete  [Enter] edit  [Esc] done",
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.push(Line::from(""));

        // Setup strategy (view-only summary).
        lines.push(Line::from(Span::styled(
            "Setup Strategy:",
            Style::default().fg(Color::DarkGray),
        )));
        if let Some(strat) = &p.setup_strategy {
            let line_count = strat.lines().count();
            let preview: String = strat.lines().take(3).collect::<Vec<_>>().join(" ");
            for chunk in wrap_text(&preview, w) {
                lines.push(Line::from(Span::styled(
                    chunk,
                    Style::default().fg(Color::White),
                )));
            }
            if line_count > 3 {
                lines.push(Line::from(Span::styled(
                    format!("  ... ({} lines total)", line_count),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  (not set)",
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.push(Line::from(""));

        // Strategy guide (view-only summary).
        lines.push(Line::from(Span::styled(
            "Strategy Guide:",
            Style::default().fg(Color::DarkGray),
        )));
        if let Some(guide) = &p.strategy_guide {
            let line_count = guide.lines().count();
            let preview: String = guide.lines().take(3).collect::<Vec<_>>().join(" ");
            for chunk in wrap_text(&preview, w) {
                lines.push(Line::from(Span::styled(
                    chunk,
                    Style::default().fg(Color::White),
                )));
            }
            if line_count > 3 {
                lines.push(Line::from(Span::styled(
                    format!("  ... ({} lines total)", line_count),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  (not set)",
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Name editing overlay: replace the first line with input buffer.
        if matches!(
            state.focus,
            PersonalitiesFocus::EditText(PersonalityField::Name)
        ) {
            let before = &state.input_buf[..state.input_cursor];
            let after = &state.input_buf[state.input_cursor..];
            lines[0] = Line::from(vec![
                Span::styled(before, Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::styled("|", Style::default().fg(Color::Yellow).bg(Color::Cyan)),
                Span::styled(after, Style::default().fg(Color::Black).bg(Color::Cyan)),
            ]);
        }

        let content = Paragraph::new(lines)
            .scroll((state.detail_scroll, 0))
            .wrap(Wrap { trim: false });
        let padded = Rect::new(
            detail_inner.x + 1,
            detail_inner.y,
            detail_inner.width.saturating_sub(2),
            detail_inner.height,
        );
        f.render_widget(content, padded);
    }

    // -- Delete confirmation overlay --
    if matches!(state.focus, PersonalitiesFocus::ConfirmDelete) {
        if let Some((p, _)) = state.entries.get(state.selected) {
            let popup_w = 48u16.min(area.width.saturating_sub(4));
            let popup_h = 3u16;
            let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
            let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
            let overlay = Rect::new(x, y, popup_w, popup_h);
            f.render_widget(Clear, overlay);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red));
            let inner = block.inner(overlay);
            f.render_widget(block, overlay);
            let msg = format!("Delete \"{}\"? [y] yes  [n] no", truncate_str(&p.name, 20));
            f.render_widget(
                Paragraph::new(msg)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::Red).bold()),
                inner,
            );
        }
    }

    // -- Hint bar --
    let hint_y = area.y + area.height - 1;
    let hint_area = Rect::new(area.x, hint_y, area.width, 1);
    let hint_text = match &state.focus {
        PersonalitiesFocus::List => {
            "[j/k] select  [Enter] edit  [n] new  [D] duplicate  [d] delete  [Tab] detail  [Esc] back"
        }
        PersonalitiesFocus::Detail => "[j/k] scroll  [Tab] list  [Enter] edit  [Esc] back",
        PersonalitiesFocus::EditText(_) => {
            "Type to edit  |  [Enter/Tab] next field  |  [Esc] cancel"
        }
        PersonalitiesFocus::EditSlider(_) => {
            "[Left/Right] adjust  |  [Enter/Tab] next field  |  [Esc] cancel"
        }
        PersonalitiesFocus::EditCatchphrases => {
            "[j/k] select  [a] add  [d] delete  [Enter] edit  [Esc] done"
        }
        PersonalitiesFocus::EditCatchphraseText => {
            "Type phrase  |  [Enter] save  |  [Esc] cancel"
        }
        PersonalitiesFocus::ConfirmDelete => "[y] confirm  [n] cancel",
    };
    f.render_widget(
        Paragraph::new(hint_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        hint_area,
    );
}

/// Build a slider visualization line for the detail panel.
fn draw_slider_line(
    lines: &mut Vec<Line>,
    label: &str,
    value: f32,
    fill_color: Color,
    editing: bool,
) {
    let bar_len = 10;
    let filled = (value * bar_len as f32).round() as usize;
    let empty = bar_len - filled;
    let bar_filled = "\u{2588}".repeat(filled);
    let bar_empty = "\u{2591}".repeat(empty);

    let mut spans = Vec::new();
    if editing {
        spans.push(Span::styled("< ", Style::default().fg(Color::Yellow)));
    } else {
        spans.push(Span::styled("  ", Style::default().fg(Color::DarkGray)));
    }
    spans.push(Span::styled(
        format!("{:<14}", format!("{}:", label)),
        Style::default().fg(Color::DarkGray),
    ));
    spans.push(Span::styled(bar_filled, Style::default().fg(fill_color)));
    spans.push(Span::styled(
        bar_empty,
        Style::default().fg(Color::DarkGray),
    ));
    spans.push(Span::styled(
        format!("  {:.1}", value),
        Style::default().fg(Color::White),
    ));
    if editing {
        spans.push(Span::styled(" >", Style::default().fg(Color::Yellow)));
    }

    lines.push(Line::from(spans));
}

/// Simple word-wrap: split text into lines of at most `width` characters.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut result = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in line.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                result.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
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
