//! Headless (text-mode) game runner — preserves the original CLI-driven behavior
//! for scripting, CI, and non-interactive use.

use clap::Parser;

use crate::game;
use crate::player;
use crate::replay;

/// CLI arguments for headless mode.
#[derive(Parser)]
#[command(
    name = "settl",
    about = "Play Settlers of Catan in your terminal with AI opponents"
)]
pub struct HeadlessCli {
    /// Number of players (2-4)
    #[arg(short, long, default_value = "4")]
    pub players: usize,

    /// Run a demo game with random AI players (no API keys needed)
    #[arg(long)]
    pub demo: bool,

    /// Model for LLM players (e.g. "claude-sonnet-4-6", "gpt-4o-mini")
    #[arg(short, long, default_value = "claude-sonnet-4-6")]
    pub model: String,

    /// Per-player models, comma-separated
    #[arg(long)]
    pub models: Option<String>,

    /// Path to a TOML personality file
    #[arg(long)]
    pub personality: Option<String>,

    /// Replay a saved game (JSON replay or JSONL event log)
    #[arg(long)]
    pub replay: Option<String>,

    /// Random seed for reproducible games
    #[arg(long)]
    pub seed: Option<u64>,

    /// Resume a saved game from a JSON save file
    #[arg(long)]
    pub resume: Option<String>,

    /// Use local llamafile AI instead of cloud LLM providers
    #[arg(long)]
    pub llamafile: bool,

    /// Run in headless mode (no TUI)
    #[arg(long)]
    pub headless: bool,
}

pub async fn run(cli: HeadlessCli) {
    // Handle replay mode.
    if let Some(ref replay_path) = cli.replay {
        run_replay(replay_path);
        return;
    }

    // Handle resume mode.
    if let Some(ref save_path) = cli.resume {
        run_resume(save_path).await;
        return;
    }

    assert!(
        (2..=4).contains(&cli.players),
        "Player count must be 2-4, got {}",
        cli.players
    );

    // Use the fixed beginner board layout (randomization deferred to a future design).
    let board = game::board::Board::default_board();

    let state = game::state::GameState::new(board.clone(), cli.players);

    // Create players.
    let players: Vec<Box<dyn player::Player>> = if cli.demo {
        let name_list = ["Alice", "Bob", "Charlie", "Diana"];
        (0..cli.players)
            .map(|i| {
                Box::new(player::random::RandomPlayer::new(name_list[i].into()))
                    as Box<dyn player::Player>
            })
            .collect()
    } else if cli.llamafile {
        let port = setup_llamafile_headless().await;
        let client = player::llm::llamafile_client(port);

        let default_personalities = [
            player::personality::Personality::default_personality(),
            player::personality::Personality::aggressive(),
            player::personality::Personality::grudge_holder(),
            player::personality::Personality::builder(),
        ];

        let name_list = ["Alice", "Bob", "Charlie", "Diana"];
        (0..cli.players)
            .map(|i| {
                let personality = default_personalities[i].clone();
                Box::new(player::llm::LlmPlayer::with_client(
                    name_list[i].into(),
                    player::llm::LLAMAFILE_MODEL.into(),
                    personality,
                    client.clone(),
                )) as Box<dyn player::Player>
            })
            .collect()
    } else {
        let per_models: Vec<String> = if let Some(ref models_str) = cli.models {
            models_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
        } else {
            vec![cli.model.clone(); cli.players]
        };

        let custom_personality = cli.personality.as_ref().map(|path| {
            player::personality::Personality::from_toml_file(std::path::Path::new(path))
                .unwrap_or_else(|e| {
                    eprintln!("Warning: {}, using default personality", e);
                    player::personality::Personality::default()
                })
        });

        let default_personalities = [
            player::personality::Personality::default_personality(),
            player::personality::Personality::aggressive(),
            player::personality::Personality::grudge_holder(),
            player::personality::Personality::builder(),
        ];

        let name_list = ["Claude", "GPT", "Gemini", "Llama"];
        (0..cli.players)
            .map(|i| {
                let model = per_models
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| cli.model.clone());
                let personality = custom_personality
                    .clone()
                    .unwrap_or_else(|| default_personalities[i].clone());
                Box::new(player::llm::LlmPlayer::new(
                    name_list[i].into(),
                    model,
                    personality,
                )) as Box<dyn player::Player>
            })
            .collect()
    };

    // Run game in text mode.
    println!("Catan - Terminal Edition with LLM Players");
    println!("==========================================\n");
    println!("{}\n", player::prompt::ascii_board(&board));

    if cli.demo {
        println!("Starting demo game with random AI players...\n");
    } else {
        println!("Starting game with LLM players (model: {})...\n", cli.model);
    }

    let mut orchestrator = game::orchestrator::GameOrchestrator::new(state, players);

    match orchestrator.run().await {
        Ok(_winner) => {
            println!(
                "\nFinal scores: {}",
                (0..cli.players)
                    .map(|p| format!(
                        "Player {} ({}): {} VP",
                        p,
                        orchestrator.player_names[p],
                        orchestrator.state.victory_points(p)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let log_path = std::path::Path::new("game_log.jsonl");
            if let Err(e) = orchestrator.log.write_jsonl(log_path) {
                eprintln!("Warning: failed to write game log: {}", e);
            } else {
                println!("Game log saved to {}", log_path.display());
            }

            let replay_path = std::path::Path::new("game_replay.json");
            if let Ok(json) = serde_json::to_string_pretty(&orchestrator.replay) {
                if let Err(e) = std::fs::write(replay_path, json) {
                    eprintln!("Warning: failed to write replay: {}", e);
                } else {
                    println!("Replay saved to {}", replay_path.display());
                    println!("\n{}", orchestrator.replay.stats());
                }
            }
        }
        Err(e) => {
            eprintln!("Game ended: {}", e);
            let model_ids: Vec<String> = if let Some(ref models_str) = cli.models {
                models_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect()
            } else if cli.demo {
                vec!["".into(); cli.players]
            } else {
                vec![cli.model.clone(); cli.players]
            };
            let save = replay::save::SaveGame::new(
                orchestrator.state.clone(),
                &orchestrator.log,
                orchestrator.player_names.clone(),
                model_ids,
            );
            if let Err(e) = save.save_to_file(std::path::Path::new("game_save.json")) {
                eprintln!("Warning: failed to save game: {}", e);
            } else {
                println!("Game progress saved to game_save.json — resume with --headless --resume game_save.json");
            }
        }
    }
}

fn run_replay(replay_path: &str) {
    let path = std::path::Path::new(replay_path);

    if replay_path.ends_with(".json") {
        match std::fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str::<replay::recorder::GameReplay>(&contents) {
                Ok(replay) => {
                    println!("Replaying game: {} players", replay.num_players);
                    println!("Players: {}\n", replay.player_names.join(", "));
                    for (i, frame) in replay.frames.iter().enumerate() {
                        let vp: String = frame
                            .victory_points
                            .iter()
                            .enumerate()
                            .map(|(p, v)| format!("P{}:{}", p, v))
                            .collect::<Vec<_>>()
                            .join(" ");
                        println!(
                            "{:>4}. [T{:>3}] {} [{}]",
                            i + 1,
                            frame.turn,
                            frame.description,
                            vp
                        );
                    }
                    println!("\n{}", replay.stats());
                }
                Err(e) => eprintln!("Failed to parse replay: {}", e),
            },
            Err(e) => eprintln!("Failed to read replay file: {}", e),
        }
    } else {
        match replay::event::GameLog::read_jsonl(path) {
            Ok(log) => {
                println!("Replaying game from: {}", replay_path);
                println!("Total events: {}\n", log.events().len());
                for (i, event) in log.events().iter().enumerate() {
                    println!("{:>4}. {:?}", i + 1, event);
                }
            }
            Err(e) => eprintln!("Failed to read replay file: {}", e),
        }
    }
}

async fn run_resume(save_path: &str) {
    let path = std::path::Path::new(save_path);
    match replay::save::SaveGame::load_from_file(path) {
        Ok(save) => {
            println!("Resuming game from: {}", save_path);
            println!("Players: {}", save.player_names.join(", "));
            println!(
                "Turn: {}, Events: {}\n",
                save.state.turn_number,
                save.events.len()
            );

            let players: Vec<Box<dyn player::Player>> = save
                .player_names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let model = save.player_models.get(i).filter(|m| !m.is_empty()).cloned();
                    if let Some(model_id) = model {
                        Box::new(player::llm::LlmPlayer::new(
                            name.clone(),
                            model_id,
                            player::personality::Personality::default(),
                        )) as Box<dyn player::Player>
                    } else {
                        Box::new(player::random::RandomPlayer::new(name.clone()))
                            as Box<dyn player::Player>
                    }
                })
                .collect();

            let log = save.recent_log();
            let mut orchestrator = game::orchestrator::GameOrchestrator::new(save.state, players);
            orchestrator.log = log;

            match orchestrator.run().await {
                Ok(winner) => {
                    println!(
                        "\nPlayer {} ({}) wins!",
                        winner, orchestrator.player_names[winner]
                    );
                    let _ = orchestrator
                        .log
                        .write_jsonl(std::path::Path::new("game_log.jsonl"));
                    if let Ok(json) = serde_json::to_string_pretty(&orchestrator.replay) {
                        let _ = std::fs::write("game_replay.json", json);
                    }
                    println!("\n{}", orchestrator.replay.stats());
                }
                Err(e) => {
                    eprintln!("Game ended: {}", e);
                    let save = replay::save::SaveGame::new(
                        orchestrator.state.clone(),
                        &orchestrator.log,
                        orchestrator.player_names.clone(),
                        save.player_models.clone(),
                    );
                    if let Err(e) = save.save_to_file(std::path::Path::new("game_save.json")) {
                        eprintln!("Warning: failed to save game: {}", e);
                    } else {
                        println!("Game progress saved to game_save.json");
                    }
                }
            }
        }
        Err(e) => eprintln!("Failed to load save file: {}", e),
    }
}

/// Download (if needed) and start a local llamafile, printing progress to stderr.
/// Returns the port the server is listening on. Panics on failure.
async fn setup_llamafile_headless() -> u16 {
    use crate::llamafile::{format_bytes, LlamafileStatus};

    let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        let path = crate::llamafile::ensure_llamafile(status_tx.clone())
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
                eprint!("Checking for Bonsai-1.7B...");
            }
            Some(LlamafileStatus::Downloading { bytes, total }) => {
                if total > 0 {
                    let pct = (bytes as f64 / total as f64 * 100.0) as u16;
                    if pct != last_pct {
                        eprint!(
                            "\rDownloading Bonsai-1.7B... {} / {} ({}%)",
                            format_bytes(bytes),
                            format_bytes(total),
                            pct
                        );
                        last_pct = pct;
                    }
                } else {
                    eprint!("\rDownloading Bonsai-1.7B... {}", format_bytes(bytes));
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
                // Keep the process alive by leaking it. It will be killed when
                // the main process exits.
                let process = handle.await.expect("llamafile setup task panicked");
                std::mem::forget(process);
                return port;
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
