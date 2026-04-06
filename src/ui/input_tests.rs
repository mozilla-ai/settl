//! Tests for TUI input handling across all screens and input modes.

use crossterm::event::{KeyCode, MouseEventKind};

use crate::game::actions::TradeOffer;
use crate::game::board::Resource;
use crate::player::tui_human::HumanResponse;

use super::screens::*;
use super::testing::*;
use super::*;

// ── Size Warning ────────────────────────────────────────────────────

#[test]
fn size_warning_dismissed_on_any_key() {
    let mut app = main_menu_app();
    app.show_size_warning = true;
    let action = handle_input(&mut app, KeyCode::Enter);
    assert!(matches!(action, Action::None));
    assert!(!app.show_size_warning);
}

#[test]
fn size_warning_blocks_underlying_input() {
    let mut app = main_menu_app();
    app.show_size_warning = true;
    // 'q' would normally quit from the main menu, but the warning intercepts it.
    let action = handle_input(&mut app, KeyCode::Char('q'));
    assert!(matches!(action, Action::None));
    assert!(!app.show_size_warning);
}

// ── Docs ────────────────────────────────────────────────────────────

#[test]
fn docs_esc_returns_to_main_menu() {
    let mut app = make_test_app(Screen::Docs(DocsState::new()));
    let action = handle_input(&mut app, KeyCode::Esc);
    assert!(matches!(action, Action::Transition(Screen::MainMenu(_))));
}

#[test]
fn docs_q_returns_to_main_menu() {
    let mut app = make_test_app(Screen::Docs(DocsState::new()));
    let action = handle_input(&mut app, KeyCode::Char('q'));
    assert!(matches!(action, Action::Transition(Screen::MainMenu(_))));
}

#[test]
fn docs_down_advances_page() {
    let mut app = make_test_app(Screen::Docs(DocsState::new()));
    handle_input(&mut app, KeyCode::Down);
    if let Screen::Docs(ref state) = app.screen {
        assert_eq!(state.page_index, 1);
        assert_eq!(state.scroll, 0, "scroll resets on page change");
    } else {
        panic!("should still be on Docs");
    }
}

#[test]
fn docs_up_at_zero_stays() {
    let mut app = make_test_app(Screen::Docs(DocsState::new()));
    handle_input(&mut app, KeyCode::Up);
    if let Screen::Docs(ref state) = app.screen {
        assert_eq!(state.page_index, 0);
    }
}

#[test]
fn docs_jk_scrolls_content() {
    let mut app = make_test_app(Screen::Docs(DocsState::new()));
    handle_input(&mut app, KeyCode::Char('j'));
    handle_input(&mut app, KeyCode::Char('j'));
    if let Screen::Docs(ref state) = app.screen {
        assert_eq!(state.scroll, 2);
    }
    handle_input(&mut app, KeyCode::Char('k'));
    if let Screen::Docs(ref state) = app.screen {
        assert_eq!(state.scroll, 1);
    }
}

// ── Main Menu ────────────────────────────────────────────────────────

#[test]
fn main_menu_down_increments_selection() {
    let mut app = main_menu_app();
    handle_input(&mut app, KeyCode::Down);
    if let Screen::MainMenu(ref state) = app.screen {
        assert_eq!(state.selected, 1);
    } else {
        panic!("should still be on MainMenu");
    }
}

#[test]
fn main_menu_up_wraps_around() {
    let mut app = main_menu_app();
    handle_input(&mut app, KeyCode::Up);
    if let Screen::MainMenu(ref state) = app.screen {
        let items = state.menu_items();
        assert_eq!(state.selected, items.len() - 1);
    } else {
        panic!("should still be on MainMenu");
    }
}

#[test]
fn main_menu_enter_on_new_game() {
    let mut app = main_menu_app();
    // "New Game" is the first item (selected=0), so just press Enter.
    let action = handle_input(&mut app, KeyCode::Enter);
    assert!(
        matches!(action, Action::Transition(Screen::NewGame(_))),
        "Enter on 'New Game' should transition to NewGame"
    );
}

#[test]
fn main_menu_q_quits() {
    let mut app = main_menu_app();
    let action = handle_input(&mut app, KeyCode::Char('q'));
    assert!(matches!(action, Action::Quit));
}

#[test]
fn main_menu_esc_quits() {
    let mut app = main_menu_app();
    let action = handle_input(&mut app, KeyCode::Esc);
    assert!(matches!(action, Action::Quit));
}

// ── New Game ─────────────────────────────────────────────────────────

#[test]
fn new_game_esc_returns_to_main_menu() {
    let mut app = new_game_app();
    let action = handle_input(&mut app, KeyCode::Esc);
    assert!(matches!(action, Action::Transition(Screen::MainMenu(_))));
}

#[test]
fn new_game_player_count_toggle() {
    let mut app = new_game_app();
    // Default is 4 players. Focus starts on StartButton.
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::StartButton);
        assert!(state.four_players);
        assert_eq!(state.num_players(), 4);
    }
    // Move down to PlayerCount, then toggle to 3 players.
    handle_input(&mut app, KeyCode::Down);
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert!(!state.four_players);
        assert_eq!(state.num_players(), 3);
    }
    // Toggle back to 4.
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert!(state.four_players);
        assert_eq!(state.num_players(), 4);
    }
}

#[test]
fn new_game_focus_navigation() {
    let mut app = new_game_app();
    // Start on StartButton. Down should go to PlayerCount.
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::StartButton);
    }
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::PlayerCount);
    }
    // Down to Player { row: 1 }.
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::Player { row: 1 });
    }
    // Down to Player { row: 2 }.
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::Player { row: 2 });
    }
    // Down to Player { row: 3 } (4-player mode).
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::Player { row: 3 });
    }
    // Down to FriendlyRobber.
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::FriendlyRobber);
    }
    // Down to BoardLayout.
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::BoardLayout);
    }
    // Down to AiModel.
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::AiModel);
    }
    // Down to ReasoningEffort.
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::ReasoningEffort);
    }
}

#[test]
fn reasoning_effort_toggle() {
    let mut app = new_game_app();
    // Navigate to ReasoningEffort row.
    // StartButton -> PlayerCount -> P1 -> P2 -> P3 -> FriendlyRobber -> BoardLayout -> AiModel -> ReasoningEffort
    for _ in 0..8 {
        handle_input(&mut app, KeyCode::Down);
    }
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::ReasoningEffort);
        assert_eq!(state.effort_index, 0); // "low"
    }
    // Cycle forward: low -> medium.
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.effort_index, 1); // "medium"
    }
    // Cycle forward: medium -> high.
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.effort_index, 2); // "high"
    }
    // Cycle backward: high -> medium.
    handle_input(&mut app, KeyCode::Left);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.effort_index, 1); // "medium"
    }
}

#[test]
fn reasoning_effort_updates_all_players() {
    let mut app = new_game_app();
    // Navigate to ReasoningEffort row.
    for _ in 0..8 {
        handle_input(&mut app, KeyCode::Down);
    }
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::ReasoningEffort);
    }
    // Cycle forward: low -> medium.
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.effort_index, 1);
        // All player configs should be updated.
        for pc in &state.players {
            assert_eq!(pc.effort_index, 1, "all players should have effort_index=1");
        }
    }
}

#[test]
fn new_game_focus_skips_player_4_in_3_player_mode() {
    let mut app = new_game_app();
    // Move to PlayerCount and toggle to 3-player mode.
    handle_input(&mut app, KeyCode::Down);
    handle_input(&mut app, KeyCode::Right);
    // Navigate: PlayerCount -> Player 2 -> Player 3 -> FriendlyRobber (skip Player 4).
    handle_input(&mut app, KeyCode::Down);
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::Player { row: 2 });
    }
    handle_input(&mut app, KeyCode::Down);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(
            state.focus,
            NewGameFocus::FriendlyRobber,
            "should skip Player 4 in 3-player mode"
        );
    }
}

#[test]
fn new_game_player_0_is_always_human() {
    let app = new_game_app();
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(
            state.players[0].kind,
            PlayerKind::Human,
            "player 0 should always be Human"
        );
    }
}

#[test]
fn llamafile_setup_esc_returns_to_new_game() {
    let mut app = llamafile_setup_app();
    let action = handle_input(&mut app, KeyCode::Esc);
    assert!(
        matches!(action, Action::Transition(Screen::NewGame(_))),
        "Esc on LlamafileSetup should return to NewGame"
    );
}

#[test]
fn new_game_ai_players_are_llamafile() {
    let app = new_game_app();
    if let Screen::NewGame(ref state) = app.screen {
        for i in 1..state.players.len() {
            assert_eq!(
                state.players[i].kind,
                PlayerKind::Llamafile,
                "AI player {} should be Llamafile",
                i
            );
        }
    }
}

#[test]
fn new_game_llamafile_personality_cycles() {
    let mut app = new_game_llamafile_app();
    // Focus on row 1 (a Llamafile player).
    if let Screen::NewGame(ref mut state) = app.screen {
        state.focus = NewGameFocus::Player { row: 1 };
    }
    let initial = if let Screen::NewGame(ref state) = app.screen {
        state.players[1].personality_index
    } else {
        panic!("should be on NewGame");
    };
    // Cycle personality forward.
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.players[1].personality_index, initial + 1);
    }
}

#[test]
fn new_game_friendly_robber_toggle() {
    let mut app = new_game_app();
    if let Screen::NewGame(ref mut state) = app.screen {
        state.focus = NewGameFocus::FriendlyRobber;
    }
    if let Screen::NewGame(ref state) = app.screen {
        assert!(!state.friendly_robber);
    }
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert!(state.friendly_robber);
    }
}

#[test]
fn new_game_board_layout_toggle() {
    let mut app = new_game_app();
    if let Screen::NewGame(ref mut state) = app.screen {
        state.focus = NewGameFocus::BoardLayout;
    }
    if let Screen::NewGame(ref state) = app.screen {
        assert!(!state.random_board);
    }
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert!(state.random_board);
    }
}

#[test]
fn new_game_model_toggle() {
    let mut app = new_game_app();
    if let Screen::NewGame(ref mut state) = app.screen {
        state.focus = NewGameFocus::AiModel;
    }
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.model_index, 0);
    }
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.model_index, 1);
    }
    handle_input(&mut app, KeyCode::Right);
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.model_index, 0);
    }
}

#[test]
fn new_game_enter_starts_game_from_any_focus() {
    // Enter should trigger StartGame regardless of which row is focused.
    for focus in &[
        NewGameFocus::StartButton,
        NewGameFocus::PlayerCount,
        NewGameFocus::Player { row: 1 },
        NewGameFocus::FriendlyRobber,
        NewGameFocus::BoardLayout,
        NewGameFocus::AiModel,
    ] {
        let mut app = new_game_app();
        if let Screen::NewGame(ref mut state) = app.screen {
            state.focus = *focus;
        }
        let action = handle_input(&mut app, KeyCode::Enter);
        assert!(
            matches!(action, Action::StartGame),
            "Enter on {:?} should trigger StartGame",
            focus,
        );
    }
}

#[test]
fn new_game_ram_warning_enter_proceeds() {
    let mut app = new_game_app();
    if let Screen::NewGame(ref mut state) = app.screen {
        state.ram_warning = Some((12, 6));
    }
    // Enter should dismiss warning and trigger StartGame.
    let action = handle_input(&mut app, KeyCode::Enter);
    assert!(matches!(action, Action::StartGame));
    if let Screen::NewGame(ref state) = app.screen {
        assert!(state.ram_warning.is_none(), "warning should be cleared");
    }
}

#[test]
fn new_game_ram_warning_esc_dismisses() {
    let mut app = new_game_app();
    if let Screen::NewGame(ref mut state) = app.screen {
        state.ram_warning = Some((12, 6));
    }
    // Esc should just dismiss the warning, not navigate away.
    let action = handle_input(&mut app, KeyCode::Esc);
    assert!(matches!(action, Action::None));
    if let Screen::NewGame(ref state) = app.screen {
        assert!(state.ram_warning.is_none(), "warning should be cleared");
    }
}

// ── ActionBar ────────────────────────────────────────────────────────

#[test]
fn action_bar_right_increments_selection() {
    let (ps, _rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices_minimal(),
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Right);
    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::ActionBar { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 1);
        } else {
            panic!("should still be in ActionBar mode");
        }
    }
}

#[test]
fn action_bar_left_at_zero_stays() {
    let (ps, _rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices_minimal(),
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Left);
    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::ActionBar { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 0);
        }
    }
}

#[test]
fn action_bar_enter_sends_index_and_returns_to_spectating() {
    let (ps, mut rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices_minimal(),
        selected: 1,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Enter);

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(1)));

    if let Screen::Playing(ref ps) = app.screen {
        assert!(matches!(ps.input_mode, InputMode::Spectating));
    }
}

#[test]
fn action_bar_shortcut_e_selects_end_turn() {
    // [Settlement=0, Road=1, EndTurn=2]
    let (ps, mut rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices_minimal(),
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('e'));

    let resp = rx.try_recv().unwrap();
    assert!(
        matches!(resp, HumanResponse::Index(2)),
        "'e' should select End Turn (index 2)"
    );
}

#[test]
fn action_bar_shortcut_s_selects_build_settlement() {
    // [Settlement=0, Road=1, EndTurn=2]
    let (ps, mut rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices_minimal(),
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('s'));

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(0)));
}

#[test]
fn action_bar_shortcut_r_selects_build_road() {
    // [Settlement=0, Road=1, EndTurn=2]
    let (ps, mut rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices_minimal(),
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('r'));

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(1)));
}

#[test]
fn action_bar_shortcut_t_selects_propose_trade() {
    // [Settlement=0, Road=1, DevCard=2, Trade=3, BankTrade=4, EndTurn=5]
    let (ps, mut rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices(),
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('t'));

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(3)));
}

#[test]
fn action_bar_shortcut_d_selects_buy_dev_card() {
    // [Settlement=0, Road=1, DevCard=2, Trade=3, BankTrade=4, EndTurn=5]
    let (ps, mut rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices(),
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('d'));

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(2)));
}

#[test]
fn action_bar_shortcut_b_selects_bank_trade() {
    // [Settlement=0, Road=1, DevCard=2, Trade=3, BankTrade=4, EndTurn=5]
    let (ps, mut rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices(),
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('b'));

    let resp = rx.try_recv().unwrap();
    assert!(
        matches!(resp, HumanResponse::Index(4)),
        "'b' should select Bank Trade (index 4)"
    );
}

#[test]
fn action_bar_esc_selects_end_turn() {
    // [Settlement=0, Road=1, EndTurn=2]
    let (ps, mut rx) = make_test_playing_state(InputMode::ActionBar {
        choices: test_action_choices_minimal(),
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Esc);

    let resp = rx.try_recv().unwrap();
    assert!(
        matches!(resp, HumanResponse::Index(2)),
        "Esc should select End Turn"
    );
}

// ── TradeBuilder ─────────────────────────────────────────────────────

#[test]
fn trade_builder_resource_keys_add_to_give() {
    let (ps, _rx) = make_test_playing_state(InputMode::TradeBuilder {
        give: [0; 5],
        get: [0; 5],
        side: TradeSide::Give,
        available: [3, 2, 1, 4, 2],
        player_id: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('w'));
    handle_input(&mut app, KeyCode::Char('w'));

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::TradeBuilder { give, .. } = &ps.input_mode {
            assert_eq!(give[0], 2, "two 'w' presses should give 2 wood");
        }
    }
}

#[test]
fn trade_builder_give_capped_at_available() {
    let (ps, _rx) = make_test_playing_state(InputMode::TradeBuilder {
        give: [0; 5],
        get: [0; 5],
        side: TradeSide::Give,
        available: [1, 0, 0, 0, 0],
        player_id: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    // Try to add 3 wood when only 1 available.
    handle_input(&mut app, KeyCode::Char('w'));
    handle_input(&mut app, KeyCode::Char('w'));
    handle_input(&mut app, KeyCode::Char('w'));

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::TradeBuilder { give, .. } = &ps.input_mode {
            assert_eq!(give[0], 1, "should be capped at available (1)");
        }
    }
}

#[test]
fn trade_builder_tab_switches_side() {
    let (ps, _rx) = make_test_playing_state(InputMode::TradeBuilder {
        give: [0; 5],
        get: [0; 5],
        side: TradeSide::Give,
        available: [3, 2, 1, 4, 2],
        player_id: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Tab);

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::TradeBuilder { side, .. } = &ps.input_mode {
            assert_eq!(*side, TradeSide::Get);
        }
    }
}

#[test]
fn trade_builder_backspace_removes_last() {
    let (ps, _rx) = make_test_playing_state(InputMode::TradeBuilder {
        give: [2, 1, 0, 0, 0],
        get: [0; 5],
        side: TradeSide::Give,
        available: [3, 2, 1, 4, 2],
        player_id: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Backspace);

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::TradeBuilder { give, .. } = &ps.input_mode {
            // Backspace removes highest-index resource with count > 0.
            assert_eq!(give[1], 0, "brick should be decremented");
            assert_eq!(give[0], 2, "wood should be unchanged");
        }
    }
}

#[test]
fn trade_builder_enter_sends_trade_offer() {
    let (ps, mut rx) = make_test_playing_state(InputMode::TradeBuilder {
        give: [1, 0, 0, 0, 0],
        get: [0, 0, 0, 0, 1],
        side: TradeSide::Give,
        available: [3, 2, 1, 4, 2],
        player_id: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Enter);

    let resp = rx.try_recv().unwrap();
    if let HumanResponse::Trade(Some(offer)) = resp {
        assert_eq!(offer.from, 0);
        assert_eq!(offer.offering, vec![(Resource::Wood, 1)]);
        assert_eq!(offer.requesting, vec![(Resource::Ore, 1)]);
    } else {
        panic!("expected Trade response with offer, got {:?}", resp);
    }
}

#[test]
fn trade_builder_enter_requires_both_sides() {
    let (ps, mut rx) = make_test_playing_state(InputMode::TradeBuilder {
        give: [1, 0, 0, 0, 0],
        get: [0; 5], // Nothing on get side
        side: TradeSide::Give,
        available: [3, 2, 1, 4, 2],
        player_id: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Enter);

    // Should not send anything since get side is empty.
    assert!(rx.try_recv().is_err());
}

#[test]
fn trade_builder_esc_cancels() {
    let (ps, mut rx) = make_test_playing_state(InputMode::TradeBuilder {
        give: [1, 0, 0, 0, 0],
        get: [0, 0, 0, 0, 1],
        side: TradeSide::Give,
        available: [3, 2, 1, 4, 2],
        player_id: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Esc);

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Trade(None)));
}

// ── Discard ──────────────────────────────────────────────────────────

#[test]
fn discard_resource_keys_add_to_selection() {
    let (ps, _rx) = make_test_playing_state(InputMode::Discard {
        selected: Vec::new(),
        count: 4,
        remaining: [3, 2, 1, 4, 2],
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('w'));
    handle_input(&mut app, KeyCode::Char('b'));

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::Discard {
            selected,
            remaining,
            ..
        } = &ps.input_mode
        {
            assert_eq!(selected.len(), 2);
            assert_eq!(selected[0], Resource::Wood);
            assert_eq!(selected[1], Resource::Brick);
            assert_eq!(remaining[0], 2, "wood remaining should decrease");
            assert_eq!(remaining[1], 1, "brick remaining should decrease");
        }
    }
}

#[test]
fn discard_stops_at_count() {
    let (ps, _rx) = make_test_playing_state(InputMode::Discard {
        selected: Vec::new(),
        count: 2,
        remaining: [3, 2, 1, 4, 2],
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('w'));
    handle_input(&mut app, KeyCode::Char('w'));
    handle_input(&mut app, KeyCode::Char('w')); // Should be ignored, count reached.

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::Discard { selected, .. } = &ps.input_mode {
            assert_eq!(selected.len(), 2, "should stop at count");
        }
    }
}

#[test]
fn discard_backspace_removes_last() {
    let (ps, _rx) = make_test_playing_state(InputMode::Discard {
        selected: vec![Resource::Wood, Resource::Brick],
        count: 4,
        remaining: [2, 1, 1, 4, 2],
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Backspace);

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::Discard {
            selected,
            remaining,
            ..
        } = &ps.input_mode
        {
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0], Resource::Wood);
            assert_eq!(remaining[1], 2, "brick remaining should increase");
        }
    }
}

#[test]
fn discard_enter_sends_when_count_met() {
    let (ps, mut rx) = make_test_playing_state(InputMode::Discard {
        selected: vec![Resource::Wood, Resource::Brick],
        count: 2,
        remaining: [2, 1, 1, 4, 2],
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Enter);

    let resp = rx.try_recv().unwrap();
    if let HumanResponse::Resources(resources) = resp {
        assert_eq!(resources.len(), 2);
    } else {
        panic!("expected Resources response");
    }
}

#[test]
fn discard_enter_does_nothing_when_count_not_met() {
    let (ps, mut rx) = make_test_playing_state(InputMode::Discard {
        selected: vec![Resource::Wood],
        count: 2,
        remaining: [2, 2, 1, 4, 2],
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Enter);

    assert!(rx.try_recv().is_err(), "should not send when count not met");
}

#[test]
fn discard_esc_auto_fills_and_sends() {
    let (ps, mut rx) = make_test_playing_state(InputMode::Discard {
        selected: vec![Resource::Wood],
        count: 3,
        remaining: [2, 2, 1, 4, 2],
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Esc);

    let resp = rx.try_recv().unwrap();
    if let HumanResponse::Resources(resources) = resp {
        assert_eq!(resources.len(), 3, "Esc should auto-fill to count");
    } else {
        panic!("expected Resources response");
    }
}

// ── ResourcePicker ───────────────────────────────────────────────────

#[test]
fn resource_picker_w_selects_wood() {
    let (ps, mut rx) = make_test_playing_state(InputMode::ResourcePicker {
        context: "Choose a resource for Monopoly".into(),
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('w'));

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(0))); // Wood = 0
}

#[test]
fn resource_picker_o_selects_ore() {
    let (ps, mut rx) = make_test_playing_state(InputMode::ResourcePicker {
        context: "Choose a resource".into(),
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('o'));

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(4))); // Ore = 4
}

#[test]
fn resource_picker_esc_defaults_to_wood() {
    let (ps, mut rx) = make_test_playing_state(InputMode::ResourcePicker {
        context: "Choose a resource".into(),
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Esc);

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(0))); // Defaults to Wood
}

// ── StealTarget ──────────────────────────────────────────────────────

#[test]
fn steal_target_up_down_navigation() {
    let targets = vec![(1, "Bob (3 cards)".into()), (2, "Charlie (5 cards)".into())];
    let (ps, _rx) = make_test_playing_state(InputMode::StealTarget {
        targets,
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Down);

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::StealTarget { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 1);
        }
    }
}

#[test]
fn steal_target_number_key_selects_player() {
    let targets = vec![
        (0, "Alice (3 cards)".into()),
        (2, "Charlie (5 cards)".into()),
    ];
    let (ps, mut rx) = make_test_playing_state(InputMode::StealTarget {
        targets,
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    // '3' should select player_id=2, which is at index 1 in targets.
    handle_input(&mut app, KeyCode::Char('3'));

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(1)));
}

#[test]
fn steal_target_enter_confirms_selection() {
    let targets = vec![(1, "Bob".into()), (2, "Charlie".into())];
    let (ps, mut rx) = make_test_playing_state(InputMode::StealTarget {
        targets,
        selected: 1,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Enter);

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(1)));
}

// ── TradeResponse ────────────────────────────────────────────────────

#[test]
fn trade_response_y_accepts() {
    let offer = TradeOffer {
        from: 1,
        offering: vec![(Resource::Wood, 1)],
        requesting: vec![(Resource::Ore, 1)],
        message: String::new(),
    };
    let (ps, mut rx) = make_test_playing_state(InputMode::TradeResponse { offer });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('y'));

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::TradeAnswer(true)));
}

#[test]
fn trade_response_n_rejects() {
    let offer = TradeOffer {
        from: 1,
        offering: vec![(Resource::Wood, 1)],
        requesting: vec![(Resource::Ore, 1)],
        message: String::new(),
    };
    let (ps, mut rx) = make_test_playing_state(InputMode::TradeResponse { offer });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('n'));

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::TradeAnswer(false)));
}

#[test]
fn trade_response_enter_accepts() {
    let offer = TradeOffer {
        from: 1,
        offering: vec![(Resource::Wood, 1)],
        requesting: vec![(Resource::Ore, 1)],
        message: String::new(),
    };
    let (ps, mut rx) = make_test_playing_state(InputMode::TradeResponse { offer });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Enter);

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::TradeAnswer(true)));
}

#[test]
fn trade_response_esc_rejects() {
    let offer = TradeOffer {
        from: 1,
        offering: vec![(Resource::Wood, 1)],
        requesting: vec![(Resource::Ore, 1)],
        message: String::new(),
    };
    let (ps, mut rx) = make_test_playing_state(InputMode::TradeResponse { offer });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Esc);

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::TradeAnswer(false)));
}

// ── Spectating ───────────────────────────────────────────────────────

#[test]
fn spectating_space_toggles_pause() {
    let (ps, _rx) = make_test_playing_state(InputMode::Spectating);
    let mut app = make_test_app(Screen::Playing(ps));

    assert!(!match &app.screen {
        Screen::Playing(ps) => ps.paused,
        _ => panic!(),
    });

    handle_input(&mut app, KeyCode::Char(' '));

    assert!(match &app.screen {
        Screen::Playing(ps) => ps.paused,
        _ => panic!(),
    });
}

#[test]
fn spectating_tab_toggles_right_panel_tab() {
    let (ps, _rx) = make_test_playing_state(InputMode::Spectating);
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Tab);

    assert!(match &app.screen {
        Screen::Playing(ps) => ps.right_tab == RightPanelTab::Ai,
        _ => panic!(),
    });

    handle_input(&mut app, KeyCode::Tab);

    assert!(match &app.screen {
        Screen::Playing(ps) => ps.right_tab == RightPanelTab::Game,
        _ => panic!(),
    });
}

#[test]
fn spectating_l_toggles_llamafile_log() {
    let (mut ps, _rx) = make_test_playing_state(InputMode::Spectating);
    // Set a log buffer so the L key is active.
    ps.llamafile_log = Some(std::sync::Arc::new(std::sync::Mutex::new(vec![
        "test log line".into(),
    ])));
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('L'));
    assert!(match &app.screen {
        Screen::Playing(ps) => ps.show_llamafile_log,
        _ => panic!(),
    });

    handle_input(&mut app, KeyCode::Char('L'));
    assert!(!match &app.screen {
        Screen::Playing(ps) => ps.show_llamafile_log,
        _ => panic!(),
    });
}

#[test]
fn spectating_l_ignored_without_llamafile() {
    let (ps, _rx) = make_test_playing_state(InputMode::Spectating);
    // No llamafile_log set (None) -- L should do nothing.
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('L'));
    assert!(!match &app.screen {
        Screen::Playing(ps) => ps.show_llamafile_log,
        _ => panic!(),
    });
}

#[test]
fn spectating_jk_scrolls_chat_when_ai_tab_active() {
    let (mut ps, _rx) = make_test_playing_state(InputMode::Spectating);
    ps.right_tab = RightPanelTab::Ai;
    ps.chat_scroll = 5;
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('j'));
    let chat_scroll = match &app.screen {
        Screen::Playing(ps) => ps.chat_scroll,
        _ => panic!(),
    };
    assert_eq!(chat_scroll, 6);

    handle_input(&mut app, KeyCode::Char('k'));
    let chat_scroll = match &app.screen {
        Screen::Playing(ps) => ps.chat_scroll,
        _ => panic!(),
    };
    assert_eq!(chat_scroll, 5);
}

#[test]
fn spectating_q_returns_to_main_menu() {
    let (ps, _rx) = make_test_playing_state(InputMode::Spectating);
    let mut app = make_test_app(Screen::Playing(ps));

    let action = handle_input(&mut app, KeyCode::Char('q'));

    assert!(matches!(action, Action::Transition(Screen::MainMenu(_))));
}

// ── Mouse Scroll ─────────────────────────────────────────────────────

#[test]
fn mouse_scroll_down_scrolls_chat_when_ai_tab_active() {
    let (mut ps, _rx) = make_test_playing_state(InputMode::Spectating);
    ps.right_tab = RightPanelTab::Ai;
    ps.chat_scroll = 0;

    handle_mouse_scroll(&mut ps, MouseEventKind::ScrollDown);
    assert_eq!(ps.chat_scroll, MOUSE_SCROLL_LINES);

    handle_mouse_scroll(&mut ps, MouseEventKind::ScrollDown);
    assert_eq!(ps.chat_scroll, MOUSE_SCROLL_LINES * 2);
}

#[test]
fn mouse_scroll_up_scrolls_chat_when_ai_tab_active() {
    let (mut ps, _rx) = make_test_playing_state(InputMode::Spectating);
    ps.right_tab = RightPanelTab::Ai;
    ps.chat_scroll = 10;

    handle_mouse_scroll(&mut ps, MouseEventKind::ScrollUp);
    assert_eq!(ps.chat_scroll, 10 - MOUSE_SCROLL_LINES);
}

#[test]
fn mouse_scroll_affects_game_log_when_game_tab_active() {
    let (mut ps, _rx) = make_test_playing_state(InputMode::Spectating);
    ps.right_tab = RightPanelTab::Game;
    ps.log_scroll = 0;

    handle_mouse_scroll(&mut ps, MouseEventKind::ScrollDown);
    assert_eq!(ps.log_scroll, MOUSE_SCROLL_LINES);
}

#[test]
fn mouse_scroll_affects_llamafile_log_when_visible() {
    let (mut ps, _rx) = make_test_playing_state(InputMode::Spectating);
    ps.show_llamafile_log = true;
    ps.llamafile_log_scroll = 10;

    handle_mouse_scroll(&mut ps, MouseEventKind::ScrollDown);
    assert_eq!(ps.llamafile_log_scroll, 10 + MOUSE_SCROLL_LINES);

    handle_mouse_scroll(&mut ps, MouseEventKind::ScrollUp);
    assert_eq!(ps.llamafile_log_scroll, 10);
}

#[test]
fn mouse_scroll_up_does_not_underflow() {
    let (mut ps, _rx) = make_test_playing_state(InputMode::Spectating);
    ps.right_tab = RightPanelTab::Ai;
    ps.chat_scroll = 1;

    handle_mouse_scroll(&mut ps, MouseEventKind::ScrollUp);
    assert_eq!(ps.chat_scroll, 0);
}

// ── PostGame ─────────────────────────────────────────────────────────

#[test]
fn post_game_navigation() {
    let mut app = post_game_app();

    handle_input(&mut app, KeyCode::Down);
    if let Screen::PostGame(ref state) = app.screen {
        assert_eq!(state.selected, 1);
    }

    handle_input(&mut app, KeyCode::Up);
    if let Screen::PostGame(ref state) = app.screen {
        assert_eq!(state.selected, 0);
    }
}

#[test]
fn post_game_quit() {
    let mut app = post_game_app();
    let action = handle_input(&mut app, KeyCode::Char('q'));
    assert!(matches!(action, Action::Quit));
}

// ── BoardCursor ──────────────────────────────────────────────────────

#[test]
fn board_cursor_n_cycles_forward() {
    let positions = vec![
        CursorTarget {
            screen_col: 10,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 20,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 30,
            screen_row: 5,
        },
    ];
    let (ps, _rx) = make_test_playing_state(InputMode::BoardCursor {
        legal: CursorLegal::Settlements(Vec::new()),
        positions,
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('n'));

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::BoardCursor { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 1);
        }
    }
}

#[test]
fn board_cursor_n_wraps_around() {
    let positions = vec![
        CursorTarget {
            screen_col: 10,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 20,
            screen_row: 5,
        },
    ];
    let (ps, _rx) = make_test_playing_state(InputMode::BoardCursor {
        legal: CursorLegal::Settlements(Vec::new()),
        positions,
        selected: 1,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('n'));

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::BoardCursor { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 0, "should wrap to beginning");
        }
    }
}

#[test]
fn board_cursor_p_cycles_backward() {
    let positions = vec![
        CursorTarget {
            screen_col: 10,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 20,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 30,
            screen_row: 5,
        },
    ];
    let (ps, _rx) = make_test_playing_state(InputMode::BoardCursor {
        legal: CursorLegal::Settlements(Vec::new()),
        positions,
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Char('p'));

    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::BoardCursor { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 2, "should wrap to end");
        }
    }
}

#[test]
fn board_cursor_hjkl_navigates() {
    // Three positions in a row (left, center, right) and one below center.
    let positions = vec![
        CursorTarget {
            screen_col: 10,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 20,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 30,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 20,
            screen_row: 10,
        },
    ];
    let (ps, _rx) = make_test_playing_state(InputMode::BoardCursor {
        legal: CursorLegal::Settlements(Vec::new()),
        positions,
        selected: 1, // Start at center (col=20, row=5)
    });
    let mut app = make_test_app(Screen::Playing(ps));

    // 'l' should move right (to index 2, col=30).
    handle_input(&mut app, KeyCode::Char('l'));
    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::BoardCursor { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 2, "'l' should move right");
        }
    }

    // 'h' should move left (back to index 1, col=20).
    handle_input(&mut app, KeyCode::Char('h'));
    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::BoardCursor { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 1, "'h' should move left");
        }
    }

    // 'j' should move down (to index 3, row=10).
    handle_input(&mut app, KeyCode::Char('j'));
    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::BoardCursor { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 3, "'j' should move down");
        }
    }

    // 'k' should move up (back to index 1, row=5).
    handle_input(&mut app, KeyCode::Char('k'));
    if let Screen::Playing(ref ps) = app.screen {
        if let InputMode::BoardCursor { selected, .. } = &ps.input_mode {
            assert_eq!(*selected, 1, "'k' should move up");
        }
    }
}

#[test]
fn board_cursor_enter_sends_selected_index() {
    let positions = vec![
        CursorTarget {
            screen_col: 10,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 20,
            screen_row: 5,
        },
    ];
    let (ps, mut rx) = make_test_playing_state(InputMode::BoardCursor {
        legal: CursorLegal::Settlements(Vec::new()),
        positions,
        selected: 1,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Enter);

    let resp = rx.try_recv().unwrap();
    assert!(matches!(resp, HumanResponse::Index(1)));
}

#[test]
fn board_cursor_esc_does_not_send_response() {
    let positions = vec![
        CursorTarget {
            screen_col: 10,
            screen_row: 5,
        },
        CursorTarget {
            screen_col: 20,
            screen_row: 5,
        },
    ];
    let (ps, mut rx) = make_test_playing_state(InputMode::BoardCursor {
        legal: CursorLegal::Settlements(Vec::new()),
        positions,
        selected: 0,
    });
    let mut app = make_test_app(Screen::Playing(ps));

    handle_input(&mut app, KeyCode::Esc);

    // Esc should NOT send a response -- placement is mandatory.
    assert!(rx.try_recv().is_err(), "Esc should not confirm placement");
    // Should remain in BoardCursor mode.
    if let Screen::Playing(ps) = &app.screen {
        assert!(matches!(ps.input_mode, InputMode::BoardCursor { .. }));
    } else {
        panic!("Expected Playing screen");
    }
}

// ── Personalities Screen ────────────────────────────────────────────

#[test]
fn personalities_esc_returns_to_main_menu() {
    let mut app = personalities_app();
    let action = handle_input(&mut app, KeyCode::Esc);
    assert!(matches!(action, Action::Transition(Screen::MainMenu(_))));
}

#[test]
fn personalities_jk_navigates_list() {
    let mut app = personalities_app();
    handle_input(&mut app, KeyCode::Char('j'));
    if let Screen::Personalities(ref state) = app.screen {
        assert_eq!(state.selected, 1);
    } else {
        panic!("Expected Personalities screen");
    }
    handle_input(&mut app, KeyCode::Char('k'));
    if let Screen::Personalities(ref state) = app.screen {
        assert_eq!(state.selected, 0);
    } else {
        panic!("Expected Personalities screen");
    }
}

#[test]
fn personalities_k_at_zero_stays() {
    let mut app = personalities_app();
    handle_input(&mut app, KeyCode::Char('k'));
    if let Screen::Personalities(ref state) = app.screen {
        assert_eq!(state.selected, 0);
    }
}

#[test]
fn personalities_tab_switches_to_detail() {
    let mut app = personalities_app();
    handle_input(&mut app, KeyCode::Tab);
    if let Screen::Personalities(ref state) = app.screen {
        assert!(matches!(state.focus, PersonalitiesFocus::Detail));
    } else {
        panic!("Expected Personalities screen");
    }
}

#[test]
fn personalities_detail_tab_returns_to_list() {
    let mut app = personalities_app();
    // Switch to detail.
    handle_input(&mut app, KeyCode::Tab);
    // Switch back to list.
    handle_input(&mut app, KeyCode::Tab);
    if let Screen::Personalities(ref state) = app.screen {
        assert!(matches!(state.focus, PersonalitiesFocus::List));
    }
}

#[test]
fn personalities_detail_jk_scrolls() {
    let mut app = personalities_app();
    handle_input(&mut app, KeyCode::Tab);
    handle_input(&mut app, KeyCode::Char('j'));
    handle_input(&mut app, KeyCode::Char('j'));
    if let Screen::Personalities(ref state) = app.screen {
        assert_eq!(state.detail_scroll, 2);
    }
    handle_input(&mut app, KeyCode::Char('k'));
    if let Screen::Personalities(ref state) = app.screen {
        assert_eq!(state.detail_scroll, 1);
    }
}

#[test]
fn personalities_enter_on_builtin_shows_detail() {
    let mut app = personalities_app();
    // First item is built-in, Enter should switch to Detail (not edit).
    handle_input(&mut app, KeyCode::Enter);
    if let Screen::Personalities(ref state) = app.screen {
        assert!(
            matches!(state.focus, PersonalitiesFocus::Detail),
            "Enter on built-in should switch to Detail view"
        );
    }
}

#[test]
fn personalities_d_on_builtin_is_noop() {
    let mut app = personalities_app();
    // Try to delete a built-in personality.
    handle_input(&mut app, KeyCode::Char('d'));
    if let Screen::Personalities(ref state) = app.screen {
        assert!(
            matches!(state.focus, PersonalitiesFocus::List),
            "Should stay in List mode"
        );
    }
}

#[test]
fn personalities_duplicate_creates_custom() {
    let mut app = personalities_app();
    let initial_count = if let Screen::Personalities(ref state) = app.screen {
        state.entries.len()
    } else {
        panic!("Expected Personalities screen");
    };
    handle_input(&mut app, KeyCode::Char('D'));
    if let Screen::Personalities(ref state) = app.screen {
        assert_eq!(state.entries.len(), initial_count + 1);
        assert!(state.selected_is_custom());
        // Should be in edit mode.
        assert!(matches!(
            state.focus,
            PersonalitiesFocus::EditText(PersonalityField::Name)
        ));
    }
}

#[test]
fn personalities_new_creates_custom() {
    let mut app = personalities_app();
    let initial_count = if let Screen::Personalities(ref state) = app.screen {
        state.entries.len()
    } else {
        panic!("Expected Personalities screen");
    };
    handle_input(&mut app, KeyCode::Char('n'));
    if let Screen::Personalities(ref state) = app.screen {
        assert_eq!(state.entries.len(), initial_count + 1);
        assert!(state.selected_is_custom());
        assert!(matches!(
            state.focus,
            PersonalitiesFocus::EditText(PersonalityField::Name)
        ));
    }
}

#[test]
fn personalities_edit_text_esc_cancels() {
    let mut app = personalities_app();
    // Create a new personality to enter edit mode.
    handle_input(&mut app, KeyCode::Char('n'));
    // Should be in EditText(Name).
    handle_input(&mut app, KeyCode::Esc);
    if let Screen::Personalities(ref state) = app.screen {
        assert!(matches!(state.focus, PersonalitiesFocus::List));
    }
}

#[test]
fn personalities_edit_text_enter_advances() {
    let mut app = personalities_app();
    handle_input(&mut app, KeyCode::Char('n'));
    // In EditText(Name), Enter should advance to EditText(Style).
    handle_input(&mut app, KeyCode::Enter);
    if let Screen::Personalities(ref state) = app.screen {
        assert!(matches!(
            state.focus,
            PersonalitiesFocus::EditText(PersonalityField::Style)
        ));
    }
}

#[test]
fn personalities_edit_slider_advances() {
    let mut app = personalities_app();
    handle_input(&mut app, KeyCode::Char('n'));
    // Name -> Style -> Aggression.
    handle_input(&mut app, KeyCode::Enter);
    handle_input(&mut app, KeyCode::Enter);
    if let Screen::Personalities(ref state) = app.screen {
        assert!(matches!(
            state.focus,
            PersonalitiesFocus::EditSlider(PersonalityField::Aggression)
        ));
    }
    // Adjust slider right.
    handle_input(&mut app, KeyCode::Right);
    if let Screen::Personalities(ref state) = app.screen {
        let (p, _) = &state.entries[state.selected];
        assert!((p.aggression - 0.6).abs() < 0.01);
    }
    // Advance to Cooperation.
    handle_input(&mut app, KeyCode::Enter);
    if let Screen::Personalities(ref state) = app.screen {
        assert!(matches!(
            state.focus,
            PersonalitiesFocus::EditSlider(PersonalityField::Cooperation)
        ));
    }
    // Advance to Catchphrases.
    handle_input(&mut app, KeyCode::Enter);
    if let Screen::Personalities(ref state) = app.screen {
        assert!(matches!(state.focus, PersonalitiesFocus::EditCatchphrases));
    }
}

#[test]
fn personalities_edit_catchphrases_add_and_done() {
    let mut app = personalities_app();
    handle_input(&mut app, KeyCode::Char('n'));
    // Skip to catchphrases: Name -> Style -> Agg -> Coop -> Catchphrases.
    handle_input(&mut app, KeyCode::Enter); // commit Name
    handle_input(&mut app, KeyCode::Enter); // commit Style
    handle_input(&mut app, KeyCode::Enter); // commit Aggression
    handle_input(&mut app, KeyCode::Enter); // commit Cooperation
                                            // Now in EditCatchphrases.
    assert!(matches!(
        app.screen,
        Screen::Personalities(PersonalitiesState {
            focus: PersonalitiesFocus::EditCatchphrases,
            ..
        })
    ));
    // Add a catchphrase.
    handle_input(&mut app, KeyCode::Char('a'));
    handle_input(&mut app, KeyCode::Char('H'));
    handle_input(&mut app, KeyCode::Char('i'));
    handle_input(&mut app, KeyCode::Enter);
    if let Screen::Personalities(ref state) = app.screen {
        let (p, _) = &state.entries[state.selected];
        assert_eq!(p.catchphrases.len(), 1);
        assert_eq!(p.catchphrases[0], "Hi");
    }
    // Esc returns to List.
    handle_input(&mut app, KeyCode::Esc);
    if let Screen::Personalities(ref state) = app.screen {
        assert!(matches!(state.focus, PersonalitiesFocus::List));
    }
}
