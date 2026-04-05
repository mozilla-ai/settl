pub mod board_view;
pub mod chat_panel;
pub mod game_log;
pub mod layout;
pub mod menu;
pub mod resource_bar;
pub mod screens;

#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod testing;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::game::board::Board;
use crate::game::event::GameEvent;
use crate::game::orchestrator::GameOrchestrator;
use crate::game::state::GameState;
use crate::player;
use crate::player::personality::Personality;

use screens::*;

/// Bright player colors for buildings and roads on the board.
pub const PLAYER_COLORS: [Color; 4] = [
    Color::LightRed,
    Color::LightBlue,
    Color::LightGreen,
    Color::LightMagenta,
];

/// Standard player colors for text labels in panels.
pub const PLAYER_TEXT_COLORS: [Color; 4] = [Color::Red, Color::Blue, Color::Green, Color::Magenta];

/// Events sent from the game orchestrator to the TUI.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// A game state update with the latest event and a human-readable message.
    StateUpdate {
        state: Arc<GameState>,
        event: Option<GameEvent>,
        message: String,
    },
    /// AI reasoning trace from an LLM or random player (complete, final).
    AiReasoning {
        player_id: usize,
        player_name: String,
        reasoning: String,
    },
    /// A streaming text chunk from an AI player's reasoning (appended progressively).
    AiReasoningChunk {
        player_id: usize,
        player_name: String,
        chunk: String,
    },
    /// Game event narration for the AI Reasoning panel (turn markers, dice, actions).
    Narration { message: String },
    /// The game has ended.
    GameOver { winner: usize, message: String },
}

// ── Screen State Machine ───────────────────────────────────────────────

/// The active screen.
#[allow(clippy::large_enum_variant)]
pub enum Screen {
    MainMenu(MainMenuState),
    NewGame(NewGameState),
    About(AboutState),
    Docs(DocsState),
    Settings(SettingsState),
    Personalities(PersonalitiesState),
    LlamafileSetup(LlamafileSetupState),
    Playing(PlayingState),
    PostGame(PostGameState),
}

use crate::game::actions::TradeOffer;
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};
use crate::player::tui_human::{HumanResponse, PromptKind};
use crate::player::PlayerChoice;

/// Map a resource key character (w/b/s/h/o) to its index in `Resource::all()`.
fn resource_key_index(key: KeyCode) -> Option<usize> {
    match key {
        KeyCode::Char('w') => Some(0),
        KeyCode::Char('b') => Some(1),
        KeyCode::Char('s') => Some(2),
        KeyCode::Char('h') => Some(3),
        KeyCode::Char('o') => Some(4),
        _ => None,
    }
}

// ── Input Mode State Machine ──────────────────────────────────────────

/// Legal game-coordinate positions for the board cursor, tagged by type.
/// Replaces the previous `CursorKind` + three mutually-exclusive Vecs.
#[derive(Debug, Clone)]
pub enum CursorLegal {
    Settlements(Vec<VertexCoord>),
    Roads(Vec<EdgeCoord>),
    Hexes(Vec<HexCoord>),
}

impl CursorLegal {
    pub fn kind_name(&self) -> &'static str {
        match self {
            CursorLegal::Settlements(_) => "settlement",
            CursorLegal::Roads(_) => "road",
            CursorLegal::Hexes(_) => "robber",
        }
    }
}

/// A cursor target with its screen position for navigation.
#[derive(Debug, Clone)]
pub struct CursorTarget {
    pub screen_col: u16,
    pub screen_row: u16,
}

/// Which side of the trade builder is active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TradeSide {
    Give,
    Get,
}

/// The input mode determines what the TUI renders in the context bar
/// and how keyboard input is handled.
pub enum InputMode {
    /// No human prompt active; spectating AI play or idle.
    Spectating,
    /// Choosing a game action from a horizontal bar.
    ActionBar {
        choices: Vec<PlayerChoice>,
        selected: usize,
    },
    /// Navigating legal positions on the board with arrow keys.
    BoardCursor {
        legal: CursorLegal,
        positions: Vec<CursorTarget>,
        selected: usize,
    },
    /// Building a trade offer with resource keys.
    TradeBuilder {
        give: [u32; 5],
        get: [u32; 5],
        side: TradeSide,
        available: [u32; 5],
        player_id: usize,
    },
    /// Discarding cards with resource keys.
    Discard {
        selected: Vec<Resource>,
        count: usize,
        remaining: [u32; 5],
    },
    /// Picking a single resource with w/b/s/h/o keys.
    ResourcePicker { context: String },
    /// Choosing a steal target with number keys.
    StealTarget {
        targets: Vec<(usize, String)>,
        selected: usize,
    },
    /// Accepting or rejecting a trade with y/n.
    TradeResponse { offer: TradeOffer },
}

/// State for the active game screen.
pub struct PlayingState {
    pub rx: mpsc::UnboundedReceiver<UiEvent>,
    pub state: Option<Arc<GameState>>,
    pub messages: Vec<String>,
    pub chat_messages: Vec<chat_panel::ChatMessage>,
    pub player_names: Vec<String>,
    pub game_over: bool,
    pub game_over_winner: Option<(usize, String)>,
    pub log_scroll: u16,
    pub chat_scroll: u16,
    pub paused: bool,
    /// Whether to show AI reasoning panel (Tab toggle).
    pub show_ai_panel: bool,
    /// Whether to show the help overlay (? toggle).
    pub show_help: bool,
    /// Whether to show the llamafile server log (L toggle).
    pub show_llamafile_log: bool,
    /// Scroll offset for the llamafile log viewer.
    pub llamafile_log_scroll: u16,
    /// Shared buffer of llamafile stderr lines (None if not using llamafile).
    pub llamafile_log: Option<crate::llamafile::process::LogBuffer>,
    /// Current input mode (replaces pending_prompt).
    pub input_mode: InputMode,
    /// Channel to receive human prompts from the engine.
    pub human_prompt_rx: Option<mpsc::UnboundedReceiver<player::tui_human::HumanPrompt>>,
    /// Channel to send human responses back to the engine.
    pub human_response_tx: Option<mpsc::UnboundedSender<HumanResponse>>,
    /// Cached hex grid for board rendering (computed once on first state).
    pub hex_grid: Option<board_view::HexGrid>,
    /// Last dice roll: (die1, die2, total). Displayed persistently in the status bar.
    pub last_roll: Option<(u8, u8, u8)>,
    /// Index of the local human player (None in spectator/all-AI mode).
    pub human_player_index: Option<usize>,
}

impl PlayingState {
    fn new(
        rx: mpsc::UnboundedReceiver<UiEvent>,
        player_names: Vec<String>,
        has_human: bool,
    ) -> Self {
        let start_msg = if has_human {
            "Game started -- your turn will show a prompt".into()
        } else {
            "Game started -- spectator mode".into()
        };
        Self {
            rx,
            state: None,
            messages: vec![
                start_msg,
                "q:quit  Space:pause  j/k:scroll  Tab:AI panel".into(),
            ],
            chat_messages: Vec::new(),
            player_names,
            game_over: false,
            game_over_winner: None,
            log_scroll: 0,
            chat_scroll: 0,
            paused: false,
            show_ai_panel: false,
            show_help: false,
            show_llamafile_log: false,
            llamafile_log_scroll: 0,
            llamafile_log: None,
            input_mode: InputMode::Spectating,
            human_prompt_rx: None,
            human_response_tx: None,
            hex_grid: None,
            last_roll: None,
            human_player_index: None,
        }
    }

    /// Send a response back to the game engine.
    fn send_response(&self, response: HumanResponse) {
        if let Some(ref tx) = self.human_response_tx {
            let _ = tx.send(response);
        }
    }

    /// Send an index response and return to spectating mode.
    fn respond_index(&mut self, idx: usize) {
        self.send_response(HumanResponse::Index(idx));
        self.input_mode = InputMode::Spectating;
    }

    /// Push a message to the game log, capping at MAX_MESSAGES to prevent
    /// unbounded growth. Auto-scrolls to the latest message.
    fn push_message(&mut self, msg: String) {
        const MAX_MESSAGES: usize = 2000;
        self.messages.push(msg);
        if self.messages.len() > MAX_MESSAGES {
            self.messages.drain(..self.messages.len() - MAX_MESSAGES);
        }
        self.log_scroll = self.messages.len().saturating_sub(1) as u16;
    }

    /// Convert an incoming HumanPrompt into the appropriate InputMode.
    fn apply_prompt(&mut self, prompt: player::tui_human::HumanPrompt) {
        self.input_mode = match prompt.kind {
            PromptKind::ChooseAction { choices } => InputMode::ActionBar {
                choices,
                selected: 0,
            },
            PromptKind::PlaceSettlement { legal } => {
                let positions = self.compute_cursor_positions(
                    &legal,
                    |g, v| g.vertex_screen_pos(v).unwrap_or((0, 0)),
                    4,
                );
                InputMode::BoardCursor {
                    legal: CursorLegal::Settlements(legal),
                    positions,
                    selected: 0,
                }
            }
            PromptKind::PlaceRoad { legal } => {
                let positions = self.compute_cursor_positions(
                    &legal,
                    |g, e| g.edge_screen_pos(e).unwrap_or((0, 0)),
                    4,
                );
                InputMode::BoardCursor {
                    legal: CursorLegal::Roads(legal),
                    positions,
                    selected: 0,
                }
            }
            PromptKind::PlaceRobber { legal } => {
                let positions = self.compute_cursor_positions(
                    &legal,
                    |g, h| g.hex_center_pos(h).unwrap_or((0, 0)),
                    8,
                );
                InputMode::BoardCursor {
                    legal: CursorLegal::Hexes(legal),
                    positions,
                    selected: 0,
                }
            }
            PromptKind::ChooseStealTarget { targets } => InputMode::StealTarget {
                targets,
                selected: 0,
            },
            PromptKind::Discard { count, available } => InputMode::Discard {
                selected: Vec::new(),
                count,
                remaining: available,
            },
            PromptKind::ChooseResource { context } => InputMode::ResourcePicker { context },
            PromptKind::ProposeTrade { available } => InputMode::TradeBuilder {
                give: [0; 5],
                get: [0; 5],
                side: TradeSide::Give,
                available,
                player_id: prompt.player_id,
            },
            PromptKind::RespondToTrade { offer } => InputMode::TradeResponse { offer },
        };
    }

    /// Compute screen positions for cursor targets from a position-lookup closure.
    fn compute_cursor_positions<T>(
        &self,
        items: &[T],
        lookup: impl Fn(&board_view::HexGrid, &T) -> (u16, u16),
        fallback_spacing: u16,
    ) -> Vec<CursorTarget> {
        if let Some(ref grid) = self.hex_grid {
            items
                .iter()
                .map(|item| {
                    let (col, row) = lookup(grid, item);
                    CursorTarget {
                        screen_col: col,
                        screen_row: row,
                    }
                })
                .collect()
        } else {
            items
                .iter()
                .enumerate()
                .map(|(i, _)| CursorTarget {
                    screen_col: i as u16 * fallback_spacing,
                    screen_row: 0,
                })
                .collect()
        }
    }

    /// Apply a UI event from the game engine.
    fn handle_game_event(&mut self, ui_event: UiEvent) {
        match ui_event {
            UiEvent::StateUpdate {
                state,
                event,
                message,
            } => {
                if self.hex_grid.is_none() {
                    self.hex_grid = Some(board_view::HexGrid::new());
                }
                self.state = Some(state);
                if let Some(GameEvent::DiceRolled {
                    values: (d1, d2),
                    total,
                    ..
                }) = event
                {
                    self.last_roll = Some((d1, d2, total));
                }
                if !message.is_empty() {
                    self.push_message(message);
                }
            }
            UiEvent::AiReasoning {
                player_id,
                player_name,
                reasoning,
            } => {
                // If the last message is a Reasoning from the same player
                // (streaming in-progress), replace its text with the final
                // clean version.
                let is_continuation = self.chat_messages.last().is_some_and(|last| {
                    last.player_id == player_id
                        && last.kind == chat_panel::ChatMessageKind::Reasoning
                });
                if is_continuation {
                    self.chat_messages.last_mut().unwrap().text = reasoning;
                } else {
                    self.chat_messages.push(chat_panel::ChatMessage {
                        player: player_name,
                        player_id,
                        text: reasoning,
                        kind: chat_panel::ChatMessageKind::Reasoning,
                    });
                }
                const MAX_CHAT: usize = 500;
                if self.chat_messages.len() > MAX_CHAT {
                    self.chat_messages
                        .drain(..self.chat_messages.len() - MAX_CHAT);
                }
                self.chat_scroll = u16::MAX; // Auto-scroll to bottom.
            }
            UiEvent::AiReasoningChunk {
                player_id,
                player_name,
                chunk,
            } => {
                // Append to the last message if it is a Reasoning from the
                // same player, otherwise start a new one.
                let appended = if let Some(last) = self.chat_messages.last_mut() {
                    if last.player_id == player_id
                        && last.kind == chat_panel::ChatMessageKind::Reasoning
                    {
                        last.text.push_str(&chunk);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !appended {
                    self.chat_messages.push(chat_panel::ChatMessage {
                        player: player_name,
                        player_id,
                        text: chunk,
                        kind: chat_panel::ChatMessageKind::Reasoning,
                    });
                }
                self.chat_scroll = u16::MAX; // Auto-scroll to bottom.
            }
            UiEvent::Narration { message } => {
                self.chat_messages.push(chat_panel::ChatMessage {
                    player: String::new(),
                    player_id: usize::MAX,
                    text: message,
                    kind: chat_panel::ChatMessageKind::Narration,
                });
                const MAX_CHAT: usize = 500;
                if self.chat_messages.len() > MAX_CHAT {
                    self.chat_messages
                        .drain(..self.chat_messages.len() - MAX_CHAT);
                }
                self.chat_scroll = u16::MAX; // Auto-scroll to bottom.
            }
            UiEvent::GameOver { winner, message } => {
                let winner_name = self
                    .player_names
                    .get(winner)
                    .cloned()
                    .unwrap_or_else(|| "?".into());
                self.push_message(format!("GAME OVER: {} wins!", winner_name));
                self.push_message(message.clone());
                self.game_over = true;
                self.game_over_winner = Some((winner, winner_name));
            }
        }
    }
}

/// Minimum terminal width for a good experience (from DESIGN.md).
pub const MIN_TERM_WIDTH: u16 = 130;
/// Minimum terminal height for a good experience (from DESIGN.md).
pub const MIN_TERM_HEIGHT: u16 = 60;

/// Top-level app -- holds the current screen and shared config.
pub struct App {
    pub screen: Screen,
    pub personalities: Vec<Personality>,
    /// Running llamafile process, if any. Survives across "Play Again" restarts.
    pub llamafile_process: Option<crate::llamafile::LlamafileProcess>,
    /// Whether to show the terminal-too-small warning popup.
    pub show_size_warning: bool,
    /// Model registry and app settings, loaded from `~/.settl/config.toml`.
    pub config: crate::config::Config,
}

// ── Main Entry Point ───────────────────────────────────────────────────

/// Run the full TUI application (title → menu → game → post-game loop).
pub async fn run_app() -> io::Result<()> {
    let personalities = discover_personalities();

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Install panic hook to restore terminal on panic.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    let size = terminal.size()?;
    let show_size_warning = size.width < MIN_TERM_WIDTH || size.height < MIN_TERM_HEIGHT;

    let mut config = crate::config::load_config();

    // Discover Anthropic models if API key is set.
    if let Some(api_key) = crate::anthropic_api::detect_api_key() {
        match crate::anthropic_api::list_models(&api_key).await {
            Ok(models) => {
                let entries = crate::anthropic_api::to_model_entries(&api_key, &models);
                log::info!("Discovered {} Anthropic model(s)", entries.len());
                config.merge_anthropic_models(entries);
            }
            Err(e) => {
                log::warn!("Failed to fetch Anthropic models: {e}");
            }
        }
    }

    let mut app = App {
        screen: Screen::MainMenu(MainMenuState::new()),
        personalities,
        llamafile_process: None,
        show_size_warning,
        config,
    };

    let result = run_event_loop(&mut terminal, &mut app).await;

    // Restore terminal.
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

/// The main event loop — dispatches draw and input per screen.
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        // Draw.
        terminal.draw(|f| {
            draw_screen(f, &app.screen);
            if app.show_size_warning {
                draw_size_warning(f);
            }
        })?;

        // Poll timeout depends on screen type.
        let timeout = match &app.screen {
            Screen::LlamafileSetup(_) => Duration::from_millis(50), // Fast refresh for progress
            Screen::Playing(_) => Duration::from_millis(50),
            _ => Duration::from_millis(100),
        };

        // Wait for the first event, then drain ALL pending events before
        // the next draw. This prevents keyboard backlog: if 10 arrow keys
        // arrived during a slow protocol rebuild, we process them all and
        // only rebuild once.
        if event::poll(timeout)? {
            loop {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        if key.modifiers.contains(KeyModifiers::CONTROL)
                            && key.code == KeyCode::Char('c')
                        {
                            return Ok(());
                        }
                        let action = handle_input(app, key.code);
                        match action {
                            Action::None => {}
                            Action::Quit => return Ok(()),
                            Action::Transition(screen) => app.screen = screen,
                            Action::StartGame => {
                                if let Screen::NewGame(ref ng) = app.screen {
                                    let model_entry = app.config.models.get(ng.model_index);
                                    let is_api = matches!(
                                        model_entry.map(|e| &e.backend),
                                        Some(crate::config::ModelBackend::Api { .. })
                                    );
                                    if is_api {
                                        // API models don't need llamafile -- launch directly.
                                        let screen = launch_game(
                                            ng,
                                            &app.personalities,
                                            &app.config,
                                            None,
                                            None,
                                        );
                                        app.screen = screen;
                                    } else if let Some(ref process) = app.llamafile_process {
                                        let log_buf = Some(process.log_buffer.clone());
                                        let screen = launch_game(
                                            ng,
                                            &app.personalities,
                                            &app.config,
                                            Some(process.port),
                                            log_buf,
                                        );
                                        app.screen = screen;
                                    } else {
                                        // Check RAM before starting llamafile.
                                        if let Some(required) =
                                            model_entry.and_then(|e| e.min_ram_gb())
                                        {
                                            if let Some(available) =
                                                crate::system_info::total_ram_gb()
                                            {
                                                if available < required {
                                                    if let Screen::NewGame(ref mut ng) = app.screen
                                                    {
                                                        ng.ram_warning =
                                                            Some((required, available));
                                                    }
                                                    continue;
                                                }
                                            }
                                        }
                                        let (status_tx, status_rx) = mpsc::unbounded_channel();
                                        let saved_config = clone_new_game_state(ng);
                                        let (url, filename) = llamafile_url_filename(model_entry);
                                        let (handle, process_rx) =
                                            spawn_llamafile_setup(url, filename, status_tx);
                                        let setup_state = LlamafileSetupState {
                                            status: crate::llamafile::LlamafileStatus::Checking,
                                            status_rx,
                                            saved_config,
                                            task_handle: Some(handle),
                                            process_rx: Some(process_rx),
                                            resume_save: None,
                                        };
                                        app.screen = Screen::LlamafileSetup(setup_state);
                                    }
                                }
                            }
                            Action::ResumeGame => {
                                if let Some(save) = crate::game::save::load_autosave() {
                                    // Find the model from the save in the current config.
                                    let model_entry = resolve_save_model(&save, &app.config);
                                    let is_api = matches!(
                                        model_entry.map(|e| &e.backend),
                                        Some(crate::config::ModelBackend::Api { .. })
                                    );
                                    if is_api {
                                        let screen = resume_game(
                                            save,
                                            &app.personalities,
                                            &app.config,
                                            None,
                                            None,
                                        );
                                        app.screen = screen;
                                    } else if let Some(ref process) = app.llamafile_process {
                                        let log_buf = Some(process.log_buffer.clone());
                                        let screen = resume_game(
                                            save,
                                            &app.personalities,
                                            &app.config,
                                            Some(process.port),
                                            log_buf,
                                        );
                                        app.screen = screen;
                                    } else {
                                        let (status_tx, status_rx) = mpsc::unbounded_channel();
                                        let (url, filename) = llamafile_url_filename(model_entry);
                                        let (handle, process_rx) =
                                            spawn_llamafile_setup(url, filename, status_tx);
                                        let setup_state = LlamafileSetupState {
                                            status: crate::llamafile::LlamafileStatus::Checking,
                                            status_rx,
                                            saved_config: NewGameState::new(
                                                &app.personalities,
                                                &app.config,
                                            ),
                                            task_handle: Some(handle),
                                            process_rx: Some(process_rx),
                                            resume_save: Some(save),
                                        };
                                        app.screen = Screen::LlamafileSetup(setup_state);
                                    }
                                }
                            }
                        }
                    }
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }

        // Poll llamafile setup progress.
        if let Screen::LlamafileSetup(ref mut setup) = app.screen {
            while let Ok(status) = setup.status_rx.try_recv() {
                setup.status = status;
            }
            // When ready, pick up the process and launch the game.
            if let crate::llamafile::LlamafileStatus::Ready(port) = &setup.status {
                let port = *port;
                // Take the process from the oneshot receiver.
                if let Some(mut rx) = setup.process_rx.take() {
                    if let Ok(process) = rx.try_recv() {
                        app.llamafile_process = Some(process);
                    }
                }
                // Move config out of the setup state and launch/resume.
                if let Screen::LlamafileSetup(setup) =
                    std::mem::replace(&mut app.screen, Screen::MainMenu(MainMenuState::new()))
                {
                    let log_buf = app.llamafile_process.as_ref().map(|p| p.log_buffer.clone());
                    let screen = if let Some(save) = setup.resume_save {
                        resume_game(save, &app.personalities, &app.config, Some(port), log_buf)
                    } else {
                        launch_game(
                            &setup.saved_config,
                            &app.personalities,
                            &app.config,
                            Some(port),
                            log_buf,
                        )
                    };
                    app.screen = screen;
                }
            }
        }

        // Drain game events for Playing screen.
        if let Screen::Playing(ref mut ps) = app.screen {
            if !ps.paused {
                while let Ok(ui_event) = ps.rx.try_recv() {
                    ps.handle_game_event(ui_event);
                }
            }

            // Check for incoming human prompts.
            if matches!(ps.input_mode, InputMode::Spectating) {
                if let Some(ref mut prompt_rx) = ps.human_prompt_rx {
                    if let Ok(prompt) = prompt_rx.try_recv() {
                        ps.apply_prompt(prompt);
                    }
                }
            }
        }
    }
}

// ── Drawing Dispatch ───────────────────────────────────────────────────

fn draw_screen(f: &mut Frame, screen: &Screen) {
    match screen {
        Screen::MainMenu(state) => screens::draw_main_menu(f, state),
        Screen::NewGame(state) => screens::draw_new_game(f, state),
        Screen::About(_) => screens::draw_about(f),
        Screen::Docs(state) => screens::draw_docs(f, state),
        Screen::Settings(state) => screens::draw_settings(f, state),
        Screen::Personalities(state) => screens::draw_personalities(f, state),
        Screen::LlamafileSetup(state) => screens::draw_llamafile_setup(f, state),
        Screen::Playing(ps) => layout::draw_playing(f, ps),
        Screen::PostGame(state) => screens::draw_post_game(f, state),
    }
}

// ── Size Warning Overlay ──────────────────────────────────────────────

/// Draw a centered warning popup when the terminal is smaller than the minimum.
fn draw_size_warning(f: &mut Frame) {
    let area = f.area();
    let popup_w = 52u16.min(area.width.saturating_sub(2));
    let popup_h = 9u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let overlay = Rect::new(x, y, popup_w, popup_h);

    f.render_widget(Clear, overlay);

    let block = Block::default()
        .title(" Terminal Too Small ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Current size: {}x{}", area.width, area.height),
            Style::default().fg(Color::Red).bold(),
        )),
        Line::from(Span::styled(
            format!("  Minimum size: {}x{}", MIN_TERM_WIDTH, MIN_TERM_HEIGHT),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Resize your terminal for the best experience.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to continue...",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let para = Paragraph::new(text).wrap(Wrap { trim: false });
    f.render_widget(para, inner);
}

// ── Input Dispatch ─────────────────────────────────────────────────────

#[allow(clippy::large_enum_variant)]
enum Action {
    None,
    Quit,
    Transition(Screen),
    StartGame,
    ResumeGame,
}

fn handle_input(app: &mut App, key: KeyCode) -> Action {
    if app.show_size_warning {
        app.show_size_warning = false;
        return Action::None;
    }
    match &mut app.screen {
        Screen::MainMenu(state) => {
            let items = state.menu_items();
            match key {
                KeyCode::Up | KeyCode::Char('k') => {
                    state.selected = state.selected.checked_sub(1).unwrap_or(items.len() - 1);
                    Action::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.selected = (state.selected + 1) % items.len();
                    Action::None
                }
                KeyCode::Enter => {
                    let selected_item = items[state.selected];
                    match selected_item {
                        "Continue" => Action::ResumeGame,
                        "New Game" => Action::Transition(Screen::NewGame(NewGameState::new(
                            &app.personalities,
                            &app.config,
                        ))),
                        "Personalities" => Action::Transition(Screen::Personalities(
                            PersonalitiesState::new(&app.personalities),
                        )),
                        "Settings" => Action::Transition(Screen::Settings(
                            SettingsState::from_config(&app.config),
                        )),
                        "Docs" => Action::Transition(Screen::Docs(DocsState::new())),
                        "About" => Action::Transition(Screen::About(AboutState)),
                        "Quit" => Action::Quit,
                        _ => Action::None,
                    }
                }
                KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
                _ => Action::None,
            }
        }

        Screen::About(_) => match key {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                Action::Transition(Screen::MainMenu(MainMenuState::new()))
            }
            _ => Action::None,
        },

        Screen::Docs(state) => match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                Action::Transition(Screen::MainMenu(MainMenuState::new()))
            }
            KeyCode::Up => {
                if state.page_index > 0 {
                    state.page_index -= 1;
                    state.scroll = 0;
                }
                Action::None
            }
            KeyCode::Down => {
                if state.page_index + 1 < state.page_count {
                    state.page_index += 1;
                    state.scroll = 0;
                }
                Action::None
            }
            KeyCode::Char('j') => {
                state.scroll = state.scroll.saturating_add(1);
                Action::None
            }
            KeyCode::Char('k') => {
                state.scroll = state.scroll.saturating_sub(1);
                Action::None
            }
            KeyCode::PageDown => {
                state.scroll = state.scroll.saturating_add(20);
                Action::None
            }
            KeyCode::PageUp => {
                state.scroll = state.scroll.saturating_sub(20);
                Action::None
            }
            _ => Action::None,
        },

        Screen::Settings(state) => handle_settings_input(state, key, &mut app.config),

        Screen::Personalities(state) => {
            let action = handle_personalities_input(state, key);
            // When leaving the screen, refresh discovered personalities.
            if matches!(action, Action::Transition(Screen::MainMenu(_))) {
                app.personalities = discover_personalities();
            }
            action
        }

        Screen::NewGame(state) => {
            // RAM warning popup intercepts input when visible.
            if state.ram_warning.is_some() {
                match key {
                    KeyCode::Enter => {
                        // User chose to proceed anyway.
                        state.ram_warning = None;
                        return Action::StartGame;
                    }
                    _ => {
                        // Any other key dismisses the warning.
                        state.ram_warning = None;
                        return Action::None;
                    }
                }
            }
            match key {
                KeyCode::Esc => Action::Transition(Screen::MainMenu(MainMenuState::new())),
                KeyCode::Up | KeyCode::Char('k') => {
                    move_new_game_focus_up(state);
                    Action::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    move_new_game_focus_down(state);
                    Action::None
                }
                KeyCode::Left => {
                    cycle_new_game_value(state, false);
                    Action::None
                }
                KeyCode::Right => {
                    cycle_new_game_value(state, true);
                    Action::None
                }
                KeyCode::Enter => Action::StartGame,
                _ => Action::None,
            }
        }

        Screen::LlamafileSetup(_) => match key {
            KeyCode::Esc => {
                // Cancel setup: abort the background task.
                if let Screen::LlamafileSetup(mut setup) =
                    std::mem::replace(&mut app.screen, Screen::MainMenu(MainMenuState::new()))
                {
                    if let Some(handle) = setup.task_handle.take() {
                        handle.abort();
                    }
                    // If resuming, go back to main menu; otherwise back to NewGame.
                    if setup.resume_save.is_some() {
                        Action::Transition(Screen::MainMenu(MainMenuState::new()))
                    } else {
                        Action::Transition(Screen::NewGame(setup.saved_config))
                    }
                } else {
                    Action::Transition(Screen::NewGame(NewGameState::new(
                        &app.personalities,
                        &app.config,
                    )))
                }
            }
            _ => Action::None,
        },

        Screen::Playing(ps) => {
            // Help overlay intercepts all input when visible.
            if ps.show_help {
                ps.show_help = false;
                return Action::None;
            }

            // Global keys (always active regardless of mode).
            match key {
                KeyCode::Char('?') => {
                    ps.show_help = true;
                    return Action::None;
                }
                KeyCode::Char('q') => {
                    if matches!(ps.input_mode, InputMode::Spectating) {
                        if ps.game_over {
                            let post = build_post_game(ps);
                            return Action::Transition(Screen::PostGame(post));
                        } else {
                            return Action::Transition(Screen::MainMenu(MainMenuState::new()));
                        }
                    }
                }
                // Tab toggles AI panel ONLY when not in TradeBuilder (where Tab
                // switches give/get sides).
                KeyCode::Tab if !matches!(ps.input_mode, InputMode::TradeBuilder { .. }) => {
                    ps.show_ai_panel = !ps.show_ai_panel;
                    return Action::None;
                }
                // L toggles llamafile server log (only when llamafile is active).
                KeyCode::Char('L') if ps.llamafile_log.is_some() => {
                    ps.show_llamafile_log = !ps.show_llamafile_log;
                    return Action::None;
                }
                _ => {}
            }

            // Mode-specific input handling.
            match &mut ps.input_mode {
                InputMode::Spectating => {
                    match key {
                        KeyCode::Esc => {
                            if ps.game_over {
                                let post = build_post_game(ps);
                                return Action::Transition(Screen::PostGame(post));
                            } else {
                                return Action::Transition(Screen::MainMenu(MainMenuState::new()));
                            }
                        }
                        KeyCode::Enter if ps.game_over => {
                            let post = build_post_game(ps);
                            return Action::Transition(Screen::PostGame(post));
                        }
                        KeyCode::Char(' ') => ps.paused = !ps.paused,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if ps.show_llamafile_log {
                                ps.llamafile_log_scroll = ps.llamafile_log_scroll.saturating_sub(1);
                            } else if ps.show_ai_panel {
                                ps.chat_scroll = ps.chat_scroll.saturating_sub(1);
                            } else {
                                ps.log_scroll = ps.log_scroll.saturating_sub(1);
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if ps.show_llamafile_log {
                                ps.llamafile_log_scroll = ps.llamafile_log_scroll.saturating_add(1);
                            } else if ps.show_ai_panel {
                                ps.chat_scroll = ps.chat_scroll.saturating_add(1);
                            } else {
                                ps.log_scroll = ps.log_scroll.saturating_add(1);
                            }
                        }
                        _ => {}
                    }
                    Action::None
                }

                InputMode::ActionBar { choices, selected } => {
                    match key {
                        KeyCode::Left | KeyCode::Up | KeyCode::Char('k') => {
                            *selected = selected.saturating_sub(1);
                        }
                        KeyCode::Right | KeyCode::Down | KeyCode::Char('j') => {
                            if *selected + 1 < choices.len() {
                                *selected += 1;
                            }
                        }
                        KeyCode::Enter => {
                            let idx = *selected;
                            ps.respond_index(idx);
                        }
                        KeyCode::Char(ch) => {
                            if let Some(idx) =
                                choices.iter().position(|c| c.shortcut_key() == Some(ch))
                            {
                                ps.respond_index(idx);
                            }
                        }
                        KeyCode::Esc => {
                            if let Some(idx) = choices.iter().position(|c| c.is_end_turn()) {
                                ps.respond_index(idx);
                            }
                        }
                        _ => {}
                    }
                    Action::None
                }

                InputMode::BoardCursor {
                    positions,
                    selected,
                    ..
                } => {
                    let len = positions.len();
                    match key {
                        KeyCode::Left
                        | KeyCode::Right
                        | KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::Char('h')
                        | KeyCode::Char('j')
                        | KeyCode::Char('k')
                        | KeyCode::Char('l') => {
                            if len > 0 {
                                let cur = &positions[*selected];
                                let cur_col = cur.screen_col as i32;
                                let cur_row = cur.screen_row as i32;
                                if let Some(next) = find_nearest_in_direction(
                                    cur_col, cur_row, key, positions, *selected,
                                ) {
                                    *selected = next;
                                }
                            }
                        }
                        KeyCode::Char('n') => {
                            if len > 0 {
                                *selected = (*selected + 1) % len;
                            }
                        }
                        KeyCode::Char('p') => {
                            if len > 0 {
                                *selected = selected.checked_sub(1).unwrap_or(len - 1);
                            }
                        }
                        KeyCode::Enter => {
                            if len > 0 {
                                let idx = *selected;
                                ps.respond_index(idx);
                            }
                        }
                        // Esc does nothing in BoardCursor -- placement is mandatory
                        // (setup settlements, robber, Road Building). Only Enter confirms.
                        _ => {}
                    }
                    Action::None
                }

                InputMode::TradeBuilder {
                    give,
                    get,
                    side,
                    available,
                    player_id,
                } => {
                    if let Some(idx) = resource_key_index(key) {
                        match side {
                            TradeSide::Give => {
                                if give[idx] < available[idx] {
                                    give[idx] += 1;
                                }
                            }
                            TradeSide::Get => {
                                get[idx] += 1;
                            }
                        }
                    }
                    match key {
                        KeyCode::Tab => {
                            *side = match side {
                                TradeSide::Give => TradeSide::Get,
                                TradeSide::Get => TradeSide::Give,
                            };
                        }
                        KeyCode::Backspace => {
                            let arr = match side {
                                TradeSide::Give => give,
                                TradeSide::Get => get,
                            };
                            // Remove last-added resource (highest index with count > 0)
                            for i in (0..5).rev() {
                                if arr[i] > 0 {
                                    arr[i] -= 1;
                                    break;
                                }
                            }
                        }
                        KeyCode::Enter => {
                            let has_give = give.iter().any(|&g| g > 0);
                            let has_get = get.iter().any(|&g| g > 0);
                            if has_give && has_get {
                                let resources = Resource::all();
                                let offering: Vec<(Resource, u32)> = give
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, &c)| c > 0)
                                    .map(|(i, &c)| (resources[i], c))
                                    .collect();
                                let requesting: Vec<(Resource, u32)> = get
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, &c)| c > 0)
                                    .map(|(i, &c)| (resources[i], c))
                                    .collect();
                                let offer = TradeOffer {
                                    from: *player_id,
                                    offering,
                                    requesting,
                                    message: String::new(),
                                };
                                ps.send_response(HumanResponse::Trade(Some(offer)));
                                ps.input_mode = InputMode::Spectating;
                            }
                        }
                        KeyCode::Esc => {
                            ps.send_response(HumanResponse::Trade(None));
                            ps.input_mode = InputMode::Spectating;
                        }
                        _ => {}
                    }
                    Action::None
                }

                InputMode::Discard {
                    selected: sel_resources,
                    count,
                    remaining,
                    ..
                } => {
                    let resources = Resource::all();
                    if let Some(idx) = resource_key_index(key) {
                        if sel_resources.len() < *count && remaining[idx] > 0 {
                            remaining[idx] -= 1;
                            sel_resources.push(resources[idx]);
                        }
                    }
                    match key {
                        KeyCode::Backspace => {
                            if let Some(last) = sel_resources.pop() {
                                let idx = resources.iter().position(|&r| r == last).unwrap_or(0);
                                remaining[idx] += 1;
                            }
                        }
                        KeyCode::Enter => {
                            if sel_resources.len() == *count {
                                let result = sel_resources.clone();
                                ps.send_response(HumanResponse::Resources(result));
                                ps.input_mode = InputMode::Spectating;
                            }
                        }
                        KeyCode::Esc => {
                            // Auto-complete: fill remaining discards with first available resources.
                            while sel_resources.len() < *count {
                                let mut filled = false;
                                for i in 0..5 {
                                    if remaining[i] > 0 {
                                        remaining[i] -= 1;
                                        sel_resources.push(resources[i]);
                                        filled = true;
                                        break;
                                    }
                                }
                                if !filled {
                                    break;
                                }
                            }
                            let result = sel_resources.clone();
                            ps.send_response(HumanResponse::Resources(result));
                            ps.input_mode = InputMode::Spectating;
                        }
                        _ => {}
                    }
                    Action::None
                }

                InputMode::ResourcePicker { .. } => {
                    let idx = resource_key_index(key).or(match key {
                        KeyCode::Esc => Some(0), // Default to Wood on Esc
                        _ => None,
                    });
                    if let Some(i) = idx {
                        ps.respond_index(i);
                    }
                    Action::None
                }

                InputMode::StealTarget { targets, selected } => {
                    match key {
                        KeyCode::Up | KeyCode::Char('k') => {
                            *selected = selected.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if *selected + 1 < targets.len() {
                                *selected += 1;
                            }
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            let num = c.to_digit(10).unwrap_or(0) as usize;
                            let player_id = num.saturating_sub(1);
                            if let Some(idx) = targets.iter().position(|(id, _)| *id == player_id) {
                                ps.respond_index(idx);
                            }
                        }
                        KeyCode::Enter | KeyCode::Esc => {
                            if !targets.is_empty() {
                                let idx = *selected;
                                ps.respond_index(idx);
                            }
                        }
                        _ => {}
                    }
                    Action::None
                }

                InputMode::TradeResponse { .. } => {
                    match key {
                        KeyCode::Char('y') | KeyCode::Enter => {
                            ps.send_response(HumanResponse::TradeAnswer(true));
                            ps.input_mode = InputMode::Spectating;
                        }
                        KeyCode::Char('n') | KeyCode::Esc => {
                            ps.send_response(HumanResponse::TradeAnswer(false));
                            ps.input_mode = InputMode::Spectating;
                        }
                        _ => {}
                    }
                    Action::None
                }
            }
        }

        Screen::PostGame(state) => match key {
            KeyCode::Up | KeyCode::Char('k') => {
                state.selected = state
                    .selected
                    .checked_sub(1)
                    .unwrap_or(POST_GAME_ITEMS.len() - 1);
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.selected = (state.selected + 1) % POST_GAME_ITEMS.len();
                Action::None
            }
            KeyCode::Enter => match POST_GAME_ITEMS[state.selected] {
                "Play Again" => Action::Transition(Screen::NewGame(NewGameState::new(
                    &app.personalities,
                    &app.config,
                ))),
                "Main Menu" => Action::Transition(Screen::MainMenu(MainMenuState::new())),
                "Quit" => Action::Quit,
                _ => Action::None,
            },
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            _ => Action::None,
        },
    }
}

// ── Board Cursor Navigation ───────────────────────────────────────────

/// Find the nearest cursor position in the pressed arrow direction.
fn find_nearest_in_direction(
    cur_col: i32,
    cur_row: i32,
    key: KeyCode,
    positions: &[CursorTarget],
    current: usize,
) -> Option<usize> {
    let mut best: Option<(usize, i64)> = None;
    for (i, pos) in positions.iter().enumerate() {
        if i == current {
            continue;
        }
        let dx = pos.screen_col as i32 - cur_col;
        let dy = pos.screen_row as i32 - cur_row;

        // Check if position is in the direction of the pressed key (90-degree cone).
        let in_direction = match key {
            KeyCode::Up | KeyCode::Char('k') => dy < 0 && dy.abs() >= dx.abs(),
            KeyCode::Down | KeyCode::Char('j') => dy > 0 && dy.abs() >= dx.abs(),
            KeyCode::Left | KeyCode::Char('h') => dx < 0 && dx.abs() >= dy.abs(),
            KeyCode::Right | KeyCode::Char('l') => dx > 0 && dx.abs() >= dy.abs(),
            _ => false,
        };

        if in_direction {
            let dist = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
            if best.is_none_or(|(_, d)| dist < d) {
                best = Some((i, dist));
            }
        }
    }
    best.map(|(i, _)| i)
}

// ── New Game Input Helpers ─────────────────────────────────────────────

/// Ordered list of focusable rows, skipping player 4 when in 3-player mode.
fn focusable_rows(state: &NewGameState) -> Vec<NewGameFocus> {
    let mut rows = vec![NewGameFocus::StartButton, NewGameFocus::PlayerCount];
    // AI player rows: indices 1, 2, 3 (skip index 3 in 3-player mode).
    for i in 1..4 {
        if i == 3 && !state.four_players {
            continue;
        }
        rows.push(NewGameFocus::Player { row: i });
    }
    rows.push(NewGameFocus::FriendlyRobber);
    rows.push(NewGameFocus::BoardLayout);
    rows.push(NewGameFocus::AiModel);
    rows.push(NewGameFocus::ReasoningEffort);
    rows
}

fn move_new_game_focus_up(state: &mut NewGameState) {
    let rows = focusable_rows(state);
    if let Some(pos) = rows.iter().position(|r| *r == state.focus) {
        if pos > 0 {
            state.focus = rows[pos - 1];
        }
    }
}

fn move_new_game_focus_down(state: &mut NewGameState) {
    let rows = focusable_rows(state);
    if let Some(pos) = rows.iter().position(|r| *r == state.focus) {
        if pos + 1 < rows.len() {
            state.focus = rows[pos + 1];
        }
    }
}

fn cycle_new_game_value(state: &mut NewGameState, forward: bool) {
    match state.focus {
        NewGameFocus::PlayerCount => {
            state.four_players = !state.four_players;
        }
        NewGameFocus::Player { row } => {
            let player = &mut state.players[row];
            if player.kind == PlayerKind::Llamafile {
                let n = state.personality_names.len();
                player.personality_index = if forward {
                    (player.personality_index + 1) % n
                } else {
                    player.personality_index.checked_sub(1).unwrap_or(n - 1)
                };
            }
        }
        NewGameFocus::FriendlyRobber => {
            state.friendly_robber = !state.friendly_robber;
        }
        NewGameFocus::BoardLayout => {
            state.random_board = !state.random_board;
        }
        NewGameFocus::AiModel => {
            if !state.model_names.is_empty() {
                state.model_index = if forward {
                    (state.model_index + 1) % state.model_names.len()
                } else {
                    state
                        .model_index
                        .checked_sub(1)
                        .unwrap_or(state.model_names.len() - 1)
                };
            }
        }
        NewGameFocus::ReasoningEffort => {
            let n = crate::config::EFFORT_LEVELS.len();
            state.effort_index = if forward {
                (state.effort_index + 1) % n
            } else {
                state.effort_index.checked_sub(1).unwrap_or(n - 1)
            };
            // Update all AI player configs to match.
            for pc in &mut state.players {
                pc.effort_index = state.effort_index;
            }
        }
        NewGameFocus::StartButton => {}
    }
}

// ── Game Launch ────────────────────────────────────────────────────────

fn launch_game(
    ng: &NewGameState,
    discovered_personalities: &[Personality],
    config: &crate::config::Config,
    llamafile_port: Option<u16>,
    llamafile_log: Option<crate::llamafile::process::LogBuffer>,
) -> Screen {
    use player::tui_human::{HumanInputChannel, TuiHumanPlayer};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let board = if ng.random_board {
        Board::generate(&mut rand::rng())
    } else {
        Board::default_board()
    };

    let mut state = GameState::new(board, ng.num_players());
    state.friendly_robber = ng.friendly_robber;

    // Build players.
    let built_in_personalities = [
        Personality::default_personality(),
        Personality::aggressive(),
        Personality::grudge_holder(),
        Personality::builder(),
        Personality::chaos_agent(),
    ];

    // Create human input channels if any human players exist.
    let active_players = &ng.players[..ng.num_players()];
    let has_human = active_players.iter().any(|p| p.kind == PlayerKind::Human);
    let human_channels: Option<(
        Arc<HumanInputChannel>,
        mpsc::UnboundedReceiver<player::tui_human::HumanPrompt>,
        mpsc::UnboundedSender<HumanResponse>,
    )> = if has_human {
        let (prompt_tx, prompt_rx) = mpsc::unbounded_channel();
        let (response_tx, response_rx) = mpsc::unbounded_channel::<HumanResponse>();
        let channel = Arc::new(HumanInputChannel {
            prompt_tx,
            response_rx: Mutex::new(response_rx),
        });
        Some((channel, prompt_rx, response_tx))
    } else {
        None
    };

    // Build a shared AI client from the selected model entry.
    let ai_client = build_ai_client(config, ng.model_index, llamafile_port);

    // Create UI event channel.
    let (tx, rx) = mpsc::unbounded_channel();

    let mut players: Vec<Box<dyn player::Player>> = Vec::new();
    for (slot_id, pc) in active_players.iter().enumerate() {
        match pc.kind {
            PlayerKind::Llamafile => {
                let personality = if pc.personality_index < built_in_personalities.len() {
                    built_in_personalities[pc.personality_index].clone()
                } else {
                    let disc_idx = pc.personality_index - built_in_personalities.len();
                    discovered_personalities
                        .get(disc_idx)
                        .cloned()
                        .unwrap_or_default()
                };
                let client = ai_client
                    .clone()
                    .expect("AI client should exist when AI players are used");
                let mut llm = player::llm_player::LlmPlayer::new(
                    pc.name.clone(),
                    client,
                    personality,
                    Some(slot_id),
                );

                // Set reasoning effort from the player config.
                if let Some(&level) = crate::config::EFFORT_LEVELS.get(pc.effort_index) {
                    llm.set_effort(level.to_string());
                }

                // Enable forced tool reasoning for small models (e.g. 1.7B).
                if let Some(entry) = config.models.get(ng.model_index) {
                    if entry.needs_forced_reasoning() {
                        llm.set_force_tool_reasoning(true);
                    }
                }

                // Set up streaming reasoning bridge: LlmPlayer -> String chunks -> UiEvent.
                let (reasoning_tx, mut reasoning_rx) = mpsc::unbounded_channel::<String>();
                llm.set_reasoning_sender(reasoning_tx);
                let ui_tx_clone = tx.clone();
                let player_name_clone = pc.name.clone();
                tokio::spawn(async move {
                    while let Some(chunk) = reasoning_rx.recv().await {
                        let _ = ui_tx_clone.send(UiEvent::AiReasoningChunk {
                            player_id: slot_id,
                            player_name: player_name_clone.clone(),
                            chunk,
                        });
                    }
                });

                players.push(Box::new(llm) as Box<dyn player::Player>);
            }
            PlayerKind::Human => {
                let channel = human_channels.as_ref().unwrap().0.clone();
                players.push(Box::new(TuiHumanPlayer::new(pc.name.clone(), channel))
                    as Box<dyn player::Player>);
            }
        }
    }

    let player_names: Vec<String> = active_players.iter().map(|p| p.name.clone()).collect();

    let save_configs: Vec<crate::game::save::SavedPlayerConfig> = active_players
        .iter()
        .map(|pc| crate::game::save::SavedPlayerConfig {
            name: pc.name.clone(),
            is_human: pc.kind == PlayerKind::Human,
            personality_index: pc.personality_index,
        })
        .collect();
    let model_name = config
        .models
        .get(ng.model_index)
        .map(|e| e.name.clone())
        .unwrap_or_default();

    // Spawn game engine.
    let hooks = config.hooks.clone();
    tokio::spawn(async move {
        let mut orchestrator = GameOrchestrator::new(state, players);
        orchestrator.ui_tx = Some(tx);
        orchestrator.hooks = hooks;
        orchestrator.player_configs = save_configs;
        orchestrator.model_name = model_name;

        orchestrator.run().await
    });

    let human_player_index = active_players
        .iter()
        .position(|p| p.kind == PlayerKind::Human);

    let mut ps = PlayingState::new(rx, player_names, has_human);
    ps.llamafile_log = llamafile_log;
    ps.human_player_index = human_player_index;
    if let Some((_, prompt_rx, response_tx)) = human_channels {
        ps.human_prompt_rx = Some(prompt_rx);
        ps.human_response_tx = Some(response_tx);
    }
    Screen::Playing(ps)
}

/// Resume a saved game, recreating players from the save file.
fn resume_game(
    save: crate::game::save::SaveFile,
    discovered_personalities: &[Personality],
    config: &crate::config::Config,
    llamafile_port: Option<u16>,
    llamafile_log: Option<crate::llamafile::process::LogBuffer>,
) -> Screen {
    use player::tui_human::{HumanInputChannel, TuiHumanPlayer};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let built_in_personalities = [
        Personality::default_personality(),
        Personality::aggressive(),
        Personality::grudge_holder(),
        Personality::builder(),
        Personality::chaos_agent(),
    ];

    let has_human = save.player_configs.iter().any(|p| p.is_human);
    let human_channels: Option<(
        Arc<HumanInputChannel>,
        mpsc::UnboundedReceiver<player::tui_human::HumanPrompt>,
        mpsc::UnboundedSender<HumanResponse>,
    )> = if has_human {
        let (prompt_tx, prompt_rx) = mpsc::unbounded_channel();
        let (response_tx, response_rx) = mpsc::unbounded_channel::<HumanResponse>();
        let channel = Arc::new(HumanInputChannel {
            prompt_tx,
            response_rx: Mutex::new(response_rx),
        });
        Some((channel, prompt_rx, response_tx))
    } else {
        None
    };

    // Find the model from the save in the current config.
    let model_index = resolve_save_model_index(&save, config);
    let ai_client = build_ai_client(config, model_index, llamafile_port);

    let (tx, rx) = mpsc::unbounded_channel();

    let mut players: Vec<Box<dyn player::Player>> = Vec::new();
    for (slot_id, pc) in save.player_configs.iter().enumerate() {
        if pc.is_human {
            let channel = human_channels.as_ref().unwrap().0.clone();
            players
                .push(Box::new(TuiHumanPlayer::new(pc.name.clone(), channel))
                    as Box<dyn player::Player>);
        } else {
            let personality = if pc.personality_index < built_in_personalities.len() {
                built_in_personalities[pc.personality_index].clone()
            } else {
                let disc_idx = pc.personality_index - built_in_personalities.len();
                discovered_personalities
                    .get(disc_idx)
                    .cloned()
                    .unwrap_or_default()
            };
            let client = ai_client
                .clone()
                .expect("AI client should exist when AI players are used");
            let mut llm = player::llm_player::LlmPlayer::new(
                pc.name.clone(),
                client,
                personality,
                Some(slot_id),
            );

            // Set reasoning effort from config default.
            llm.set_effort(config.default_effort.clone());

            // Enable forced tool reasoning for small models (e.g. 1.7B).
            if save.model_name.contains("1.7B") {
                llm.set_force_tool_reasoning(true);
            }

            let (reasoning_tx, mut reasoning_rx) = mpsc::unbounded_channel::<String>();
            llm.set_reasoning_sender(reasoning_tx);
            let ui_tx_clone = tx.clone();
            let player_name_clone = pc.name.clone();
            tokio::spawn(async move {
                while let Some(chunk) = reasoning_rx.recv().await {
                    let _ = ui_tx_clone.send(UiEvent::AiReasoningChunk {
                        player_id: slot_id,
                        player_name: player_name_clone.clone(),
                        chunk,
                    });
                }
            });

            players.push(Box::new(llm) as Box<dyn player::Player>);
        }
    }

    let player_names = save.player_names.clone();
    let save_configs = save.player_configs.clone();
    let model_name = save.model_name.clone();
    let events = save.events;
    let state = save.game_state;

    let hooks = config.hooks.clone();
    tokio::spawn(async move {
        let mut orchestrator = GameOrchestrator::new(state, players);
        orchestrator.ui_tx = Some(tx);
        orchestrator.hooks = hooks;
        orchestrator.player_configs = save_configs;
        orchestrator.model_name = model_name;
        orchestrator.events = events;

        orchestrator.run().await
    });

    let human_player_index = save.player_configs.iter().position(|p| p.is_human);

    let mut ps = PlayingState::new(rx, player_names, has_human);
    ps.llamafile_log = llamafile_log;
    ps.human_player_index = human_player_index;
    if let Some((_, prompt_rx, response_tx)) = human_channels {
        ps.human_prompt_rx = Some(prompt_rx);
        ps.human_response_tx = Some(response_tx);
    }
    Screen::Playing(ps)
}

fn build_post_game(ps: &PlayingState) -> PostGameState {
    let (winner_index, winner_name) = ps.game_over_winner.clone().unwrap_or((0, "Unknown".into()));

    let scores: Vec<(String, u8)> = if let Some(ref state) = ps.state {
        ps.player_names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), state.victory_points(i)))
            .collect()
    } else {
        ps.player_names
            .iter()
            .map(|name| (name.clone(), 0u8))
            .collect()
    };

    PostGameState {
        winner_name,
        winner_index,
        scores,
        selected: 0,
    }
}

// ── Llamafile Helpers ──────────────────────────────────────────────────

/// Spawn a background task that downloads (if needed) and starts the llamafile.
///
/// Returns the `JoinHandle` and a oneshot receiver for the process.
fn spawn_llamafile_setup(
    url: String,
    filename: String,
    status_tx: mpsc::UnboundedSender<crate::llamafile::LlamafileStatus>,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::oneshot::Receiver<crate::llamafile::LlamafileProcess>,
) {
    let (process_tx, process_rx) = tokio::sync::oneshot::channel();

    let handle = tokio::spawn(async move {
        match crate::llamafile::ensure_llamafile_custom(&url, &filename, status_tx.clone()).await {
            Ok(path) => {
                let _ = status_tx.send(crate::llamafile::LlamafileStatus::Starting);
                let _ = status_tx.send(crate::llamafile::LlamafileStatus::WaitingForReady);
                match crate::llamafile::LlamafileProcess::start_with_port_scan(&path).await {
                    Ok(process) => {
                        let port = process.port;
                        if process_tx.send(process).is_ok() {
                            let _ = status_tx.send(crate::llamafile::LlamafileStatus::Ready(port));
                        }
                    }
                    Err(e) => {
                        let _ = status_tx.send(crate::llamafile::LlamafileStatus::Error(e));
                    }
                }
            }
            Err(e) => {
                let _ = status_tx.send(crate::llamafile::LlamafileStatus::Error(e));
            }
        }
    });

    (handle, process_rx)
}

/// Extract the llamafile URL and filename from a model entry, with defaults.
fn llamafile_url_filename(entry: Option<&crate::config::ModelEntry>) -> (String, String) {
    match entry.map(|e| &e.backend) {
        Some(crate::config::ModelBackend::Llamafile { url, filename }) => {
            (url.clone(), filename.clone())
        }
        _ => {
            // Fallback to default Bonsai 1.7B.
            let default = crate::llamafile::LlamafileModel::default();
            (default.url().to_string(), default.filename().to_string())
        }
    }
}

/// Build an AI client from the selected model entry in the config.
fn build_ai_client(
    config: &crate::config::Config,
    model_index: usize,
    llamafile_port: Option<u16>,
) -> Option<Arc<player::anthropic_client::AnthropicClient>> {
    let entry = config.models.get(model_index)?;
    match &entry.backend {
        crate::config::ModelBackend::Llamafile { .. } => llamafile_port.map(|port| {
            player::anthropic_client::AnthropicClient::new(
                format!("http://127.0.0.1:{}", port),
                "no-key",
                player::llm_player::LLAMAFILE_MODEL,
            )
        }),
        crate::config::ModelBackend::Api {
            base_url,
            api_key,
            model,
        } => Some(player::anthropic_client::AnthropicClient::new(
            base_url, api_key, model,
        )),
    }
}

/// Look up a save file's model in the current config, returning the entry.
fn resolve_save_model<'a>(
    save: &crate::game::save::SaveFile,
    config: &'a crate::config::Config,
) -> Option<&'a crate::config::ModelEntry> {
    let idx = resolve_save_model_index(save, config);
    config.models.get(idx)
}

/// Look up a save file's model in the current config, returning the index.
fn resolve_save_model_index(
    save: &crate::game::save::SaveFile,
    config: &crate::config::Config,
) -> usize {
    // Try to find by model_name.
    if !save.model_name.is_empty() {
        if let Some(idx) = config.models.iter().position(|m| m.name == save.model_name) {
            return idx;
        }
    }
    // Fallback: try to match by LlamafileModel (backward compat with old saves).
    if let Some(ref lm) = save.llamafile_model {
        let filename = lm.filename();
        if let Some(idx) = config.models.iter().position(|m| {
            matches!(&m.backend, crate::config::ModelBackend::Llamafile { filename: f, .. } if f == filename)
        }) {
            return idx;
        }
    }
    // Default to first entry.
    0
}

/// Clone a `NewGameState` for saving across screen transitions.
fn clone_new_game_state(ng: &NewGameState) -> NewGameState {
    NewGameState {
        players: ng.players.clone(),
        focus: ng.focus,
        personality_names: ng.personality_names.clone(),
        four_players: ng.four_players,
        friendly_robber: ng.friendly_robber,
        random_board: ng.random_board,
        model_index: ng.model_index,
        model_names: ng.model_names.clone(),
        effort_index: ng.effort_index,
        ram_warning: None,
    }
}

// ── Settings Input ────────────────────────────────────────────────────

fn handle_settings_input(
    state: &mut SettingsState,
    key: KeyCode,
    config: &mut crate::config::Config,
) -> Action {
    use crate::config::{ModelBackend, ModelEntry};
    use screens::{ModelField, SettingsFocus};

    match state.focus {
        SettingsFocus::ModelList => match key {
            KeyCode::Esc => {
                if state.dirty {
                    *config = state.save();
                }
                Action::Transition(Screen::MainMenu(MainMenuState::new()))
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if !state.models.is_empty() {
                    state.selected = state
                        .selected
                        .checked_sub(1)
                        .unwrap_or(state.models.len() - 1);
                }
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !state.models.is_empty() {
                    state.selected = (state.selected + 1) % state.models.len();
                }
                Action::None
            }
            KeyCode::Enter => {
                if !state.models.is_empty() {
                    state.begin_edit(ModelField::Name);
                }
                Action::None
            }
            KeyCode::Char('a') => {
                state.models.push(ModelEntry {
                    name: "New Llamafile".into(),
                    backend: ModelBackend::Llamafile {
                        url: String::new(),
                        filename: String::new(),
                    },
                });
                state.selected = state.models.len() - 1;
                state.dirty = true;
                state.begin_edit(ModelField::Name);
                Action::None
            }
            KeyCode::Char('A') => {
                state.models.push(ModelEntry {
                    name: "New API Model".into(),
                    backend: ModelBackend::Api {
                        base_url: "https://api.anthropic.com".into(),
                        api_key: String::new(),
                        model: String::new(),
                    },
                });
                state.selected = state.models.len() - 1;
                state.dirty = true;
                state.begin_edit(ModelField::Name);
                Action::None
            }
            KeyCode::Char('d') => {
                if !state.models.is_empty() {
                    state.focus = SettingsFocus::ConfirmDelete;
                }
                Action::None
            }
            _ => Action::None,
        },

        SettingsFocus::EditField(field) => match key {
            KeyCode::Esc => {
                state.focus = SettingsFocus::ModelList;
                Action::None
            }
            KeyCode::Enter | KeyCode::Tab => {
                state.commit_edit(field);
                // Advance to next field, or return to list if done.
                if let Some(entry) = state.models.get(state.selected) {
                    if let Some(next) = field.next(&entry.backend) {
                        state.begin_edit(next);
                    } else {
                        state.focus = SettingsFocus::ModelList;
                    }
                } else {
                    state.focus = SettingsFocus::ModelList;
                }
                Action::None
            }
            KeyCode::Backspace => {
                state.input_backspace();
                Action::None
            }
            KeyCode::Delete => {
                state.input_delete();
                Action::None
            }
            KeyCode::Left => {
                state.input_left();
                Action::None
            }
            KeyCode::Right => {
                state.input_right();
                Action::None
            }
            KeyCode::Home => {
                state.input_cursor = 0;
                Action::None
            }
            KeyCode::End => {
                state.input_cursor = state.input_buf.len();
                Action::None
            }
            KeyCode::Char(ch) => {
                state.input_insert(ch);
                Action::None
            }
            _ => Action::None,
        },

        SettingsFocus::ConfirmDelete => match key {
            KeyCode::Char('y') => {
                if state.selected < state.models.len() {
                    state.models.remove(state.selected);
                    if state.selected >= state.models.len() && !state.models.is_empty() {
                        state.selected = state.models.len() - 1;
                    }
                    state.dirty = true;
                }
                state.focus = SettingsFocus::ModelList;
                Action::None
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                state.focus = SettingsFocus::ModelList;
                Action::None
            }
            _ => Action::None,
        },
    }
}

// ── Personalities Input Handler ────────────────────────────────────────

fn handle_personalities_input(state: &mut PersonalitiesState, key: KeyCode) -> Action {
    use screens::{PersonalitiesFocus, PersonalityField, PersonalitySource};

    match state.focus {
        PersonalitiesFocus::List => match key {
            KeyCode::Esc => Action::Transition(Screen::MainMenu(MainMenuState::new())),
            KeyCode::Up | KeyCode::Char('k') => {
                if state.selected > 0 {
                    state.selected -= 1;
                    state.detail_scroll = 0;
                }
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if state.selected + 1 < state.entries.len() {
                    state.selected += 1;
                    state.detail_scroll = 0;
                }
                Action::None
            }
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
                if !state.entries.is_empty() {
                    state.focus = PersonalitiesFocus::Detail;
                }
                Action::None
            }
            KeyCode::Enter => {
                if state.selected_is_custom() {
                    state.begin_edit_text(PersonalityField::Name);
                } else if !state.entries.is_empty() {
                    // Built-in: switch to detail view.
                    state.focus = PersonalitiesFocus::Detail;
                }
                Action::None
            }
            KeyCode::Char('n') => {
                // Create a new blank personality.
                let new_p = Personality {
                    name: "New Personality".into(),
                    style: String::new(),
                    aggression: 0.5,
                    cooperation: 0.5,
                    catchphrases: Vec::new(),
                    setup_strategy: None,
                    strategy_guide: None,
                };
                let stem = find_unique_stem(&state.base_dir, "new-personality");
                state.entries.push((new_p, PersonalitySource::Custom(stem)));
                state.selected = state.entries.len() - 1;
                state.detail_scroll = 0;
                state.save_current();
                state.begin_edit_text(PersonalityField::Name);
                Action::None
            }
            KeyCode::Char('D') => {
                // Duplicate selected personality as a custom one.
                if let Some((p, _)) = state.entries.get(state.selected).cloned() {
                    let mut dup = p;
                    dup.name = format!("Copy of {}", dup.name);
                    let base_stem = Personality::filename_from_name(&dup.name);
                    let stem = find_unique_stem(&state.base_dir, &base_stem);
                    state.entries.push((dup, PersonalitySource::Custom(stem)));
                    state.selected = state.entries.len() - 1;
                    state.detail_scroll = 0;
                    state.save_current();
                    state.begin_edit_text(PersonalityField::Name);
                }
                Action::None
            }
            KeyCode::Char('d') => {
                if state.selected_is_custom() {
                    state.focus = PersonalitiesFocus::ConfirmDelete;
                }
                Action::None
            }
            _ => Action::None,
        },

        PersonalitiesFocus::Detail => match key {
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                state.focus = PersonalitiesFocus::List;
                Action::None
            }
            KeyCode::Tab => {
                state.focus = PersonalitiesFocus::List;
                Action::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                state.detail_scroll = state.detail_scroll.saturating_add(1);
                Action::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                state.detail_scroll = state.detail_scroll.saturating_sub(1);
                Action::None
            }
            KeyCode::PageDown => {
                state.detail_scroll = state.detail_scroll.saturating_add(10);
                Action::None
            }
            KeyCode::PageUp => {
                state.detail_scroll = state.detail_scroll.saturating_sub(10);
                Action::None
            }
            KeyCode::Enter => {
                if state.selected_is_custom() {
                    state.begin_edit_text(PersonalityField::Name);
                }
                Action::None
            }
            _ => Action::None,
        },

        PersonalitiesFocus::EditText(field) => match key {
            KeyCode::Esc => {
                state.focus = PersonalitiesFocus::List;
                Action::None
            }
            KeyCode::Enter | KeyCode::Tab => {
                // Reject empty names.
                if field == PersonalityField::Name && state.input_buf.trim().is_empty() {
                    return Action::None;
                }
                state.commit_edit_text(field);
                // Update the filename stem if name changed.
                if field == PersonalityField::Name {
                    let base_dir = state.base_dir.clone();
                    if let Some((p, PersonalitySource::Custom(ref mut stem))) =
                        state.entries.get_mut(state.selected)
                    {
                        let old_path = format!("{}/{}.toml", base_dir, stem);
                        let new_stem = Personality::filename_from_name(&p.name);
                        let new_stem = find_unique_stem_excluding(&base_dir, &new_stem, stem);
                        let new_path = format!("{}/{}.toml", base_dir, new_stem);
                        if old_path != new_path {
                            let _ = std::fs::rename(&old_path, &new_path);
                        }
                        *stem = new_stem;
                    }
                }
                state.save_current();
                state.next_field(field);
                Action::None
            }
            KeyCode::Backspace => {
                state.input_backspace();
                Action::None
            }
            KeyCode::Delete => {
                state.input_delete();
                Action::None
            }
            KeyCode::Left => {
                state.input_left();
                Action::None
            }
            KeyCode::Right => {
                state.input_right();
                Action::None
            }
            KeyCode::Home => {
                state.input_cursor = 0;
                Action::None
            }
            KeyCode::End => {
                state.input_cursor = state.input_buf.len();
                Action::None
            }
            KeyCode::Char(ch) => {
                state.input_insert(ch);
                Action::None
            }
            _ => Action::None,
        },

        PersonalitiesFocus::EditSlider(field) => match key {
            KeyCode::Esc => {
                state.focus = PersonalitiesFocus::List;
                Action::None
            }
            KeyCode::Enter | KeyCode::Tab => {
                state.save_current();
                state.next_field(field);
                Action::None
            }
            KeyCode::Left => {
                if let Some((p, _)) = state.entries.get_mut(state.selected) {
                    let val = match field {
                        PersonalityField::Aggression => &mut p.aggression,
                        PersonalityField::Cooperation => &mut p.cooperation,
                        _ => return Action::None,
                    };
                    *val = (*val - 0.1).max(0.0);
                    // Round to avoid floating-point drift.
                    *val = (*val * 10.0).round() / 10.0;
                    state.dirty = true;
                }
                Action::None
            }
            KeyCode::Right => {
                if let Some((p, _)) = state.entries.get_mut(state.selected) {
                    let val = match field {
                        PersonalityField::Aggression => &mut p.aggression,
                        PersonalityField::Cooperation => &mut p.cooperation,
                        _ => return Action::None,
                    };
                    *val = (*val + 0.1).min(1.0);
                    *val = (*val * 10.0).round() / 10.0;
                    state.dirty = true;
                }
                Action::None
            }
            _ => Action::None,
        },

        PersonalitiesFocus::EditCatchphrases => match key {
            KeyCode::Esc | KeyCode::Tab => {
                state.save_current();
                state.focus = PersonalitiesFocus::List;
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if state.catchphrase_selected > 0 {
                    state.catchphrase_selected -= 1;
                }
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some((p, _)) = state.entries.get(state.selected) {
                    if state.catchphrase_selected + 1 < p.catchphrases.len() {
                        state.catchphrase_selected += 1;
                    }
                }
                Action::None
            }
            KeyCode::Char('a') => {
                // Add new catchphrase.
                if let Some((p, _)) = state.entries.get(state.selected) {
                    state.catchphrase_selected = p.catchphrases.len();
                }
                state.input_buf.clear();
                state.input_cursor = 0;
                state.focus = PersonalitiesFocus::EditCatchphraseText;
                Action::None
            }
            KeyCode::Char('d') => {
                // Delete selected catchphrase.
                if let Some((p, _)) = state.entries.get_mut(state.selected) {
                    if state.catchphrase_selected < p.catchphrases.len() {
                        p.catchphrases.remove(state.catchphrase_selected);
                        if state.catchphrase_selected >= p.catchphrases.len()
                            && state.catchphrase_selected > 0
                        {
                            state.catchphrase_selected -= 1;
                        }
                        state.dirty = true;
                        state.save_current();
                    }
                }
                Action::None
            }
            KeyCode::Enter => {
                // Edit selected catchphrase.
                if let Some((p, _)) = state.entries.get(state.selected) {
                    if let Some(phrase) = p.catchphrases.get(state.catchphrase_selected) {
                        state.input_buf = phrase.clone();
                        state.input_cursor = state.input_buf.len();
                        state.focus = PersonalitiesFocus::EditCatchphraseText;
                    }
                }
                Action::None
            }
            _ => Action::None,
        },

        PersonalitiesFocus::EditCatchphraseText => match key {
            KeyCode::Esc => {
                state.focus = PersonalitiesFocus::EditCatchphrases;
                Action::None
            }
            KeyCode::Enter => {
                if !state.input_buf.is_empty() {
                    if let Some((p, _)) = state.entries.get_mut(state.selected) {
                        if state.catchphrase_selected < p.catchphrases.len() {
                            p.catchphrases[state.catchphrase_selected] = state.input_buf.clone();
                        } else {
                            p.catchphrases.push(state.input_buf.clone());
                        }
                        state.dirty = true;
                        state.save_current();
                    }
                }
                state.focus = PersonalitiesFocus::EditCatchphrases;
                Action::None
            }
            KeyCode::Backspace => {
                state.input_backspace();
                Action::None
            }
            KeyCode::Delete => {
                state.input_delete();
                Action::None
            }
            KeyCode::Left => {
                state.input_left();
                Action::None
            }
            KeyCode::Right => {
                state.input_right();
                Action::None
            }
            KeyCode::Char(ch) => {
                state.input_insert(ch);
                Action::None
            }
            _ => Action::None,
        },

        PersonalitiesFocus::ConfirmDelete => match key {
            KeyCode::Char('y') => {
                state.delete_current();
                state.focus = PersonalitiesFocus::List;
                Action::None
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                state.focus = PersonalitiesFocus::List;
                Action::None
            }
            _ => Action::None,
        },
    }
}

/// Find a unique filename stem in a directory, appending -2, -3, etc. if needed.
fn find_unique_stem(dir: &str, base: &str) -> String {
    let path = format!("{}/{}.toml", dir, base);
    if !std::path::Path::new(&path).exists() {
        return base.to_string();
    }
    for i in 2..100 {
        let candidate = format!("{}-{}", base, i);
        let path = format!("{}/{}.toml", dir, candidate);
        if !std::path::Path::new(&path).exists() {
            return candidate;
        }
    }
    base.to_string()
}

/// Find a unique stem, excluding a specific existing stem (for renames).
fn find_unique_stem_excluding(dir: &str, base: &str, exclude: &str) -> String {
    if base == exclude {
        return base.to_string();
    }
    let path = format!("{}/{}.toml", dir, base);
    if !std::path::Path::new(&path).exists() {
        return base.to_string();
    }
    for i in 2..100 {
        let candidate = format!("{}-{}", base, i);
        if candidate == exclude {
            return candidate;
        }
        let path = format!("{}/{}.toml", dir, candidate);
        if !std::path::Path::new(&path).exists() {
            return candidate;
        }
    }
    base.to_string()
}

// ── Helpers ────────────────────────────────────────────────────────────

fn discover_personalities() -> Vec<Personality> {
    let mut personalities = Vec::new();
    if let Ok(entries) = std::fs::read_dir("./personalities") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                if let Ok(p) = Personality::from_toml_file(&path) {
                    personalities.push(p);
                }
            }
        }
    }
    personalities.sort_by(|a, b| a.name.cmp(&b.name));
    personalities
}
