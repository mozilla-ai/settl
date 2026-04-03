//! TUI-based human player that communicates with the ratatui UI via channels.
//!
//! The game engine sends typed decision prompts through a channel, and the TUI
//! renders them as mode-specific UI (board cursor, action bar, trade builder, etc.).
//! The human interacts, and the response flows back through a second channel.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::game::actions::{PlayerId, TradeOffer, TradeResponse};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};
use crate::game::state::GameState;
use crate::player::{Player, PlayerChoice};

// ── Prompt types (engine → TUI) ──────────────────────────────────────

/// The kind of decision the TUI must present.
#[derive(Debug)]
pub enum PromptKind {
    /// Pick from a list of game actions (action bar with shortcuts).
    ChooseAction { choices: Vec<PlayerChoice> },
    /// Place a settlement on one of these legal vertices (board cursor).
    PlaceSettlement { legal: Vec<VertexCoord> },
    /// Place a road on one of these legal edges (board cursor).
    PlaceRoad { legal: Vec<EdgeCoord> },
    /// Move the robber to one of these hexes (board cursor).
    PlaceRobber { legal: Vec<HexCoord> },
    /// Steal from one of these players.
    ChooseStealTarget { targets: Vec<(PlayerId, String)> },
    /// Discard exactly `count` cards from the given resource pool.
    Discard { count: usize, available: [u32; 5] },
    /// Pick a single resource (for Monopoly, Year of Plenty, etc.).
    ChooseResource { context: String },
    /// Build a trade offer using the trade builder.
    ProposeTrade { available: [u32; 5] },
    /// Accept or reject an incoming trade offer.
    RespondToTrade { offer: TradeOffer },
}

/// A prompt sent from the game engine to the TUI for human input.
pub struct HumanPrompt {
    pub player_id: PlayerId,
    pub kind: PromptKind,
}

// ── Response types (TUI → engine) ────────────────────────────────────

/// The response sent from the TUI back to the game engine.
#[derive(Debug)]
pub enum HumanResponse {
    /// An index into the presented list (actions, vertices, edges, hexes, targets, resources).
    Index(usize),
    /// A list of resources (for discard).
    Resources(Vec<Resource>),
    /// A trade offer, or None if cancelled.
    Trade(Option<TradeOffer>),
    /// Accept (true) or reject (false) a trade.
    TradeAnswer(bool),
}

// ── Channel ──────────────────────────────────────────────────────────

/// Shared channel endpoints for TUI↔engine human input.
///
/// NOTE: All human players share this single channel pair. The game engine
/// calls players sequentially, so responses are matched by ordering. This
/// means only ONE human player per game is safe. Multiple human players
/// would cause misrouted responses when the engine calls `respond_to_trade`
/// on several players concurrently. If multi-human is needed in the future,
/// switch to per-player channels.
pub struct HumanInputChannel {
    pub prompt_tx: mpsc::UnboundedSender<HumanPrompt>,
    pub response_rx: Mutex<mpsc::UnboundedReceiver<HumanResponse>>,
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

    /// Send a typed prompt to the TUI and wait for the response.
    async fn send_prompt(&self, player_id: PlayerId, kind: PromptKind) -> HumanResponse {
        if self
            .channel
            .prompt_tx
            .send(HumanPrompt { player_id, kind })
            .is_err()
        {
            log::error!("TuiHumanPlayer: prompt channel closed, defaulting to Index(0)");
            return HumanResponse::Index(0);
        }
        let mut rx = self.channel.response_rx.lock().await;
        match rx.recv().await {
            Some(resp) => resp,
            None => {
                log::error!("TuiHumanPlayer: response channel closed, defaulting to Index(0)");
                HumanResponse::Index(0)
            }
        }
    }

    /// Send a prompt and extract the index, clamped to max.
    async fn pick_index(&self, player_id: PlayerId, kind: PromptKind, max: usize) -> usize {
        match self.send_prompt(player_id, kind).await {
            HumanResponse::Index(i) => i.min(max),
            _ => 0,
        }
    }
}

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
        let max = choices.len().saturating_sub(1);
        let idx = self
            .pick_index(
                player_id,
                PromptKind::ChooseAction {
                    choices: choices.to_vec(),
                },
                max,
            )
            .await;
        (idx, String::new())
    }

    async fn choose_settlement(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        legal_vertices: &[VertexCoord],
        _round: u8,
    ) -> (usize, String) {
        let max = legal_vertices.len().saturating_sub(1);
        let idx = self
            .pick_index(
                player_id,
                PromptKind::PlaceSettlement {
                    legal: legal_vertices.to_vec(),
                },
                max,
            )
            .await;
        (idx, String::new())
    }

    async fn choose_road(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        legal_edges: &[EdgeCoord],
    ) -> (usize, String) {
        let max = legal_edges.len().saturating_sub(1);
        let idx = self
            .pick_index(
                player_id,
                PromptKind::PlaceRoad {
                    legal: legal_edges.to_vec(),
                },
                max,
            )
            .await;
        (idx, String::new())
    }

    async fn choose_robber_hex(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        legal_hexes: &[HexCoord],
    ) -> (usize, String) {
        let max = legal_hexes.len().saturating_sub(1);
        let idx = self
            .pick_index(
                player_id,
                PromptKind::PlaceRobber {
                    legal: legal_hexes.to_vec(),
                },
                max,
            )
            .await;
        (idx, String::new())
    }

    async fn choose_steal_target(
        &self,
        state: &GameState,
        player_id: PlayerId,
        targets: &[PlayerId],
    ) -> (usize, String) {
        let target_info: Vec<(PlayerId, String)> = targets
            .iter()
            .map(|&p| {
                (
                    p,
                    format!(
                        "Player {} ({} cards)",
                        p,
                        state.players[p].total_resources()
                    ),
                )
            })
            .collect();
        let max = targets.len().saturating_sub(1);
        let idx = self
            .pick_index(
                player_id,
                PromptKind::ChooseStealTarget {
                    targets: target_info,
                },
                max,
            )
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
        let available = [
            ps.resource_count(Resource::Wood),
            ps.resource_count(Resource::Brick),
            ps.resource_count(Resource::Sheep),
            ps.resource_count(Resource::Wheat),
            ps.resource_count(Resource::Ore),
        ];
        let response = self
            .send_prompt(player_id, PromptKind::Discard { count, available })
            .await;
        match response {
            HumanResponse::Resources(resources) => (resources, String::new()),
            _ => {
                // Fallback: discard first N resources
                let mut result = Vec::new();
                let mut rem = available;
                for _ in 0..count {
                    for (i, r) in rem.iter_mut().enumerate() {
                        if *r > 0 {
                            *r -= 1;
                            result.push(RESOURCES[i]);
                            break;
                        }
                    }
                }
                (result, String::new())
            }
        }
    }

    async fn choose_resource(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        context: &str,
    ) -> (Resource, String) {
        let max = RESOURCES.len() - 1;
        let idx = self
            .pick_index(
                player_id,
                PromptKind::ChooseResource {
                    context: context.to_string(),
                },
                max,
            )
            .await;
        (RESOURCES[idx.min(max)], String::new())
    }

    async fn propose_trade(
        &self,
        state: &GameState,
        player_id: PlayerId,
    ) -> Option<(TradeOffer, String)> {
        let ps = &state.players[player_id];
        let available = [
            ps.resource_count(Resource::Wood),
            ps.resource_count(Resource::Brick),
            ps.resource_count(Resource::Sheep),
            ps.resource_count(Resource::Wheat),
            ps.resource_count(Resource::Ore),
        ];
        let response = self
            .send_prompt(player_id, PromptKind::ProposeTrade { available })
            .await;
        match response {
            HumanResponse::Trade(Some(offer)) => Some((offer, String::new())),
            _ => None,
        }
    }

    async fn respond_to_trade(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        offer: &TradeOffer,
    ) -> (TradeResponse, String) {
        let response = self
            .send_prompt(
                player_id,
                PromptKind::RespondToTrade {
                    offer: offer.clone(),
                },
            )
            .await;
        match response {
            HumanResponse::TradeAnswer(true) => (TradeResponse::Accept, String::new()),
            _ => (
                TradeResponse::Reject {
                    reason: "Declined".into(),
                },
                String::new(),
            ),
        }
    }
}
