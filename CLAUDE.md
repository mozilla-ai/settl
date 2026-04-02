# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
cargo build                              # Debug build
cargo build --release                    # Release build (binary: target/release/settl)
cargo test                               # All tests
cargo test game::rules                   # Tests in a specific module
cargo test test_name                     # Single test by name
cargo run                                # Launch TUI (title screen -> menus -> game)
```

The binary boots into a TUI (title screen -> main menu -> game setup). LLM mode requires provider API keys as env vars: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_API_KEY`.

## Architecture

**settl** is a terminal Catan game where LLMs play via tool/function calling. The codebase has four modules:

### `game/` -- Core engine (stateless rules + stateful orchestrator)
- **`board.rs`** -- Hex grid using axial coordinates `(q, r)`. Vertices and edges are expressed as `(HexCoord, Direction)` pairs. Only canonical edge directions (NE, E, SE) are stored; opposites resolve to the neighbor's canonical form.
- **`state.rs`** -- `GameState` holds the full mutable game: board, per-player resources/cards (`PlayerState`), buildings, roads, robber position, dev card deck, longest road/largest army tracking. `GamePhase` enum drives the state machine (Setup -> Playing -> Discarding -> PlacingRobber -> Stealing -> GameOver).
- **`rules.rs`** -- Pure validation functions. Given a `GameState`, returns legal moves. Enforces distance rule, connectivity, resource costs, dev card logic, longest road calculation. Largest file (~1760 lines).
- **`event.rs`** -- `GameEvent` enum for all discrete game actions; `format_event()` renders them as human-readable text for LLM context.
- **`orchestrator.rs`** -- Drives the game loop. Calls `Player` trait methods at decision points, applies actions through the rules engine, tracks events for LLM context, sends UI updates via `mpsc` channel. Runs the setup snake-draft and main turn loop.
- **`dice.rs`** -- Dice rolling and resource distribution per hex/number.

### `player/` -- Player abstraction (async trait)
- **`mod.rs`** -- `Player` trait with async methods: `choose_action`, `choose_settlement`, `choose_road`, `choose_resource`, `respond_to_trade`, etc. Each returns `(choice, reasoning_string)`.
- **`llm.rs`** -- `LlmPlayer` uses the `genai` crate for multi-provider LLM support. Defines JSON-schema tools (`choose_index`, `choose_resource`, `discard_tool`, `propose_trade_tool`) for structured responses. Retries up to 2x on parse failure, falls back to random.
- **`random.rs`** -- `RandomPlayer` for testing and `--demo` mode.
- **`human.rs`** -- `HumanPlayer` for raw stdin input (non-TUI).
- **`tui_human.rs`** -- `TuiHumanPlayer` for TUI mode; communicates with the UI via channels to show a selection overlay.
- **`prompt.rs`** -- Serializes board/state into text for LLM context.
- **`personality.rs`** -- Loads TOML personality configs (aggression/cooperation scores, style text, catchphrases) and injects into system prompts. Built-in personalities: Default Strategist, Aggressive Trader, Grudge Holder, Cautious Builder, Chaos Agent. Custom ones go in `personalities/*.toml`.

### `trading/` -- Trade negotiation
- **`negotiation.rs`** -- Multi-round trade protocol: propose -> respond (accept/reject/counter) -> execute.
- **`offers.rs`** -- Trade validation (both sides have resources, no self-trades) and `trade_value_heuristic()` scoring.

### `ui/` -- TUI (ratatui + crossterm)
- Async game engine runs in a background tokio task; TUI runs on the main thread.
- Communication via `mpsc::unbounded_channel` sending `StateUpdate` events.
- `board_view.rs` renders the hex board, `chat_panel.rs` shows AI reasoning, `resource_bar.rs` shows player stats, `game_log.rs` is scrollable event history.

## Key Design Decisions

- **Coordinate system**: Axial hex coordinates with vertex/edge pairs reduce duplication. Canonical edge storage means the same physical edge is never represented two ways.
- **Tool-based LLM integration**: JSON schemas enforce structured responses rather than parsing free text. Every decision captures reasoning separately from the action.
- **Game logic is UI-independent**: The engine runs headless; TUI is an optional observer via channel.
- **Personality = system prompt injection**: No hardcoded behavioral branches; personality is entirely expressed as LLM prompt text.

## Coding Style

- Keep code `cargo fmt`-clean and `cargo clippy`-clean.
- Run `cargo fmt`, `cargo clippy`, and `cargo test` before finishing any task.
- **Never use emdashes** in documentation or comments.
- Rust naming: `snake_case` for modules/functions, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Add comments where they aid understanding, but remove obvious ones (section headers restating the next line, comments that just name what the code does).

## Testing

- Unit tests go in-module (`#[cfg(test)]`); integration tests in `tests/`.
- Tests must be deterministic. Use seeded RNG where randomness is needed.

### TUI Test Framework

The TUI has dedicated test infrastructure in `src/ui/` (`#[cfg(test)]` child modules):

- **`testing.rs`** -- Helpers: `render_to_buffer()` renders any `Screen` to a ratatui `TestBackend`, `buffer_to_string()` converts to plain text, `make_test_playing_state()` creates a `PlayingState` with real channels so `send_response()` works (returns the receiver for assertions).
- **`input_tests.rs`** -- Tests for `handle_input()` across all screens and input modes. Verifies state transitions, keyboard shortcuts, and that the correct `HumanResponse` is sent on the channel.
- **`snapshot_tests.rs`** -- Insta snapshot tests rendering each screen/mode to a 120x40 buffer. Catches visual regressions.

Key patterns:
- `handle_input()` and `draw_screen()` are already pure functions on `App` state -- no refactoring needed to test them.
- `make_test_playing_state(input_mode)` returns `(PlayingState, UnboundedReceiver<HumanResponse>)`. Set the input mode, call `handle_input`, then `rx.try_recv()` to assert what was sent.
- Snapshot workflow: `cargo test` fails on visual changes, `cargo insta review` shows diffs, accept/reject interactively.

```bash
cargo test ui::input_tests              # Input handling tests (55 tests)
cargo test ui::snapshot_tests           # Visual snapshot tests (15 tests)
cargo insta review                      # Review snapshot diffs after UI changes
```

## Commits & PRs

- Branch names: `feature/...`, `fix/...`, `docs/...`, `refactor/...`.
- Commit messages: use conventional prefixes (`feat:`, `fix:`, `docs:`, `refactor:`).

## Design System
Always read DESIGN.md before making any visual or UI decisions.
All color choices, character vocabulary, layout constraints, keyboard shortcuts, and interaction patterns are defined there.
Do not deviate without explicit user approval.
In QA mode, flag any code that doesn't match DESIGN.md.

## Catan Rules

See [`CATAN_RULES.md`](CATAN_RULES.md) for the complete game rules reference (setup, turn structure, building, trading, robber, development cards, special cards, victory conditions).

**Quick cost reference**: Road: 1 Wood + 1 Brick | Settlement: 1 Wood + 1 Brick + 1 Sheep + 1 Wheat | City: 2 Wheat + 3 Ore | Dev Card: 1 Wheat + 1 Sheep + 1 Ore
