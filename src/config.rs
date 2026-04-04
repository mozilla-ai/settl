//! Application configuration: model registry and persistence.
//!
//! The config file lives at `~/.settl/config.toml` and stores a list of
//! AI model entries (llamafiles and API endpoints) that the user can manage
//! from the Settings screen.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A single model configuration in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// User-visible name, e.g. "Bonsai 1.7B" or "Claude Sonnet".
    pub name: String,
    /// What kind of backend this model uses.
    pub backend: ModelBackend,
}

/// The backend type for a model entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ModelBackend {
    /// A llamafile to download and run locally.
    Llamafile {
        /// Download URL for the .llamafile binary.
        url: String,
        /// Filename on disk (inside `~/.settl/llamafiles/`).
        filename: String,
    },
    /// A remote API endpoint (Anthropic Messages API compatible).
    Api {
        /// Base URL, e.g. "https://api.anthropic.com".
        base_url: String,
        /// API key (empty string means no auth / local server).
        api_key: String,
        /// Model identifier sent in the request, e.g. "claude-sonnet-4-20250514".
        model: String,
    },
}

impl ModelEntry {
    /// Estimate minimum RAM in GB needed for a llamafile model.
    /// Returns `None` for API models (no local RAM needed).
    pub fn min_ram_gb(&self) -> Option<u32> {
        match &self.backend {
            ModelBackend::Llamafile { filename, .. } => {
                // Check known built-in models first.
                if filename.contains("1.7B") {
                    Some(crate::llamafile::LlamafileModel::Bonsai1B.min_ram_gb())
                } else if filename.contains("8B") {
                    Some(crate::llamafile::LlamafileModel::Bonsai8B.min_ram_gb())
                } else {
                    // Custom llamafile: estimate from file size if it exists on disk.
                    let path = crate::llamafile::download::llamafile_dir().join(filename);
                    if let Ok(meta) = std::fs::metadata(&path) {
                        Some(crate::llamafile::download::estimate_ram_gb_from_file_size(
                            meta.len(),
                        ))
                    } else {
                        None
                    }
                }
            }
            ModelBackend::Api { .. } => None,
        }
    }
}

/// Valid effort levels for the Anthropic Messages API.
pub const EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "max"];

/// Default effort level index (points to "low" in EFFORT_LEVELS).
pub const DEFAULT_EFFORT_INDEX: usize = 0;

/// A hook that runs a shell command when a game event fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    /// Event name to match (e.g. "DiceRolled", "GameWon") or "*" for all events.
    pub event: String,
    /// Shell command to execute. Event data is piped as JSON to stdin.
    pub command: String,
}

/// Top-level application config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Registered AI models.
    pub models: Vec<ModelEntry>,
    /// Event hooks -- shell commands triggered by game events.
    #[serde(default)]
    pub hooks: Vec<HookConfig>,
    /// Default reasoning effort level for AI players.
    #[serde(default = "default_effort")]
    pub default_effort: String,
}

pub fn default_effort() -> String {
    EFFORT_LEVELS[DEFAULT_EFFORT_INDEX].to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hooks: Vec::new(),
            default_effort: default_effort(),
            models: vec![
                ModelEntry {
                    name: "Bonsai 1.7B (fast)".into(),
                    backend: ModelBackend::Llamafile {
                        url: "https://huggingface.co/mozilla-ai/llamafile_0.10.0/resolve/main/Bonsai-1.7B.llamafile?download=true".into(),
                        filename: "Bonsai-1.7B.llamafile".into(),
                    },
                },
                ModelEntry {
                    name: "Bonsai 8B (smart)".into(),
                    backend: ModelBackend::Llamafile {
                        url: "https://huggingface.co/mozilla-ai/llamafile_0.10.0/resolve/main/Bonsai-8B.llamafile?download=true".into(),
                        filename: "Bonsai-8B.llamafile".into(),
                    },
                },
            ],
        }
    }
}

impl Config {
    /// Merge auto-discovered Anthropic API models into the registry.
    ///
    /// Adds new models and updates existing ones (matched by model ID).
    /// Discovered models are ephemeral -- they are not persisted to disk.
    pub fn merge_anthropic_models(&mut self, entries: Vec<ModelEntry>) {
        for entry in entries {
            let model_id = match &entry.backend {
                ModelBackend::Api { model, .. } => model.clone(),
                _ => continue,
            };
            // Update if a model with the same API model ID already exists.
            if let Some(existing) = self.models.iter_mut().find(
                |m| matches!(&m.backend, ModelBackend::Api { model, .. } if *model == model_id),
            ) {
                existing.name = entry.name;
                existing.backend = entry.backend;
            } else {
                self.models.push(entry);
            }
        }
    }
}

/// Return the path to `~/.settl/config.toml`.
pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".settl").join("config.toml")
}

/// Load config from disk, falling back to defaults if missing or malformed.
pub fn load_config() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(data) => toml::from_str(&data).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Save config to disk.
pub fn save_config(config: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let toml = toml::to_string_pretty(config).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, toml).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_two_models() {
        let config = Config::default();
        assert_eq!(config.models.len(), 2);
        assert!(config.models[0].name.contains("1.7B"));
        assert!(config.models[1].name.contains("8B"));
    }

    #[test]
    fn roundtrip_toml() {
        let config = Config {
            hooks: vec![HookConfig {
                event: "GameWon".into(),
                command: "echo win".into(),
            }],
            default_effort: default_effort(),
            models: vec![
                ModelEntry {
                    name: "Test Llamafile".into(),
                    backend: ModelBackend::Llamafile {
                        url: "https://example.com/model.llamafile".into(),
                        filename: "model.llamafile".into(),
                    },
                },
                ModelEntry {
                    name: "Test API".into(),
                    backend: ModelBackend::Api {
                        base_url: "https://api.example.com".into(),
                        api_key: "sk-test".into(),
                        model: "test-model".into(),
                    },
                },
            ],
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.models.len(), 2);
        assert_eq!(parsed.models[0].name, "Test Llamafile");
        assert_eq!(parsed.models[1].name, "Test API");

        match &parsed.models[0].backend {
            ModelBackend::Llamafile { url, filename } => {
                assert_eq!(url, "https://example.com/model.llamafile");
                assert_eq!(filename, "model.llamafile");
            }
            _ => panic!("Expected Llamafile backend"),
        }

        match &parsed.models[1].backend {
            ModelBackend::Api {
                base_url,
                api_key,
                model,
            } => {
                assert_eq!(base_url, "https://api.example.com");
                assert_eq!(api_key, "sk-test");
                assert_eq!(model, "test-model");
            }
            _ => panic!("Expected Api backend"),
        }

        assert_eq!(parsed.hooks.len(), 1);
        assert_eq!(parsed.hooks[0].event, "GameWon");
        assert_eq!(parsed.hooks[0].command, "echo win");
    }

    #[test]
    fn merge_anthropic_models_adds_new() {
        let mut config = Config::default();
        assert_eq!(config.models.len(), 2);

        let entries = vec![ModelEntry {
            name: "Claude Sonnet 4".into(),
            backend: ModelBackend::Api {
                base_url: "https://api.anthropic.com".into(),
                api_key: "sk-test".into(),
                model: "claude-sonnet-4-20250514".into(),
            },
        }];
        config.merge_anthropic_models(entries);
        assert_eq!(config.models.len(), 3);
        assert_eq!(config.models[2].name, "Claude Sonnet 4");
    }

    #[test]
    fn merge_anthropic_models_updates_existing() {
        let mut config = Config {
            hooks: Vec::new(),
            default_effort: default_effort(),
            models: vec![ModelEntry {
                name: "Old Name".into(),
                backend: ModelBackend::Api {
                    base_url: "https://api.anthropic.com".into(),
                    api_key: "sk-old".into(),
                    model: "claude-sonnet-4-20250514".into(),
                },
            }],
        };

        let entries = vec![ModelEntry {
            name: "Claude Sonnet 4".into(),
            backend: ModelBackend::Api {
                base_url: "https://api.anthropic.com".into(),
                api_key: "sk-new".into(),
                model: "claude-sonnet-4-20250514".into(),
            },
        }];
        config.merge_anthropic_models(entries);

        // Should update, not duplicate.
        assert_eq!(config.models.len(), 1);
        assert_eq!(config.models[0].name, "Claude Sonnet 4");
        match &config.models[0].backend {
            ModelBackend::Api { api_key, .. } => assert_eq!(api_key, "sk-new"),
            _ => panic!("Expected Api backend"),
        }
    }

    #[test]
    fn merge_anthropic_models_ignores_llamafile_entries() {
        let mut config = Config::default();
        let initial_count = config.models.len();

        // Passing a llamafile entry should be ignored.
        let entries = vec![ModelEntry {
            name: "Not an API model".into(),
            backend: ModelBackend::Llamafile {
                url: "https://example.com".into(),
                filename: "test.llamafile".into(),
            },
        }];
        config.merge_anthropic_models(entries);
        assert_eq!(config.models.len(), initial_count);
    }
}
