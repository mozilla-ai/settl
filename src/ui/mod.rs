pub mod board_view;
pub mod chat_panel;
pub mod game_log;
pub mod layout;
pub mod menu;
pub mod resource_bar;
pub mod screens;

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
use crate::game::orchestrator::GameOrchestrator;
use crate::game::state::GameState;
use crate::player;
use crate::player::personality::Personality;
use crate::replay::event::GameEvent;
use crate::replay::save::SaveGame;

use screens::*;

/// Player colors shared across all UI panels.
pub const PLAYER_COLORS: [Color; 4] = [Color::Red, Color::Blue, Color::Green, Color::Magenta];

/// Events sent from the game orchestrator to the TUI.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
pub enum Screen {
    Title { frame: u64 },
    MainMenu(MainMenuState),
    NewGame(NewGameState),
    FilePicker(FilePickerState),
    Playing(PlayingState),
    PostGame(PostGameState),
}

/// A pending human input prompt displayed as an overlay.
pub struct PendingHumanPrompt {
    pub title: String,
    pub options: Vec<String>,
    pub selected: usize,
}

/// State for the active game screen (formerly the flat App fields).
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
    /// Pending human decision prompt (shown as an overlay).
    pub pending_prompt: Option<PendingHumanPrompt>,
    /// Channel to receive human prompts from the engine.
    pub human_prompt_rx: Option<mpsc::UnboundedReceiver<player::tui_human::HumanPrompt>>,
    /// Channel to send human responses back to the engine.
    pub human_response_tx: Option<mpsc::UnboundedSender<usize>>,
}

impl PlayingState {
    fn new(
        rx: mpsc::UnboundedReceiver<UiEvent>,
        player_names: Vec<String>,
        has_human: bool,
    ) -> Self {
        let start_msg = if has_human {
            "Game started — your turn will show a prompt".into()
        } else {
            "Game started — spectator mode".into()
        };
        Self {
            rx,
            state: None,
            messages: vec![
                start_msg,
                "q:quit  Space:pause  +/-:speed  j/k:scroll".into(),
            ],
            chat_messages: Vec::new(),
            player_names,
            game_over: false,
            game_over_winner: None,
            log_scroll: 0,
            chat_scroll: 0,
            speed_ms: 100,
            paused: false,
            pending_prompt: None,
            human_prompt_rx: None,
            human_response_tx: None,
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
                self.state = Some(state);
                if !message.is_empty() {
                    self.messages.push(message);
                    let total = self.messages.len() as u16;
                    self.log_scroll = total.saturating_sub(1);
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
                let total = self.chat_messages.len() as u16;
                self.chat_scroll = total.saturating_sub(1);
            }
            UiEvent::GameOver { winner, message } => {
                let winner_name = self
                    .player_names
                    .get(winner)
                    .cloned()
                    .unwrap_or_else(|| "?".into());
                self.messages.push(format!(
                    "GAME OVER: Player {} ({}) wins!",
                    winner, winner_name,
                ));
                self.messages.push(message.clone());
                self.game_over = true;
                self.game_over_winner = Some((winner, winner_name));
                let total = self.messages.len() as u16;
                self.log_scroll = total.saturating_sub(1);
            }
        }
    }
}

/// Top-level app — just holds the current screen and shared config.
pub struct App {
    pub screen: Screen,
    pub personalities: Vec<Personality>,
}

// ── Main Entry Point ───────────────────────────────────────────────────

/// Run the full TUI application (title → menu → game → post-game loop).
pub async fn run_app() -> io::Result<()> {
    // Discover personalities from ./personalities/*.toml.
    let personalities = discover_personalities();

    // Setup terminal.
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
        screen: Screen::Title { frame: 0 },
        personalities,
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
            Screen::Title { .. } => Duration::from_millis(33), // ~30fps for blink
            Screen::Playing(ps) => {
                Duration::from_millis(if ps.paused { 50 } else { ps.speed_ms.min(50) })
            }
            _ => Duration::from_millis(100),
        };

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Ctrl+C exits from any screen.
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        return Ok(());
                    }
                    let action = handle_input(app, key.code);
                    match action {
                        Action::None => {}
                        Action::Quit => return Ok(()),
                        Action::Transition(screen) => app.screen = *screen,
                        Action::StartGame => {
                            if let Screen::NewGame(ref ng) = app.screen {
                                let screen = launch_game(ng, &app.personalities);
                                app.screen = screen;
                            }
                        }
                        Action::LoadFile => {
                            if let Screen::FilePicker(ref fp) = app.screen {
                                if let Some(path) = fp.files.get(fp.selected) {
                                    match fp.purpose {
                                        FilePickerPurpose::Resume => {
                                            if let Some(screen) = launch_resume(path) {
                                                app.screen = screen;
                                            }
                                        }
                                        FilePickerPurpose::Replay => {
                                            if let Some(screen) = launch_replay(path) {
                                                app.screen = screen;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Tick frame counter for title blink.
        if let Screen::Title { ref mut frame } = app.screen {
            *frame += 1;
        }

        // Drain game events for Playing screen.
        if let Screen::Playing(ref mut ps) = app.screen {
            if !ps.paused {
                while let Ok(ui_event) = ps.rx.try_recv() {
                    ps.handle_game_event(ui_event);
                }
            }

            // Check for incoming human prompts.
            if ps.pending_prompt.is_none() {
                if let Some(ref mut prompt_rx) = ps.human_prompt_rx {
                    if let Ok(prompt) = prompt_rx.try_recv() {
                        ps.pending_prompt = Some(PendingHumanPrompt {
                            title: prompt.title,
                            options: prompt.options,
                            selected: 0,
                        });
                    }
                }
            }
        }
    }
}

// ── Drawing Dispatch ───────────────────────────────────────────────────

fn draw_screen(f: &mut Frame, screen: &Screen) {
    match screen {
        Screen::Title { frame } => screens::draw_title(f, *frame),
        Screen::MainMenu(state) => screens::draw_main_menu(f, state),
        Screen::NewGame(state) => screens::draw_new_game(f, state),
        Screen::FilePicker(state) => screens::draw_file_picker(f, state),
        Screen::Playing(ps) => layout::draw_playing(f, ps),
        Screen::PostGame(state) => screens::draw_post_game(f, state),
    }
}

// ── Input Dispatch ─────────────────────────────────────────────────────

enum Action {
    None,
    Quit,
    Transition(Box<Screen>),
    StartGame,
    LoadFile,
}

fn handle_input(app: &mut App, key: KeyCode) -> Action {
    match &mut app.screen {
        Screen::Title { .. } => {
            // Any key → main menu.
            Action::Transition(Box::new(Screen::MainMenu(MainMenuState::default())))
        }

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
                        "New Game" => Action::Transition(Box::new(Screen::NewGame(
                            NewGameState::new(&app.personalities),
                        ))),
                        "Continue Game" => Action::Transition(Box::new(Screen::FilePicker(
                            FilePickerState::new(FilePickerPurpose::Resume),
                        ))),
                        "Replay Game" => Action::Transition(Box::new(Screen::FilePicker(
                            FilePickerState::new(FilePickerPurpose::Replay),
                        ))),
                        "Quit" => Action::Quit,
                        _ => Action::None,
                    }
                }
                KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
                _ => Action::None,
            }
        }

        Screen::NewGame(state) => {
            if state.editing {
                return handle_new_game_editing(state, key);
            }
            match key {
                KeyCode::Esc => {
                    Action::Transition(Box::new(Screen::MainMenu(MainMenuState::default())))
                }
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
                        NewGameFocus::Setting(_) => {
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

        Screen::FilePicker(state) => match key {
            KeyCode::Esc => {
                Action::Transition(Box::new(Screen::MainMenu(MainMenuState::default())))
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.selected = state
                    .selected
                    .checked_sub(1)
                    .unwrap_or(state.files.len().saturating_sub(1));
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !state.files.is_empty() {
                    state.selected = (state.selected + 1) % state.files.len();
                }
                Action::None
            }
            KeyCode::Enter => {
                if !state.files.is_empty() {
                    Action::LoadFile
                } else {
                    Action::None
                }
            }
            _ => Action::None,
        },

        Screen::Playing(ps) => {
            // If a human prompt is pending, intercept input for selection.
            if let Some(ref mut prompt) = ps.pending_prompt {
                match key {
                    KeyCode::Up | KeyCode::Char('k') => {
                        prompt.selected = prompt.selected.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if prompt.selected + 1 < prompt.options.len() {
                            prompt.selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        let selected = prompt.selected;
                        if let Some(ref tx) = ps.human_response_tx {
                            let _ = tx.send(selected);
                        }
                        ps.pending_prompt = None;
                    }
                    _ => {}
                }
                return Action::None;
            }

            match key {
                KeyCode::Char('q') | KeyCode::Esc => {
                    if ps.game_over {
                        // Build post-game from final state.
                        let post = build_post_game(ps);
                        Action::Transition(Box::new(Screen::PostGame(post)))
                    } else {
                        // Quit back to menu mid-game.
                        Action::Transition(Box::new(Screen::MainMenu(MainMenuState::default())))
                    }
                }
                KeyCode::Enter if ps.game_over => {
                    let post = build_post_game(ps);
                    Action::Transition(Box::new(Screen::PostGame(post)))
                }
                KeyCode::Char(' ') => {
                    ps.paused = !ps.paused;
                    Action::None
                }
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    ps.speed_ms = ps.speed_ms.saturating_sub(25).max(25);
                    Action::None
                }
                KeyCode::Char('-') => {
                    ps.speed_ms = (ps.speed_ms + 25).min(500);
                    Action::None
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    ps.log_scroll = ps.log_scroll.saturating_sub(1);
                    Action::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    ps.log_scroll = ps.log_scroll.saturating_add(1);
                    Action::None
                }
                _ => Action::None,
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
                "Play Again" => Action::Transition(Box::new(Screen::NewGame(NewGameState::new(
                    &app.personalities,
                )))),
                "Main Menu" => {
                    Action::Transition(Box::new(Screen::MainMenu(MainMenuState::default())))
                }
                "Quit" => Action::Quit,
                _ => Action::None,
            },
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            _ => Action::None,
        },
    }
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
            match state.focus {
                NewGameFocus::Player {
                    row,
                    col: NewGameCol::Name,
                } => {
                    state.players[row].name.pop();
                }
                NewGameFocus::Setting(0) => {
                    state.seed_input.pop();
                }
                NewGameFocus::Setting(1) => {
                    state.max_turns_input.pop();
                }
                _ => {}
            }
            Action::None
        }
        KeyCode::Char(c) => {
            match state.focus {
                NewGameFocus::Player {
                    row,
                    col: NewGameCol::Name,
                } => {
                    if state.players[row].name.len() < 12 {
                        state.players[row].name.push(c);
                    }
                }
                NewGameFocus::Setting(0) => {
                    if c.is_ascii_digit() && state.seed_input.len() < 18 {
                        state.seed_input.push(c);
                    }
                }
                NewGameFocus::Setting(1) => {
                    if c.is_ascii_digit() && state.max_turns_input.len() < 5 {
                        state.max_turns_input.push(c);
                    }
                }
                _ => {}
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
        NewGameFocus::Setting(0) => NewGameFocus::Player {
            row: state.num_players() - 1,
            col: NewGameCol::Kind,
        },
        NewGameFocus::Setting(1) => NewGameFocus::Setting(0),
        NewGameFocus::StartButton => NewGameFocus::Setting(1),
        _ => state.focus,
    };
}

fn move_new_game_focus_down(state: &mut NewGameState) {
    state.focus = match state.focus {
        NewGameFocus::Player { row, col } => {
            if row + 1 < state.num_players() {
                NewGameFocus::Player { row: row + 1, col }
            } else {
                NewGameFocus::Setting(0)
            }
        }
        NewGameFocus::Setting(0) => NewGameFocus::Setting(1),
        NewGameFocus::Setting(1) => NewGameFocus::StartButton,
        NewGameFocus::StartButton => NewGameFocus::StartButton,
        _ => state.focus,
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
            NewGameCol::Kind => {
                player.kind = if forward {
                    player.kind.next()
                } else {
                    player.kind.prev()
                };
            }
            NewGameCol::Model => {
                if player.kind == PlayerKind::Llm {
                    let n = AVAILABLE_MODELS.len();
                    player.model_index = if forward {
                        (player.model_index + 1) % n
                    } else {
                        player.model_index.checked_sub(1).unwrap_or(n - 1)
                    };
                }
            }
            NewGameCol::Personality => {
                if player.kind == PlayerKind::Llm {
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

fn launch_game(ng: &NewGameState, discovered_personalities: &[Personality]) -> Screen {
    use player::tui_human::{HumanInputChannel, TuiHumanPlayer};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // Build board.
    let board = if let Some(seed) = ng.seed() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        Board::generate(&mut rng)
    } else {
        let mut rng = rand::thread_rng();
        Board::generate(&mut rng)
    };

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
    let human_channels = if has_human {
        let (prompt_tx, prompt_rx) = mpsc::unbounded_channel();
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        let channel = Arc::new(HumanInputChannel {
            prompt_tx,
            response_rx: Mutex::new(response_rx),
        });
        Some((channel, prompt_rx, response_tx))
    } else {
        None
    };

    let players: Vec<Box<dyn player::Player>> = ng
        .players
        .iter()
        .map(|pc| match pc.kind {
            PlayerKind::Random => Box::new(player::random::RandomPlayer::new(pc.name.clone()))
                as Box<dyn player::Player>,
            PlayerKind::Llm => {
                let model = AVAILABLE_MODELS
                    .get(pc.model_index)
                    .copied()
                    .unwrap_or("claude-sonnet-4-6")
                    .to_string();
                let personality = if pc.personality_index < built_in_personalities.len() {
                    built_in_personalities[pc.personality_index].clone()
                } else {
                    let disc_idx = pc.personality_index - built_in_personalities.len();
                    discovered_personalities
                        .get(disc_idx)
                        .cloned()
                        .unwrap_or_default()
                };
                Box::new(player::llm::LlmPlayer::new(
                    pc.name.clone(),
                    model,
                    personality,
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
    let max_turns = ng.max_turns();

    // Spawn game engine.
    tokio::spawn(async move {
        let mut orchestrator = GameOrchestrator::new(state, players);
        orchestrator.max_turns = max_turns;
        orchestrator.ui_tx = Some(tx);

        let result = orchestrator.run().await;

        // Save game log and replay.
        let _ = orchestrator
            .log
            .write_jsonl(std::path::Path::new("game_log.jsonl"));
        if let Ok(json) = serde_json::to_string_pretty(&orchestrator.replay) {
            let _ = std::fs::write("game_replay.json", json);
        }

        // Save game state on error for resume.
        if result.is_err() {
            let model_ids: Vec<String> = orchestrator
                .player_names
                .iter()
                .map(|_| String::new()) // Can't recover model IDs here easily
                .collect();
            let save = SaveGame::new(
                orchestrator.state.clone(),
                &orchestrator.log,
                orchestrator.player_names.clone(),
                model_ids,
            );
            let _ = save.save_to_file(std::path::Path::new("game_save.json"));
        }

        result
    });

    let mut ps = PlayingState::new(rx, player_names, has_human);
    if let Some((_, prompt_rx, response_tx)) = human_channels {
        ps.human_prompt_rx = Some(prompt_rx);
        ps.human_response_tx = Some(response_tx);
    }
    Screen::Playing(ps)
}

fn launch_resume(path: &std::path::Path) -> Option<Screen> {
    let save = SaveGame::load_from_file(path).ok()?;
    let player_names = save.player_names.clone();

    let players: Vec<Box<dyn player::Player>> = save
        .player_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let model = save.player_models.get(i).filter(|m| !m.is_empty()).cloned();
            if let Some(model_id) = model {
                Box::new(player::llm::LlmPlayer::new(
                    name.clone(),
                    model_id,
                    Personality::default(),
                )) as Box<dyn player::Player>
            } else {
                Box::new(player::random::RandomPlayer::new(name.clone())) as Box<dyn player::Player>
            }
        })
        .collect();

    let log = save.recent_log();
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let mut orchestrator = GameOrchestrator::new(save.state, players);
        orchestrator.log = log;
        orchestrator.max_turns = 500;
        orchestrator.ui_tx = Some(tx);

        let result = orchestrator.run().await;

        let _ = orchestrator
            .log
            .write_jsonl(std::path::Path::new("game_log.jsonl"));
        if let Ok(json) = serde_json::to_string_pretty(&orchestrator.replay) {
            let _ = std::fs::write("game_replay.json", json);
        }

        result
    });

    Some(Screen::Playing(PlayingState::new(rx, player_names, false)))
}

fn launch_replay(path: &std::path::Path) -> Option<Screen> {
    use crate::replay::recorder::GameReplay;

    let contents = std::fs::read_to_string(path).ok()?;

    // Try structured replay first.
    if let Ok(replay) = serde_json::from_str::<GameReplay>(&contents) {
        let player_names = replay.player_names.clone();
        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            // Feed replay frames as synthetic UiEvents.
            for frame in &replay.frames {
                let vp_str: String = frame
                    .victory_points
                    .iter()
                    .enumerate()
                    .map(|(p, v)| format!("P{}:{}", p, v))
                    .collect::<Vec<_>>()
                    .join(" ");
                let message = format!("[T{}] {} [{}]", frame.turn, frame.description, vp_str);

                let _ = tx.send(UiEvent::StateUpdate {
                    state: Arc::new(GameState::new(Board::default_board(), replay.num_players)),
                    event: None,
                    message,
                });

                tokio::time::sleep(Duration::from_millis(50)).await;
            }

            // Send game over.
            if let Some(winner) = replay.winner {
                let _ = tx.send(UiEvent::GameOver {
                    winner,
                    message: replay.stats().to_string(),
                });
            }
        });

        return Some(Screen::Playing(PlayingState::new(rx, player_names, false)));
    }

    // Fall back to JSONL event log — simpler playback.
    if let Ok(log) = crate::replay::event::GameLog::read_jsonl(path) {
        let (tx, rx) = mpsc::unbounded_channel();
        let player_names = vec![
            "Player 0".into(),
            "Player 1".into(),
            "Player 2".into(),
            "Player 3".into(),
        ];

        tokio::spawn(async move {
            for event in log.events() {
                let _ = tx.send(UiEvent::StateUpdate {
                    state: Arc::new(GameState::new(Board::default_board(), 4)),
                    event: None,
                    message: format!("{:?}", event),
                });
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });

        return Some(Screen::Playing(PlayingState::new(rx, player_names, false)));
    }

    None
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
