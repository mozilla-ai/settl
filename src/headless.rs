//! Headless (text-mode) game runner -- preserves the original CLI-driven behavior
//! for scripting, CI, and non-interactive use.

use clap::Parser;

use crate::game;
use crate::player;

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

    /// Path to a TOML personality file
    #[arg(long)]
    pub personality: Option<String>,

    /// Run in headless mode (no TUI)
    #[arg(long)]
    pub headless: bool,
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

    // Start local llamafile AI server.
    let port = setup_llamafile_headless().await;
    let client = player::llamafile_player::llamafile_client(port);

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
            Box::new(player::llamafile_player::LlamafilePlayer::with_client(
                name_list[i].into(),
                player::llamafile_player::LLAMAFILE_MODEL.into(),
                personality,
                client.clone(),
            )) as Box<dyn player::Player>
        })
        .collect();

    // Run game in text mode.
    println!("Catan - Terminal Edition with LLM Players");
    println!("==========================================\n");
    println!("{}\n", player::prompt::ascii_board(&board));
    println!("Starting game with local AI (Bonsai-1.7B)...\n");

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
        }
        Err(e) => {
            eprintln!("Game ended: {}", e);
        }
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
                // Intentionally leak the process so it stays alive for the
                // duration of the program. The OS cleans it up on exit.
                let process = handle.await.expect("llamafile setup task panicked");
                Box::leak(Box::new(process));
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
