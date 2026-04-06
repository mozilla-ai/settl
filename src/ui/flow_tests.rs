//! E2E flow tests for the TUI.
//!
//! These tests exercise the full game flow: orchestrator spawning, channel
//! communication between engine and human player, screen transitions, setup
//! phase, turn cycles, and game completion.
//!
//! The key pattern: create an orchestrator with 1 TuiHumanPlayer (channel-based)
//! + 3 RandomPlayers, spawn it as an async task, then act as the "TUI" by
//!   receiving HumanPrompts and sending HumanResponses back.

use std::sync::Arc;
use std::time::Duration;

use crossterm::event::KeyCode;
use tokio::sync::{mpsc, Mutex};

use crate::game::board::{Board, Resource};
use crate::game::orchestrator::{GameOrchestrator, OrchestratorError};
use crate::game::state::GameState;
use crate::player::random::RandomPlayer;
use crate::player::tui_human::{
    HumanInputChannel, HumanPrompt, HumanResponse, PromptKind, TuiHumanPlayer,
};
use crate::player::Player;

use super::screens::*;
use super::testing::*;
use super::*;

// ── Types ───────────────────────────────────────────────────────────

type HumanGameSetup = (
    mpsc::UnboundedReceiver<HumanPrompt>,
    mpsc::UnboundedSender<HumanResponse>,
    Option<mpsc::UnboundedReceiver<UiEvent>>,
    tokio::task::JoinHandle<Result<usize, OrchestratorError>>,
);

// ── Shared Helpers ───────────────────────────────────────────────────

/// Build a list of `count` resources to discard from the available pool.
///
/// Picks the first available resources in order: Wood, Brick, Sheep, Wheat, Ore.
fn first_n_resources(count: usize, available: [u32; 5]) -> Vec<Resource> {
    let resources = [
        Resource::Wood,
        Resource::Brick,
        Resource::Sheep,
        Resource::Wheat,
        Resource::Ore,
    ];
    let mut result = Vec::with_capacity(count);
    let mut remaining = available;
    for _ in 0..count {
        for (i, r) in remaining.iter_mut().enumerate() {
            if *r > 0 {
                *r -= 1;
                result.push(resources[i]);
                break;
            }
        }
    }
    result
}

/// Name the prompt kind for logging/assertions.
fn prompt_kind_name(kind: &PromptKind) -> &'static str {
    match kind {
        PromptKind::ChooseAction { .. } => "ChooseAction",
        PromptKind::PlaceSettlement { .. } => "PlaceSettlement",
        PromptKind::PlaceRoad { .. } => "PlaceRoad",
        PromptKind::PlaceRobber { .. } => "PlaceRobber",
        PromptKind::ChooseStealTarget { .. } => "ChooseStealTarget",
        PromptKind::Discard { .. } => "Discard",
        PromptKind::ChooseResource { .. } => "ChooseResource",
        PromptKind::ProposeTrade { .. } => "ProposeTrade",
        PromptKind::RespondToTrade { .. } => "RespondToTrade",
    }
}

/// Respond to a single prompt with a valid default.
fn default_response(kind: &PromptKind) -> HumanResponse {
    match kind {
        PromptKind::ChooseAction { .. }
        | PromptKind::PlaceSettlement { .. }
        | PromptKind::PlaceRoad { .. }
        | PromptKind::PlaceRobber { .. }
        | PromptKind::ChooseStealTarget { .. }
        | PromptKind::ChooseResource { .. } => HumanResponse::Index(0),
        PromptKind::Discard { count, available } => {
            HumanResponse::Resources(first_n_resources(*count, *available))
        }
        PromptKind::ProposeTrade { .. } => HumanResponse::Trade(None),
        PromptKind::RespondToTrade { .. } => HumanResponse::TradeAnswer(false),
    }
}

/// Auto-respond to all prompts, recording the prompt kinds received.
/// Returns the list of prompt kind names after the channel closes.
async fn auto_respond(
    mut prompt_rx: mpsc::UnboundedReceiver<HumanPrompt>,
    response_tx: mpsc::UnboundedSender<HumanResponse>,
) -> Vec<String> {
    let mut log = Vec::new();
    while let Some(prompt) = prompt_rx.recv().await {
        let name = prompt_kind_name(&prompt.kind).to_string();
        let response = default_response(&prompt.kind);
        log.push(name);
        if response_tx.send(response).is_err() {
            break;
        }
    }
    log
}

/// Create an orchestrator with 1 human (player 0) + 3 random players.
///
/// Returns the channel endpoints and a join handle for the spawned game task.
/// If `with_ui_tx` is true, wires up a UiEvent channel (note: this adds a
/// 50ms sleep per turn in the orchestrator).
fn setup_human_game(max_turns: u32, with_ui_tx: bool) -> HumanGameSetup {
    let board = Board::default_board();
    let state = GameState::new(board, 4);

    // Create human input channels.
    let (prompt_tx, prompt_rx) = mpsc::unbounded_channel::<HumanPrompt>();
    let (response_tx, response_rx) = mpsc::unbounded_channel::<HumanResponse>();
    let channel = Arc::new(HumanInputChannel {
        prompt_tx,
        response_rx: Mutex::new(response_rx),
    });

    // Build players: human + 3 randoms.
    let players: Vec<Box<dyn Player>> = vec![
        Box::new(TuiHumanPlayer::new("Human".into(), channel)),
        Box::new(RandomPlayer::new("Bot1".into())),
        Box::new(RandomPlayer::new("Bot2".into())),
        Box::new(RandomPlayer::new("Bot3".into())),
    ];

    // Optional UI event channel.
    let (ui_tx_opt, ui_rx_opt) = if with_ui_tx {
        let (tx, rx) = mpsc::unbounded_channel::<UiEvent>();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    // Spawn the orchestrator.
    let handle = tokio::spawn(async move {
        let mut orchestrator = GameOrchestrator::new(state, players);
        orchestrator.max_turns = max_turns;
        orchestrator.ui_tx = ui_tx_opt;
        orchestrator.run().await
    });

    (prompt_rx, response_tx, ui_rx_opt, handle)
}

/// Drain all currently available UiEvents from a receiver.
fn drain_ui_events(rx: &mut mpsc::UnboundedReceiver<UiEvent>) -> Vec<UiEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

// ── Test A: Screen Navigation Flow ───────────────────────────────────

#[test]
fn screen_navigation_menu_to_newgame_and_back() {
    let mut app = main_menu_app();

    // Main menu: Enter on "New Game" (selected=0).
    let action = handle_input(&mut app, KeyCode::Enter);
    assert!(
        matches!(action, Action::Transition(Screen::NewGame(_))),
        "Enter on 'New Game' should go to NewGame"
    );
    app.screen = match action {
        Action::Transition(s) => s,
        _ => panic!("expected transition"),
    };

    // NewGame: Esc goes back to main menu.
    let action = handle_input(&mut app, KeyCode::Esc);
    assert!(
        matches!(action, Action::Transition(Screen::MainMenu(_))),
        "Esc on NewGame should go back to MainMenu"
    );
    app.screen = match action {
        Action::Transition(s) => s,
        _ => panic!("expected transition"),
    };

    // Main menu: 'q' quits.
    let action = handle_input(&mut app, KeyCode::Char('q'));
    assert!(
        matches!(action, Action::Quit),
        "'q' on MainMenu should quit"
    );
}

#[test]
fn screen_navigation_new_game_configure_and_start() {
    let mut app = new_game_app();

    // Default focus is StartButton -- Enter immediately starts the game.
    if let Screen::NewGame(ref state) = app.screen {
        assert_eq!(state.focus, NewGameFocus::StartButton);
    } else {
        panic!("should still be on NewGame");
    }

    let action = handle_input(&mut app, KeyCode::Enter);
    assert!(
        matches!(action, Action::StartGame),
        "Enter on StartButton should trigger StartGame"
    );
}

#[test]
fn screen_navigation_post_game_to_new_game() {
    let mut app = post_game_app();

    // "Play Again" is selected by default (index 0).
    let action = handle_input(&mut app, KeyCode::Enter);
    assert!(
        matches!(action, Action::Transition(Screen::NewGame(_))),
        "Enter on 'Play Again' should go to NewGame"
    );
}

#[test]
fn screen_navigation_post_game_to_main_menu() {
    let mut app = post_game_app();

    // Navigate down to "Main Menu" (index 1).
    handle_input(&mut app, KeyCode::Down);
    let action = handle_input(&mut app, KeyCode::Enter);
    assert!(
        matches!(action, Action::Transition(Screen::MainMenu(_))),
        "Enter on 'Main Menu' should go to MainMenu"
    );
}

// ── Test B: Setup Phase via Channels ─────────────────────────────────

#[tokio::test]
async fn setup_phase_completes_with_human_player() {
    let (prompt_rx, response_tx, _, handle) = setup_human_game(10, false);

    // Auto-respond to all prompts.
    let respond_handle = tokio::spawn(auto_respond(prompt_rx, response_tx));

    // Wait for the game to finish (or timeout).
    let result = tokio::time::timeout(Duration::from_secs(30), handle).await;

    // Collect the prompt log.
    let prompt_log = respond_handle.await.unwrap();

    assert!(result.is_ok(), "game should finish within 30 seconds");

    // The human player (player 0) is at positions 0 and 7 in the snake draft
    // [0,1,2,3,3,2,1,0]. Each position gets a PlaceSettlement + PlaceRoad prompt.
    // So the human should receive exactly 4 setup prompts before gameplay begins.
    // (PlaceRoad also appears during gameplay for build-road intents, so we only
    // count prompts before the first ChooseAction.)
    let setup_prompts: Vec<&String> = prompt_log
        .iter()
        .take_while(|p| *p != "ChooseAction")
        .filter(|p| *p == "PlaceSettlement" || *p == "PlaceRoad")
        .collect();
    assert_eq!(
        setup_prompts.len(),
        4,
        "human should get 4 setup prompts (2 settlements + 2 roads), got: {:?}",
        setup_prompts
    );

    // First two should be PlaceSettlement then PlaceRoad (player 0 goes first).
    assert_eq!(
        prompt_log[0], "PlaceSettlement",
        "first prompt should be PlaceSettlement"
    );
    assert_eq!(
        prompt_log[1], "PlaceRoad",
        "second prompt should be PlaceRoad"
    );
}

// ── Test C: Full Game Auto-Play ──────────────────────────────────────

#[tokio::test]
async fn full_game_completes_with_human_autoplay() {
    let (prompt_rx, response_tx, _, handle) = setup_human_game(300, false);

    let respond_handle = tokio::spawn(auto_respond(prompt_rx, response_tx));

    let result = tokio::time::timeout(Duration::from_secs(60), handle).await;
    let prompt_log = respond_handle.await.unwrap();

    assert!(result.is_ok(), "game task should not be cancelled");
    let game_result = result.unwrap().unwrap();

    match game_result {
        Ok(winner) => {
            assert!(winner < 4, "winner should be a valid player index");
        }
        Err(OrchestratorError::GameStuck(_)) => {
            // Acceptable for random/auto-play.
        }
        Err(e) => {
            panic!("unexpected error: {}", e);
        }
    }

    // The game should have progressed beyond setup.
    assert!(
        prompt_log.len() > 4,
        "auto-play should receive more than just setup prompts, got {}",
        prompt_log.len()
    );

    // Should have received at least one ChooseAction prompt (human's turn).
    assert!(
        prompt_log.iter().any(|p| p == "ChooseAction"),
        "should receive at least one ChooseAction prompt during the game"
    );
}

// ── Test D: UiEvent Collection ───────────────────────────────────────

#[tokio::test]
async fn setup_emits_state_update_events() {
    let (prompt_rx, response_tx, ui_rx, handle) = setup_human_game(10, true);
    let mut ui_rx = ui_rx.unwrap();

    let respond_handle = tokio::spawn(auto_respond(prompt_rx, response_tx));

    let _ = tokio::time::timeout(Duration::from_secs(30), handle).await;
    let _ = respond_handle.await;

    // Drain all events.
    let events = drain_ui_events(&mut ui_rx);

    // Count StateUpdate events.
    let state_updates: Vec<&UiEvent> = events
        .iter()
        .filter(|e| matches!(e, UiEvent::StateUpdate { .. }))
        .collect();

    assert!(
        state_updates.len() >= 8,
        "should have at least 8 StateUpdate events from setup (got {})",
        state_updates.len()
    );

    // Extract messages from StateUpdates.
    let messages: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            UiEvent::StateUpdate { message, .. } => Some(message.as_str()),
            _ => None,
        })
        .collect();

    // Should have setup messages.
    let setup_messages: Vec<&&str> = messages.iter().filter(|m| m.contains("Setup:")).collect();
    assert!(
        setup_messages.len() >= 8,
        "should have at least 8 setup messages (got {})",
        setup_messages.len()
    );

    // At least one should mention P0 (the human player).
    assert!(
        setup_messages.iter().any(|m| m.contains("P0")),
        "at least one setup message should mention P0 (the human)"
    );
}

// ── Test E: ChooseAction Prompt After Setup ──────────────────────────

#[tokio::test]
async fn human_receives_choose_action_during_turn() {
    let (mut prompt_rx, response_tx, _, handle) = setup_human_game(20, false);

    // Custom response loop that records prompt details.
    let respond_handle = tokio::spawn(async move {
        let mut log: Vec<String> = Vec::new();
        let mut first_action_choices: Option<Vec<crate::player::PlayerChoice>> = None;

        while let Some(prompt) = prompt_rx.recv().await {
            let name = prompt_kind_name(&prompt.kind).to_string();

            // Capture the choices from the first ChooseAction prompt.
            if first_action_choices.is_none() {
                if let PromptKind::ChooseAction { ref choices } = prompt.kind {
                    first_action_choices = Some(choices.clone());
                }
            }

            let response = default_response(&prompt.kind);
            log.push(name);
            if response_tx.send(response).is_err() {
                break;
            }
        }
        (log, first_action_choices)
    });

    let _ = tokio::time::timeout(Duration::from_secs(30), handle).await;
    let (prompt_log, first_action_choices) = respond_handle.await.unwrap();

    // First 4 prompts should be the setup phase for player 0.
    assert!(
        prompt_log.len() >= 4,
        "should receive at least 4 prompts, got {}",
        prompt_log.len()
    );
    assert_eq!(prompt_log[0], "PlaceSettlement");
    assert_eq!(prompt_log[1], "PlaceRoad");

    // The second pair may not be exactly at indices 2,3 because the human is
    // player 0 and goes first AND last in the snake draft [0,1,2,3,3,2,1,0].
    // Between the human's first and second setup placement, the 6 other
    // placements happen (by random bots, which don't go through the channel).
    // So prompts 2,3 should also be PlaceSettlement, PlaceRoad.
    assert_eq!(prompt_log[2], "PlaceSettlement");
    assert_eq!(prompt_log[3], "PlaceRoad");

    // After setup, player 0 gets the first turn. We should see ChooseAction.
    assert!(
        prompt_log.iter().any(|p| p == "ChooseAction"),
        "should have received a ChooseAction prompt after setup"
    );

    // The ChooseAction prompt should have had non-empty choices.
    let choices = first_action_choices.expect("should have captured ChooseAction choices");
    assert!(
        !choices.is_empty(),
        "ChooseAction choices should not be empty"
    );

    // EndTurn should always be one of the choices.
    assert!(
        choices.iter().any(|c| c.is_end_turn()),
        "ChooseAction should include End Turn, got: {:?}",
        choices
    );
}

// ── Test F: Game Over UiEvent ────────────────────────────────────────

#[tokio::test]
async fn game_over_sends_ui_event() {
    let (prompt_rx, response_tx, ui_rx, handle) = setup_human_game(300, true);
    let mut ui_rx = ui_rx.unwrap();

    let respond_handle = tokio::spawn(auto_respond(prompt_rx, response_tx));

    let result = tokio::time::timeout(Duration::from_secs(60), handle).await;
    let _ = respond_handle.await;

    let events = drain_ui_events(&mut ui_rx);

    let game_over_events: Vec<&UiEvent> = events
        .iter()
        .filter(|e| matches!(e, UiEvent::GameOver { .. }))
        .collect();

    match result {
        Ok(Ok(Ok(winner))) => {
            // Game completed with a winner -- should have exactly one GameOver event.
            assert_eq!(
                game_over_events.len(),
                1,
                "should have exactly 1 GameOver event on victory"
            );
            if let UiEvent::GameOver {
                winner: evt_winner, ..
            } = game_over_events[0]
            {
                assert_eq!(
                    *evt_winner, winner,
                    "GameOver event winner should match the game result"
                );
            }
        }
        Ok(Ok(Err(OrchestratorError::GameStuck(_)))) => {
            // Game hit max_turns -- no GameOver event expected.
            assert!(
                game_over_events.is_empty(),
                "GameStuck should not produce a GameOver event"
            );
        }
        Ok(Ok(Err(e))) => {
            panic!("unexpected orchestrator error: {}", e);
        }
        Ok(Err(e)) => {
            panic!("orchestrator task panicked: {:?}", e);
        }
        Err(_) => {
            panic!("game timed out after 60 seconds");
        }
    }
}

// ── Test: Prompt/Response Channel Protocol ───────────────────────────

#[tokio::test]
async fn human_prompt_response_round_trip() {
    // Verify the basic channel protocol works: send a prompt, get a response.
    let (prompt_tx, mut prompt_rx) = mpsc::unbounded_channel::<HumanPrompt>();
    let (response_tx, response_rx) = mpsc::unbounded_channel::<HumanResponse>();

    let channel = Arc::new(HumanInputChannel {
        prompt_tx,
        response_rx: Mutex::new(response_rx),
    });

    let player = TuiHumanPlayer::new("Test".into(), channel);

    // Spawn a task that receives the prompt and sends a response.
    let responder = tokio::spawn(async move {
        let prompt = prompt_rx.recv().await.unwrap();
        assert!(matches!(prompt.kind, PromptKind::ChooseAction { .. }));
        response_tx.send(HumanResponse::Index(2)).unwrap();
    });

    // Ask the player to choose an action (this sends via channel and waits).
    let choices = vec![
        crate::player::PlayerChoice::GameAction(crate::game::actions::Action::EndTurn),
        crate::player::PlayerChoice::ProposeTrade,
        crate::player::PlayerChoice::PlayKnight,
    ];
    let state = make_test_game_state();
    let (idx, _) = player.choose_action(&state, 0, &choices).await;

    responder.await.unwrap();
    assert_eq!(idx, 2, "should receive the index we sent");
}

#[tokio::test]
async fn human_discard_round_trip() {
    let (prompt_tx, mut prompt_rx) = mpsc::unbounded_channel::<HumanPrompt>();
    let (response_tx, response_rx) = mpsc::unbounded_channel::<HumanResponse>();

    let channel = Arc::new(HumanInputChannel {
        prompt_tx,
        response_rx: Mutex::new(response_rx),
    });

    let player = TuiHumanPlayer::new("Test".into(), channel);

    // Set up a game state with resources for player 0.
    let mut state = make_test_game_state();
    state.players[0].add_resource(Resource::Wood, 4);
    state.players[0].add_resource(Resource::Brick, 4);
    state.players[0].add_resource(Resource::Sheep, 2);

    let responder = tokio::spawn(async move {
        let prompt = prompt_rx.recv().await.unwrap();
        if let PromptKind::Discard { count, available } = &prompt.kind {
            assert_eq!(*count, 5);
            assert_eq!(available[0], 4); // Wood
            assert_eq!(available[1], 4); // Brick
            let resources = first_n_resources(*count, *available);
            response_tx
                .send(HumanResponse::Resources(resources))
                .unwrap();
        } else {
            panic!("expected Discard prompt, got {:?}", prompt.kind);
        }
    });

    let (cards, _) = player.choose_discard(&state, 0, 5).await;
    responder.await.unwrap();

    assert_eq!(cards.len(), 5, "should discard exactly 5 cards");
}

// ── Test: Multiple Turns with Human ──────────────────────────────────

#[tokio::test]
async fn human_plays_multiple_turns() {
    let (mut prompt_rx, response_tx, _, handle) = setup_human_game(30, false);

    // Count how many ChooseAction prompts the human receives.
    let respond_handle = tokio::spawn(async move {
        let mut action_count = 0usize;
        while let Some(prompt) = prompt_rx.recv().await {
            if matches!(prompt.kind, PromptKind::ChooseAction { .. }) {
                action_count += 1;
            }
            let response = default_response(&prompt.kind);
            if response_tx.send(response).is_err() {
                break;
            }
        }
        action_count
    });

    let _ = tokio::time::timeout(Duration::from_secs(30), handle).await;
    let action_count = respond_handle.await.unwrap();

    // In a 30-turn game with 4 players, the human (player 0) should get
    // roughly 30/4 = ~7 turns. Since auto_respond picks EndTurn (index 0
    // often maps to EndTurn), turns should advance quickly.
    assert!(
        action_count >= 2,
        "human should play at least 2 turns in a 30-turn game (got {})",
        action_count
    );
}

// ── Test: AI Reasoning Events ────────────────────────────────────────

#[tokio::test]
async fn ai_reasoning_events_emitted() {
    let (prompt_rx, response_tx, ui_rx, handle) = setup_human_game(10, true);
    let mut ui_rx = ui_rx.unwrap();

    let respond_handle = tokio::spawn(auto_respond(prompt_rx, response_tx));

    let _ = tokio::time::timeout(Duration::from_secs(30), handle).await;
    let _ = respond_handle.await;

    let events = drain_ui_events(&mut ui_rx);

    // Random players emit reasoning strings like "[random] chose: ..."
    let reasoning_events: Vec<&UiEvent> = events
        .iter()
        .filter(|e| matches!(e, UiEvent::AiReasoning { .. }))
        .collect();

    // The bots should have produced some reasoning during their turns.
    assert!(
        !reasoning_events.is_empty(),
        "random players should emit AiReasoning events"
    );

    // Verify the reasoning events have valid player info.
    for event in &reasoning_events {
        if let UiEvent::AiReasoning {
            player_id,
            player_name,
            reasoning,
        } = event
        {
            assert!(*player_id < 4, "player_id should be valid");
            assert!(!player_name.is_empty(), "player_name should not be empty");
            assert!(!reasoning.is_empty(), "reasoning should not be empty");
        }
    }
}

// ── Test: Resume Game Populates Game Log ────────────────────────────

#[test]
fn resume_game_populates_game_log_with_history() {
    use crate::game::board::{HexCoord, VertexCoord, VertexDirection};
    use crate::game::event::{format_event, GameEvent};

    let player_names: Vec<String> = vec!["Alice".into(), "Bob".into(), "Charlie".into()];

    let events = vec![
        GameEvent::DiceRolled {
            player: 0,
            values: (3, 4),
            total: 7,
        },
        GameEvent::RobberMoved {
            player: 0,
            to: HexCoord::new(1, 2),
            stole_from: Some(1),
        },
        GameEvent::SettlementBuilt {
            player: 1,
            vertex: VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North),
            reasoning: "good spot".into(),
        },
    ];

    // Format events the same way resume_game does.
    let history_messages: Vec<String> = events
        .iter()
        .map(|e| format_event(e, &player_names))
        .collect();

    let (_ui_tx, ui_rx) = mpsc::unbounded_channel::<UiEvent>();
    let mut ps = PlayingState::new(ui_rx, player_names, false);

    // Pre-populate messages, matching the resume_game code path.
    for msg in &history_messages {
        ps.push_message(msg.clone());
    }

    // The initial 2 messages + 3 history messages should all be present.
    assert_eq!(ps.messages.len(), 5);
    assert!(ps.messages[2].contains("Alice rolled 7"));
    assert!(ps.messages[3].contains("robber"));
    assert!(ps.messages[4].contains("Bob built settlement"));
}
