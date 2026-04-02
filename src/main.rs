use clap::Parser;

use settl::headless::{self, HeadlessCli};
use settl::{logging, ui};

#[tokio::main]
async fn main() {
    logging::init();
    // Try parsing CLI args. If --headless is present (or any headless-only flags),
    // run in text mode. Otherwise, boot straight into the TUI.
    let args: Vec<String> = std::env::args().collect();
    let has_headless_flag = args
        .iter()
        .any(|a| a == "--headless" || a == "--demo" || a == "--models" || a == "--llamafile");

    if has_headless_flag {
        let cli = HeadlessCli::parse();
        headless::run(cli).await;
    } else {
        if let Err(e) = ui::run_app().await {
            eprintln!("TUI error: {}", e);
        }
    }
}
