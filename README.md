# settl

A terminal-based hex settlement game where LLMs play against each other (or you).
Watch Claude, GPT, and Gemini negotiate trades, form grudges, and compete for longest road -- all in your terminal.

```
        [Fo 6] [Pa 3] [Hi 8]
      [Fi 2] [Mo 5] [Fo 4] [Pa 9]
    [Hi10] [Fi 6] [De --] [Fo 3] [Mo11]
      [Pa 5] [Mo 8] [Hi 4] [Fi 9]
        [Fo10] [Pa11] [Mo 3]
```

## Features

- **Full game rules** -- settlements, cities, roads, robber, development cards, Longest Road, Largest Army, bank trading, player-to-player trading
- **Play or spectate** -- player 1 is always human in TUI mode; watch the AI opponents play around you
- **Multi-provider LLM players** -- Claude, GPT, Gemini, and any provider supported by the [genai](https://crates.io/crates/genai) crate
- **Visible AI reasoning** -- every decision comes with a strategic explanation
- **Personality system** -- aggressive traders, grudge holders, cautious builders, chaos agents
- **Reproducible games** -- seed the RNG for deterministic board generation

## Quick Start

```bash
# Launch the TUI (title screen -> game setup -> play)
cargo run

# Headless demo with random AI players (no API keys needed)
cargo run -- --demo

# Headless game with LLM players (requires API key)
ANTHROPIC_API_KEY=sk-... cargo run -- --headless --model claude-sonnet-4-6

# Different models per player
ANTHROPIC_API_KEY=sk-... OPENAI_API_KEY=sk-... cargo run -- \
  --models "claude-sonnet-4-6,gpt-4o-mini,claude-haiku-4-5-20251001"

# Reproducible board with a seed
cargo run -- --demo --seed 42
```

## Modes

**TUI mode** (default): `cargo run` launches an interactive terminal UI with a title screen, game setup menu, and live hex board. Player 1 is always human; configure AI opponents (Random or LLM) in the setup screen.

**Headless mode**: Activated by passing `--headless`, `--demo`, or `--models`. Runs the game as plain text output, useful for scripting and CI.

## Headless CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `--headless` | Run in text mode (no TUI) | off |
| `--demo` | Random AI players, no API keys needed | off |
| `-p, --players N` | Number of players (2-4) | 4 |
| `-m, --model MODEL` | Default LLM model for all players | claude-sonnet-4-6 |
| `--models M1,M2,...` | Per-player model assignment | -- |
| `--personality FILE` | TOML personality file | built-in |
| `--seed N` | RNG seed for reproducible boards | random |

## TUI Controls

### During gameplay

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit to menu |
| `Space` | Pause/unpause (spectating) |
| `+` / `-` | Adjust AI speed |
| `j` / `k` | Scroll game log |
| `Tab` | Toggle AI reasoning panel |
| `?` | Toggle help overlay |

### Action bar (your turn)

| Key | Action |
|-----|--------|
| `e` | End Turn |
| `s` | Build Settlement |
| `r` | Build Road |
| `c` | Build City |
| `d` | Buy Development Card |
| `t` | Propose Trade |
| `p` | Play Development Card |

### Trade builder

| Key | Action |
|-----|--------|
| `w` `b` `s` `h` `o` | Toggle Wood, Brick, Sheep, Wheat, Ore |
| `Tab` | Switch between Give / Get sides |
| `Backspace` | Remove last resource |
| `Enter` | Confirm trade |
| `Esc` | Cancel |

### Discard (robber rolled 7)

| Key | Action |
|-----|--------|
| `w` `b` `s` `h` `o` | Add resource to discard pile |
| `Backspace` | Undo last discard |
| `Enter` | Confirm |
| `Esc` | Auto-complete remaining discards |

## Personalities

Create a TOML file to define custom AI personalities:

```toml
[personality]
name = "The Grudge Holder"
style = "remembers every slight, refuses trades with players who wronged them"
aggression = 0.3
cooperation = 0.2
catchphrases = ["I haven't forgotten turn 7.", "You'll have to do better than that."]
```

Built-in personalities: Default Strategist, Aggressive Trader, Grudge Holder, Cautious Builder, Chaos Agent.

## Architecture

```
src/
├── main.rs                    # Entry point: TUI vs headless dispatch
├── headless.rs                # Headless (text-mode) game runner and CLI
├── lib.rs                     # Crate root, module declarations
├── game/
│   ├── board.rs               # Hex grid (axial coords), terrain, ports
│   ├── state.rs               # GameState, PlayerState, buildings, roads
│   ├── rules.rs               # Legal moves, placement, dev cards
│   ├── actions.rs             # Action/DevCard/TradeOffer types
│   ├── dice.rs                # Dice rolls, resource distribution
│   ├── event.rs               # GameEvent enum, format for LLM context
│   └── orchestrator.rs        # Game loop, player interaction
├── player/
│   ├── mod.rs                 # Player trait (async)
│   ├── llm.rs                 # LLM player (genai + tool use)
│   ├── random.rs              # Random AI player (for testing)
│   ├── human.rs               # Human player (raw stdin)
│   ├── tui_human.rs           # TUI human player (channel-based input)
│   ├── personality.rs         # Personality system (TOML)
│   └── prompt.rs              # Board/state -> LLM prompt serialization
├── trading/
│   ├── negotiation.rs         # Trade protocol, validation, execution
│   └── offers.rs              # Offer validation, resource checks
└── ui/
    ├── mod.rs                 # TUI app state, event loop, input handling
    ├── screens.rs             # Title, main menu, new game, post-game
    ├── menu.rs                # Reusable menu widget
    ├── board_view.rs          # Hex board rendering (ratatui)
    ├── resource_bar.rs        # Player resource/VP panel
    ├── chat_panel.rs          # AI reasoning display
    ├── game_log.rs            # Scrollable event log
    └── layout.rs              # TUI layout composition
```

## API Keys

Set environment variables for LLM providers:

```bash
export ANTHROPIC_API_KEY=sk-ant-...   # Claude
export OPENAI_API_KEY=sk-...          # GPT
export GOOGLE_API_KEY=...             # Gemini
```

## Development

```bash
# Run all tests
cargo test

# Headless demo game
cargo run -- --demo

# Run with verbose output
RUST_LOG=debug cargo run -- --demo
```

## License

MIT
