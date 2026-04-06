pub mod anthropic_client;
pub mod human;
pub mod llm_player;
pub mod personality;
pub mod prompt;
#[cfg(test)]
pub mod random;
pub mod tui_human;

use async_trait::async_trait;

use crate::game::actions::{Action, PlayerId, TradeOffer, TradeResponse};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};
use crate::game::state::GameState;

/// A choice the orchestrator presents to a player, wrapping both direct game
/// actions and dev-card intents (whose parameters are collected in follow-up
/// calls).
#[derive(Debug, Clone)]
pub enum PlayerChoice {
    /// A direct game action (BuildSettlement, BuildRoad, BankTrade, EndTurn, etc.)
    GameAction(Action),
    /// Intent to play a Knight — robber hex and steal target collected separately.
    PlayKnight,
    /// Intent to play Monopoly — resource collected separately.
    PlayMonopoly,
    /// Intent to play Year of Plenty — two resources collected separately.
    PlayYearOfPlenty,
    /// Intent to play Road Building — two edges collected separately.
    PlayRoadBuilding,
    /// Intent to propose a trade — offer details collected separately.
    ProposeTrade,
    /// Intent to build a road — edge collected via board cursor.
    BuildRoadIntent,
    /// Intent to build a settlement — vertex collected via board cursor.
    BuildSettlementIntent,
    /// Intent to build a city — vertex collected via board cursor.
    BuildCityIntent,
    /// Intent to do a bank trade — specific give/get collected in a follow-up step.
    BankTradeIntent,
    /// Roll the dice (used in pre-roll knight prompt as alternative to playing a knight).
    RollDice,
}

impl PlayerChoice {
    /// Short human-readable label for the TUI action bar (no coordinates).
    pub fn label(&self) -> String {
        match self {
            PlayerChoice::GameAction(Action::BuildSettlement(_)) => "Build Settlement".into(),
            PlayerChoice::GameAction(Action::BuildCity(_)) => "Build City".into(),
            PlayerChoice::GameAction(Action::BuildRoad(_)) => "Build Road".into(),
            PlayerChoice::GameAction(Action::BuyDevCard) => "Buy Development Card".into(),
            PlayerChoice::GameAction(Action::EndTurn) => "End Turn".into(),
            PlayerChoice::RollDice => "Roll Dice".into(),
            PlayerChoice::GameAction(Action::ProposeTrade) => "Propose Trade".into(),
            PlayerChoice::GameAction(Action::BankTrade { give, get }) => {
                format!("{} -> {}", give, get)
            }
            PlayerChoice::GameAction(Action::PlayDevCard(_, _)) => "Play Dev Card".into(),
            PlayerChoice::PlayKnight => "Play Knight".into(),
            PlayerChoice::PlayMonopoly => "Play Monopoly".into(),
            PlayerChoice::PlayYearOfPlenty => "Play Year of Plenty".into(),
            PlayerChoice::PlayRoadBuilding => "Play Road Building".into(),
            PlayerChoice::ProposeTrade => "Propose Trade".into(),
            PlayerChoice::BuildRoadIntent => "Build Road".into(),
            PlayerChoice::BuildSettlementIntent => "Build Settlement".into(),
            PlayerChoice::BuildCityIntent => "Build City".into(),
            PlayerChoice::BankTradeIntent => "Bank Trade".into(),
        }
    }

    /// Keyboard shortcut for this choice in the TUI action bar.
    pub fn shortcut_key(&self) -> Option<char> {
        match self {
            PlayerChoice::GameAction(Action::EndTurn) => Some('e'),
            PlayerChoice::GameAction(Action::BuildSettlement(_)) => Some('s'),
            PlayerChoice::GameAction(Action::BuildRoad(_)) => Some('r'),
            PlayerChoice::GameAction(Action::BuildCity(_)) => Some('c'),
            PlayerChoice::GameAction(Action::BuyDevCard) => Some('d'),
            PlayerChoice::ProposeTrade => Some('t'),
            PlayerChoice::PlayKnight
            | PlayerChoice::PlayMonopoly
            | PlayerChoice::PlayYearOfPlenty
            | PlayerChoice::PlayRoadBuilding => Some('p'),
            PlayerChoice::BuildRoadIntent => Some('r'),
            PlayerChoice::BuildSettlementIntent => Some('s'),
            PlayerChoice::BuildCityIntent => Some('c'),
            PlayerChoice::BankTradeIntent => Some('b'),
            PlayerChoice::RollDice => Some('r'),
            _ => None,
        }
    }

    /// Whether this choice represents an "End Turn" action.
    pub fn is_end_turn(&self) -> bool {
        matches!(self, PlayerChoice::GameAction(Action::EndTurn))
    }

    /// Whether this is a "Play dev card" intent.
    pub fn is_play_dev_card(&self) -> bool {
        matches!(
            self,
            PlayerChoice::PlayKnight
                | PlayerChoice::PlayMonopoly
                | PlayerChoice::PlayYearOfPlenty
                | PlayerChoice::PlayRoadBuilding
        )
    }
}

impl std::fmt::Display for PlayerChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayerChoice::GameAction(a) => write!(f, "{}", a),
            PlayerChoice::PlayKnight => write!(f, "Play Knight"),
            PlayerChoice::PlayMonopoly => write!(f, "Play Monopoly"),
            PlayerChoice::PlayYearOfPlenty => write!(f, "Play Year of Plenty"),
            PlayerChoice::PlayRoadBuilding => write!(f, "Play Road Building"),
            PlayerChoice::ProposeTrade => write!(f, "Propose Trade"),
            PlayerChoice::BuildRoadIntent => write!(f, "Build Road"),
            PlayerChoice::BuildSettlementIntent => write!(f, "Build Settlement"),
            PlayerChoice::BuildCityIntent => write!(f, "Build City"),
            PlayerChoice::BankTradeIntent => write!(f, "Bank Trade"),
            PlayerChoice::RollDice => write!(f, "Roll Dice"),
        }
    }
}

/// The interface every player type (human, LLM, random) implements.
///
/// Each method returns a decision plus a reasoning string. For LLM players the
/// reasoning is the model's strategic explanation; for humans it's empty or a
/// user-typed note.
#[async_trait]
pub trait Player: Send + Sync {
    /// Display name for this player (e.g. "Claude", "GPT-4o", "Alice").
    fn name(&self) -> &str;

    /// Choose a game action from the list of legal choices.
    /// Returns `(index into choices, reasoning)`.
    async fn choose_action(
        &self,
        state: &GameState,
        player_id: PlayerId,
        choices: &[PlayerChoice],
    ) -> (usize, String);

    /// Choose a vertex to place a settlement during setup or normal play.
    /// `round` is the setup round (1 or 2); 0 during normal play.
    /// Returns `(index into legal_vertices, reasoning)`.
    async fn choose_settlement(
        &self,
        state: &GameState,
        player_id: PlayerId,
        legal_vertices: &[VertexCoord],
        round: u8,
        player_names: &[String],
    ) -> (usize, String);

    /// Choose an edge to place a road.
    /// Returns `(index into legal_edges, reasoning)`.
    async fn choose_road(
        &self,
        state: &GameState,
        player_id: PlayerId,
        legal_edges: &[EdgeCoord],
        player_names: &[String],
    ) -> (usize, String);

    /// Choose a hex to move the robber to (after rolling 7 or playing Knight).
    /// Returns `(index into legal_hexes, reasoning)`.
    async fn choose_robber_hex(
        &self,
        state: &GameState,
        player_id: PlayerId,
        legal_hexes: &[HexCoord],
    ) -> (usize, String);

    /// Choose a player to steal from.
    /// Returns `(index into targets, reasoning)`.
    async fn choose_steal_target(
        &self,
        state: &GameState,
        player_id: PlayerId,
        targets: &[PlayerId],
        player_names: &[String],
    ) -> (usize, String);

    /// Choose which cards to discard (when holding >7 cards on a 7-roll).
    /// Must return exactly `count` resources.
    async fn choose_discard(
        &self,
        state: &GameState,
        player_id: PlayerId,
        count: usize,
    ) -> (Vec<Resource>, String);

    /// Choose a single resource (for Monopoly or each pick in Year of Plenty).
    async fn choose_resource(
        &self,
        state: &GameState,
        player_id: PlayerId,
        context: &str,
    ) -> (Resource, String);

    /// Create a trade offer to propose to other players.
    /// Returns `None` if the player decides not to trade after all.
    async fn propose_trade(
        &self,
        state: &GameState,
        player_id: PlayerId,
    ) -> Option<(TradeOffer, String)>;

    /// Respond to a trade offer from another player.
    async fn respond_to_trade(
        &self,
        state: &GameState,
        player_id: PlayerId,
        offer: &TradeOffer,
        player_names: &[String],
    ) -> (TradeResponse, String);

    /// Provide extra game context (recent history, trade log) for the player's
    /// next decision. Default is a no-op; LLM players use this to enrich prompts.
    async fn set_game_context(&self, _context: &str) {}
}
