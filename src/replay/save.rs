//! Save/resume game state — serialize a running game to disk and resume later.
//!
//! The save file bundles the full `GameState` with the event log so that
//! LLM players can be given recent history for context when resuming.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::game::state::GameState;
use crate::replay::event::{GameEvent, GameLog};

/// Everything needed to resume a game in progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveGame {
    /// The full game state at the point of saving.
    pub state: GameState,
    /// Event history up to the save point (used for LLM context on resume).
    pub events: Vec<GameEvent>,
    /// Player names.
    pub player_names: Vec<String>,
    /// Per-player model IDs (empty string for non-LLM players).
    pub player_models: Vec<String>,
    /// Format version for forward compatibility.
    pub version: u32,
}

impl SaveGame {
    /// Create a save game from the current orchestrator state.
    pub fn new(
        state: GameState,
        log: &GameLog,
        player_names: Vec<String>,
        player_models: Vec<String>,
    ) -> Self {
        Self {
            state,
            events: log.events().to_vec(),
            player_names,
            player_models,
            version: 1,
        }
    }

    /// Write the save to a JSON file.
    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Load a save game from a JSON file.
    pub fn load_from_file(path: &Path) -> std::io::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        serde_json::from_str(&contents)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Get the last N events as a GameLog for injecting into a resumed orchestrator.
    pub fn recent_log(&self) -> GameLog {
        let mut log = GameLog::new();
        for event in &self.events {
            log.push(event.clone());
        }
        log
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::Board;
    use tempfile::NamedTempFile;

    #[test]
    fn save_and_load_round_trip() {
        let board = Board::default_board();
        let state = GameState::new(board, 3);
        let mut log = GameLog::new();
        log.push(GameEvent::DiceRolled {
            player: 0,
            values: (3, 4),
            total: 7,
        });
        log.push(GameEvent::RobberMoved {
            player: 0,
            to: crate::game::board::HexCoord::new(1, 0),
            stole_from: Some(1),
        });

        let save = SaveGame::new(
            state.clone(),
            &log,
            vec!["A".into(), "B".into(), "C".into()],
            vec!["model-a".into(), "model-b".into(), "model-c".into()],
        );

        let tmp = NamedTempFile::new().unwrap();
        save.save_to_file(tmp.path()).unwrap();

        let loaded = SaveGame::load_from_file(tmp.path()).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.player_names, vec!["A", "B", "C"]);
        assert_eq!(loaded.player_models, vec!["model-a", "model-b", "model-c"]);
        assert_eq!(loaded.events.len(), 2);
        assert_eq!(loaded.state.num_players, 3);
    }

    #[test]
    fn recent_log_preserves_events() {
        let board = Board::default_board();
        let state = GameState::new(board, 2);
        let mut log = GameLog::new();
        log.push(GameEvent::DiceRolled {
            player: 0,
            values: (1, 2),
            total: 3,
        });

        let save = SaveGame::new(
            state,
            &log,
            vec!["A".into(), "B".into()],
            vec!["".into(), "".into()],
        );

        let recent = save.recent_log();
        assert_eq!(recent.events().len(), 1);
    }

    #[test]
    fn save_game_serializes_cleanly() {
        let board = Board::default_board();
        let state = GameState::new(board, 2);
        let log = GameLog::new();

        let save = SaveGame::new(
            state,
            &log,
            vec!["X".into(), "Y".into()],
            vec!["".into(), "".into()],
        );

        let json = serde_json::to_string(&save).unwrap();
        assert!(json.contains("\"version\":1"));
        assert!(json.contains("\"player_names\""));
    }
}
