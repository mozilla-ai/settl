//! TUI-based human player that communicates with the ratatui UI via channels.
//!
//! The game engine sends decision prompts through a channel, and the TUI
//! renders them as a selection overlay. The human picks with arrow keys + Enter,
//! and the response flows back through a second channel.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::game::actions::{PlayerId, TradeOffer, TradeResponse};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};
use crate::game::state::GameState;
use crate::player::{Player, PlayerChoice};

/// A prompt sent from the game engine to the TUI for human input.
pub struct HumanPrompt {
    pub player_id: PlayerId,
    pub title: String,
    pub options: Vec<String>,
}

/// Shared channel endpoints for TUI↔engine human input.
pub struct HumanInputChannel {
    pub prompt_tx: mpsc::UnboundedSender<HumanPrompt>,
    pub response_rx: Mutex<mpsc::UnboundedReceiver<usize>>,
}

/// A human player that integrates with the TUI via channels.
pub struct TuiHumanPlayer {
    name: String,
    channel: Arc<HumanInputChannel>,
}

impl TuiHumanPlayer {
    pub fn new(name: String, channel: Arc<HumanInputChannel>) -> Self {
        Self { name, channel }
    }

    /// Send a prompt to the TUI and wait for the user's selection.
    async fn pick_index(&self, player_id: PlayerId, title: String, options: Vec<String>) -> usize {
        let max = options.len().saturating_sub(1);
        let _ = self.channel.prompt_tx.send(HumanPrompt {
            player_id,
            title,
            options,
        });
        let mut rx = self.channel.response_rx.lock().await;
        rx.recv().await.unwrap_or(0).min(max)
    }
}

const RESOURCE_NAMES: &[&str] = &["Wood", "Brick", "Sheep", "Wheat", "Ore"];
const RESOURCES: &[Resource] = &[
    Resource::Wood,
    Resource::Brick,
    Resource::Sheep,
    Resource::Wheat,
    Resource::Ore,
];

#[async_trait]
impl Player for TuiHumanPlayer {
    fn name(&self) -> &str {
        &self.name
    }

    async fn choose_action(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        choices: &[PlayerChoice],
    ) -> (usize, String) {
        let options: Vec<String> = choices.iter().map(|c| format!("{}", c)).collect();
        let idx = self.pick_index(player_id, "Choose action".into(), options).await;
        (idx, String::new())
    }

    async fn choose_settlement(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        legal_vertices: &[VertexCoord],
    ) -> (usize, String) {
        let options: Vec<String> = legal_vertices
            .iter()
            .map(|v| format!("({},{},{:?})", v.hex.q, v.hex.r, v.dir))
            .collect();
        let idx = self
            .pick_index(player_id, "Place settlement".into(), options)
            .await;
        (idx, String::new())
    }

    async fn choose_road(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        legal_edges: &[EdgeCoord],
    ) -> (usize, String) {
        let options: Vec<String> = legal_edges.iter().map(|e| format!("{}", e)).collect();
        let idx = self
            .pick_index(player_id, "Place road".into(), options)
            .await;
        (idx, String::new())
    }

    async fn choose_robber_hex(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        legal_hexes: &[HexCoord],
    ) -> (usize, String) {
        let options: Vec<String> = legal_hexes
            .iter()
            .map(|h| format!("({},{})", h.q, h.r))
            .collect();
        let idx = self
            .pick_index(player_id, "Move robber".into(), options)
            .await;
        (idx, String::new())
    }

    async fn choose_steal_target(
        &self,
        state: &GameState,
        player_id: PlayerId,
        targets: &[PlayerId],
    ) -> (usize, String) {
        let options: Vec<String> = targets
            .iter()
            .map(|&p| {
                format!(
                    "Player {} ({} cards)",
                    p,
                    state.players[p].total_resources()
                )
            })
            .collect();
        let idx = self
            .pick_index(player_id, "Steal from".into(), options)
            .await;
        (idx, String::new())
    }

    async fn choose_discard(
        &self,
        state: &GameState,
        player_id: PlayerId,
        count: usize,
    ) -> (Vec<Resource>, String) {
        let ps = &state.players[player_id];
        let mut discards = Vec::with_capacity(count);
        // Track remaining resources as we pick
        let mut remaining = [
            ps.resource_count(Resource::Wood),
            ps.resource_count(Resource::Brick),
            ps.resource_count(Resource::Sheep),
            ps.resource_count(Resource::Wheat),
            ps.resource_count(Resource::Ore),
        ];

        for i in 0..count {
            let mut options = Vec::new();
            let mut resource_indices = Vec::new();
            for (j, &name) in RESOURCE_NAMES.iter().enumerate() {
                if remaining[j] > 0 {
                    options.push(format!("{} (have {})", name, remaining[j]));
                    resource_indices.push(j);
                }
            }
            let title = format!("Discard card {}/{}", i + 1, count);
            let idx = self.pick_index(player_id, title, options).await;
            let res_idx = resource_indices[idx.min(resource_indices.len() - 1)];
            remaining[res_idx] -= 1;
            discards.push(RESOURCES[res_idx]);
        }

        (discards, String::new())
    }

    async fn choose_resource(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        context: &str,
    ) -> (Resource, String) {
        let options: Vec<String> = RESOURCE_NAMES.iter().map(|s| s.to_string()).collect();
        let idx = self.pick_index(player_id, context.to_string(), options).await;
        (RESOURCES[idx.min(RESOURCES.len() - 1)], String::new())
    }

    async fn propose_trade(
        &self,
        state: &GameState,
        player_id: PlayerId,
    ) -> Option<(TradeOffer, String)> {
        let ps = &state.players[player_id];

        // Pick resource to give (only show resources the player has)
        let mut give_options = Vec::new();
        let mut give_indices = Vec::new();
        for (i, &name) in RESOURCE_NAMES.iter().enumerate() {
            let count = ps.resource_count(RESOURCES[i]);
            if count > 0 {
                give_options.push(format!("{} (have {})", name, count));
                give_indices.push(i);
            }
        }
        give_options.push("Cancel".into());

        let give_idx = self
            .pick_index(player_id, "Trade: give what?".into(), give_options.clone())
            .await;
        if give_idx >= give_indices.len() {
            return None; // cancelled
        }
        let give_resource = RESOURCES[give_indices[give_idx]];

        // Pick resource to request
        let mut get_options: Vec<String> = RESOURCE_NAMES.iter().map(|s| s.to_string()).collect();
        get_options.push("Cancel".into());
        let get_idx = self
            .pick_index(player_id, "Trade: want what?".into(), get_options)
            .await;
        if get_idx >= RESOURCES.len() {
            return None;
        }
        let get_resource = RESOURCES[get_idx];

        Some((
            TradeOffer {
                from: player_id,
                offering: vec![(give_resource, 1)],
                requesting: vec![(get_resource, 1)],
                message: String::new(),
            },
            String::new(),
        ))
    }

    async fn respond_to_trade(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        offer: &TradeOffer,
    ) -> (TradeResponse, String) {
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

        let title = format!("P{} offers [{}] for [{}]", offer.from, offering, requesting);
        let options = vec!["Accept".into(), "Reject".into()];
        let idx = self.pick_index(player_id, title, options).await;
        if idx == 0 {
            (TradeResponse::Accept, String::new())
        } else {
            (
                TradeResponse::Reject {
                    reason: "Declined".into(),
                },
                String::new(),
            )
        }
    }
}
