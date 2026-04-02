use serde::{Deserialize, Serialize};
use std::fmt;

use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};

/// Player identifier: 0-3 for a 4-player game.
pub type PlayerId = usize;

/// Development card types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DevCard {
    Knight,
    VictoryPoint,
    RoadBuilding,
    YearOfPlenty,
    Monopoly,
}

impl fmt::Display for DevCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DevCard::Knight => write!(f, "Knight"),
            DevCard::VictoryPoint => write!(f, "Victory Point"),
            DevCard::RoadBuilding => write!(f, "Road Building"),
            DevCard::YearOfPlenty => write!(f, "Year of Plenty"),
            DevCard::Monopoly => write!(f, "Monopoly"),
        }
    }
}

/// The specific effect when a development card is played.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DevCardAction {
    /// Play a Knight: move the robber and optionally steal from a player.
    Knight {
        robber_to: HexCoord,
        steal_from: Option<PlayerId>,
    },
    /// Take any two resources from the bank.
    YearOfPlenty(Resource, Resource),
    /// Name a resource; all other players hand over all of that resource.
    Monopoly(Resource),
    /// Place two roads for free.
    RoadBuilding(EdgeCoord, EdgeCoord),
}

/// A game action that a player can take on their turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Place a settlement at a vertex.
    BuildSettlement(VertexCoord),
    /// Upgrade a settlement to a city at a vertex.
    BuildCity(VertexCoord),
    /// Place a road along an edge.
    BuildRoad(EdgeCoord),
    /// Buy a development card from the deck.
    BuyDevCard,
    /// Play a development card with its associated action.
    PlayDevCard(DevCard, DevCardAction),
    /// Propose a trade to other players.
    ProposeTrade,
    /// Trade with the bank (4:1 or better with ports).
    BankTrade { give: Resource, get: Resource },
    /// End the current turn.
    EndTurn,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::BuildSettlement(v) => {
                let dir_name = match v.dir {
                    crate::game::board::VertexDirection::North => "N",
                    crate::game::board::VertexDirection::South => "S",
                };
                write!(
                    f,
                    "Build Settlement at ({}, {}, {})",
                    v.hex.q, v.hex.r, dir_name
                )
            }
            Action::BuildCity(v) => {
                let dir_name = match v.dir {
                    crate::game::board::VertexDirection::North => "N",
                    crate::game::board::VertexDirection::South => "S",
                };
                write!(f, "Build City at ({}, {}, {})", v.hex.q, v.hex.r, dir_name)
            }
            Action::BuildRoad(e) => {
                write!(f, "Build Road at {}", e)
            }
            Action::BuyDevCard => write!(f, "Buy Development Card"),
            Action::PlayDevCard(card, _) => write!(f, "Play {}", card),
            Action::ProposeTrade => write!(f, "Propose Trade"),
            Action::BankTrade { give, get } => {
                write!(f, "Bank Trade: {} -> {}", give, get)
            }
            Action::EndTurn => write!(f, "End Turn"),
        }
    }
}

/// A trade offer from one player to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeOffer {
    /// The player making the offer.
    pub from: PlayerId,
    /// Resources the offering player will give.
    pub offering: Vec<(Resource, u32)>,
    /// Resources the offering player wants in return.
    pub requesting: Vec<(Resource, u32)>,
    /// Optional message accompanying the trade offer.
    pub message: String,
}

/// A response to a trade offer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeResponse {
    /// Accept the trade as proposed.
    Accept,
    /// Reject the trade with an optional reason.
    Reject { reason: String },
    /// Propose a counter-offer.
    Counter(TradeOffer),
}
