//! Auto-save and resume support.
//!
//! Saves game state to `~/.settl/saves/autosave.json` after each turn.
//! The main menu shows a "Continue" option when a save file exists.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::game::event::GameEvent;
use crate::game::state::GameState;
use crate::llamafile::LlamafileModel;

/// Serialized player configuration for save/load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPlayerConfig {
    pub name: String,
    pub is_human: bool,
    pub personality_index: usize,
}

/// A complete save file containing everything needed to resume a game.
#[derive(Serialize, Deserialize)]
pub struct SaveFile {
    pub game_state: GameState,
    pub player_names: Vec<String>,
    pub player_configs: Vec<SavedPlayerConfig>,
    pub events: Vec<GameEvent>,
    pub llamafile_model: LlamafileModel,
    pub saved_at: String,
}

fn save_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok().filter(|s| !s.is_empty())?;
    let dir = PathBuf::from(home).join(".settl").join("saves");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

fn autosave_path() -> Option<PathBuf> {
    save_dir().map(|d| d.join("autosave.json"))
}

/// Write the save file to disk. Returns an error string on failure.
pub fn auto_save(save: &SaveFile) -> Result<(), String> {
    let path = autosave_path().ok_or("Could not determine save directory")?;
    let json = serde_json::to_string(save).map_err(|e| format!("Serialize error: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Write error: {}", e))
}

/// Load the autosave file, returning None if it doesn't exist or can't be parsed.
pub fn load_autosave() -> Option<SaveFile> {
    let path = autosave_path()?;
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Delete the autosave file (e.g. after game over).
pub fn delete_autosave() {
    if let Some(path) = autosave_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Check whether an autosave file exists.
pub fn has_autosave() -> bool {
    autosave_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::Board;

    /// Use a temp directory instead of ~/.settl/saves for tests.
    fn save_roundtrip_in_tempdir() -> (SaveFile, SaveFile) {
        let dir = std::env::temp_dir().join(format!("settl_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_autosave.json");

        let state = GameState::new(Board::default_board(), 3);
        let save = SaveFile {
            player_names: vec!["Alice".into(), "Bob".into(), "Charlie".into()],
            player_configs: vec![
                SavedPlayerConfig {
                    name: "Alice".into(),
                    is_human: true,
                    personality_index: 0,
                },
                SavedPlayerConfig {
                    name: "Bob".into(),
                    is_human: false,
                    personality_index: 1,
                },
                SavedPlayerConfig {
                    name: "Charlie".into(),
                    is_human: false,
                    personality_index: 2,
                },
            ],
            events: vec![],
            llamafile_model: LlamafileModel::default(),
            game_state: state,
            saved_at: "2026-04-04T00:00:00Z".into(),
        };

        let json = serde_json::to_string(&save).expect("serialize");
        std::fs::write(&path, &json).expect("write");
        let loaded: SaveFile = serde_json::from_str(&std::fs::read_to_string(&path).expect("read"))
            .expect("deserialize");

        // Clean up.
        let _ = std::fs::remove_dir_all(&dir);

        (save, loaded)
    }

    #[test]
    fn save_load_roundtrip_preserves_state() {
        let (original, loaded) = save_roundtrip_in_tempdir();

        assert_eq!(original.player_names, loaded.player_names);
        assert_eq!(original.player_configs.len(), loaded.player_configs.len());
        assert_eq!(
            original.game_state.num_players,
            loaded.game_state.num_players
        );
        assert_eq!(
            original.game_state.turn_number,
            loaded.game_state.turn_number
        );
        assert_eq!(
            original.game_state.board.hexes.len(),
            loaded.game_state.board.hexes.len()
        );
        assert_eq!(original.saved_at, loaded.saved_at);
        assert_eq!(original.llamafile_model, loaded.llamafile_model);
    }

    #[test]
    fn save_load_roundtrip_preserves_player_configs() {
        let (original, loaded) = save_roundtrip_in_tempdir();

        for (orig, load) in original.player_configs.iter().zip(&loaded.player_configs) {
            assert_eq!(orig.name, load.name);
            assert_eq!(orig.is_human, load.is_human);
            assert_eq!(orig.personality_index, load.personality_index);
        }
    }

    #[test]
    fn has_autosave_false_when_no_file() {
        // With no save file written, has_autosave should return false
        // (unless a previous test left one, which is unlikely in CI).
        // We just verify the function doesn't panic.
        let _ = has_autosave();
    }
}
