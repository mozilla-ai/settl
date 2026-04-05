<p align="center">
  <h1 align="center">settl</h1>
  <p align="center">
    A terminal hex settlement game where you play against other LLMs, backed by <a href="https://github.com/mozilla-ai/llamafile">llamafile</a> and <a href="https://prismml.com/">Bonsai Models</a> (with extensibility to other llamafiles)
    <br><br>
    <a href="https://github.com/Brake-Labs/settl/actions/workflows/ci.yml"><img src="https://github.com/Brake-Labs/settl/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache_2.0-blue.svg" alt="License: Apache 2.0"></a>
    <a href="https://settl.dev"><img src="https://img.shields.io/badge/docs-settl.dev-green" alt="Docs"></a>
    <br>
    <a href="https://x.com/natebrake"><img src="https://img.shields.io/badge/follow-%40natebrake-black?logo=x&logoColor=white" alt="Follow @natebrake"></a>
  </p>
</p>

<p align="center">
  <img src="assets/demo.gif" alt="settl demo" width="800" />
</p>

## Features

- **Full game rules**: settlements, cities, roads, robber, dev cards, trading, Longest Road, Largest Army
- **Local AI**: runs entirely offline with [llamafile](https://github.com/mozilla-ai/llamafile), no API keys required
- **Personality system**: aggressive traders, grudge holders, cautious builders, chaos agents
- **Visible AI reasoning**: watch each AI's strategic thinking in real time

## Quick Start

```bash
git clone https://github.com/Brake-Labs/settl.git
cd settl
cargo run
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
