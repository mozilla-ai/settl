//! Test helpers for TUI unit and snapshot tests.
//!
//! This module is only compiled under `#[cfg(test)]`.

use std::sync::Arc;

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::game::board::Board;
use crate::game::state::GameState;
use crate::player::tui_human::HumanResponse;

use super::board_view::HexGrid;
use super::screens::*;
use super::*;

/// Render an `App` into an in-memory buffer of the given size.
///
/// Uses the same `draw_screen` path as the real event loop, so Playing
/// screens go through the pixel board renderer (or placeholder if the
/// renderer hasn't been initialized, as in most tests).
pub fn render_app_to_buffer(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| draw_screen(f, &app.screen)).unwrap();
    terminal.backend().buffer().clone()
}

/// Convert a ratatui `Buffer` into a plain-text string (one line per row).
/// Strips trailing whitespace from each line for cleaner snapshots.
pub fn buffer_to_string(buf: &Buffer) -> String {
    let area = buf.area;
    let mut lines = Vec::with_capacity(area.height as usize);
    for y in area.y..area.y + area.height {
        let mut line = String::with_capacity(area.width as usize);
        for x in area.x..area.x + area.width {
            let cell = &buf[(x, y)];
            line.push_str(cell.symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    // Trim trailing empty lines.
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Create a `PlayingState` in a given `InputMode` for testing input handling.
///
/// Sets up real channels so `send_response` works. Returns the state and
/// a receiver that captures any `HumanResponse` the input handler sends.
pub fn make_test_playing_state(
    input_mode: InputMode,
) -> (PlayingState, mpsc::UnboundedReceiver<HumanResponse>) {
    let (_ui_tx, ui_rx) = mpsc::unbounded_channel::<UiEvent>();
    let (response_tx, response_rx) = mpsc::unbounded_channel::<HumanResponse>();

    let state = make_test_game_state();

    let mut ps = PlayingState::new(
        ui_rx,
        vec![
            "Alice".into(),
            "Bob".into(),
            "Charlie".into(),
            "Diana".into(),
        ],
        true,
    );
    ps.state = Some(Arc::new(state));
    ps.hex_grid = Some(HexGrid::new());
    ps.input_mode = input_mode;
    ps.human_response_tx = Some(response_tx);

    (ps, response_rx)
}

/// Create a deterministic `GameState` for use in tests.
pub fn make_test_game_state() -> GameState {
    let board = Board::default_board();
    GameState::new(board, 4)
}

/// Create an `App` with a given screen for testing.
pub fn make_test_app(screen: Screen) -> App {
    App {
        screen,
        personalities: vec![],
        llamafile_process: None,
    }
}

/// Create an `App` on the MainMenu screen.
pub fn main_menu_app() -> App {
    make_test_app(Screen::MainMenu(MainMenuState::new()))
}

/// Create an `App` on the NewGame screen.
pub fn new_game_app() -> App {
    make_test_app(Screen::NewGame(NewGameState::new(&[])))
}

/// Create an `App` on the PostGame screen.
pub fn post_game_app() -> App {
    make_test_app(Screen::PostGame(PostGameState {
        winner_name: "Alice".into(),
        winner_index: 0,
        scores: vec![
            ("Alice".into(), 10),
            ("Bob".into(), 7),
            ("Charlie".into(), 5),
            ("Diana".into(), 4),
        ],
        selected: 0,
    }))
}

/// Create an `App` in Playing/Spectating mode with game state.
pub fn playing_spectating_app() -> App {
    let (ps, _rx) = make_test_playing_state(InputMode::Spectating);
    make_test_app(Screen::Playing(ps))
}

/// Create an `App` in Playing/ActionBar mode.
pub fn playing_action_bar_app() -> App {
    let (ps, _rx) = make_test_playing_state(InputMode::ActionBar {
        choices: vec![
            "Build Settlement".into(),
            "Build Road".into(),
            "Buy Development Card".into(),
            "Propose Trade".into(),
            "End Turn".into(),
        ],
        selected: 0,
    });
    make_test_app(Screen::Playing(ps))
}

/// Create an `App` in Playing/TradeBuilder mode.
pub fn playing_trade_builder_app() -> App {
    let (ps, _rx) = make_test_playing_state(InputMode::TradeBuilder {
        give: [0; 5],
        get: [0; 5],
        side: TradeSide::Give,
        available: [3, 2, 1, 4, 2],
        player_id: 0,
    });
    make_test_app(Screen::Playing(ps))
}

/// Create an `App` in Playing/Discard mode.
pub fn playing_discard_app() -> App {
    let (ps, _rx) = make_test_playing_state(InputMode::Discard {
        selected: Vec::new(),
        count: 4,
        remaining: [3, 2, 1, 4, 2],
    });
    make_test_app(Screen::Playing(ps))
}

/// Create an `App` on the LlamafileSetup screen (downloading status).
pub fn llamafile_setup_app() -> App {
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let setup = LlamafileSetupState {
        status: crate::llamafile::LlamafileStatus::Downloading {
            bytes: 524_288_000,
            total: 1_073_741_824,
        },
        status_rx: rx,
        saved_config: NewGameState::new(&[]),
        task_handle: None,
    };
    make_test_app(Screen::LlamafileSetup(setup))
}

/// Create a NewGame app with Llamafile players (for testing kind cycling).
pub fn new_game_llamafile_app() -> App {
    let mut ng = NewGameState::new(&[]);
    // Ensure AI players are Llamafile type.
    for p in ng.players.iter_mut().skip(1) {
        p.kind = PlayerKind::Llamafile;
    }
    make_test_app(Screen::NewGame(ng))
}
