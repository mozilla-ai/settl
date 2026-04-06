# Design System -- settl

## Product Context
- **What this is:** A terminal-based hex settlement game where LLMs play via tool calling, with human player support
- **Who it's for:** Developers, board game enthusiasts, AI-curious players who live in the terminal
- **Space:** Terminal games, TUI applications, AI demos
- **Project type:** Interactive TUI game (ratatui + crossterm)

## Design Thesis

**The board IS the interface.** Every game decision should happen visually on the board, not in a coordinate list or popup menu. The TUI should feel like looking down at a physical game board, not reading a spreadsheet.

## Aesthetic Direction
- **Direction:** Industrial/utilitarian with game-board warmth
- **Mood:** Clean, readable, functional -- like a well-made board game app, not a hacker tool. The terminal aesthetic serves the game, not the other way around.
- **Decoration level:** Minimal -- box-drawing characters for structure, Unicode symbols for game pieces, color for meaning. No decorative flourishes.

## Character Vocabulary (TUI Typography)

### Game Pieces
| Element | Character | Notes |
|---------|-----------|-------|
| Empty vertex | `·` (U+00B7) | Middle dot, dim gray |
| Settlement | 5w x 2h block | Player-colored background, white ▲ on top row |
| City | 5w x 3h block | Player-colored background, white ■ centered |
| Robber | `R` | Black on red background, unmissable |
| Port | `*` | On coastal vertex pairs |
| Cursor | reverse video | Highlighted legal position during placement |

### Board Structure
| Element | Characters | Notes |
|---------|------------|-------|
| Hex boundary | colored fill only | No border characters, terrain color defines shape |
| Road (diagonal) | colored block | Player-colored background, 3 cells diagonal |
| Road (vertical) | colored block | Player-colored background, 5 cells tall |
| Hex interior | Terrain + number | Each on dedicated row (see template) |

### Terrain Labels
Full resource names displayed on hex tiles (what the terrain produces, not terrain name):
| Terrain | Label | Produces |
|---------|-------|----------|
| Forest | `Wood` | Wood |
| Hills | `Brick` | Brick |
| Pasture | `Sheep` | Sheep |
| Fields | `Wheat` | Wheat |
| Mountains | `Ore` | Ore |
| Desert | `Desert` | Nothing |

## Color System

### Terrain Colors (background fill inside hex)
| Terrain | Color | Ratatui Value |
|---------|-------|---------------|
| Forest | Dark green | `Color::Rgb(34, 120, 34)` |
| Hills | Warm red-brown | `Color::Rgb(178, 102, 51)` |
| Pasture | Bright green | `Color::Rgb(80, 180, 60)` |
| Fields | Golden yellow | `Color::Rgb(200, 170, 50)` |
| Mountains | Cool gray | `Color::Rgb(140, 140, 150)` |
| Desert | Warm amber | `Color::Rgb(194, 150, 80)` |

**Fallback (256-color terminals):** Forest=Green, Hills=Red, Pasture=LightGreen, Fields=Yellow, Mountains=Gray, Desert=DarkGray.

### Player Colors
| Player | Color | Ratatui Value |
|--------|-------|---------------|
| P0 | Red | `Color::LightRed` |
| P1 | Blue | `Color::LightBlue` |
| P2 | Green | `Color::LightGreen` |
| P3 | Magenta | `Color::LightMagenta` |

Code uses `Light` variants for buildings/roads via `PLAYER_COLORS` constant. Standard variants for text labels in player panel.

### Resource Colors (in player panel and trade UI)
| Resource | Color | Panel Label | Key |
|----------|-------|-------------|-----|
| Wood | Green | `Wood` | `w` |
| Brick | Red | `Brick` | `b` |
| Sheep | Light green | `Sheep` | `s` |
| Wheat | Yellow | `Wheat` | `h` (harvest) |
| Ore | Gray | `Ore` | `o` |

### UI Chrome
| Element | Color | Purpose |
|---------|-------|---------|
| Active panel border | `Color::Cyan` | Shows which panel has focus |
| Inactive panel border | `Color::DarkGray` | Recedes visually |
| Primary text | `Color::White` | Main content |
| Secondary text | `Color::DarkGray` | Labels, hints, timestamps |
| Selection highlight | `Color::Black` on `Color::Cyan` | Current menu item |
| Legal position highlight | `Color::Yellow` + bold | Where you CAN place |
| Cursor position | Reverse video | Where you WILL place |
| VP warning (>=8) | `Color::Yellow` + bold | Close to winning |
| Danger/robber | `Color::Red` + bold | Robber, discarding |
| Success | `Color::Green` | Completed actions |

## Layout

### Screen Regions
```
┌──────────────────┬─────────────────────────────────┐
│ [Game] AI        │                                 │
│  Players, VP     │         BOARD (~65%)            │
│  ────────────    │   Hex grid with pieces          │
│  Game Log        │                                 │
│                  │                                 │
├──────────────────┴─────────────────────────────────┤
│  CONTEXT BAR (hidden when spectating)              │
├────────────────────────────────────────────────────┤
│  STATUS BAR (1 line)                               │
└────────────────────────────────────────────────────┘
```

The left panel is a tabbed sidebar. Tab switches between Game (players + log) and AI (reasoning).
The board is always visible, even when viewing AI thoughts.

### Layout Constraints (ratatui)
- **Board panel:** `Constraint::Fill(1)` width (takes remaining space), `Constraint::Min(15)` height
- **Left column:** `Constraint::Length(38)` width with tab bar (1 row) + content
- **Context bar:** `Constraint::Length(5)` height (collapses to 0 during Spectating)
- **Status bar:** `Constraint::Length(1)` height

### Minimum Terminal Size
- **Width:** 130 columns (warn if smaller)
- **Height:** 60 rows (warn if smaller)
- **Recommended:** 170x65 for comfortable play

### Context Bar Modes
The bottom panel is context-sensitive -- it shows different content based on game state.
It collapses to zero height during Spectating mode, giving the board more vertical space.

| Mode | Content | When Active |
|------|---------|-------------|
| **Action bar** | Horizontal menu: Build Settlement, Road, Dev Card, Trade, End Turn | Human player's turn, choosing action |
| **Placement mode** | "Select position with arrow keys, Enter to confirm" + description of highlighted position | Placing settlement, road, or robber (mandatory) |
| **Trade interface** | Two-column give/get with resource selectors | Proposing or responding to trade |
| **Discard interface** | Multi-select resource checklist with counter | Discarding on 7 roll |
| **Resource picker** | Resource key selector | Year of Plenty, Monopoly |
| **Steal target** | Player selection | After robber placement |
| **Trade response** | Accept/reject prompt | Incoming trade offer |
| *(hidden)* | Context bar not shown | Spectating / AI turns |

## Board Rendering Specification

### Hex Grid Geometry
Pointy-top hexes, interlocking. Each hex cell:
- **Width:** ~20 characters between left and right vertices (HEX_COL_Q=20)
- **Height:** 13 lines (top vertex to bottom vertex, VERT_OFF=6)
- **Row spacing:** 8 lines between hex centers (HEX_ROW=8)
- **Overlap with neighbors:** Shared vertices and edges
- **Vertex spacing:** Even 4-row gaps between all vertex rows (N, NE/NW, SE/SW, S)

### Hex Cell Template
```
              ·               <- cy-6: N vertex
            ╱   ╲             <- cy-5
          ╱       ╲           <- cy-4: fill starts
        ╱           ╲         <- cy-3
      ╱               ╲       <- cy-2: NE/NW vertices
     |   Wood          |     <- cy-1: TERRAIN label (dedicated row)
     |       6          |     <- cy:   NUMBER token (dedicated row)
     |                  |     <- cy+1
      ╲               ╱       <- cy+2: SE/SW vertices
        ╲           ╱         <- cy+3
          ╲       ╱           <- cy+4: fill ends
            ╲   ╱             <- cy+5
              ·               <- cy+6: S vertex
```

### Full Board Layout (3-4-5-4-3)
The standard board has 19 hexes arranged in 5 rows. The widest row (5 hexes) determines the overall width. Narrower rows are centered with indentation.

Row offsets (character indentation from left):
- Row r=-2 (3 hexes): indent 16
- Row r=-1 (4 hexes): indent 8
- Row r=0 (5 hexes): indent 0
- Row r=1 (4 hexes): indent 8
- Row r=2 (3 hexes): indent 16

### Building Placement on Board
When a vertex has a building, replace the `·` at that position with the building character (`▲` or `■`) in the player's color. The building should be clearly visible without disrupting the hex structure.

### Road Placement on Board
When an edge has a road, render the edge segment (`───`, `╱`, or `╲`) in the owning player's color instead of the default dim gray.

### Robber Display
The robber hex shows `R` in the center with a red background, replacing the terrain abbreviation. The number token is still visible.

## Interaction Patterns

### 1. Action Selection (during turn)
```
 ▸ Build Settlement   Build Road   Buy Dev Card   Propose Trade   End Turn
   [S]                [R]          [D]             [T]             [E]
```
- Left/right arrows or letter shortcuts to navigate
- Enter to select
- Shows in context bar

### 2. Board Cursor Navigation (during placement)
When placing settlements, roads, or the robber:
1. All legal positions highlighted in **yellow + bold** on the board
2. Current cursor position shown in **reverse video** (distinct from just "legal")
3. Arrow keys move cursor to nearest legal position in that direction
4. Context bar shows description of current position: "Forest/Hills/Pasture junction" or "Edge between Forest and Hills"
5. Enter to confirm (placement is mandatory -- no cancel)

### 3. Trade Builder (resource-key driven)
```
┌─ Trade Builder ──────────────────────────────────┐
│                                                   │
│  GIVE: ww          GET: o                         │
│  (2 Wood)          (1 Ore)                        │
│                                                   │
│  Ports: 3:1 generic, 2:1 Brick                    │
│  [Enter] send to all   [1-4] send to player       │
│  [Backspace] undo      [Esc] cancel               │
└───────────────────────────────────────────────────┘
```
- Type resource keys (w/b/s/h/o) to build the offer
- Tab switches between GIVE and GET sides
- Enter broadcasts, number keys target a specific player
- Shows port access so player knows their bank rates
- Backspace removes last resource added
- Live description auto-updates as you type

### 4. Discard (resource-key driven)
```
┌─ Discard 4 cards ────────────────────────────────┐
│                                                   │
│  Discarding: ww b         (3/4)                   │
│  (2 Wood, 1 Brick)                                │
│                                                   │
│  Have: W:3 B:2 S:1 H:4 O:2  (total: 12)          │
│  [w/b/s/h/o] add   [Backspace] undo  [Enter] done │
└───────────────────────────────────────────────────┘
```
- Same resource-key pattern as trade builder for muscle memory
- Type resource keys to add to discard pile
- Backspace removes last added
- Enter confirms when count is met
- NOT one-card-at-a-time prompts

### 5. Trade Response
```
┌─ Trade Offer from Alice ─────────────────────────┐
│  Offering: 1 Wood, 1 Brick                       │
│  Wanting:  1 Ore                                  │
│                                                   │
│  ▸ Accept     Reject                              │
└───────────────────────────────────────────────────┘
```

## Keyboard Design Philosophy

Shortcuts are optimized for **speed of play**, not discoverability. The most frequent
actions (end turn, build, trade) should have the fewest keystrokes. Resource
keys are consistent everywhere so muscle memory transfers between contexts.

### Resource Keys (universal, same everywhere)
| Key | Resource |
|-----|----------|
| `w` | Wood |
| `b` | Brick |
| `s` | Sheep |
| `h` | Wheat (harvest) |
| `o` | Ore |

These work in trade builder, discard, year of plenty, monopoly -- anywhere you pick resources.

### Player Keys (universal, same everywhere)
| Key | Target |
|-----|--------|
| `1` | Player 1 |
| `2` | Player 2 |
| `3` | Player 3 |
| `4` | Player 4 |

These work in trade targeting, steal targeting, and any player-selection context.

### Keyboard Controls

#### Global (always active)
| Key | Action |
|-----|--------|
| `q` | Quit (with confirm prompt) |
| `Tab` | Switch sidebar (Game/AI) |
| `?` | Help overlay |
| `L` | Llamafile server log |
| `+` / `-` | Speed up/slow down AI play |
| `j` / `k` | Scroll sidebar (game log or AI thoughts) |
| Scroll wheel | Scroll active panel (3 lines per tick) |

#### Action Selection (during your turn)
| Key | Action | Frequency |
|-----|--------|-----------|
| `e` | End Turn | Every turn |
| `r` | Build Road | Very common |
| `s` (hold: see below) | Build Settlement | Common |
| `c` | Upgrade to City | Occasional |
| `d` | Buy Dev Card | Occasional |
| `p` | Play Dev Card | Occasional |
| `b` | Bank Trade | Occasional |
| `t` | Open Trade Builder | Common |
| `Enter` | Select highlighted action | Always |
| `←` `→` | Navigate action bar | Always |

Note: `s` does double duty -- in action context it means Build Settlement, in resource
context it means Sheep. Context makes it unambiguous since you're never picking
resources and actions in the same prompt.

#### Board Cursor (during placement)
| Key | Action |
|-----|--------|
| `←` `→` `↑` `↓` | Move cursor between legal positions |
| `Enter` | Confirm placement |
| `n` / `p` | Next/previous legal position (cycles through list) |
| `j` / `k` / `l` / `m` | Quick-select road position by label (roads only) |

Arrow navigation uses nearest-neighbor in the pressed direction. `n`/`p` provide
a linear cycle when arrow navigation is ambiguous on the hex grid.

During road placement, legal positions are labeled `j`/`k`/`l`/`m` on the board.
Pressing the label key selects and confirms that road in one keystroke.

#### Quick Trade (the key innovation)
Trade is the most keyboard-intensive action in the game. The trade builder optimizes for
minimal keystrokes:

```
┌─ Trade Builder ──────────────────────────────────┐
│                                                   │
│  GIVE: ww          GET: o                         │
│  (2 Wood)          (1 Ore)                        │
│                                                   │
│  [Enter] send to all   [1-4] send to player       │
│  [Backspace] undo      [Esc] cancel               │
└───────────────────────────────────────────────────┘
```

Flow: `t` opens builder. Type resource keys to add to GIVE side. `Tab` switches to
GET side. Type resource keys to add. `Enter` broadcasts to all, or `1`-`4` targets
a specific player.

**Example: trade 2 wood for 1 ore:**
`t` `w` `w` `Tab` `o` `Enter` -- 6 keystrokes total.

**Example: trade 1 brick for 1 wheat with player 2:**
`t` `b` `Tab` `h` `2` -- 5 keystrokes total.

`Backspace` removes the last-added resource. The builder shows a live description
below the raw keys so you always see what you're building.

#### Bank/Port Trade
When you have port access, the trade builder detects it:
- 4:1 bank trade: type 4 of the same resource on GIVE side, resource auto-appears as available
- 3:1 generic port: type 3 of any resource
- 2:1 specific port: type 2 of the port's resource

The context bar shows your port access so you know your rates.

#### Discard (7 rolled, >7 cards)
| Key | Action |
|-----|--------|
| `w` `b` `s` `h` `o` | Add resource to discard pile |
| `Backspace` | Remove last-added resource |
| `Enter` | Confirm discard (when count met) |

Same resource-key pattern as trade builder. Counter shows progress: "Discarding: 2/4".
Faster than a multi-select grid for most cases.

#### Dev Card Play
| Key | Action |
|-----|--------|
| `p` | Opens dev card hand |
| `1`-`5` | Play card by position in hand |
| Resource keys | For Year of Plenty / Monopoly resource selection |

#### Trade Response (incoming offer)
| Key | Action |
|-----|--------|
| `y` / `Enter` | Accept |
| `n` / `Esc` | Reject |

Two keys. Fast response keeps the game moving.

## Information Hierarchy

### Always Visible
1. **Board** with all pieces, roads, robber, ports
2. **Left sidebar** with tabbed content (Game tab or AI tab)
3. **Status bar** with turn info, speed, key hints

### Sidebar Tabs
4. **Game tab** (default): Players info + Game log
5. **AI tab** (Tab key): AI reasoning / thoughts

### On Demand
6. **Help overlay** (? key)

### Player Panel Layout
```
 ▸ Alice        7VP
   Wood:2 Brick:1 Sheep:0
   Wheat:3 Ore:2
   Dev Cards:3 Kn:2  ★LR

   Bob          5VP
   Resources: 4
   Dev Cards:1

   Carol        4VP
   Resources: 3
   Dev Cards:1 Kn:1

 Turn 15 | Playing
 Deck: 20 cards
```

- `▸` marks current player (bright color)
- VP in bold, yellow if >= 8
- `★LR` = Longest Road, `★LA` = Largest Army (yellow bold)
- Human player sees own resources with full names across two lines
- Opponents show only total card count (resources are hidden)
- In spectator mode (all AI), all players show full resources
- Resources colored per resource type

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-01 | Full TUI redesign | Board was illegible, human controls used raw coordinates, game was unplayable |
| 2026-04-01 | Board-cursor interaction over popups | Spatial game needs spatial interaction; coordinate lists are hostile UX |
| 2026-04-01 | Context-sensitive bottom panel | Fixed panels waste space; dynamic panel gives more room to the board |
| 2026-04-01 | AI thoughts hidden by default | Spectator feature that hurts playability; still accessible via Tab |
| 2026-04-06 | Left-panel tabs replace fullscreen AI toggle | Board should always be visible; AI thoughts in sidebar tab keeps game context |
| 2026-04-06 | Context bar collapses during Spectating | No interactive controls during AI turns; extra rows for the board |
| 2026-04-01 | Multi-select discard | One-at-a-time discard of 4+ cards is tedious; batch selection is standard |
| 2026-04-01 | True color with 256-color fallback | Modern terminals support RGB; graceful degradation for older terminals |
| 2026-04-01 | Universal resource keys (w/b/s/h/o) | Same keys everywhere (trade, discard, dev cards) for muscle memory |
| 2026-04-01 | Resource-key trade builder over grid UI | 6 keystrokes for a trade vs. 10+ with arrow navigation; typing is faster than navigating |
| 2026-04-01 | Player targeting with number keys (1-4) | Direct targeting without menus; works in trade + robber steal contexts |
| 2026-04-01 | y/n for trade responses | Two-key response keeps game moving; no navigating Accept/Reject menu |
| 2026-04-02 | Scale hex from 12x7 to 16x9 | Cramped hexes made terrain, numbers, and dots compete for space; each now gets a dedicated row |
| 2026-04-02 | 3-char buildings: /▲\ and [■] | Single-char buildings blended into hex grid; multi-cell pieces are unmissable |
| 2026-04-02 | Full-length diagonal roads | Short ═══ segments didn't read as "roads"; full-edge ╱╲ lines match the hex structure |
| 2026-04-02 | Terrain-tinted hex edges | Uniform DarkGray edges felt flat; darkened terrain colors give organic board-game warmth |
| 2026-04-02 | Light* player colors for pieces | Standard color variants too dim on dark terminals; Light variants per DESIGN.md spec |
