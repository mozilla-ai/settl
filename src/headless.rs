//! Headless (text-mode) game runner -- preserves the original CLI-driven behavior
//! for scripting, CI, and non-interactive use.

use clap::Parser;

use std::sync::Arc;

use crate::game;
use crate::player;

/// CLI arguments for headless mode.
#[derive(Parser)]
#[command(
    name = "settl",
    about = "Play a hex-based resource trading game in your terminal with AI opponents"
)]
pub struct HeadlessCli {
    /// Number of players (2-4)
    #[arg(short, long, default_value = "4")]
    pub players: usize,

    /// Path to a TOML personality file
    #[arg(long)]
    pub personality: Option<String>,

    /// Run in headless mode (no TUI)
    #[arg(long)]
    pub headless: bool,

    /// Model name from the registry to use (e.g. "Claude Sonnet 4").
    /// If not set, uses a local llamafile.
    #[arg(long)]
    pub model: Option<String>,

    /// Reasoning effort level: low, medium, high, or max.
    #[arg(long, default_value = "low")]
    pub effort: String,
}

pub async fn run(cli: HeadlessCli) {
    assert!(
        (2..=4).contains(&cli.players),
        "Player count must be 2-4, got {}",
        cli.players
    );

    // Use the fixed beginner board layout (randomization deferred to a future design).
    let board = game::board::Board::default_board();

    let state = game::state::GameState::new(board.clone(), cli.players);

    // Resolve the AI client: either from --model (API) or local llamafile.
    let (client, _llamafile_process) = setup_ai_client(&cli).await;

    let custom_personality = cli.personality.as_ref().map(|path| {
        player::personality::Personality::from_toml_file(std::path::Path::new(path)).unwrap_or_else(
            |e| {
                eprintln!("Warning: {}, using default personality", e);
                player::personality::Personality::default()
            },
        )
    });

    let default_personalities = [
        player::personality::Personality::default_personality(),
        player::personality::Personality::aggressive(),
        player::personality::Personality::grudge_holder(),
        player::personality::Personality::builder(),
    ];

    let name_list = ["Alice", "Bob", "Charlie", "Diana"];
    let players: Vec<Box<dyn player::Player>> = (0..cli.players)
        .map(|i| {
            let personality = custom_personality
                .clone()
                .unwrap_or_else(|| default_personalities[i % default_personalities.len()].clone());
            let mut llm = player::llm_player::LlmPlayer::new(
                name_list[i].into(),
                Arc::clone(&client),
                personality,
                Some(i),
            );
            llm.set_effort(cli.effort.clone());
            Box::new(llm) as Box<dyn player::Player>
        })
        .collect();

    // Run game in text mode.
    println!("settl - Terminal Edition with LLM Players");
    println!("==========================================\n");
    println!("{}\n", player::prompt::ascii_board(&board));
    println!("Starting game with {} players...\n", cli.players);

    let config = crate::config::load_config();
    let mut orchestrator = game::orchestrator::GameOrchestrator::new(state, players);
    orchestrator.hooks = config.hooks;

    match orchestrator.run().await {
        Ok(_winner) => {
            println!(
                "\nFinal scores: {}",
                (0..cli.players)
                    .map(|p| format!(
                        "{}: {} VP",
                        orchestrator.player_names[p],
                        orchestrator.state.victory_points(p)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        Err(e) => {
            eprintln!("Game ended: {}", e);
        }
    }
}

/// Set up the AI client based on CLI args.
///
/// If `--model` is given, looks up the model in the config registry (including
/// auto-discovered Anthropic models). Otherwise, falls back to local llamafile.
///
/// Returns the client and an optional llamafile process handle (caller must keep
/// it alive to prevent the process from being killed).
async fn setup_ai_client(
    cli: &HeadlessCli,
) -> (
    Arc<player::anthropic_client::AnthropicClient>,
    Option<crate::llamafile::LlamafileProcess>,
) {
    let mut config = crate::config::load_config();

    // Discover Anthropic models if API key is set.
    if let Some(api_key) = crate::anthropic_api::detect_api_key() {
        match crate::anthropic_api::list_models(&api_key).await {
            Ok(models) => {
                let entries = crate::anthropic_api::to_model_entries(&api_key, &models);
                eprintln!("Discovered {} Anthropic model(s)", entries.len());
                config.merge_anthropic_models(entries);
            }
            Err(e) => {
                eprintln!("Warning: Failed to fetch Anthropic models: {e}");
            }
        }
    }

    if let Some(ref model_name) = cli.model {
        // Find the model in the registry by name (case-insensitive substring match).
        let entry = config
            .models
            .iter()
            .find(|m| m.name.to_lowercase().contains(&model_name.to_lowercase()));

        match entry {
            Some(crate::config::ModelEntry {
                name,
                backend:
                    crate::config::ModelBackend::Api {
                        base_url,
                        api_key,
                        model,
                    },
            }) => {
                eprintln!("Using API model: {name}");
                let client =
                    player::anthropic_client::AnthropicClient::new(base_url, api_key, model);
                return (client, None);
            }
            Some(entry) => {
                eprintln!(
                    "Warning: Model '{}' is a llamafile, not an API model. Falling back to llamafile.",
                    entry.name
                );
            }
            None => {
                let available: Vec<&str> = config.models.iter().map(|m| m.name.as_str()).collect();
                eprintln!("Error: Model '{model_name}' not found in registry.");
                eprintln!("Available models: {}", available.join(", "));
                std::process::exit(1);
            }
        }
    }

    // Fallback: start local llamafile.
    let (port, process) = setup_llamafile_headless().await;
    let client = player::anthropic_client::AnthropicClient::new(
        format!("http://127.0.0.1:{}", port),
        "no-key",
        player::llm_player::LLAMAFILE_MODEL,
    );
    (client, Some(process))
}

/// Download (if needed) and start a local llamafile, printing progress to stderr.
/// Returns the port and the process handle (caller must keep it alive).
async fn setup_llamafile_headless() -> (u16, crate::llamafile::LlamafileProcess) {
    use crate::llamafile::{format_bytes, LlamafileStatus};

    let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        let path = crate::llamafile::ensure_llamafile(
            crate::llamafile::LlamafileModel::Bonsai8B,
            status_tx.clone(),
        )
        .await
        .expect("Failed to download llamafile");
        let _ = status_tx.send(LlamafileStatus::Starting);
        let _ = status_tx.send(LlamafileStatus::WaitingForReady);
        let process = crate::llamafile::LlamafileProcess::start_with_port_scan(&path)
            .await
            .expect("Failed to start llamafile");
        let port = process.port;
        let _ = status_tx.send(LlamafileStatus::Ready(port));
        process
    });

    // Print progress while waiting.
    let mut last_pct = 0u16;
    loop {
        match status_rx.recv().await {
            Some(LlamafileStatus::Checking) => {
                eprint!("Checking for Bonsai-8B...");
            }
            Some(LlamafileStatus::Downloading { bytes, total }) => {
                if total > 0 {
                    let pct = (bytes as f64 / total as f64 * 100.0) as u16;
                    if pct != last_pct {
                        eprint!(
                            "\rDownloading Bonsai-8B... {} / {} ({}%)",
                            format_bytes(bytes),
                            format_bytes(total),
                            pct
                        );
                        last_pct = pct;
                    }
                } else {
                    eprint!("\rDownloading Bonsai-8B... {}", format_bytes(bytes));
                }
            }
            Some(LlamafileStatus::Preparing) => {
                eprintln!("\nPreparing llamafile...");
            }
            Some(LlamafileStatus::Starting) => {
                eprintln!("Starting local AI server...");
            }
            Some(LlamafileStatus::WaitingForReady) => {
                eprint!("Waiting for server...");
            }
            Some(LlamafileStatus::Ready(port)) => {
                eprintln!(" ready on port {}!", port);
                let process = handle.await.expect("llamafile setup task panicked");
                return (port, process);
            }
            Some(LlamafileStatus::Error(e)) => {
                panic!("Llamafile setup failed: {}", e);
            }
            None => {
                panic!("Llamafile setup channel closed unexpectedly");
            }
        }
    }
}
