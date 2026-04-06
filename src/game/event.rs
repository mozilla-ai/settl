//! Game events -- every discrete action that occurs during a game.
//!
//! Used for LLM context (recent history) and UI event streaming.

use serde::{Deserialize, Serialize};

use crate::game::actions::{DevCard, PlayerId, TradeOffer};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};

/// Reason the game is waiting for a human player.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaitingReason {
    YourTurn,
    TradeResponse,
    DiscardCards,
    PlaceRobber,
}

/// Every discrete game event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    // -- Lifecycle --
    TurnStarted {
        player: PlayerId,
        is_human: bool,
    },
    WaitingForHuman {
        player: PlayerId,
        reason: WaitingReason,
    },

    // -- Setup --
    InitialSettlementPlaced {
        player: PlayerId,
        vertex: VertexCoord,
    },
    InitialRoadPlaced {
        player: PlayerId,
        edge: EdgeCoord,
    },

    // -- Turn flow --
    DiceRolled {
        player: PlayerId,
        values: (u8, u8),
        total: u8,
    },
    ResourcesDistributed {
        distributions: Vec<(PlayerId, Resource, u32)>,
    },

    // -- Building --
    SettlementBuilt {
        player: PlayerId,
        vertex: VertexCoord,
        reasoning: String,
    },
    CityUpgraded {
        player: PlayerId,
        vertex: VertexCoord,
        reasoning: String,
    },
    RoadBuilt {
        player: PlayerId,
        edge: EdgeCoord,
        reasoning: String,
    },

    // -- Trading --
    TradeProposed {
        from: PlayerId,
        offer: TradeOffer,
        reasoning: String,
    },
    TradeAccepted {
        by: PlayerId,
        reasoning: String,
    },
    TradeRejected {
        by: PlayerId,
        reasoning: String,
    },
    TradeWithdrawn {
        by: PlayerId,
    },
    PlayerTradeExecuted {
        proposer: PlayerId,
        acceptor: PlayerId,
        gave: Vec<(Resource, u32)>,
        got: Vec<(Resource, u32)>,
    },
    BankTradeExecuted {
        player: PlayerId,
        gave: (Resource, u32),
        got: (Resource, u32),
    },

    // -- Development cards --
    DevCardBought {
        player: PlayerId,
    },
    DevCardPlayed {
        player: PlayerId,
        card: DevCard,
        reasoning: String,
    },

    // -- Robber --
    RobberMoved {
        player: PlayerId,
        to: HexCoord,
        stole_from: Option<PlayerId>,
    },
    CardsDiscarded {
        player: PlayerId,
        cards: Vec<Resource>,
    },

    // -- Game end --
    GameWon {
        player: PlayerId,
        final_vp: u8,
    },
}

/// Format a game event for human-readable display (used for LLM context).
pub fn format_event(event: &GameEvent, player_names: &[String]) -> String {
    let name = |p: PlayerId| -> &str { player_names.get(p).map(|s| s.as_str()).unwrap_or("???") };

    match event {
        GameEvent::TurnStarted { player, is_human } => {
            let kind = if *is_human { "human" } else { "AI" };
            format!("{}'s turn ({kind})", name(*player))
        }
        GameEvent::WaitingForHuman { player, reason } => {
            format!("Waiting for {} ({:?})", name(*player), reason)
        }
        GameEvent::InitialSettlementPlaced { player, vertex } => {
            format!(
                "{} placed settlement at ({},{},{:?})",
                name(*player),
                vertex.hex.q,
                vertex.hex.r,
                vertex.dir
            )
        }
        GameEvent::InitialRoadPlaced { player, edge } => {
            format!("{} placed road at {}", name(*player), edge)
        }
        GameEvent::DiceRolled { player, total, .. } => {
            format!("{} rolled {}", name(*player), total)
        }
        GameEvent::ResourcesDistributed { distributions } => {
            if distributions.is_empty() {
                return "No resources produced".into();
            }
            let parts: Vec<String> = distributions
                .iter()
                .map(|(p, r, c)| format!("{}: {} {}", name(*p), c, r))
                .collect();
            format!("Resources: {}", parts.join(", "))
        }
        GameEvent::SettlementBuilt {
            player,
            vertex,
            reasoning,
        } => {
            format!(
                "{} built settlement at ({},{},{:?}) -- {}",
                name(*player),
                vertex.hex.q,
                vertex.hex.r,
                vertex.dir,
                reasoning
            )
        }
        GameEvent::CityUpgraded {
            player,
            vertex,
            reasoning,
        } => {
            format!(
                "{} upgraded to city at ({},{},{:?}) -- {}",
                name(*player),
                vertex.hex.q,
                vertex.hex.r,
                vertex.dir,
                reasoning
            )
        }
        GameEvent::RoadBuilt {
            player,
            edge,
            reasoning,
        } => {
            format!("{} built road at {} -- {}", name(*player), edge, reasoning)
        }
        GameEvent::TradeProposed {
            from,
            offer,
            reasoning,
        } => {
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
            format!(
                "{} proposed trade: [{}] for [{}] -- {}",
                name(*from),
                offering,
                requesting,
                reasoning
            )
        }
        GameEvent::TradeAccepted { by, reasoning } => {
            format!("{} accepted trade -- {}", name(*by), reasoning)
        }
        GameEvent::TradeRejected { by, reasoning } => {
            format!("{} rejected trade -- {}", name(*by), reasoning)
        }
        GameEvent::TradeWithdrawn { by } => {
            format!("{} withdrew trade", name(*by))
        }
        GameEvent::PlayerTradeExecuted {
            proposer,
            acceptor,
            gave,
            got,
        } => {
            let gave_str: String = gave
                .iter()
                .map(|(r, n)| format!("{} {}", n, r))
                .collect::<Vec<_>>()
                .join(", ");
            let got_str: String = got
                .iter()
                .map(|(r, n)| format!("{} {}", n, r))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Trade complete: {} gave [{}] to {} for [{}]",
                name(*proposer),
                gave_str,
                name(*acceptor),
                got_str
            )
        }
        GameEvent::BankTradeExecuted { player, gave, got } => {
            format!(
                "{} bank traded {} {} for {} {}",
                name(*player),
                gave.1,
                gave.0,
                got.1,
                got.0
            )
        }
        GameEvent::DevCardBought { player } => {
            format!("{} bought a dev card", name(*player))
        }
        GameEvent::DevCardPlayed {
            player,
            card,
            reasoning,
        } => {
            format!("{} played {} -- {}", name(*player), card, reasoning)
        }
        GameEvent::RobberMoved {
            player,
            to,
            stole_from,
        } => {
            let steal = stole_from
                .map(|t| format!(", stole from {}", name(t)))
                .unwrap_or_default();
            format!(
                "{} moved robber to ({},{}){}",
                name(*player),
                to.q,
                to.r,
                steal
            )
        }
        GameEvent::CardsDiscarded { player, cards } => {
            let card_str: Vec<String> = cards.iter().map(|r| format!("{}", r)).collect();
            format!("{} discarded {}", name(*player), card_str.join(", "))
        }
        GameEvent::GameWon { player, final_vp } => {
            format!("{} wins with {} VP!", name(*player), final_vp)
        }
    }
}
