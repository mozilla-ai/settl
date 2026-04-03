//! Serializes game state into prompts for LLM players.
//!
//! Two representations are produced:
//! 1. An ASCII hex board for spatial reasoning.
//! 2. Structured JSON for precise resource/action data.

use std::collections::HashMap;

use crate::game::actions::PlayerId;
use crate::game::board::{self, Board, HexCoord, PortType, Resource, VertexCoord, VertexDirection};
use crate::game::event::{self, GameEvent};
use crate::game::state::{Building, GameState};
use crate::player::PlayerChoice;

/// Render the board as an ASCII hex grid showing terrain + number tokens.
///
/// ```text
///         [Fo 6] [Pa 3] [Hi 8]
///       [Fi 2] [Mo 5] [Fo 4] [Pa 9]
///     [Hi10] [Fi 6] [De --] [Fo 3] [Mo11]
///       [Pa 5] [Mo 8] [Hi 4] [Fi 9]
///         [Fo10] [Pa11] [Mo 3]
/// ```
pub fn ascii_board(board: &Board) -> String {
    let mut lines = Vec::new();

    // Group hexes by row (r coordinate).
    let mut rows: HashMap<i8, Vec<&crate::game::board::Hex>> = HashMap::new();
    for hex in &board.hexes {
        rows.entry(hex.coord.r).or_default().push(hex);
    }

    let mut row_keys: Vec<i8> = rows.keys().copied().collect();
    row_keys.sort();

    // Widest row is r=0 with 5 hexes (standard board).
    let max_width = rows.values().map(|v| v.len()).max().unwrap_or(0);

    for &r in &row_keys {
        let mut hex_row = rows.get_mut(&r).unwrap().clone();
        hex_row.sort_by_key(|h| h.coord.q);

        // Indent narrower rows to center them.
        let indent = (max_width - hex_row.len()) * 3; // Each hex label is ~6 chars
        let padding = " ".repeat(indent);

        let mut cells = Vec::new();
        for hex in &hex_row {
            let terrain_abbr = hex.terrain.abbr();
            let number = hex
                .number_token
                .map(|n| format!("{:>2}", n))
                .unwrap_or_else(|| "--".to_string());
            cells.push(format!("[{}{:>2}]", terrain_abbr, number));
        }

        lines.push(format!("{}{}", padding, cells.join(" ")));
    }

    lines.join("\n")
}

/// Serialize the visible game state as JSON for precise LLM context.
pub fn game_state_json(state: &GameState, viewer: PlayerId) -> serde_json::Value {
    let mut players = Vec::new();
    for (i, ps) in state.players.iter().enumerate() {
        let is_self = i == viewer;
        let mut player_obj = serde_json::json!({
            "player_id": i,
            "victory_points": state.victory_points(i),
            "settlements_on_board": count_buildings(state, i, false),
            "cities_on_board": count_buildings(state, i, true),
            "road_count": state.roads.iter().filter(|(_, &p)| p == i).count(),
            "knights_played": ps.knights_played,
            "total_resource_cards": ps.total_resources(),
        });

        if is_self {
            // Show own resources and dev cards.
            let resources: HashMap<String, u32> = Resource::all()
                .iter()
                .map(|&r| (format!("{}", r), ps.resource_count(r)))
                .collect();
            player_obj["resources"] = serde_json::to_value(&resources).unwrap();
            let dev_cards: Vec<String> = ps.dev_cards.iter().map(|c| format!("{}", c)).collect();
            player_obj["dev_cards"] = serde_json::to_value(&dev_cards).unwrap();
        }

        players.push(player_obj);
    }

    let buildings: Vec<serde_json::Value> = state
        .buildings
        .iter()
        .map(|(v, b)| {
            let (owner, btype) = match b {
                Building::Settlement(p) => (*p, "settlement"),
                Building::City(p) => (*p, "city"),
            };
            let dir = match v.dir {
                VertexDirection::North => "N",
                VertexDirection::South => "S",
            };
            serde_json::json!({
                "vertex": format!("({},{},{})", v.hex.q, v.hex.r, dir),
                "owner": owner,
                "type": btype,
            })
        })
        .collect();

    let roads: Vec<serde_json::Value> = state
        .roads
        .iter()
        .map(|(e, &p)| {
            serde_json::json!({
                "edge": format!("{}", e),
                "owner": p,
            })
        })
        .collect();

    serde_json::json!({
        "turn_number": state.turn_number,
        "phase": format!("{:?}", state.phase),
        "robber_hex": format!("({},{})", state.robber_hex.q, state.robber_hex.r),
        "longest_road": {
            "player": state.longest_road_player,
            "length": state.longest_road_length,
        },
        "largest_army": {
            "player": state.largest_army_player,
            "size": state.largest_army_size,
        },
        "dev_cards_remaining": state.dev_card_deck.len(),
        "players": players,
        "buildings": buildings,
        "roads": roads,
    })
}

fn count_buildings(state: &GameState, player: PlayerId, cities: bool) -> usize {
    state
        .buildings
        .values()
        .filter(|b| match b {
            Building::Settlement(p) => !cities && *p == player,
            Building::City(p) => cities && *p == player,
        })
        .count()
}

/// Format the list of legal choices for display to an LLM.
pub fn format_choices(choices: &[PlayerChoice]) -> String {
    choices
        .iter()
        .enumerate()
        .map(|(i, c)| format!("  {}. {}", i, c))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format legal hexes for robber placement.
pub fn format_hex_options(hexes: &[HexCoord]) -> String {
    hexes
        .iter()
        .enumerate()
        .map(|(i, h)| format!("  {}. ({}, {})", i, h.q, h.r))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build the system prompt for an LLM player (e.g. llamafile).
///
/// Includes a condensed rules summary inline.
pub fn system_prompt_compact(player_name: &str, personality_prompt: &str) -> String {
    format!(
        "You are playing settl, a hex-based resource trading and building game. Your name is {player_name}.\n\n\
         {personality_prompt}\n\n\
         RULES:\n\
         First to 10 victory points wins. You can only win on your own turn.\n\n\
         Resources: Lumber (forest), Brick (hills), Ore (mountains), Grain (fields), Wool (pasture). Desert produces nothing.\n\n\
         Setup (snake draft): Round 1 clockwise, Round 2 reverse. Each round: place 1 settlement + 1 adjacent road. Second settlement grants starting resources.\n\
         Distance rule: no settlement if any of the 3 adjacent intersections is occupied.\n\n\
         Turn phases:\n\
         1. Roll dice -- matching hexes produce for adjacent settlements (1) and cities (2). Robber's hex produces nothing.\n\
         2. If 7: players with >7 cards discard half (rounded down), roller moves robber and steals 1 card from adjacent opponent.\n\
         3. Trade (optional) -- domestic with players, or maritime: 4:1 default, 3:1 generic harbor, 2:1 matching harbor.\n\
         4. Build (optional, as many as affordable):\n\
            Road = 1 Brick + 1 Lumber. Settlement = 1 Brick + 1 Lumber + 1 Wool + 1 Grain (1 VP). City upgrade = 3 Ore + 2 Grain (2 VP). Dev Card = 1 Ore + 1 Wool + 1 Grain.\n\
            Supply limits: 15 roads, 5 settlements, 4 cities. Roads must connect to your network. Settlements must connect and obey distance rule. Cities replace a settlement.\n\n\
         Dev cards: play at most 1 per turn, not on turn purchased.\n\
         Knight (14): move robber + steal. Road Building (2): 2 free roads. Year of Plenty (2): take any 2 from bank. Monopoly (2): take all of 1 resource from opponents. VP cards (5): 1 VP each, reveal only when winning.\n\n\
         Longest Road (2 VP): first with 5+ continuous segments. Largest Army (2 VP): first with 3+ knights played.\n\n\
         INSTRUCTIONS:\n\
         - Explain your reasoning, then call the tool to make your choice.\n\
         - Reference coordinates and resource counts.",
    )
}

/// Format recent game events as a history summary for LLM context.
///
/// Shows the last `max_events` events with human-readable descriptions.
/// This gives LLMs context about recent trades, attacks, and game flow.
pub fn format_recent_history(
    events: &[GameEvent],
    player_names: &[String],
    max_events: usize,
) -> String {
    let recent = if events.len() > max_events {
        &events[events.len() - max_events..]
    } else {
        events
    };

    if recent.is_empty() {
        return String::new();
    }

    let lines: Vec<String> = recent
        .iter()
        .map(|e| format!("- {}", event::format_event(e, player_names)))
        .collect();

    format!("RECENT HISTORY:\n{}", lines.join("\n"))
}

/// Build the turn prompt with board state, recent history, and choices.
pub fn turn_prompt(state: &GameState, player_id: PlayerId, choices: &[PlayerChoice]) -> String {
    turn_prompt_with_history(state, player_id, choices, &[], &[])
}

/// Build the turn prompt with board state, recent history, and choices.
pub fn turn_prompt_with_history(
    state: &GameState,
    player_id: PlayerId,
    choices: &[PlayerChoice],
    recent_events: &[GameEvent],
    player_names: &[String],
) -> String {
    let board_ascii = ascii_board(&state.board);
    let state_json = game_state_json(state, player_id);
    let history = format_recent_history(recent_events, player_names, 20);

    let history_section = if history.is_empty() {
        String::new()
    } else {
        format!("\n{}\n", history)
    };

    format!(
        "BOARD:\n{board_ascii}\n\n\
         GAME STATE:\n{state_json}\n{history_section}\n\
         You are Player {player_id}.\n\n\
         LEGAL ACTIONS:\n{choices}\n\n\
         Choose your action by calling the choose_action tool.",
        choices = format_choices(choices),
    )
}

/// Annotate a single vertex with resources, pips, port, and spatial context.
///
/// Includes opponent proximity, shared production hexes, and expansion potential
/// so the LLM can reason about spatial relationships, not just resource quality.
pub fn annotate_vertex(
    index: usize,
    v: &VertexCoord,
    state: &GameState,
    player_id: PlayerId,
    player_names: &[String],
) -> String {
    let board = &state.board;
    let dir = match v.dir {
        VertexDirection::North => "N",
        VertexDirection::South => "S",
    };

    // Resource and pip info.
    let adj_hexes = board::vertex_neighbors(*v);
    let mut resources = Vec::new();
    let mut total_pips: u8 = 0;

    for hex_coord in &adj_hexes {
        if let Some(hex) = board.get_hex(*hex_coord) {
            if let Some(resource) = hex.terrain.resource() {
                let token = hex.number_token.unwrap_or(0);
                let pips = board::pip_count(token);
                resources.push(format!("{}({})", resource, token));
                total_pips += pips;
            }
        }
    }

    let resources_str = if resources.is_empty() {
        "Desert only".to_string()
    } else {
        resources.join(", ")
    };

    // Port info.
    let port_str = if let Some(port) = board.port_at_vertex(*v) {
        match port.port_type {
            PortType::Generic => " | 3:1 port".to_string(),
            PortType::Specific(r) => format!(" | 2:1 {} port", r),
        }
    } else {
        String::new()
    };

    // Spatial context: opponents on shared hexes.
    let mut opponents_nearby: Vec<String> = Vec::new();
    for hex_coord in &adj_hexes {
        if !board::is_board_hex(*hex_coord) {
            continue;
        }
        let hex_verts = board::hex_vertices(*hex_coord);
        for hv in &hex_verts {
            if hv == v {
                continue;
            }
            if let Some(building) = state.buildings.get(hv) {
                let owner = match building {
                    Building::Settlement(p) | Building::City(p) => *p,
                };
                if owner != player_id {
                    let name = player_names
                        .get(owner)
                        .cloned()
                        .unwrap_or_else(|| format!("P{}", owner));
                    opponents_nearby.push(name);
                }
            }
        }
    }
    opponents_nearby.sort();
    opponents_nearby.dedup();

    let opponent_str = if opponents_nearby.is_empty() {
        String::new()
    } else {
        format!(" | near: {}", opponents_nearby.join(", "))
    };

    // Expansion potential: how many adjacent vertices are open (no building,
    // no building on *their* neighbors either = satisfies distance rule).
    let adj_verts = board::adjacent_vertices(*v);
    let open_count = adj_verts
        .iter()
        .filter(|av| {
            // Must be on the board (at least one adjacent hex exists).
            let av_hexes = board::vertex_neighbors(**av);
            let on_board = av_hexes.iter().any(|h| board::is_board_hex(*h));
            if !on_board {
                return false;
            }
            // Must not already have a building.
            if state.buildings.contains_key(av) {
                return false;
            }
            // Must satisfy distance rule (no building on *its* neighbors).
            let av_neighbors = board::adjacent_vertices(**av);
            !av_neighbors
                .iter()
                .any(|n| n != v && state.buildings.contains_key(n))
        })
        .count();

    // Your existing buildings (for round 2 context).
    let your_buildings: Vec<String> = state
        .buildings
        .iter()
        .filter(|(_, b)| match b {
            Building::Settlement(p) | Building::City(p) => *p == player_id,
        })
        .map(|(bv, _)| {
            let d = match bv.dir {
                VertexDirection::North => "N",
                VertexDirection::South => "S",
            };
            format!("({},{},{})", bv.hex.q, bv.hex.r, d)
        })
        .collect();

    let your_str = if your_buildings.is_empty() {
        String::new()
    } else {
        format!(" | your settlements: {}", your_buildings.join(", "))
    };

    format!(
        "  {index}. ({},{},{dir}) | {resources_str} | pips={total_pips} | \
         expand={open_count}{port_str}{opponent_str}{your_str}",
        v.hex.q, v.hex.r,
    )
}

/// Build a prompt for settlement placement during setup.
pub fn setup_settlement_prompt(
    state: &GameState,
    player_id: PlayerId,
    round: u8,
    legal_vertices: &[VertexCoord],
    player_names: &[String],
) -> String {
    let board_ascii = ascii_board(&state.board);

    let vertex_list: String = legal_vertices
        .iter()
        .enumerate()
        .map(|(i, v)| annotate_vertex(i, v, state, player_id, player_names))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "BOARD:\n{board_ascii}\n\n\
         SETUP PHASE -- Round {round}\n\
         You are Player {player_id}. Place your settlement.\n\
         {round_hint}\n\n\
         VERTEX KEY: index. (q,r,dir) | resources | pips=probability | expand=open_adjacent_spots | port | nearby_opponents | your_buildings\n\
         - pips: total probability dots (higher=more production, max 5 per hex for 6/8)\n\
         - expand: number of adjacent vertices where you could later build (satisfying distance rule)\n\n\
         LEGAL SETTLEMENT LOCATIONS:\n{vertex_list}\n\n\
         Choose by calling the choose_index tool.",
        round_hint = if round == 2 {
            "This is your second settlement. You'll receive one of each adjacent resource."
        } else {
            "This is your first settlement. Choose a location with good resource diversity."
        },
    )
}

/// Build a prompt for road placement during setup.
pub fn setup_road_prompt(
    state: &GameState,
    player_id: PlayerId,
    legal_edges: &[crate::game::board::EdgeCoord],
) -> String {
    let board_ascii = ascii_board(&state.board);

    let edge_list: String = legal_edges
        .iter()
        .enumerate()
        .map(|(i, e)| format!("  {}. {}", i, e))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "BOARD:\n{board_ascii}\n\n\
         SETUP PHASE — Place your road.\n\
         You are Player {player_id}.\n\n\
         LEGAL ROAD LOCATIONS:\n{edge_list}\n\n\
         Choose by calling the choose_index tool.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::Board;
    use crate::game::state::GameState;

    #[test]
    fn ascii_board_has_correct_row_count() {
        let board = Board::default_board();
        let ascii = ascii_board(&board);
        let lines: Vec<&str> = ascii.lines().collect();
        assert_eq!(lines.len(), 5, "standard board has 5 rows");
    }

    #[test]
    fn ascii_board_shows_desert() {
        let board = Board::default_board();
        let ascii = ascii_board(&board);
        assert!(ascii.contains("De--"), "desert should show De--");
    }

    #[test]
    fn game_state_json_includes_own_resources() {
        let state = GameState::new(Board::default_board(), 4);
        let json = game_state_json(&state, 0);
        // Player 0 should have a "resources" key.
        assert!(json["players"][0]["resources"].is_object());
        // Player 1 should NOT have a "resources" key (hidden).
        assert!(json["players"][1]["resources"].is_null());
    }

    #[test]
    fn format_choices_numbers_correctly() {
        use crate::game::actions::Action;
        let choices = vec![
            PlayerChoice::GameAction(Action::EndTurn),
            PlayerChoice::PlayKnight,
        ];
        let formatted = format_choices(&choices);
        assert!(formatted.contains("0. End Turn"));
        assert!(formatted.contains("1. Play Knight"));
    }

    fn test_names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("P{}", i)).collect()
    }

    #[test]
    fn annotate_vertex_shows_resources_and_pips() {
        let state = GameState::new(Board::default_board(), 3);
        let names = test_names(3);
        // North vertex of (0,-2): adjacent to hexes (0,-2), (0,-3), (1,-3).
        // Only (0,-2) is on the board (Mountains/10). The others are off-board.
        let v = crate::game::board::VertexCoord::new(
            crate::game::board::HexCoord::new(0, -2),
            crate::game::board::VertexDirection::North,
        );
        let annotation = annotate_vertex(0, &v, &state, 0, &names);
        assert!(
            annotation.contains("Ore(10)"),
            "should list Ore(10): {}",
            annotation
        );
        assert!(
            annotation.contains("pips="),
            "should show pip count: {}",
            annotation
        );
        assert!(
            annotation.contains("expand="),
            "should show expansion potential: {}",
            annotation
        );
    }

    #[test]
    fn annotate_vertex_shows_port() {
        let state = GameState::new(Board::default_board(), 3);
        let names = test_names(3);
        // North vertex of (2,-2) has a 2:1 Wheat port.
        let v = crate::game::board::VertexCoord::new(
            crate::game::board::HexCoord::new(2, -2),
            crate::game::board::VertexDirection::North,
        );
        let annotation = annotate_vertex(0, &v, &state, 0, &names);
        assert!(
            annotation.contains("2:1 Wheat port"),
            "should show wheat port: {}",
            annotation
        );
    }

    #[test]
    fn annotate_vertex_interior_no_port() {
        let state = GameState::new(Board::default_board(), 3);
        let names = test_names(3);
        // South vertex of (0,-1): adjacent to (0,-1), (0,0), (-1,0).
        // (0,-1) = Fields/12, (0,0) = Desert, (-1,0) = Fields/11
        let v = crate::game::board::VertexCoord::new(
            crate::game::board::HexCoord::new(0, -1),
            crate::game::board::VertexDirection::South,
        );
        let annotation = annotate_vertex(0, &v, &state, 0, &names);
        assert!(
            annotation.contains("Wheat"),
            "should contain Wheat: {}",
            annotation
        );
        assert!(
            !annotation.contains("port"),
            "interior vertex should have no port: {}",
            annotation
        );
    }

    #[test]
    fn annotate_vertex_shows_opponent_nearby() {
        let mut state = GameState::new(Board::default_board(), 3);
        let names = vec!["Alice".into(), "Bob".into(), "Charlie".into()];
        // Place Bob's settlement at North(0,-2) -- on the Mountains(10) hex.
        let bob_v = crate::game::board::VertexCoord::new(
            crate::game::board::HexCoord::new(0, -2),
            crate::game::board::VertexDirection::North,
        );
        state.buildings.insert(bob_v, Building::Settlement(1));

        // South vertex of (1,-2) also touches hex (0,-2), so Bob is nearby.
        let v = crate::game::board::VertexCoord::new(
            crate::game::board::HexCoord::new(1, -3),
            crate::game::board::VertexDirection::South,
        );
        let annotation = annotate_vertex(0, &v, &state, 0, &names);
        assert!(
            annotation.contains("near: Bob"),
            "should show Bob nearby: {}",
            annotation
        );
    }

    #[test]
    fn annotate_vertex_shows_own_buildings_in_round2() {
        let mut state = GameState::new(Board::default_board(), 3);
        let names = test_names(3);
        // Place player 0's first settlement.
        let my_v = crate::game::board::VertexCoord::new(
            crate::game::board::HexCoord::new(0, -2),
            crate::game::board::VertexDirection::North,
        );
        state.buildings.insert(my_v, Building::Settlement(0));

        // Any other vertex should show "your settlements: (0,-2,N)".
        let v = crate::game::board::VertexCoord::new(
            crate::game::board::HexCoord::new(-2, 2),
            crate::game::board::VertexDirection::South,
        );
        let annotation = annotate_vertex(0, &v, &state, 0, &names);
        assert!(
            annotation.contains("your settlements:"),
            "should show own buildings: {}",
            annotation
        );
        assert!(
            annotation.contains("(0,-2,N)"),
            "should reference first settlement coords: {}",
            annotation
        );
    }

    #[test]
    fn setup_settlement_prompt_contains_annotation_legend() {
        let state = GameState::new(Board::default_board(), 3);
        let names = test_names(3);
        let legal = crate::game::rules::legal_setup_vertices(&state);
        let prompt = setup_settlement_prompt(&state, 0, 1, &legal, &names);
        assert!(
            prompt.contains("pips="),
            "prompt should contain pip annotations"
        );
        assert!(
            prompt.contains("VERTEX KEY:"),
            "prompt should contain legend"
        );
        assert!(
            prompt.contains("expand="),
            "prompt should contain expansion info"
        );
    }
}
