//! Insta snapshot tests for TUI rendering.
//!
//! These tests render each screen/mode to a `TestBackend` buffer, convert
//! to plain text, and compare against saved snapshots. Run `cargo insta review`
//! to accept or reject changes after modifying the UI.

use super::screens::*;
use super::testing::*;
use super::*;

const WIDTH: u16 = 170;
const HEIGHT: u16 = 65;

// ── Non-game screens ─────────────────────────────────────────────────

#[test]
fn snapshot_title_screen() {
    let screen = Screen::Title { frame: 0 };
    let buf = render_to_buffer(&screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("title_screen", buffer_to_string(&buf));
}

#[test]
fn snapshot_title_screen_blink_off() {
    // frame=15 should hide the "press any key" prompt.
    let screen = Screen::Title { frame: 15 };
    let buf = render_to_buffer(&screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("title_screen_blink_off", buffer_to_string(&buf));
}

#[test]
fn snapshot_main_menu() {
    // Use explicit state to avoid filesystem-dependent menu items.
    let state = MainMenuState { selected: 0 };
    let screen = Screen::MainMenu(state);
    let buf = render_to_buffer(&screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("main_menu", buffer_to_string(&buf));
}

#[test]
fn snapshot_new_game() {
    // Set USER env var for deterministic player name.
    std::env::set_var("USER", "Player");
    let screen = Screen::NewGame(NewGameState::new(&[]));
    let buf = render_to_buffer(&screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("new_game", buffer_to_string(&buf));
}

#[test]
fn snapshot_post_game() {
    let app = post_game_app();
    let buf = render_to_buffer(&app.screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("post_game", buffer_to_string(&buf));
}

// ── Playing screen: various input modes ──────────────────────────────

#[test]
fn snapshot_playing_spectating() {
    let app = playing_spectating_app();
    let buf = render_to_buffer(&app.screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("playing_spectating", buffer_to_string(&buf));
}

#[test]
fn snapshot_playing_action_bar() {
    let app = playing_action_bar_app();
    let buf = render_to_buffer(&app.screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("playing_action_bar", buffer_to_string(&buf));
}

#[test]
fn snapshot_playing_trade_builder() {
    let app = playing_trade_builder_app();
    let buf = render_to_buffer(&app.screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("playing_trade_builder", buffer_to_string(&buf));
}

#[test]
fn snapshot_playing_discard() {
    let app = playing_discard_app();
    let buf = render_to_buffer(&app.screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("playing_discard", buffer_to_string(&buf));
}

#[test]
fn snapshot_playing_resource_picker() {
    let (ps, _rx) = make_test_playing_state(InputMode::ResourcePicker {
        context: "Choose a resource for Monopoly".into(),
    });
    let screen = Screen::Playing(ps);
    let buf = render_to_buffer(&screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("playing_resource_picker", buffer_to_string(&buf));
}

#[test]
fn snapshot_playing_steal_target() {
    let (ps, _rx) = make_test_playing_state(InputMode::StealTarget {
        targets: vec![
            (1, "Player 2 (3 cards)".into()),
            (2, "Player 3 (5 cards)".into()),
        ],
        selected: 0,
    });
    let screen = Screen::Playing(ps);
    let buf = render_to_buffer(&screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("playing_steal_target", buffer_to_string(&buf));
}

#[test]
fn snapshot_playing_trade_response() {
    use crate::game::actions::TradeOffer;
    use crate::game::board::Resource;

    let offer = TradeOffer {
        from: 1,
        offering: vec![(Resource::Wood, 2)],
        requesting: vec![(Resource::Ore, 1)],
        message: String::new(),
    };
    let (ps, _rx) = make_test_playing_state(InputMode::TradeResponse { offer });
    let screen = Screen::Playing(ps);
    let buf = render_to_buffer(&screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("playing_trade_response", buffer_to_string(&buf));
}

#[test]
fn snapshot_playing_board_cursor() {
    use crate::game::board::{HexCoord, VertexCoord, VertexDirection};

    let legal = vec![
        VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North),
        VertexCoord::new(HexCoord::new(1, 0), VertexDirection::North),
        VertexCoord::new(HexCoord::new(0, 1), VertexDirection::South),
    ];
    let positions = vec![
        CursorTarget {
            screen_col: 26,
            screen_row: 8,
        },
        CursorTarget {
            screen_col: 34,
            screen_row: 8,
        },
        CursorTarget {
            screen_col: 30,
            screen_row: 14,
        },
    ];
    let (ps, _rx) = make_test_playing_state(InputMode::BoardCursor {
        kind: CursorKind::Settlement,
        legal_vertices: legal,
        legal_edges: Vec::new(),
        legal_hexes: Vec::new(),
        positions,
        selected: 0,
    });
    let screen = Screen::Playing(ps);
    let buf = render_to_buffer(&screen, WIDTH, HEIGHT);
    insta::assert_snapshot!("playing_board_cursor", buffer_to_string(&buf));
}

// ── Responsive layout ────────────────────────────────────────────────

#[test]
fn snapshot_title_small_terminal() {
    let screen = Screen::Title { frame: 0 };
    let buf = render_to_buffer(&screen, 80, 24);
    insta::assert_snapshot!("title_small_terminal", buffer_to_string(&buf));
}

#[test]
fn snapshot_playing_small_terminal() {
    let app = playing_action_bar_app();
    let buf = render_to_buffer(&app.screen, 80, 30);
    insta::assert_snapshot!("playing_action_bar_small", buffer_to_string(&buf));
}
