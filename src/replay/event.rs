use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::game::actions::{DevCard, DevCardAction, PlayerId, TradeOffer};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};

/// Every discrete game event, suitable for event sourcing and replay.
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

/// An append-only log of game events, supporting serialization to JSONL.
///
/// JSONL (JSON Lines) stores one JSON object per line, making it easy to
/// stream, append, and inspect with standard command-line tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameLog {
    events: Vec<GameEvent>,
}

impl GameLog {
    /// Create a new, empty game log.
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Append an event to the log.
    pub fn push(&mut self, event: GameEvent) {
        self.events.push(event);
    }

    /// Return a slice of all recorded events.
    pub fn events(&self) -> &[GameEvent] {
        &self.events
    }

    /// Write the log to a file in JSONL format (one JSON object per line).
    ///
    /// Logs a warning to stderr on failure but does not panic.
    pub fn write_jsonl(&self, path: &Path) -> std::io::Result<()> {
        let file = match File::create(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Warning: failed to create log file {:?}: {}", path, e);
                return Err(e);
            }
        };
        let mut writer = BufWriter::new(file);

        for event in &self.events {
            match serde_json::to_string(event) {
                Ok(json) => {
                    if let Err(e) = writeln!(writer, "{}", json) {
                        eprintln!("Warning: failed to write event to {:?}: {}", path, e);
                        return Err(e);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: failed to serialize event: {}", e);
                    return Err(std::io::Error::other(e));
                }
            }
        }

        if let Err(e) = writer.flush() {
            eprintln!("Warning: failed to flush log file {:?}: {}", path, e);
            return Err(e);
        }

        Ok(())
    }

    /// Read a game log from a JSONL file.
    ///
    /// Each line must be a valid JSON representation of a `GameEvent`.
    pub fn read_jsonl(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event: GameEvent = serde_json::from_str(trimmed).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("line {}: {}", line_num + 1, e),
                )
            })?;
            events.push(event);
        }

        Ok(Self { events })
    }
}

impl Default for GameLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::{
        EdgeCoord, EdgeDirection, HexCoord, Resource, VertexCoord, VertexDirection,
    };
    use std::io::Read as _;
    use tempfile::NamedTempFile;

    /// Helper: create a sample event for testing.
    fn sample_events() -> Vec<GameEvent> {
        vec![
            GameEvent::TurnStarted {
                player: 0,
                turn_number: 1,
            },
            GameEvent::DiceRolled {
                player: 0,
                values: (3, 5),
                total: 8,
            },
            GameEvent::ResourcesDistributed {
                distributions: vec![(0, Resource::Brick, 1), (1, Resource::Wheat, 2)],
            },
            GameEvent::InitialSettlementPlaced {
                player: 0,
                vertex: VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North),
            },
            GameEvent::InitialRoadPlaced {
                player: 0,
                edge: EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::East),
            },
            GameEvent::SettlementBuilt {
                player: 1,
                vertex: VertexCoord::new(HexCoord::new(1, -1), VertexDirection::South),
                reasoning: "good spot for ore".to_string(),
            },
            GameEvent::RobberMoved {
                player: 0,
                to: HexCoord::new(1, 0),
                stole_from: Some(1),
            },
            GameEvent::BankTradeExecuted {
                player: 0,
                gave: (Resource::Sheep, 4),
                got: (Resource::Ore, 1),
            },
            GameEvent::LongestRoadClaimed {
                player: 0,
                length: 5,
            },
            GameEvent::GameWon {
                player: 0,
                final_vp: 10,
            },
        ]
    }

    #[test]
    fn event_serializes_and_deserializes() {
        for event in sample_events() {
            let json = serde_json::to_string(&event).expect("serialize");
            let roundtrip: GameEvent = serde_json::from_str(&json).expect("deserialize");

            // Verify round-trip by re-serializing and comparing JSON strings.
            let json2 = serde_json::to_string(&roundtrip).expect("re-serialize");
            assert_eq!(json, json2, "round-trip should produce identical JSON");
        }
    }

    #[test]
    fn game_log_push_and_events() {
        let mut log = GameLog::new();
        assert!(log.events().is_empty());

        log.push(GameEvent::TurnStarted {
            player: 0,
            turn_number: 1,
        });
        assert_eq!(log.events().len(), 1);

        log.push(GameEvent::DiceRolled {
            player: 0,
            values: (4, 3),
            total: 7,
        });
        assert_eq!(log.events().len(), 2);
    }

    #[test]
    fn game_log_jsonl_round_trip() {
        let mut log = GameLog::new();
        for event in sample_events() {
            log.push(event);
        }

        // Write to a temporary file.
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_path_buf();

        log.write_jsonl(&path).expect("write_jsonl");

        // Verify the file has the right number of lines.
        let contents = {
            let mut s = String::new();
            File::open(&path)
                .expect("open")
                .read_to_string(&mut s)
                .expect("read");
            s
        };
        let line_count = contents.lines().count();
        assert_eq!(
            line_count,
            sample_events().len(),
            "JSONL should have one line per event"
        );

        // Read it back.
        let loaded = GameLog::read_jsonl(&path).expect("read_jsonl");
        assert_eq!(loaded.events().len(), log.events().len());

        // Verify each event round-trips correctly.
        for (original, loaded_event) in log.events().iter().zip(loaded.events().iter()) {
            let json_orig = serde_json::to_string(original).unwrap();
            let json_loaded = serde_json::to_string(loaded_event).unwrap();
            assert_eq!(json_orig, json_loaded);
        }
    }

    #[test]
    fn game_log_read_empty_file() {
        let tmp = NamedTempFile::new().expect("create temp file");
        // The file is empty — should produce an empty log.
        let log = GameLog::read_jsonl(tmp.path()).expect("read_jsonl");
        assert!(log.events().is_empty());
    }

    #[test]
    fn game_log_default() {
        let log = GameLog::default();
        assert!(log.events().is_empty());
    }
}
