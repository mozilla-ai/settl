//! Game events -- every discrete action that occurs during a game.
//!
//! Used for LLM context (recent history) and UI event streaming.

use serde::{Deserialize, Serialize};

use crate::game::actions::{DevCard, DevCardAction, PlayerId, TradeOffer};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};

/// Every discrete game event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    // -- Setup --
    InitialSettlementPlaced {
        player: PlayerId,
        vertex: VertexCoord,
    },
    InitialRoadPlaced {
        player: PlayerId,
        edge: EdgeCoord,
    },
    InitialResourcesGranted {
        player: PlayerId,
        resources: Vec<(Resource, u32)>,
    },

    // -- Turn flow --
    TurnStarted {
        player: PlayerId,
        turn_number: u32,
    },
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
    TradeCountered {
        by: PlayerId,
        counter_offer: TradeOffer,
        reasoning: String,
    },
    TradeWithdrawn {
        by: PlayerId,
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
        action: DevCardAction,
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

    // -- Special awards --
    LongestRoadClaimed {
        player: PlayerId,
        length: u8,
    },
    LargestArmyClaimed {
        player: PlayerId,
        knights: u32,
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
        GameEvent::DiceRolled {
            player,
            values,
            total,
        } => {
            format!(
                "{} rolled {} ({} + {})",
                name(*player),
                total,
                values.0,
                values.1
            )
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
        GameEvent::TradeCountered {
            by,
            counter_offer,
            reasoning,
        } => {
            let offering: String = counter_offer
                .offering
                .iter()
                .map(|(r, n)| format!("{} {}", n, r))
                .collect::<Vec<_>>()
                .join(", ");
            let requesting: String = counter_offer
                .requesting
                .iter()
                .map(|(r, n)| format!("{} {}", n, r))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} counter-offered: [{}] for [{}] -- {}",
                name(*by),
                offering,
                requesting,
                reasoning
            )
        }
        GameEvent::TradeWithdrawn { by } => {
            format!("{} withdrew trade", name(*by))
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
            action: _,
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
        GameEvent::LongestRoadClaimed { player, length } => {
            format!("{} claimed Longest Road ({})", name(*player), length)
        }
        GameEvent::LargestArmyClaimed { player, knights } => {
            format!(
                "{} claimed Largest Army ({} knights)",
                name(*player),
                knights
            )
        }
        GameEvent::GameWon { player, final_vp } => {
            format!("{} wins with {} VP!", name(*player), final_vp)
        }
        _ => format!("{:?}", event),
    }
}
