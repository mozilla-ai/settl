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

/// Format a game event for the game log (strips AI reasoning to avoid leaking
/// strategic information to the human player).
pub fn format_event_for_log(event: &GameEvent, player_names: &[String]) -> String {
    format_event_inner(event, player_names, false)
}

/// Format a game event for human-readable display (used for LLM context).
pub fn format_event(event: &GameEvent, player_names: &[String]) -> String {
    format_event_inner(event, player_names, true)
}

fn format_event_inner(
    event: &GameEvent,
    player_names: &[String],
    include_reasoning: bool,
) -> String {
    let name = |p: PlayerId| -> &str { player_names.get(p).map(|s| s.as_str()).unwrap_or("???") };

    /// Append ` -- {reasoning}` only when reasoning should be shown.
    fn maybe_reasoning(base: String, reasoning: &str, include: bool) -> String {
        if include && !reasoning.is_empty() {
            format!("{} -- {}", base, reasoning)
        } else {
            base
        }
    }

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
        } => maybe_reasoning(
            format!(
                "{} built settlement at ({},{},{:?})",
                name(*player),
                vertex.hex.q,
                vertex.hex.r,
                vertex.dir,
            ),
            reasoning,
            include_reasoning,
        ),
        GameEvent::CityUpgraded {
            player,
            vertex,
            reasoning,
        } => maybe_reasoning(
            format!(
                "{} upgraded to city at ({},{},{:?})",
                name(*player),
                vertex.hex.q,
                vertex.hex.r,
                vertex.dir,
            ),
            reasoning,
            include_reasoning,
        ),
        GameEvent::RoadBuilt {
            player,
            edge,
            reasoning,
        } => maybe_reasoning(
            format!("{} built road at {}", name(*player), edge),
            reasoning,
            include_reasoning,
        ),
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
            maybe_reasoning(
                format!(
                    "{} proposed trade: [{}] for [{}]",
                    name(*from),
                    offering,
                    requesting,
                ),
                reasoning,
                include_reasoning,
            )
        }
        GameEvent::TradeAccepted { by, reasoning } => maybe_reasoning(
            format!("{} accepted trade", name(*by)),
            reasoning,
            include_reasoning,
        ),
        GameEvent::TradeRejected { by, reasoning } => maybe_reasoning(
            format!("{} rejected trade", name(*by)),
            reasoning,
            include_reasoning,
        ),
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
        } => maybe_reasoning(
            format!("{} played {}", name(*player), card),
            reasoning,
            include_reasoning,
        ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::{HexCoord, Resource, VertexCoord, VertexDirection};

    fn names() -> Vec<String> {
        vec!["Alice".into(), "Bob".into()]
    }

    #[test]
    fn format_event_for_log_strips_trade_reasoning() {
        let event = GameEvent::TradeProposed {
            from: 0,
            offer: TradeOffer {
                from: 0,
                offering: vec![(Resource::Wood, 1)],
                requesting: vec![(Resource::Ore, 1)],
                message: String::new(),
            },
            reasoning: "I need ore for a city and have spare wood".into(),
        };
        let log_text = format_event_for_log(&event, &names());
        assert!(!log_text.contains("I need ore"));
        assert!(!log_text.contains("--"));
        assert!(log_text.contains("Alice proposed trade"));

        let llm_text = format_event(&event, &names());
        assert!(llm_text.contains("I need ore"));
    }

    #[test]
    fn format_event_for_log_strips_accept_reject_reasoning() {
        let accept = GameEvent::TradeAccepted {
            by: 1,
            reasoning: "This gives me what I need for a settlement".into(),
        };
        let reject = GameEvent::TradeRejected {
            by: 1,
            reasoning: "Bad deal, I'm saving ore".into(),
        };

        let accept_log = format_event_for_log(&accept, &names());
        assert!(!accept_log.contains("settlement"));
        assert!(accept_log.contains("Bob accepted trade"));

        let reject_log = format_event_for_log(&reject, &names());
        assert!(!reject_log.contains("saving ore"));
        assert!(reject_log.contains("Bob rejected trade"));
    }

    #[test]
    fn format_event_for_log_strips_build_reasoning() {
        let event = GameEvent::SettlementBuilt {
            player: 0,
            vertex: VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North),
            reasoning: "Best spot for wheat access".into(),
        };
        let log_text = format_event_for_log(&event, &names());
        assert!(!log_text.contains("wheat access"));
        assert!(log_text.contains("Alice built settlement"));
    }

    #[test]
    fn format_event_keeps_reasoning_for_llm() {
        let event = GameEvent::RoadBuilt {
            player: 0,
            edge: crate::game::board::EdgeCoord::new(
                HexCoord::new(0, 0),
                crate::game::board::EdgeDirection::NorthEast,
            ),
            reasoning: "Extending toward the port".into(),
        };
        let llm_text = format_event(&event, &names());
        assert!(llm_text.contains("Extending toward the port"));
    }
}
