<p align="center">
  <h1 align="center">settl</h1>
  <p align="center">
    A terminal hex settlement game where LLMs play against each other -- or you.
    <br><br>
    <a href="https://github.com/Brake-Labs/settl/actions/workflows/ci.yml"><img src="https://github.com/Brake-Labs/settl/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache_2.0-blue.svg" alt="License: Apache 2.0"></a>
    <a href="https://settl.dev"><img src="https://img.shields.io/badge/docs-settl.dev-green" alt="Docs"></a>
    <br>
    <a href="https://x.com/natebrake"><img src="https://img.shields.io/badge/follow-%40natebrake-black?logo=x&logoColor=white" alt="Follow @natebrake"></a>
  </p>
</p>

Watch Claude, GPT, and Gemini negotiate trades, form grudges, and compete for longest road -- all in your terminal.

<!-- Demo GIF goes here -->
<!-- ![settl demo](assets/demo.gif) -->

## Features

- **Full game rules** -- settlements, cities, roads, robber, dev cards, trading, Longest Road, Largest Army
- **Play or spectate** -- take a seat as Player 1, or watch AI opponents battle it out
- **Multi-provider LLM players** -- Claude, GPT, Gemini, or any provider via [genai](https://crates.io/crates/genai)
- **Local AI** -- runs entirely offline with [llamafile](https://github.com/mozilla-ai/llamafile), no API keys required
- **Personality system** -- aggressive traders, grudge holders, cautious builders, chaos agents
- **Visible AI reasoning** -- watch each AI's strategic thinking in real time
- **Headless mode** -- run AI-vs-AI games for scripting and CI

## Quick Start

```bash
git clone https://github.com/Brake-Labs/settl.git
cd settl
cargo run
```

This launches the TUI. Select **New Game** to configure AI opponents and start playing. The default AI backend is [llamafile](https://github.com/mozilla-ai/llamafile) -- no API keys needed.

### Headless mode

```bash
cargo run -- --demo            # Random AI, no API keys
cargo run -- --headless        # LLM AI via llamafile
cargo run -- --demo --seed 42  # Reproducible board
```

### Cloud providers

Set environment variables to use hosted LLMs:

```bash
export ANTHROPIC_API_KEY=sk-ant-...   # Claude
export OPENAI_API_KEY=sk-...          # GPT
export GOOGLE_API_KEY=...             # Gemini
```

Then pass `--model` to select the model:

```bash
cargo run -- --headless --model claude-sonnet-4-6
cargo run -- --models "claude-sonnet-4-6,gpt-4o-mini,claude-haiku-4-5-20251001"
```

## How It Works

settl is a turn-based hex settlement game. Players collect resources from terrain tiles, build roads and settlements, trade with each other, and race to 10 victory points.

AI players use tool-calling LLMs to make decisions. Each AI has a persistent conversation with a system prompt that includes the board state, legal moves, and a personality profile. Decisions come back as structured JSON tool calls -- no free-text parsing.

The TUI renders the hex board, player stats, a scrollable game log, and a real-time AI reasoning panel. The game engine runs headless in a background task; the TUI is an optional observer.

## Documentation

Full docs are available in `docs/` and at [settl.dev](https://settl.dev):

- **[Getting Started](https://settl.dev/docs/getting-started/)** -- installation, first game, CLI options
- **[How to Play](https://settl.dev/docs/how-to-play/)** -- game rules, building, trading, winning
- **[Controls](https://settl.dev/docs/controls/)** -- keyboard shortcuts and interaction patterns
- **[AI Players](https://settl.dev/docs/ai-players/)** -- LLM providers, personalities, spectator mode
- **[Development](https://settl.dev/docs/development/)** -- contributing, architecture, testing

Docs are also accessible from the TUI via the **Docs** menu item.

## Development

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test                     # Run tests
cargo fmt                      # Format
cargo clippy                   # Lint
```

Debug logging writes to `~/.settl/debug.log`. See the [development docs](https://settl.dev/docs/development/) for architecture details and contribution guidelines.

## License

Apache 2.0 -- see [LICENSE](LICENSE) for details.
