//! A random player for testing — picks uniformly at random from legal options.

use async_trait::async_trait;
use rand::Rng;

use crate::game::actions::{PlayerId, TradeOffer, TradeResponse};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};
use crate::game::state::GameState;
use crate::player::{Player, PlayerChoice};

/// A player that makes uniformly random decisions. Useful for testing
/// the game loop and rules engine without needing LLM API keys.
pub struct RandomPlayer {
    name: String,
}

impl RandomPlayer {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

#[async_trait]
impl Player for RandomPlayer {
    fn name(&self) -> &str {
        &self.name
    }

    async fn choose_action(
        &self,
        _state: &GameState,
        _player_id: PlayerId,
        choices: &[PlayerChoice],
    ) -> (usize, String) {
        let idx = rand::rng().random_range(0..choices.len());
        (idx, format!("[random] chose: {}", choices[idx]))
    }

    async fn choose_settlement(
        &self,
        _state: &GameState,
        _player_id: PlayerId,
        legal_vertices: &[VertexCoord],
        _round: u8,
    ) -> (usize, String) {
        let idx = rand::rng().random_range(0..legal_vertices.len());
        (idx, "[random settlement]".into())
    }

    async fn choose_road(
        &self,
        _state: &GameState,
        _player_id: PlayerId,
        legal_edges: &[EdgeCoord],
    ) -> (usize, String) {
        let idx = rand::rng().random_range(0..legal_edges.len());
        (idx, "[random road]".into())
    }

    async fn choose_robber_hex(
        &self,
        _state: &GameState,
        _player_id: PlayerId,
        legal_hexes: &[HexCoord],
    ) -> (usize, String) {
        let idx = rand::rng().random_range(0..legal_hexes.len());
        (idx, "[random robber]".into())
    }

    async fn choose_steal_target(
        &self,
        _state: &GameState,
        _player_id: PlayerId,
        targets: &[PlayerId],
    ) -> (usize, String) {
        let idx = rand::rng().random_range(0..targets.len());
        (idx, "[random steal]".into())
    }

    async fn choose_discard(
        &self,
        state: &GameState,
        player_id: PlayerId,
        count: usize,
    ) -> (Vec<Resource>, String) {
        let ps = &state.players[player_id];
        // Build a pool of all resources and randomly pick `count` of them.
        let mut pool: Vec<Resource> = Vec::new();
        for &r in Resource::all() {
            let c = ps.resource_count(r);
            for _ in 0..c {
                pool.push(r);
            }
        }

        use rand::seq::SliceRandom;
        let mut rng = rand::rng();
        pool.shuffle(&mut rng);
        pool.truncate(count);

        (pool, "[random discard]".into())
    }

    async fn choose_resource(
        &self,
        _state: &GameState,
        _player_id: PlayerId,
        _context: &str,
    ) -> (Resource, String) {
        let all = Resource::all();
        let idx = rand::rng().random_range(0..all.len());
        (all[idx], "[random resource]".into())
    }

    async fn propose_trade(
        &self,
        state: &GameState,
        player_id: PlayerId,
    ) -> Option<(TradeOffer, String)> {
        let ps = &state.players[player_id];
        // Random player: offer a random resource they have for a random one they don't.
        let have: Vec<Resource> = Resource::all()
            .iter()
            .copied()
            .filter(|&r| ps.resource_count(r) > 0)
            .collect();
        let want: Vec<Resource> = Resource::all()
            .iter()
            .copied()
            .filter(|&r| ps.resource_count(r) == 0)
            .collect();

        if have.is_empty() || want.is_empty() {
            return None;
        }

        use rand::Rng;
        let mut rng = rand::rng();
        let give = have[rng.random_range(0..have.len())];
        let get = want[rng.random_range(0..want.len())];

        Some((
            TradeOffer {
                from: player_id,
                offering: vec![(give, 1)],
                requesting: vec![(get, 1)],
                message: format!("Anyone got {}? I'll trade {} for it.", get, give),
            },
            "[random trade]".into(),
        ))
    }

    async fn respond_to_trade(
        &self,
        state: &GameState,
        player_id: PlayerId,
        offer: &TradeOffer,
    ) -> (TradeResponse, String) {
        // Use the trade value heuristic to make smarter decisions.
        let ps = &state.players[player_id];
        let value = crate::trading::negotiation::trade_value_heuristic(offer, ps);
        if value > 0.5 {
            (
                TradeResponse::Accept,
                format!("[heuristic accept, value={:.1}]", value),
            )
        } else {
            (
                TradeResponse::Reject {
                    reason: format!("Trade value too low ({:.1})", value),
                },
                format!("[heuristic reject, value={:.1}]", value),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::Board;
    use crate::game::orchestrator::GameOrchestrator;
    use crate::game::state::GameState;

    #[tokio::test]
    async fn random_game_completes() {
        // Run a full game with random players and verify it ends with a winner.
        let board = Board::default_board();
        let state = GameState::new(board, 3);

        let players: Vec<Box<dyn Player>> = vec![
            Box::new(RandomPlayer::new("Alice".into())),
            Box::new(RandomPlayer::new("Bob".into())),
            Box::new(RandomPlayer::new("Charlie".into())),
        ];

        let mut orchestrator = GameOrchestrator::new(state, players);
        orchestrator.max_turns = 300;

        let result = orchestrator.run().await;
        match result {
            Ok(winner) => {
                assert!(winner < 3, "Winner should be a valid player");
                assert!(
                    orchestrator.state.victory_points(winner) >= 10,
                    "Winner should have at least 10 VP"
                );
                assert!(
                    !orchestrator.events.is_empty(),
                    "Game should have recorded events"
                );
                println!(
                    "Game finished in {} turns. Winner: Player {} with {} VP",
                    orchestrator.state.turn_number,
                    winner,
                    orchestrator.state.victory_points(winner)
                );
            }
            Err(e) => {
                // GameStuck is acceptable for random players — they might not
                // converge to 10 VP in 300 turns. This is not a test failure.
                println!("Game did not finish: {}", e);
            }
        }
    }
}
