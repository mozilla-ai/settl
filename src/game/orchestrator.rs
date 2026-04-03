//! Game orchestrator — drives the game loop, calling Player trait methods
//! for each decision point and applying actions through the rules engine.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::game::actions::{Action, DevCard, DevCardAction, PlayerId, TradeResponse};
use crate::game::board::{board_hex_coords, Resource};
use crate::game::dice;
use crate::game::event::GameEvent;
use crate::game::rules;
use crate::game::state::{GamePhase, GameState};
use crate::player::{Player, PlayerChoice};
use crate::trading;
use crate::ui::UiEvent;

/// Default timeout for player decisions. Set high (5 minutes) to accommodate
/// streaming LLM responses with large max_tokens. The human can watch reasoning
/// stream progressively, so long wait times feel productive rather than stuck.
const PLAYER_DECISION_TIMEOUT: Duration = Duration::from_secs(300);

/// Errors that can occur during game orchestration.
#[derive(Debug)]
pub enum OrchestratorError {
    /// A rules engine error that shouldn't happen if legal_actions is correct.
    RuleViolation(String),
    /// Game got stuck (e.g. infinite loop protection).
    GameStuck(String),
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchestratorError::RuleViolation(s) => write!(f, "Rule violation: {}", s),
            OrchestratorError::GameStuck(s) => write!(f, "Game stuck: {}", s),
        }
    }
}

impl std::error::Error for OrchestratorError {}

/// Drives a complete game from setup through victory.
pub struct GameOrchestrator {
    pub state: GameState,
    pub players: Vec<Box<dyn Player>>,
    /// Event history for LLM context.
    pub events: Vec<GameEvent>,
    /// Player names, indexed by PlayerId.
    pub player_names: Vec<String>,
    /// Maximum turns before declaring the game stuck (safety valve).
    pub max_turns: u32,
    /// Optional channel to send UI events to the TUI.
    pub ui_tx: Option<mpsc::UnboundedSender<UiEvent>>,
}

impl GameOrchestrator {
    pub fn new(state: GameState, players: Vec<Box<dyn Player>>) -> Self {
        let player_names: Vec<String> = players.iter().map(|p| p.name().to_string()).collect();
        Self {
            state,
            players,
            events: Vec::new(),
            player_names,
            max_turns: 500,
            ui_tx: None,
        }
    }

    /// Send a UI event if a TUI channel is connected.
    fn send_ui(&self, message: String, event: Option<GameEvent>) {
        if let Some(tx) = &self.ui_tx {
            let _ = tx.send(UiEvent::StateUpdate {
                state: Arc::new(self.state.clone()),
                event,
                message,
            });
        }
    }

    /// Send an AI reasoning trace to the TUI.
    fn send_reasoning(&self, player_id: PlayerId, reasoning: &str) {
        if let Some(tx) = &self.ui_tx {
            if !reasoning.is_empty() {
                let _ = tx.send(UiEvent::AiReasoning {
                    player_id,
                    player_name: self.player_names[player_id].clone(),
                    reasoning: reasoning.to_string(),
                });
            }
        }
    }

    /// Wrap an async player decision in a timeout. Returns the fallback on timeout.
    async fn with_timeout<T>(
        &self,
        future: impl std::future::Future<Output = T>,
        fallback: T,
    ) -> T {
        match tokio::time::timeout(PLAYER_DECISION_TIMEOUT, future).await {
            Ok(result) => result,
            Err(_) => fallback,
        }
    }

    /// Record a game event for LLM history context.
    fn record_event(&mut self, event: GameEvent) {
        self.events.push(event);
    }

    /// Run the full game and return the winner's PlayerId.
    pub async fn run(&mut self) -> Result<PlayerId, OrchestratorError> {
        // Send initial state so the TUI can render the board before any prompts arrive.
        self.send_ui("Game starting...".into(), None);

        // Skip setup if the game is already past the setup phase (e.g. resumed).
        if matches!(self.state.phase, GamePhase::Setup { .. }) {
            self.run_setup().await?;

            // Transition to Playing phase.
            self.state.phase = GamePhase::Playing {
                current_player: 0,
                has_rolled: false,
            };
        }

        // Phase 2: Main game loop.
        loop {
            if self.state.turn_number >= self.max_turns {
                return Err(OrchestratorError::GameStuck(format!(
                    "Game exceeded {} turns",
                    self.max_turns
                )));
            }

            // Small delay when TUI is connected so the display is readable.
            if self.ui_tx.is_some() {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }

            if let Some(winner) = self.run_turn().await? {
                let vp = self.state.victory_points(winner);
                let msg = format!("{} wins with {} VP!", self.player_names[winner], vp);
                self.record_event(GameEvent::GameWon {
                    player: winner,
                    final_vp: vp,
                });
                if let Some(tx) = &self.ui_tx {
                    let _ = tx.send(UiEvent::GameOver {
                        winner,
                        message: msg,
                    });
                }
                return Ok(winner);
            }
        }
    }

    /// Run the setup phase: snake draft settlement + road placement.
    async fn run_setup(&mut self) -> Result<(), OrchestratorError> {
        let setup_order = self.state.setup_order.clone();
        let total_placements = setup_order.len();

        for (idx, &player_id) in setup_order.iter().enumerate() {
            let round: u8 = if idx < total_placements / 2 { 1 } else { 2 };

            // Step 1: Choose settlement location.
            let legal_vertices = rules::legal_setup_vertices(&self.state);

            if legal_vertices.is_empty() {
                return Err(OrchestratorError::GameStuck(
                    "No legal setup vertices".into(),
                ));
            }

            let (v_idx, v_reasoning) = self
                .with_timeout(
                    self.players[player_id].choose_settlement(
                        &self.state,
                        player_id,
                        &legal_vertices,
                        round,
                        &self.player_names,
                    ),
                    (0, "timeout fallback".into()),
                )
                .await;
            let vertex = legal_vertices[v_idx.min(legal_vertices.len() - 1)];
            self.send_reasoning(player_id, &v_reasoning);

            // Apply setup settlement.
            rules::apply_setup_settlement(&mut self.state, vertex).map_err(|e| {
                OrchestratorError::RuleViolation(format!("Setup settlement: {}", e))
            })?;

            // Step 2: Choose road location.
            let legal_edges = rules::legal_setup_roads(&self.state, vertex);
            if legal_edges.is_empty() {
                return Err(OrchestratorError::GameStuck("No legal setup roads".into()));
            }

            let (e_idx, e_reasoning) = self
                .with_timeout(
                    self.players[player_id].choose_road(
                        &self.state,
                        player_id,
                        &legal_edges,
                        &self.player_names,
                    ),
                    (0, "timeout fallback".into()),
                )
                .await;
            let edge = legal_edges[e_idx.min(legal_edges.len() - 1)];
            self.send_reasoning(player_id, &e_reasoning);

            // Apply setup road.
            rules::apply_setup_road(&mut self.state, vertex, edge)
                .map_err(|e| OrchestratorError::RuleViolation(format!("Setup road: {}", e)))?;

            // Record the events.
            self.record_event(GameEvent::InitialSettlementPlaced {
                player: player_id,
                vertex,
            });
            self.record_event(GameEvent::InitialRoadPlaced {
                player: player_id,
                edge,
            });

            let msg = format!(
                "Setup: P{} ({}) placed settlement at ({},{},{:?}), road at {}",
                player_id,
                self.player_names[player_id],
                vertex.hex.q,
                vertex.hex.r,
                vertex.dir,
                edge,
            );
            self.send_ui(msg, None);
        }

        Ok(())
    }

    /// Run a single player's turn. Returns Some(winner) if the game ends.
    async fn run_turn(&mut self) -> Result<Option<PlayerId>, OrchestratorError> {
        let player_id = self.state.current_player();

        // Inject enriched game context for LLM decisions.
        let strategic =
            crate::player::prompt::strategic_context(&self.state, player_id, &self.player_names);
        let trading =
            crate::player::prompt::trading_summary(&self.events, player_id, &self.player_names);
        let threat =
            crate::player::prompt::threat_assessment(&self.state, player_id, &self.player_names);
        let history =
            crate::player::prompt::format_recent_history(&self.events, &self.player_names, 20);
        let mut context = strategic;
        if !threat.is_empty() {
            context.push_str("\n\n");
            context.push_str(&threat);
        }
        if !trading.is_empty() {
            context.push_str("\n\n");
            context.push_str(&trading);
        }
        if !history.is_empty() {
            context.push_str("\n\n");
            context.push_str(&history);
        }
        let _ = self
            .with_timeout(self.players[player_id].set_game_context(&context), ())
            .await;

        // Step 0: Pre-roll Knight opportunity.
        // A player may play a Knight before rolling the dice.
        if let Some(winner) = self.offer_pre_roll_knight(player_id).await? {
            return Ok(Some(winner));
        }

        // Step 1: Roll dice.
        let (d1, d2) = dice::roll_dice(&mut rand::rng());
        let roll = d1 + d2;

        let dice_event = GameEvent::DiceRolled {
            player: player_id,
            values: (d1, d2),
            total: roll,
        };
        self.record_event(dice_event.clone());
        self.send_ui(
            format!(
                "Turn {} — P{} ({}) rolled {} ({} + {})",
                self.state.turn_number + 1,
                player_id,
                self.player_names[player_id],
                roll,
                d1,
                d2
            ),
            Some(dice_event),
        );

        // Step 2: Handle the roll.
        if roll == 7 {
            self.handle_seven(player_id).await?;
        } else {
            self.distribute_resources(roll);
        }

        // Mark that dice have been rolled.
        self.state.phase = GamePhase::Playing {
            current_player: player_id,
            has_rolled: true,
        };

        // Step 3: Action loop — player takes actions until EndTurn.
        let max_actions_per_turn = 50; // safety valve
        for _ in 0..max_actions_per_turn {
            let choices = self.build_choices();

            if choices.is_empty() {
                // This shouldn't happen (EndTurn should always be available).
                break;
            }

            let (choice_idx, reasoning) = self
                .with_timeout(
                    self.players[player_id].choose_action(&self.state, player_id, &choices),
                    (0, "timeout fallback".into()),
                )
                .await;

            let choice = &choices[choice_idx.min(choices.len() - 1)];
            self.send_reasoning(player_id, &reasoning);

            let action_result = match choice {
                PlayerChoice::GameAction(action) => {
                    if matches!(action, Action::EndTurn) {
                        self.end_turn(player_id);
                        return Ok(None);
                    }
                    self.apply_and_log(action.clone(), player_id, &reasoning)
                }
                PlayerChoice::PlayKnight => self.handle_knight(player_id).await,
                PlayerChoice::PlayMonopoly => self.handle_monopoly(player_id).await,
                PlayerChoice::PlayYearOfPlenty => self.handle_year_of_plenty(player_id).await,
                PlayerChoice::PlayRoadBuilding => self.handle_road_building(player_id).await,
                PlayerChoice::ProposeTrade => self.handle_trade(player_id).await,
                PlayerChoice::BuildRoadIntent => self.handle_build_road(player_id).await,
                PlayerChoice::BuildSettlementIntent => {
                    self.handle_build_settlement(player_id).await
                }
                PlayerChoice::BuildCityIntent => self.handle_build_city(player_id).await,
            };

            match action_result {
                Ok(()) => {
                    self.send_ui(
                        format!(
                            "{}: {} — {}",
                            self.player_names[player_id], choice, reasoning
                        ),
                        None,
                    );
                    if let Some(winner) = rules::check_victory(&self.state) {
                        return Ok(Some(winner));
                    }
                }
                Err(OrchestratorError::RuleViolation(_msg)) => {
                    // Action was invalid — skip it and let the player try again.
                }
                Err(e) => return Err(e),
            }
        }

        // If we hit the action limit, force end turn.
        self.end_turn(player_id);
        Ok(None)
    }

    /// Build the list of choices for the current player.
    ///
    /// Placement actions (road, settlement, city) are collapsed into single
    /// intent entries so the action bar shows one item per action type.
    /// The specific location is collected via board cursor in a follow-up step.
    fn build_choices(&self) -> Vec<PlayerChoice> {
        let actions = rules::legal_actions(&self.state);
        let mut choices: Vec<PlayerChoice> = Vec::new();
        let mut has_road = false;
        let mut has_settlement = false;
        let mut has_city = false;

        for action in actions {
            match &action {
                Action::BuildRoad(_) => {
                    if !has_road {
                        choices.push(PlayerChoice::BuildRoadIntent);
                        has_road = true;
                    }
                }
                Action::BuildSettlement(_) => {
                    if !has_settlement {
                        choices.push(PlayerChoice::BuildSettlementIntent);
                        has_settlement = true;
                    }
                }
                Action::BuildCity(_) => {
                    if !has_city {
                        choices.push(PlayerChoice::BuildCityIntent);
                        has_city = true;
                    }
                }
                _ => choices.push(PlayerChoice::GameAction(action)),
            }
        }

        // Add dev card intents if the player can play one.
        // Cards bought this turn (last N entries) cannot be played.
        if let GamePhase::Playing {
            current_player,
            has_rolled: true,
        } = &self.state.phase
        {
            let ps = &self.state.players[*current_player];
            if !ps.has_played_dev_card_this_turn {
                let mut seen_knight = false;
                let mut seen_monopoly = false;
                let mut seen_yop = false;
                let mut seen_rb = false;
                let playable_count = ps
                    .dev_cards
                    .len()
                    .saturating_sub(ps.dev_cards_bought_this_turn);

                for card in &ps.dev_cards[..playable_count] {
                    match card {
                        DevCard::Knight if !seen_knight => {
                            choices.push(PlayerChoice::PlayKnight);
                            seen_knight = true;
                        }
                        DevCard::Monopoly if !seen_monopoly => {
                            choices.push(PlayerChoice::PlayMonopoly);
                            seen_monopoly = true;
                        }
                        DevCard::YearOfPlenty if !seen_yop => {
                            choices.push(PlayerChoice::PlayYearOfPlenty);
                            seen_yop = true;
                        }
                        DevCard::RoadBuilding if !seen_rb && ps.roads_remaining >= 2 => {
                            choices.push(PlayerChoice::PlayRoadBuilding);
                            seen_rb = true;
                        }
                        _ => {}
                    }
                }
            }

            // Add ProposeTrade if the player has any resources to trade.
            if ps.total_resources() > 0 && self.state.num_players > 1 {
                choices.push(PlayerChoice::ProposeTrade);
            }
        }

        choices
    }

    /// Handle rolling a 7: discard phase + robber placement + steal.
    async fn handle_seven(&mut self, roller: PlayerId) -> Result<(), OrchestratorError> {
        // Step 1: Discard phase — any player with >7 cards must discard half.
        let players_needing_discard: Vec<PlayerId> = (0..self.state.num_players)
            .filter(|&p| self.state.players[p].total_resources() > 7)
            .collect();

        if !players_needing_discard.is_empty() {
            self.state.phase = GamePhase::Discarding {
                current_player: roller,
                players_needing_discard: players_needing_discard.clone(),
            };
        }

        for &p in &players_needing_discard {
            let total = self.state.players[p].total_resources();
            let discard_count = (total / 2) as usize;

            let (cards, discard_reasoning) = self
                .with_timeout(
                    self.players[p].choose_discard(&self.state, p, discard_count),
                    (Vec::new(), "timeout fallback".into()),
                )
                .await;
            self.send_reasoning(p, &discard_reasoning);

            rules::apply_discard(&mut self.state, p, &cards)
                .map_err(|e| OrchestratorError::RuleViolation(format!("Discard: {}", e)))?;

            self.record_event(GameEvent::CardsDiscarded {
                player: p,
                cards: cards.clone(),
            });
        }

        // Step 2: Move robber.
        self.state.phase = GamePhase::PlacingRobber {
            current_player: roller,
        };

        let legal_hexes: Vec<_> = board_hex_coords()
            .into_iter()
            .filter(|&h| h != self.state.robber_hex)
            .collect();

        let (h_idx, robber_reasoning) = self
            .with_timeout(
                self.players[roller].choose_robber_hex(&self.state, roller, &legal_hexes),
                (0, "timeout fallback".into()),
            )
            .await;
        let hex = legal_hexes[h_idx.min(legal_hexes.len() - 1)];
        self.send_reasoning(roller, &robber_reasoning);

        rules::apply_move_robber(&mut self.state, hex)
            .map_err(|e| OrchestratorError::RuleViolation(format!("Move robber: {}", e)))?;

        // Step 3: Steal (if in Stealing phase after move_robber).
        if let GamePhase::Stealing { target_hex, .. } = &self.state.phase {
            let targets = rules::steal_targets(&self.state, *target_hex, roller);
            if !targets.is_empty() {
                let (t_idx, steal_reasoning) = self
                    .with_timeout(
                        self.players[roller].choose_steal_target(
                            &self.state,
                            roller,
                            &targets,
                            &self.player_names,
                        ),
                        (0, "timeout fallback".into()),
                    )
                    .await;
                self.send_reasoning(roller, &steal_reasoning);
                let target = targets[t_idx.min(targets.len() - 1)];

                rules::apply_steal(&mut self.state, target)
                    .map_err(|e| OrchestratorError::RuleViolation(format!("Steal: {}", e)))?;

                self.record_event(GameEvent::RobberMoved {
                    player: roller,
                    to: hex,
                    stole_from: Some(target),
                });
            }
        } else {
            self.record_event(GameEvent::RobberMoved {
                player: roller,
                to: hex,
                stole_from: None,
            });
        }

        Ok(())
    }

    /// Distribute resources for a non-7 dice roll.
    fn distribute_resources(&mut self, roll: u8) {
        let distributions = dice::distribute_resources(&self.state, roll);

        if distributions.is_empty() {
            return;
        }

        // Apply resource grants.
        for (&player, resources) in &distributions {
            for &(resource, count) in resources {
                self.state.players[player].add_resource(resource, count);
            }
        }

        // Log the distributions.
        let mut flat: Vec<(PlayerId, Resource, u32)> = Vec::new();
        for (&player, resources) in &distributions {
            for &(resource, count) in resources {
                flat.push((player, resource, count));
            }
        }
        self.record_event(GameEvent::ResourcesDistributed {
            distributions: flat,
        });
    }

    /// Offer the player a chance to play a Knight before rolling dice.
    ///
    /// A Knight may be played before rolling. We present the
    /// player with a choice: "Roll Dice" or "Play Knight". If they choose the
    /// Knight, we handle it and check victory.
    async fn offer_pre_roll_knight(
        &mut self,
        player_id: PlayerId,
    ) -> Result<Option<PlayerId>, OrchestratorError> {
        let ps = &self.state.players[player_id];
        if ps.has_played_dev_card_this_turn {
            return Ok(None);
        }

        // Check if the player has a playable Knight (not bought this turn).
        let playable_count = ps
            .dev_cards
            .len()
            .saturating_sub(ps.dev_cards_bought_this_turn);
        let has_knight = ps.dev_cards[..playable_count].contains(&DevCard::Knight);
        if !has_knight {
            return Ok(None);
        }

        // Build a minimal choice set: Roll Dice or Play Knight.
        let choices = vec![
            PlayerChoice::GameAction(Action::EndTurn), // Repurposed as "Roll Dice" (index 0)
            PlayerChoice::PlayKnight,                  // Play Knight pre-roll (index 1)
        ];

        // Override the label: the first choice means "Roll Dice", not literally EndTurn.
        // The orchestrator will roll dice regardless if they pick index 0.
        let (choice_idx, reasoning) = self
            .with_timeout(
                self.players[player_id].choose_action(&self.state, player_id, &choices),
                (0, "timeout fallback".into()),
            )
            .await;

        let choice = &choices[choice_idx.min(choices.len() - 1)];
        if matches!(choice, PlayerChoice::PlayKnight) {
            self.send_reasoning(player_id, &reasoning);
            self.handle_knight(player_id).await?;
            if let Some(winner) = rules::check_victory(&self.state) {
                return Ok(Some(winner));
            }
        }

        Ok(None)
    }

    /// Handle playing a Knight dev card (multi-step: remove card, move robber, steal).
    async fn handle_knight(&mut self, player_id: PlayerId) -> Result<(), OrchestratorError> {
        // Legal hexes for robber.
        let legal_hexes: Vec<_> = board_hex_coords()
            .into_iter()
            .filter(|&h| h != self.state.robber_hex)
            .collect();

        let (h_idx, h_reasoning) = self
            .with_timeout(
                self.players[player_id].choose_robber_hex(&self.state, player_id, &legal_hexes),
                (0, "timeout fallback".into()),
            )
            .await;
        let hex = legal_hexes[h_idx.min(legal_hexes.len() - 1)];

        // Determine steal target.
        let targets = rules::steal_targets(&self.state, hex, player_id);
        let steal_from = if targets.is_empty() {
            None
        } else {
            let (t_idx, _) = self
                .with_timeout(
                    self.players[player_id].choose_steal_target(
                        &self.state,
                        player_id,
                        &targets,
                        &self.player_names,
                    ),
                    (0, "timeout fallback".into()),
                )
                .await;
            Some(targets[t_idx.min(targets.len() - 1)])
        };

        // Apply the full Knight action.
        let action = Action::PlayDevCard(
            DevCard::Knight,
            DevCardAction::Knight {
                robber_to: hex,
                steal_from,
            },
        );

        self.apply_and_log(action, player_id, &h_reasoning)?;
        Ok(())
    }

    /// Handle playing Monopoly (choose resource, take all from other players).
    async fn handle_monopoly(&mut self, player_id: PlayerId) -> Result<(), OrchestratorError> {
        let (resource, reasoning) = self.with_timeout(
            self.players[player_id].choose_resource(
                &self.state,
                player_id,
                "MONOPOLY: Choose a resource to monopolize. All other players must give you all of that resource.",
            ),
            (Resource::Wood, "timeout fallback".into()),
        ).await;

        let action = Action::PlayDevCard(DevCard::Monopoly, DevCardAction::Monopoly(resource));
        self.apply_and_log(action, player_id, &reasoning)?;
        Ok(())
    }

    /// Handle playing Year of Plenty (choose 2 free resources).
    async fn handle_year_of_plenty(
        &mut self,
        player_id: PlayerId,
    ) -> Result<(), OrchestratorError> {
        let (r1, _) = self
            .with_timeout(
                self.players[player_id].choose_resource(
                    &self.state,
                    player_id,
                    "YEAR OF PLENTY: Choose your first free resource.",
                ),
                (Resource::Wood, "timeout fallback".into()),
            )
            .await;
        let (r2, reasoning) = self
            .with_timeout(
                self.players[player_id].choose_resource(
                    &self.state,
                    player_id,
                    "YEAR OF PLENTY: Choose your second free resource.",
                ),
                (Resource::Wood, "timeout fallback".into()),
            )
            .await;

        let action =
            Action::PlayDevCard(DevCard::YearOfPlenty, DevCardAction::YearOfPlenty(r1, r2));
        self.apply_and_log(action, player_id, &reasoning)?;
        Ok(())
    }

    /// Handle playing Road Building (place 2 free roads).
    async fn handle_road_building(&mut self, player_id: PlayerId) -> Result<(), OrchestratorError> {
        // First road.
        let legal_edges_1 = rules::legal_road_edges(&self.state, player_id);
        if legal_edges_1.is_empty() {
            return Ok(());
        }

        let (e1_idx, _) = self
            .with_timeout(
                self.players[player_id].choose_road(
                    &self.state,
                    player_id,
                    &legal_edges_1,
                    &self.player_names,
                ),
                (0, "timeout fallback".into()),
            )
            .await;
        let edge1 = legal_edges_1[e1_idx.min(legal_edges_1.len() - 1)];

        // Temporarily place the first road so the second road's legal positions
        // account for the new connectivity.
        self.state.roads.insert(edge1, player_id);
        let legal_edges_2: Vec<_> = rules::legal_road_edges(&self.state, player_id)
            .into_iter()
            .filter(|e| *e != edge1)
            .collect();
        // Remove the temporary road; apply_play_dev_card will place both.
        self.state.roads.remove(&edge1);

        let edge2 = if legal_edges_2.is_empty() {
            edge1 // fallback -- will fail validation but handled gracefully
        } else {
            let (e2_idx, _) = self
                .with_timeout(
                    self.players[player_id].choose_road(
                        &self.state,
                        player_id,
                        &legal_edges_2,
                        &self.player_names,
                    ),
                    (0, "timeout fallback".into()),
                )
                .await;
            legal_edges_2[e2_idx.min(legal_edges_2.len() - 1)]
        };

        let action = Action::PlayDevCard(
            DevCard::RoadBuilding,
            DevCardAction::RoadBuilding(edge1, edge2),
        );
        self.apply_and_log(action, player_id, "Road Building")?;
        Ok(())
    }

    /// Handle a Build Road intent: show board cursor, then apply the action.
    async fn handle_build_road(&mut self, player_id: PlayerId) -> Result<(), OrchestratorError> {
        let legal = rules::legal_road_edges(&self.state, player_id);
        if legal.is_empty() {
            return Ok(());
        }
        let (idx, reasoning) = self
            .with_timeout(
                self.players[player_id].choose_road(
                    &self.state,
                    player_id,
                    &legal,
                    &self.player_names,
                ),
                (0, "timeout fallback".into()),
            )
            .await;
        let edge = legal[idx.min(legal.len() - 1)];
        self.apply_and_log(Action::BuildRoad(edge), player_id, &reasoning)
    }

    /// Handle a Build Settlement intent: show board cursor, then apply the action.
    async fn handle_build_settlement(
        &mut self,
        player_id: PlayerId,
    ) -> Result<(), OrchestratorError> {
        let legal = rules::legal_settlement_vertices(&self.state, player_id);
        if legal.is_empty() {
            return Ok(());
        }
        let (idx, reasoning) = self
            .with_timeout(
                self.players[player_id].choose_settlement(
                    &self.state,
                    player_id,
                    &legal,
                    0,
                    &self.player_names,
                ),
                (0, "timeout fallback".into()),
            )
            .await;
        let vertex = legal[idx.min(legal.len() - 1)];
        self.apply_and_log(Action::BuildSettlement(vertex), player_id, &reasoning)
    }

    /// Handle a Build City intent: show board cursor, then apply the action.
    async fn handle_build_city(&mut self, player_id: PlayerId) -> Result<(), OrchestratorError> {
        let legal = rules::legal_city_vertices(&self.state, player_id);
        if legal.is_empty() {
            return Ok(());
        }
        let (idx, reasoning) = self
            .with_timeout(
                self.players[player_id].choose_settlement(
                    &self.state,
                    player_id,
                    &legal,
                    0,
                    &self.player_names,
                ),
                (0, "timeout fallback".into()),
            )
            .await;
        let vertex = legal[idx.min(legal.len() - 1)];
        self.apply_and_log(Action::BuildCity(vertex), player_id, &reasoning)
    }

    /// Handle a player proposing a trade: collect offer, broadcast to others, execute if accepted.
    async fn handle_trade(&mut self, player_id: PlayerId) -> Result<(), OrchestratorError> {
        // Step 1: Get the trade offer from the proposing player.
        let offer_result = self
            .with_timeout(
                self.players[player_id].propose_trade(&self.state, player_id),
                None,
            )
            .await;

        let (offer, reasoning) = match offer_result {
            Some((offer, reasoning)) => (offer, reasoning),
            None => {
                return Ok(());
            }
        };

        // Validate the offer using the trading module.
        if let Err(_e) = trading::negotiation::validate_trade(&self.state, &offer) {
            return Ok(());
        }

        let offering: String = offer
            .offering
            .iter()
            .map(|(r, n)| format!("{} {}", n, r))
            .collect::<Vec<_>>()
            .join(", ");
        let requesting: String = offer
            .requesting
            .iter()
            .map(|(r, n)| format!("{} {}", n, r))
            .collect::<Vec<_>>()
            .join(", ");

        self.send_reasoning(
            player_id,
            &format!("Trade: {} for {} — {}", offering, requesting, reasoning),
        );

        self.record_event(GameEvent::TradeProposed {
            from: player_id,
            offer: offer.clone(),
            reasoning: reasoning.clone(),
        });

        // Step 2: Find eligible responders and collect responses.
        let eligible = trading::negotiation::eligible_responders(&self.state, &offer);
        let mut accepted_by: Option<PlayerId> = None;

        for other_id in 0..self.state.num_players {
            if other_id == player_id {
                continue;
            }

            if !eligible.contains(&other_id) {
                self.record_event(GameEvent::TradeRejected {
                    by: other_id,
                    reasoning: "Insufficient resources".into(),
                });
                continue;
            }

            let (response, resp_reasoning) = self
                .with_timeout(
                    self.players[other_id].respond_to_trade(
                        &self.state,
                        other_id,
                        &offer,
                        &self.player_names,
                    ),
                    (
                        TradeResponse::Reject {
                            reason: "timeout".into(),
                        },
                        "timeout fallback".into(),
                    ),
                )
                .await;

            match response {
                TradeResponse::Accept => {
                    self.record_event(GameEvent::TradeAccepted {
                        by: other_id,
                        reasoning: resp_reasoning,
                    });
                    accepted_by = Some(other_id);
                    break; // First acceptance wins.
                }
                TradeResponse::Reject { reason: _ } => {
                    self.record_event(GameEvent::TradeRejected {
                        by: other_id,
                        reasoning: resp_reasoning,
                    });
                }
                TradeResponse::Counter(counter_offer) => {
                    // Validate the counter-offer.
                    if let Err(e) =
                        trading::negotiation::validate_trade(&self.state, &counter_offer)
                    {
                        self.record_event(GameEvent::TradeRejected {
                            by: other_id,
                            reasoning: format!("invalid counter: {}", e),
                        });
                        continue;
                    }

                    let counter_offering: String = counter_offer
                        .offering
                        .iter()
                        .map(|(r, n)| format!("{} {}", n, r))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let counter_requesting: String = counter_offer
                        .requesting
                        .iter()
                        .map(|(r, n)| format!("{} {}", n, r))
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.send_reasoning(
                        other_id,
                        &format!(
                            "Counter: {} for {} — {}",
                            counter_offering, counter_requesting, resp_reasoning
                        ),
                    );

                    self.record_event(GameEvent::TradeCountered {
                        by: other_id,
                        counter_offer: counter_offer.clone(),
                        reasoning: resp_reasoning,
                    });

                    // Ask the original proposer if they accept the counter.
                    let (counter_response, counter_reasoning) = self
                        .with_timeout(
                            self.players[player_id].respond_to_trade(
                                &self.state,
                                player_id,
                                &counter_offer,
                                &self.player_names,
                            ),
                            (
                                TradeResponse::Reject {
                                    reason: "timeout".into(),
                                },
                                "timeout fallback".into(),
                            ),
                        )
                        .await;

                    match counter_response {
                        TradeResponse::Accept => {
                            self.record_event(GameEvent::TradeAccepted {
                                by: player_id,
                                reasoning: counter_reasoning,
                            });
                            // Execute the counter-offer (from other_id's perspective).
                            match trading::negotiation::execute_in_state(
                                &mut self.state,
                                &counter_offer,
                                player_id,
                            ) {
                                Ok(()) => {}
                                Err(_) => {
                                    self.record_event(GameEvent::TradeWithdrawn { by: other_id });
                                }
                            }
                            return Ok(());
                        }
                        _ => {
                            self.record_event(GameEvent::TradeRejected {
                                by: player_id,
                                reasoning: counter_reasoning,
                            });
                        }
                    }
                }
            }
        }

        // Step 3: Execute the trade using the trading module.
        if let Some(acceptor) = accepted_by {
            match trading::negotiation::execute_in_state(&mut self.state, &offer, acceptor) {
                Ok(()) => {}
                Err(_) => {
                    self.record_event(GameEvent::TradeWithdrawn { by: player_id });
                }
            }
        }

        Ok(())
    }

    /// Apply a game action and log it.
    fn apply_and_log(
        &mut self,
        action: Action,
        player_id: PlayerId,
        reasoning: &str,
    ) -> Result<(), OrchestratorError> {
        // Compute bank trade rate BEFORE applying (state still has resources).
        let event = action_to_event(&action, player_id, reasoning, &self.state);
        rules::apply_action(&mut self.state, &action)
            .map_err(|e| OrchestratorError::RuleViolation(format!("{}: {}", action, e)))?;

        if let Some(evt) = event {
            self.record_event(evt);
        }

        // Check for longest road / largest army changes.
        rules::update_longest_road(&mut self.state);
        // update_largest_army is called within apply_play_dev_card for Knights,
        // but we also call it here for safety after any action.
        for p in 0..self.state.num_players {
            rules::update_largest_army(&mut self.state, p);
        }

        Ok(())
    }

    /// Advance to the next player's turn.
    fn end_turn(&mut self, current_player: PlayerId) {
        self.state.players[current_player].dev_cards_bought_this_turn = 0;
        self.state.turn_number += 1;
        let next = (current_player + 1) % self.state.num_players;
        self.state.phase = GamePhase::Playing {
            current_player: next,
            has_rolled: false,
        };
        self.state.players[next].has_played_dev_card_this_turn = false;
    }
}

/// Convert a game action into a GameEvent for the log.
fn action_to_event(
    action: &Action,
    player: PlayerId,
    reasoning: &str,
    state: &GameState,
) -> Option<GameEvent> {
    match action {
        Action::BuildSettlement(v) => Some(GameEvent::SettlementBuilt {
            player,
            vertex: *v,
            reasoning: reasoning.to_string(),
        }),
        Action::BuildCity(v) => Some(GameEvent::CityUpgraded {
            player,
            vertex: *v,
            reasoning: reasoning.to_string(),
        }),
        Action::BuildRoad(e) => Some(GameEvent::RoadBuilt {
            player,
            edge: *e,
            reasoning: reasoning.to_string(),
        }),
        Action::BuyDevCard => Some(GameEvent::DevCardBought { player }),
        Action::PlayDevCard(card, _dev_action) => Some(GameEvent::DevCardPlayed {
            player,
            card: card.clone(),
            reasoning: reasoning.to_string(),
        }),
        Action::BankTrade { give, get } => {
            let rate = rules::trade_rate(state, player, *give);
            Some(GameEvent::BankTradeExecuted {
                player,
                gave: (*give, rate),
                got: (*get, 1),
            })
        }
        Action::EndTurn => None,
        Action::ProposeTrade => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::{Board, Resource};
    use crate::player::random::RandomPlayer;

    fn make_orchestrator(num_players: usize) -> GameOrchestrator {
        let board = Board::default_board();
        let state = GameState::new(board, num_players);
        let players: Vec<Box<dyn crate::player::Player>> = (0..num_players)
            .map(|i| {
                Box::new(RandomPlayer::new(format!("P{}", i))) as Box<dyn crate::player::Player>
            })
            .collect();
        GameOrchestrator::new(state, players)
    }

    #[test]
    fn new_orchestrator_has_correct_state() {
        let orch = make_orchestrator(3);
        assert_eq!(orch.state.num_players, 3);
        assert_eq!(orch.players.len(), 3);
        assert_eq!(orch.player_names.len(), 3);
        assert_eq!(orch.player_names[0], "P0");
        assert_eq!(orch.max_turns, 500);
        assert!(orch.ui_tx.is_none());
        assert!(orch.events.is_empty());
    }

    #[test]
    fn record_event_populates_events() {
        let mut orch = make_orchestrator(3);
        let event = GameEvent::DiceRolled {
            player: 0,
            values: (3, 4),
            total: 7,
        };
        orch.record_event(event);

        assert_eq!(orch.events.len(), 1);
    }

    #[test]
    fn build_choices_includes_end_turn() {
        let mut orch = make_orchestrator(3);
        orch.state.phase = GamePhase::Playing {
            current_player: 0,
            has_rolled: true,
        };
        let choices = orch.build_choices();
        assert!(
            choices
                .iter()
                .any(|c| matches!(c, PlayerChoice::GameAction(Action::EndTurn))),
            "Should always include EndTurn"
        );
    }

    #[test]
    fn build_choices_includes_propose_trade_when_has_resources() {
        let mut orch = make_orchestrator(3);
        orch.state.phase = GamePhase::Playing {
            current_player: 0,
            has_rolled: true,
        };
        orch.state.players[0].add_resource(Resource::Wood, 1);
        let choices = orch.build_choices();
        assert!(
            choices
                .iter()
                .any(|c| matches!(c, PlayerChoice::ProposeTrade)),
            "Should include ProposeTrade when player has resources"
        );
    }

    #[test]
    fn build_choices_no_propose_trade_when_no_resources() {
        let mut orch = make_orchestrator(3);
        orch.state.phase = GamePhase::Playing {
            current_player: 0,
            has_rolled: true,
        };
        let choices = orch.build_choices();
        assert!(
            !choices
                .iter()
                .any(|c| matches!(c, PlayerChoice::ProposeTrade)),
            "Should not include ProposeTrade when player has no resources"
        );
    }

    #[test]
    fn build_choices_includes_dev_card_intents() {
        let mut orch = make_orchestrator(3);
        orch.state.phase = GamePhase::Playing {
            current_player: 0,
            has_rolled: true,
        };
        orch.state.players[0]
            .dev_cards
            .push(crate::game::actions::DevCard::Knight);
        orch.state.players[0]
            .dev_cards
            .push(crate::game::actions::DevCard::Monopoly);
        orch.state.players[0]
            .dev_cards
            .push(crate::game::actions::DevCard::YearOfPlenty);

        let choices = orch.build_choices();
        assert!(choices
            .iter()
            .any(|c| matches!(c, PlayerChoice::PlayKnight)));
        assert!(choices
            .iter()
            .any(|c| matches!(c, PlayerChoice::PlayMonopoly)));
        assert!(choices
            .iter()
            .any(|c| matches!(c, PlayerChoice::PlayYearOfPlenty)));
    }

    #[test]
    fn build_choices_no_road_building_if_insufficient_roads() {
        let mut orch = make_orchestrator(3);
        orch.state.phase = GamePhase::Playing {
            current_player: 0,
            has_rolled: true,
        };
        orch.state.players[0]
            .dev_cards
            .push(crate::game::actions::DevCard::RoadBuilding);
        orch.state.players[0].roads_remaining = 1; // Need 2

        let choices = orch.build_choices();
        assert!(
            !choices
                .iter()
                .any(|c| matches!(c, PlayerChoice::PlayRoadBuilding)),
            "Should not include RoadBuilding with < 2 roads remaining"
        );
    }

    #[test]
    fn build_choices_no_dev_cards_if_already_played_this_turn() {
        let mut orch = make_orchestrator(3);
        orch.state.phase = GamePhase::Playing {
            current_player: 0,
            has_rolled: true,
        };
        orch.state.players[0]
            .dev_cards
            .push(crate::game::actions::DevCard::Knight);
        orch.state.players[0].has_played_dev_card_this_turn = true;

        let choices = orch.build_choices();
        assert!(
            !choices
                .iter()
                .any(|c| matches!(c, PlayerChoice::PlayKnight)),
            "Should not include dev card intents after already playing one"
        );
    }

    #[test]
    fn end_turn_advances_player() {
        let mut orch = make_orchestrator(3);
        orch.state.phase = GamePhase::Playing {
            current_player: 0,
            has_rolled: true,
        };
        orch.end_turn(0);

        assert_eq!(orch.state.turn_number, 1);
        match orch.state.phase {
            GamePhase::Playing {
                current_player,
                has_rolled,
            } => {
                assert_eq!(current_player, 1);
                assert!(!has_rolled);
            }
            _ => panic!("Expected Playing phase"),
        }
    }

    #[test]
    fn end_turn_wraps_around() {
        let mut orch = make_orchestrator(3);
        orch.end_turn(2);
        match orch.state.phase {
            GamePhase::Playing { current_player, .. } => {
                assert_eq!(current_player, 0);
            }
            _ => panic!("Expected Playing phase"),
        }
    }

    #[test]
    fn action_to_event_maps_build_settlement() {
        let state = GameState::new(Board::default_board(), 3);
        let v = crate::game::board::VertexCoord::new(
            crate::game::board::HexCoord::new(0, 0),
            crate::game::board::VertexDirection::North,
        );
        let event = action_to_event(&Action::BuildSettlement(v), 0, "test", &state);
        assert!(matches!(
            event,
            Some(GameEvent::SettlementBuilt { player: 0, .. })
        ));
    }

    #[test]
    fn action_to_event_maps_end_turn_to_none() {
        let state = GameState::new(Board::default_board(), 3);
        let event = action_to_event(&Action::EndTurn, 0, "", &state);
        assert!(event.is_none());
    }

    #[test]
    fn action_to_event_maps_bank_trade() {
        let state = GameState::new(Board::default_board(), 3);
        let event = action_to_event(
            &Action::BankTrade {
                give: Resource::Wood,
                get: Resource::Ore,
            },
            0,
            "trade",
            &state,
        );
        match event {
            Some(GameEvent::BankTradeExecuted { player, gave, got }) => {
                assert_eq!(player, 0);
                assert_eq!(gave.0, Resource::Wood);
                assert_eq!(gave.1, 4); // default 4:1 rate with no ports
                assert_eq!(got.0, Resource::Ore);
                assert_eq!(got.1, 1);
            }
            _ => panic!("Expected BankTradeExecuted"),
        }
    }

    #[tokio::test]
    async fn full_game_with_random_players() {
        let mut orch = make_orchestrator(3);
        orch.max_turns = 300;

        let result = orch.run().await;
        match result {
            Ok(winner) => {
                assert!(winner < 3);
                assert!(orch.state.victory_points(winner) >= 10);
                assert!(!orch.events.is_empty());
            }
            Err(OrchestratorError::GameStuck(_)) => {
                // Random players may not converge -- acceptable.
                assert!(!orch.events.is_empty());
            }
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }

    #[tokio::test]
    async fn setup_places_correct_number_of_pieces() {
        let mut orch = make_orchestrator(3);
        orch.run_setup().await.unwrap();

        // Each player should have 2 settlements and 2 roads after setup.
        assert_eq!(
            orch.state.buildings.len(),
            6,
            "3 players * 2 settlements = 6"
        );
        assert_eq!(orch.state.roads.len(), 6, "3 players * 2 roads = 6");
    }

    #[tokio::test]
    async fn ui_channel_receives_events() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiEvent>();
        let mut orch = make_orchestrator(3);
        orch.max_turns = 5;
        orch.ui_tx = Some(tx);

        // Run in background.
        let handle = tokio::spawn(async move { orch.run().await });

        // Collect some UI events.
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }

        let _ = handle.await;

        // Should have received state updates.
        assert!(!events.is_empty(), "Should receive UI events");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, UiEvent::StateUpdate { .. })),
            "Should have state update events"
        );
    }
}
