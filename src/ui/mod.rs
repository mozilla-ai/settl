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
    /// AI reasoning trace from an LLM or random player.
    AiReasoning {
        player_id: usize,
        player_name: String,
        reasoning: String,
    },
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
    pub speed_ms: u64,
    pub paused: bool,
    /// Whether to show AI reasoning panel (Tab toggle).
    pub show_ai_panel: bool,
    /// Whether to show the help overlay (? toggle).
    pub show_help: bool,
    /// Current input mode (replaces pending_prompt).
    pub input_mode: InputMode,
    /// Channel to receive human prompts from the engine.
    pub human_prompt_rx: Option<mpsc::UnboundedReceiver<player::tui_human::HumanPrompt>>,
    /// Channel to send human responses back to the engine.
    pub human_response_tx: Option<mpsc::UnboundedSender<HumanResponse>>,
    /// Cached hex grid for board rendering (computed once on first state).
    pub hex_grid: Option<board_view::HexGrid>,
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
                "q:quit  Space:pause  +/-:speed  j/k:scroll  Tab:AI panel".into(),
            ],
            chat_messages: Vec::new(),
            player_names,
            game_over: false,
            game_over_winner: None,
            log_scroll: 0,
            chat_scroll: 0,
            speed_ms: 100,
            paused: false,
            show_ai_panel: false,
            show_help: false,
            input_mode: InputMode::Spectating,
            human_prompt_rx: None,
            human_response_tx: None,
            hex_grid: None,
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
                event: _,
                message,
            } => {
                if self.hex_grid.is_none() {
                    self.hex_grid = Some(board_view::HexGrid::new());
                }
                self.state = Some(state);
                if !message.is_empty() {
                    self.push_message(message);
                }
            }
            UiEvent::AiReasoning {
                player_id,
                player_name,
                reasoning,
            } => {
                self.chat_messages.push(chat_panel::ChatMessage {
                    player: player_name,
                    player_id,
                    text: reasoning,
                });
                // Cap chat messages to prevent unbounded growth.
                const MAX_CHAT: usize = 500;
                if self.chat_messages.len() > MAX_CHAT {
                    self.chat_messages
                        .drain(..self.chat_messages.len() - MAX_CHAT);
                }
                self.chat_scroll = self.chat_messages.len().saturating_sub(1) as u16;
            }
            UiEvent::GameOver { winner, message } => {
                let winner_name = self
                    .player_names
                    .get(winner)
                    .cloned()
                    .unwrap_or_else(|| "?".into());
                self.push_message(format!(
                    "GAME OVER: Player {} ({}) wins!",
                    winner, winner_name,
                ));
                self.push_message(message.clone());
                self.game_over = true;
                self.game_over_winner = Some((winner, winner_name));
            }
        }
    }
}

/// Top-level app -- holds the current screen and shared config.
pub struct App {
    pub screen: Screen,
    pub personalities: Vec<Personality>,
    /// Running llamafile process, if any. Survives across "Play Again" restarts.
    pub llamafile_process: Option<crate::llamafile::LlamafileProcess>,
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

    let mut app = App {
        screen: Screen::MainMenu(MainMenuState::new()),
        personalities,
        llamafile_process: None,
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
        terminal.draw(|f| draw_screen(f, &app.screen))?;

        // Poll timeout depends on screen type.
        let timeout = match &app.screen {
            Screen::LlamafileSetup(_) => Duration::from_millis(50), // Fast refresh for progress
            Screen::Playing(ps) => {
                Duration::from_millis(if ps.paused { 50 } else { ps.speed_ms.min(50) })
            }
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
                                    if let Some(ref process) = app.llamafile_process {
                                        let screen =
                                            launch_game(ng, &app.personalities, Some(process.port));
                                        app.screen = screen;
                                    } else {
                                        let (status_tx, status_rx) = mpsc::unbounded_channel();
                                        let saved_config = clone_new_game_state(ng);
                                        let (handle, process_rx) = spawn_llamafile_setup(status_tx);
                                        let setup_state = LlamafileSetupState {
                                            status: crate::llamafile::LlamafileStatus::Checking,
                                            status_rx,
                                            saved_config,
                                            task_handle: Some(handle),
                                            process_rx: Some(process_rx),
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
                // Move saved_config out of the setup state.
                if let Screen::LlamafileSetup(setup) =
                    std::mem::replace(&mut app.screen, Screen::MainMenu(MainMenuState::new()))
                {
                    let screen = launch_game(&setup.saved_config, &app.personalities, Some(port));
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
        Screen::LlamafileSetup(state) => screens::draw_llamafile_setup(f, state),
        Screen::Playing(ps) => layout::draw_playing(f, ps),
        Screen::PostGame(state) => screens::draw_post_game(f, state),
    }
}

// ── Input Dispatch ─────────────────────────────────────────────────────

#[allow(clippy::large_enum_variant)]
enum Action {
    None,
    Quit,
    Transition(Screen),
    StartGame,
}

fn handle_input(app: &mut App, key: KeyCode) -> Action {
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
                        "New Game" => Action::Transition(Screen::NewGame(NewGameState::new(
                            &app.personalities,
                        ))),
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

        Screen::NewGame(state) => {
            if state.editing {
                return handle_new_game_editing(state, key);
            }
            match key {
                KeyCode::Esc => Action::Transition(Screen::MainMenu(MainMenuState::new())),
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    state.add_player();
                    Action::None
                }
                KeyCode::Char('-') => {
                    state.remove_player();
                    Action::None
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    move_new_game_focus_up(state);
                    Action::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    move_new_game_focus_down(state);
                    Action::None
                }
                KeyCode::Tab => {
                    move_new_game_focus_next_col(state);
                    Action::None
                }
                KeyCode::BackTab => {
                    move_new_game_focus_prev_col(state);
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
                KeyCode::Enter => {
                    match state.focus {
                        NewGameFocus::StartButton => Action::StartGame,
                        NewGameFocus::Player {
                            col: NewGameCol::Name,
                            ..
                        } => {
                            state.editing = true;
                            Action::None
                        }
                        _ => {
                            // For Kind/Model/Personality columns, Enter cycles forward.
                            cycle_new_game_value(state, true);
                            Action::None
                        }
                    }
                }
                _ => Action::None,
            }
        }

        Screen::LlamafileSetup(_) => match key {
            KeyCode::Esc => {
                // Cancel setup: abort the background task and return to NewGame.
                if let Screen::LlamafileSetup(mut setup) =
                    std::mem::replace(&mut app.screen, Screen::MainMenu(MainMenuState::new()))
                {
                    if let Some(handle) = setup.task_handle.take() {
                        handle.abort();
                    }
                    // Drop the oneshot receiver (cleaned up with the setup state).
                    Action::Transition(Screen::NewGame(setup.saved_config))
                } else {
                    Action::Transition(Screen::NewGame(NewGameState::new(&app.personalities)))
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
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            ps.speed_ms = ps.speed_ms.saturating_sub(25).max(25);
                        }
                        KeyCode::Char('-') => {
                            ps.speed_ms = (ps.speed_ms + 25).min(500);
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            ps.log_scroll = ps.log_scroll.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            ps.log_scroll = ps.log_scroll.saturating_add(1);
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
                        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
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
                "Play Again" => {
                    Action::Transition(Screen::NewGame(NewGameState::new(&app.personalities)))
                }
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
            KeyCode::Up => dy < 0 && dy.abs() >= dx.abs(),
            KeyCode::Down => dy > 0 && dy.abs() >= dx.abs(),
            KeyCode::Left => dx < 0 && dx.abs() >= dy.abs(),
            KeyCode::Right => dx > 0 && dx.abs() >= dy.abs(),
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

fn handle_new_game_editing(state: &mut NewGameState, key: KeyCode) -> Action {
    match key {
        KeyCode::Esc => {
            state.editing = false;
            Action::None
        }
        KeyCode::Enter => {
            state.editing = false;
            Action::None
        }
        KeyCode::Backspace => {
            if let NewGameFocus::Player {
                row,
                col: NewGameCol::Name,
            } = state.focus
            {
                state.players[row].name.pop();
            }
            Action::None
        }
        KeyCode::Char(c) => {
            if let NewGameFocus::Player {
                row,
                col: NewGameCol::Name,
            } = state.focus
            {
                if state.players[row].name.len() < 12 {
                    state.players[row].name.push(c);
                }
            }
            Action::None
        }
        _ => Action::None,
    }
}

fn move_new_game_focus_up(state: &mut NewGameState) {
    state.focus = match state.focus {
        NewGameFocus::Player { row, col } => {
            if row > 0 {
                NewGameFocus::Player { row: row - 1, col }
            } else {
                NewGameFocus::Player { row: 0, col }
            }
        }
        NewGameFocus::StartButton => NewGameFocus::Player {
            row: state.num_players() - 1,
            col: NewGameCol::Name,
        },
    };
}

fn move_new_game_focus_down(state: &mut NewGameState) {
    state.focus = match state.focus {
        NewGameFocus::Player { row, col } => {
            if row + 1 < state.num_players() {
                NewGameFocus::Player { row: row + 1, col }
            } else {
                NewGameFocus::StartButton
            }
        }
        NewGameFocus::StartButton => NewGameFocus::StartButton,
    };
}

fn move_new_game_focus_next_col(state: &mut NewGameState) {
    if let NewGameFocus::Player { row, col } = state.focus {
        state.focus = NewGameFocus::Player {
            row,
            col: col.next(),
        };
    }
}

fn move_new_game_focus_prev_col(state: &mut NewGameState) {
    if let NewGameFocus::Player { row, col } = state.focus {
        state.focus = NewGameFocus::Player {
            row,
            col: col.prev(),
        };
    }
}

fn cycle_new_game_value(state: &mut NewGameState, forward: bool) {
    if let NewGameFocus::Player { row, col } = state.focus {
        let player = &mut state.players[row];
        match col {
            NewGameCol::Personality => {
                // Only AI players (Llamafile) have personalities.
                if player.kind == PlayerKind::Llamafile {
                    let n = state.personality_names.len();
                    player.personality_index = if forward {
                        (player.personality_index + 1) % n
                    } else {
                        player.personality_index.checked_sub(1).unwrap_or(n - 1)
                    };
                }
            }
            NewGameCol::Name => {
                // Name doesn't cycle. Enter to edit instead.
            }
        }
    }
}

// ── Game Launch ────────────────────────────────────────────────────────

fn launch_game(
    ng: &NewGameState,
    discovered_personalities: &[Personality],
    llamafile_port: Option<u16>,
) -> Screen {
    use player::tui_human::{HumanInputChannel, TuiHumanPlayer};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // Use the fixed beginner board layout (randomization deferred to a future design).
    let board = Board::default_board();

    let state = GameState::new(board, ng.num_players());

    // Build players.
    let built_in_personalities = [
        Personality::default_personality(),
        Personality::aggressive(),
        Personality::grudge_holder(),
        Personality::builder(),
        Personality::chaos_agent(),
    ];

    // Create human input channels if any human players exist.
    let has_human = ng.players.iter().any(|p| p.kind == PlayerKind::Human);
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

    // Build a shared llamafile client if any player needs it.
    let llama_client = llamafile_port.map(player::llamafile_player::llamafile_client);

    let players: Vec<Box<dyn player::Player>> = ng
        .players
        .iter()
        .map(|pc| match pc.kind {
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
                let client = llama_client
                    .clone()
                    .expect("llamafile client should exist when Llamafile players are used");
                Box::new(player::llamafile_player::LlamafilePlayer::with_client(
                    pc.name.clone(),
                    player::llamafile_player::LLAMAFILE_MODEL.into(),
                    personality,
                    client,
                )) as Box<dyn player::Player>
            }
            PlayerKind::Human => {
                let channel = human_channels.as_ref().unwrap().0.clone();
                Box::new(TuiHumanPlayer::new(pc.name.clone(), channel)) as Box<dyn player::Player>
            }
        })
        .collect();

    let player_names: Vec<String> = ng.players.iter().map(|p| p.name.clone()).collect();

    // Create channel.
    let (tx, rx) = mpsc::unbounded_channel();

    // Spawn game engine.
    tokio::spawn(async move {
        let mut orchestrator = GameOrchestrator::new(state, players);
        orchestrator.ui_tx = Some(tx);

        orchestrator.run().await
    });

    let mut ps = PlayingState::new(rx, player_names, has_human);
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
    status_tx: mpsc::UnboundedSender<crate::llamafile::LlamafileStatus>,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::oneshot::Receiver<crate::llamafile::LlamafileProcess>,
) {
    let (process_tx, process_rx) = tokio::sync::oneshot::channel();

    let handle = tokio::spawn(async move {
        match crate::llamafile::ensure_llamafile(status_tx.clone()).await {
            Ok(path) => {
                let _ = status_tx.send(crate::llamafile::LlamafileStatus::Starting);
                let _ = status_tx.send(crate::llamafile::LlamafileStatus::WaitingForReady);
                match crate::llamafile::LlamafileProcess::start_with_port_scan(&path).await {
                    Ok(process) => {
                        let port = process.port;
                        if process_tx.send(process).is_ok() {
                            let _ = status_tx.send(crate::llamafile::LlamafileStatus::Ready(port));
                        }
                        // If send fails, the receiver was dropped (user cancelled).
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

/// Clone a `NewGameState` for saving across screen transitions.
fn clone_new_game_state(ng: &NewGameState) -> NewGameState {
    NewGameState {
        players: ng.players.clone(),
        focus: ng.focus,
        personality_names: ng.personality_names.clone(),
        editing: false,
    }
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
