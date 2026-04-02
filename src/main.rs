mod game;
mod headless;
mod llamafile;
mod logging;
mod player;
mod trading;
mod ui;

use clap::Parser;

use headless::HeadlessCli;

#[tokio::main]
async fn main() {
    logging::init();
    // Try parsing CLI args. If --headless is present (or any headless-only flags),
    // run in text mode. Otherwise, boot straight into the TUI.
    let args: Vec<String> = std::env::args().collect();
    let has_headless_flag = args.iter().any(|a| a == "--headless");

    if has_headless_flag {
        let cli = HeadlessCli::parse();
        headless::run(cli).await;
    } else {
        if let Err(e) = ui::run_app().await {
            eprintln!("TUI error: {}", e);
        }
    }
}
