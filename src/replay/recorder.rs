//! Replay recorder — captures game state snapshots alongside events
//! for richer replay playback.

use serde::{Deserialize, Serialize};

use crate::game::actions::PlayerId;
use crate::game::state::GameState;
use crate::replay::event::GameEvent;

/// A single frame in a game replay, combining an event with a state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayFrame {
    /// The event that occurred.
    pub event: GameEvent,
    /// Human-readable description of the event.
    pub description: String,
    /// Turn number when this event occurred.
    pub turn: u32,
    /// Victory points for each player at this moment.
    pub victory_points: Vec<u8>,
    /// Total resource count for each player (resources are hidden).
    pub resource_totals: Vec<u32>,
}

/// A complete game replay with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameReplay {
    /// Player names.
    pub player_names: Vec<String>,
    /// Number of players.
    pub num_players: usize,
    /// The replay frames in chronological order.
    pub frames: Vec<ReplayFrame>,
    /// The winner (if any).
    pub winner: Option<PlayerId>,
}

impl GameReplay {
    /// Create a new empty replay.
    pub fn new(player_names: Vec<String>) -> Self {
        let num_players = player_names.len();
        Self {
            player_names,
            num_players,
            frames: Vec::new(),
            winner: None,
        }
    }

    /// Record an event with the current game state.
    pub fn record(&mut self, event: GameEvent, state: &GameState, description: String) {
        let victory_points: Vec<u8> = (0..self.num_players)
            .map(|p| state.victory_points(p))
            .collect();
        let resource_totals: Vec<u32> = state.players.iter().map(|p| p.total_resources()).collect();

        self.frames.push(ReplayFrame {
            event,
            description,
            turn: state.turn_number,
            victory_points,
            resource_totals,
        });
    }

    /// Format an event for human-readable display.
    pub fn format_event(event: &GameEvent, player_names: &[String]) -> String {
        let name =
            |p: PlayerId| -> &str { player_names.get(p).map(|s| s.as_str()).unwrap_or("???") };

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
                    "{} built settlement at ({},{},{:?}) — {}",
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
                    "{} upgraded to city at ({},{},{:?}) — {}",
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
                format!("{} built road at {} — {}", name(*player), edge, reasoning)
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
                    "{} proposed trade: [{}] for [{}] — {}",
                    name(*from),
                    offering,
                    requesting,
                    reasoning
                )
            }
            GameEvent::TradeAccepted { by, reasoning } => {
                format!("{} accepted trade — {}", name(*by), reasoning)
            }
            GameEvent::TradeRejected { by, reasoning } => {
                format!("{} rejected trade — {}", name(*by), reasoning)
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
                    "{} counter-offered: [{}] for [{}] — {}",
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
                format!("{} played {} — {}", name(*player), card, reasoning)
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

    /// Generate game statistics.
    pub fn stats(&self) -> GameStats {
        let mut stats = GameStats {
            total_turns: self.frames.last().map(|f| f.turn).unwrap_or(0),
            total_events: self.frames.len(),
            trades_proposed: 0,
            trades_accepted: 0,
            robber_moves: 0,
            dev_cards_bought: 0,
            settlements_built: 0,
            cities_built: 0,
            roads_built: 0,
            winner: self.winner,
        };

        for frame in &self.frames {
            match &frame.event {
                GameEvent::TradeProposed { .. } => stats.trades_proposed += 1,
                GameEvent::TradeAccepted { .. } => stats.trades_accepted += 1,
                GameEvent::RobberMoved { .. } => stats.robber_moves += 1,
                GameEvent::DevCardBought { .. } => stats.dev_cards_bought += 1,
                GameEvent::SettlementBuilt { .. } | GameEvent::InitialSettlementPlaced { .. } => {
                    stats.settlements_built += 1;
                }
                GameEvent::CityUpgraded { .. } => stats.cities_built += 1,
                GameEvent::RoadBuilt { .. } | GameEvent::InitialRoadPlaced { .. } => {
                    stats.roads_built += 1;
                }
                _ => {}
            }
        }

        stats
    }
}

/// Summary statistics for a completed game.
#[derive(Debug, Clone)]
pub struct GameStats {
    pub total_turns: u32,
    pub total_events: usize,
    pub trades_proposed: usize,
    pub trades_accepted: usize,
    pub robber_moves: usize,
    pub dev_cards_bought: usize,
    pub settlements_built: usize,
    pub cities_built: usize,
    pub roads_built: usize,
    pub winner: Option<PlayerId>,
}

impl std::fmt::Display for GameStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Game Statistics:")?;
        writeln!(f, "  Turns:        {}", self.total_turns)?;
        writeln!(f, "  Events:       {}", self.total_events)?;
        writeln!(f, "  Settlements:  {}", self.settlements_built)?;
        writeln!(f, "  Cities:       {}", self.cities_built)?;
        writeln!(f, "  Roads:        {}", self.roads_built)?;
        writeln!(
            f,
            "  Trades:       {} proposed, {} accepted",
            self.trades_proposed, self.trades_accepted
        )?;
        writeln!(f, "  Robber moves: {}", self.robber_moves)?;
        writeln!(f, "  Dev cards:    {}", self.dev_cards_bought)?;
        if let Some(w) = self.winner {
            writeln!(f, "  Winner:       Player {}", w)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::{Board, HexCoord, VertexCoord, VertexDirection};

    #[test]
    fn replay_records_and_counts_frames() {
        let board = Board::default_board();
        let state = GameState::new(board, 3);
        let mut replay = GameReplay::new(vec!["A".into(), "B".into(), "C".into()]);

        replay.record(
            GameEvent::DiceRolled {
                player: 0,
                values: (3, 4),
                total: 7,
            },
            &state,
            "Roll".into(),
        );
        replay.record(
            GameEvent::RobberMoved {
                player: 0,
                to: HexCoord::new(1, 0),
                stole_from: None,
            },
            &state,
            "Robber".into(),
        );

        assert_eq!(replay.frames.len(), 2);
        assert_eq!(replay.frames[0].description, "Roll");
    }

    #[test]
    fn format_event_dice_roll() {
        let names = vec!["Alice".into(), "Bob".into()];
        let event = GameEvent::DiceRolled {
            player: 0,
            values: (3, 5),
            total: 8,
        };
        let formatted = GameReplay::format_event(&event, &names);
        assert!(formatted.contains("Alice"));
        assert!(formatted.contains("8"));
    }

    #[test]
    fn format_event_settlement() {
        let names = vec!["Alice".into(), "Bob".into()];
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        let event = GameEvent::SettlementBuilt {
            player: 1,
            vertex: v,
            reasoning: "good spot".into(),
        };
        let formatted = GameReplay::format_event(&event, &names);
        assert!(formatted.contains("Bob"));
        assert!(formatted.contains("good spot"));
    }

    #[test]
    fn stats_counts_events() {
        let board = Board::default_board();
        let state = GameState::new(board, 2);
        let mut replay = GameReplay::new(vec!["A".into(), "B".into()]);

        replay.record(
            GameEvent::DiceRolled {
                player: 0,
                values: (1, 2),
                total: 3,
            },
            &state,
            "".into(),
        );
        replay.record(
            GameEvent::InitialSettlementPlaced {
                player: 0,
                vertex: VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North),
            },
            &state,
            "".into(),
        );
        replay.record(GameEvent::DevCardBought { player: 1 }, &state, "".into());
        replay.record(
            GameEvent::RobberMoved {
                player: 0,
                to: HexCoord::new(1, 0),
                stole_from: Some(1),
            },
            &state,
            "".into(),
        );

        let stats = replay.stats();
        assert_eq!(stats.total_events, 4);
        assert_eq!(stats.settlements_built, 1);
        assert_eq!(stats.dev_cards_bought, 1);
        assert_eq!(stats.robber_moves, 1);
    }

    #[test]
    fn stats_display() {
        let stats = GameStats {
            total_turns: 100,
            total_events: 500,
            trades_proposed: 20,
            trades_accepted: 5,
            robber_moves: 30,
            dev_cards_bought: 15,
            settlements_built: 12,
            cities_built: 4,
            roads_built: 30,
            winner: Some(1),
        };
        let display = format!("{}", stats);
        assert!(display.contains("Turns:        100"));
        assert!(display.contains("Winner:       Player 1"));
    }
}
