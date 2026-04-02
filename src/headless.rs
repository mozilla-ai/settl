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

    /// Random seed for reproducible games
    #[arg(long)]
    pub seed: Option<u64>,

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

    // Create players.
    let players: Vec<Box<dyn player::Player>> = if cli.demo {
        let name_list = ["Alice", "Bob", "Charlie", "Diana"];
        (0..cli.players)
            .map(|i| {
                Box::new(player::random::RandomPlayer::new(name_list[i].into()))
                    as Box<dyn player::Player>
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
        }
        Err(e) => {
            eprintln!("Game ended: {}", e);
        }
    }
}
