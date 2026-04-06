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

The binary boots into a TUI (title screen -> main menu -> game setup). AI players run locally via llamafile by default, no API keys needed.

Debug logging writes to `~/.settl/debug.log`. Enabled automatically in debug builds, off in release. Override with env var:
```bash
cargo run                                # Debug build: logging on automatically
SETTL_DEBUG=0 cargo run                  # Debug build: force logging off
SETTL_DEBUG=1 cargo run --release        # Release build: force logging on
```
Use `log::debug!()` / `log::info!()` etc. from any module.

## Architecture

**settl** is a terminal hex settlement game where LLMs play via tool/function calling. The codebase has four modules:

### `game/` -- Core engine (stateless rules + stateful orchestrator)
- **`board.rs`** -- Hex grid using axial coordinates `(q, r)`. Vertices and edges are expressed as `(HexCoord, Direction)` pairs. Only canonical edge directions (NE, E, SE) are stored; opposites resolve to the neighbor's canonical form.
- **`state.rs`** -- `GameState` holds the full mutable game: board, per-player resources/cards (`PlayerState`), buildings, roads, robber position, dev card deck, longest road/largest army tracking. `GamePhase` enum drives the state machine (Setup -> Playing -> Discarding -> PlacingRobber -> Stealing -> GameOver).
- **`rules.rs`** -- Pure validation functions. Given a `GameState`, returns legal moves. Enforces distance rule, connectivity, resource costs, dev card logic, longest road calculation. Largest file (~2400 lines).
- **`actions.rs`** -- Action and dev card type definitions (`DevCard`, `PlayerAction`, etc.) used across the engine.
- **`event.rs`** -- `GameEvent` enum for all discrete game actions; `format_event()` renders them as human-readable text for LLM context.
- **`orchestrator.rs`** -- Drives the game loop. Calls `Player` trait methods at decision points, applies actions through the rules engine, tracks events for LLM context, sends UI updates via `mpsc` channel. Runs the setup snake-draft and main turn loop.
- **`dice.rs`** -- Dice rolling and resource distribution per hex/number.
- **`save.rs`** -- Auto-save and resume. Saves game state to `~/.settl/saves/autosave.json` after each turn; main menu shows "Continue" when a save exists.

### `player/` -- Player abstraction (async trait)
- **`mod.rs`** -- `Player` trait with async methods: `choose_action`, `choose_settlement`, `choose_road`, `choose_resource`, `respond_to_trade`, etc. Each returns `(choice, reasoning_string)`.
- **`anthropic_client.rs`** -- Thin HTTP client for the Anthropic Messages API (`/v1/messages`). Works with both local llamafile/llama.cpp and real Anthropic API. Includes llamafile-specific extensions (`id_slot`, `cache_prompt`) for KV cache slot management.
- **`llm_player.rs`** -- `LlmPlayer` talks to the Anthropic Messages API via `anthropic_client`. Maintains per-player conversation history for KV cache efficiency. Defines JSON-schema tools (`choose_index`, `choose_resource`, `choose_discard`, `propose_trade`, `respond_to_trade`) for structured responses. Retries up to 2x on parse failure, falls back to random.
- **`random.rs`** -- `RandomPlayer` for testing and `--demo` mode.
- **`human.rs`** -- `HumanPlayer` for raw stdin input (non-TUI).
- **`tui_human.rs`** -- `TuiHumanPlayer` for TUI mode; communicates with the UI via channels to show a selection overlay.
- **`prompt.rs`** -- Serializes board/state into text for LLM context.
- **`personality.rs`** -- Loads TOML personality configs (aggression/cooperation scores, style text, catchphrases) and injects into system prompts. Built-in personalities: Balanced Strategist, The Merchant, The Grudge Holder, The Architect, The Wild Card. Custom ones go in `personalities/*.toml`.

### `trading/` -- Trade negotiation
- **`negotiation.rs`** -- Multi-round trade protocol: propose -> respond (accept/reject/counter) -> execute.
- **`offers.rs`** -- Trade validation (both sides have resources, no self-trades) and `trade_value_heuristic()` scoring.

### `ui/` -- TUI (ratatui + crossterm)
- Async game engine runs in a background tokio task; TUI runs on the main thread.
- Communication via `mpsc::unbounded_channel` sending `StateUpdate` events.
- `board_view.rs` renders the hex board, `chat_panel.rs` shows AI reasoning, `resource_bar.rs` shows player stats, `game_log.rs` is scrollable event history.
- `layout.rs` handles responsive terminal layout and panel sizing.
- `menu.rs` renders menu screens (title, game setup, etc.).

## Key Design Decisions

- **Coordinate system**: Axial hex coordinates with vertex/edge pairs reduce duplication. Canonical edge storage means the same physical edge is never represented two ways.
- **Tool-based LLM integration**: JSON schemas enforce structured responses rather than parsing free text. Every decision captures reasoning separately from the action.
- **Game logic is UI-independent**: The engine communicates with the TUI via channels, which also enables headless mode for scripting and testing.
- **Personality = system prompt injection**: No hardcoded behavioral branches; personality is entirely expressed as LLM prompt text.

## Coding Style

- Keep code `cargo fmt`-clean and `cargo clippy`-clean.
- Run `cargo fmt`, `cargo clippy`, and `cargo test` before finishing any task.
- **Never use `#[allow(dead_code)]`** -- delete unused code instead of suppressing warnings.
- **Never use emdashes** in documentation or comments.
- **Never write to stdout/stderr** (`println!`, `eprintln!`, `dbg!`) in code that runs under the TUI. Raw terminal output corrupts the alternate screen. Use the TUI's own status/error display (e.g. `LlamafileStatus::Error`) or log to a file instead.
- Rust naming: `snake_case` for modules/functions, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Add comments where they aid understanding, but remove obvious ones (section headers restating the next line, comments that just name what the code does).
- **Renaming user-facing terms**: When changing a UI label, panel name, or key description, grep the entire codebase for the old term (case-insensitive) and update all occurrences: UI strings, comments, docs, DESIGN.md, help overlay, status bar hints. User-visible strings like placeholder text are easy to miss.

## Testing

- Unit tests go in-module (`#[cfg(test)]`); integration tests in `tests/`.
- Tests must be deterministic. Use seeded RNG where randomness is needed.

### TUI Test Framework

The TUI has dedicated test infrastructure in `src/ui/` (`#[cfg(test)]` child modules):

- **`testing.rs`** -- Helpers: `render_to_buffer()` renders any `Screen` to a ratatui `TestBackend`, `buffer_to_string()` converts to plain text, `make_test_playing_state()` creates a `PlayingState` with real channels so `send_response()` works (returns the receiver for assertions).
- **`input_tests.rs`** -- Tests for `handle_input()` across all screens and input modes. Verifies state transitions, keyboard shortcuts, and that the correct `HumanResponse` is sent on the channel.
- **`snapshot_tests.rs`** -- Insta snapshot tests rendering each screen/mode to a 120x40 buffer. Catches visual regressions.
- **`flow_tests.rs`** -- E2E flow tests exercising the full game loop: orchestrator spawning, channel communication between engine and TuiHumanPlayer, screen transitions, setup phase, turn cycles.

Key patterns:
- `handle_input()` and `draw_screen()` are already pure functions on `App` state -- no refactoring needed to test them.
- `make_test_playing_state(input_mode)` returns `(PlayingState, UnboundedReceiver<HumanResponse>)`. Set the input mode, call `handle_input`, then `rx.try_recv()` to assert what was sent.
- Snapshot workflow: `cargo test` fails on visual changes, `cargo insta review` shows diffs, accept/reject interactively.

```bash
cargo test ui::input_tests              # Input handling tests (~96 tests)
cargo test ui::snapshot_tests           # Visual snapshot tests (~18 tests)
cargo test ui::flow_tests               # E2E flow tests (~5 tests)
cargo insta review                      # Review snapshot diffs after UI changes
```

## Testing with Llamafile

The llamafile AI server can be downloaded and run inside the sandbox. **Always test AI/LLM player changes end-to-end** by running the game with llamafile, not just unit tests. Unit tests don't exercise the real API/streaming path and will miss issues like timeout mismatches, SSE parsing bugs, or broken channel wiring.

```bash
cargo run -- --headless --players 2   # Headless game with llamafile AI players
cargo run                             # Launch TUI and set up a game with AI opponents
```

The llamafile is a cosmopolitan (APE) binary. On aarch64 without binfmt_misc support, direct execution fails with "Exec format error". The code falls back to `sh <path>` which works because APE binaries embed a shell script header. If you see exec format errors, ensure the `sh` fallback path in `llamafile/process.rs` covers the error string.

## Commits & PRs

- Branch names: `feature/...`, `fix/...`, `docs/...`, `refactor/...`.
- Commit messages: use conventional prefixes (`feat:`, `fix:`, `docs:`, `refactor:`).

## Design System
Always read DESIGN.md before making any visual or UI decisions.
All color choices, character vocabulary, layout constraints, keyboard shortcuts, and interaction patterns are defined there.
Do not deviate without explicit user approval.
In QA mode, flag any code that doesn't match DESIGN.md.

## Documentation

Documentation lives in `docs/` as markdown files with YAML frontmatter. These serve two consumers:

1. **Astro website** (`website/`) -- `scripts/generate-docs.mjs` reads from `docs/` and generates Astro pages at build time. Run `node scripts/generate-docs.mjs` to regenerate.
2. **TUI binary** -- docs are embedded via `include_str!()` in `src/ui/screens.rs` (`docs_pages()` function). Accessible from the main menu via "Docs".

When adding or renaming doc files, update both:
- The `docs_pages()` function in `src/ui/screens.rs` (add/remove `include_str!` entries)
- The `docsNav` array in `website/src/data/docsNav.ts` (sidebar navigation)

### Definition of Done for docs changes

When changing game rules, controls, CLI options, or player-facing behavior:
- [ ] Update the relevant `docs/*.md` file to reflect the change
- [ ] Verify the TUI docs viewer still compiles (`cargo build`)
- [ ] Run `node scripts/generate-docs.mjs` to regenerate website pages
- [ ] Never reference "Settlers of Catan" or "Catan" anywhere in docs, README, or code comments
- [ ] No usages of the 'em-dash' (--) anywhere in the code or comments

## Game Rules

**Quick cost reference**: Road: 1 Wood + 1 Brick | Settlement: 1 Wood + 1 Brick + 1 Sheep + 1 Wheat | City: 2 Wheat + 3 Ore | Dev Card: 1 Wheat + 1 Sheep + 1 Ore
