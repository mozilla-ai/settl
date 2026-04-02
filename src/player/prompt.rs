//! Serializes game state into prompts for LLM players.
//!
//! Two representations are produced:
//! 1. An ASCII hex board for spatial reasoning.
//! 2. Structured JSON for precise resource/action data.

use std::collections::HashMap;

use crate::game::actions::PlayerId;
use crate::game::board::{Board, HexCoord, Resource, VertexDirection};
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

/// Build the full system prompt for an LLM player.
pub fn system_prompt(player_name: &str, personality_prompt: &str) -> String {
    format!(
        "You are playing a game of Settlers of Catan in a terminal environment.\n\
         Your name is {player_name}.\n\n\
         {personality_prompt}\n\n\
         RULES REMINDER:\n\
         - Build settlements at vertices, roads along edges, upgrade settlements to cities.\n\
         - Resources: Wood (forest), Brick (hills), Sheep (pasture), Wheat (fields), Ore (mountains).\n\
         - Costs: Road = Wood+Brick. Settlement = Wood+Brick+Wheat+Sheep. City = 2 Wheat+3 Ore. Dev Card = Wheat+Sheep+Ore.\n\
         - Longest Road (5+) and Largest Army (3+ knights) each give 2 VP. First to 10 VP wins.\n\
         - When you choose, ALWAYS explain your strategic reasoning before deciding.\n\
         - Be concise but specific — reference coordinates and resource counts.",
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

/// Build a prompt for settlement placement during setup.
pub fn setup_settlement_prompt(
    state: &GameState,
    player_id: PlayerId,
    round: u8,
    legal_vertices: &[crate::game::board::VertexCoord],
) -> String {
    let board_ascii = ascii_board(&state.board);

    let vertex_list: String = legal_vertices
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let dir = match v.dir {
                VertexDirection::North => "N",
                VertexDirection::South => "S",
            };
            format!("  {}. ({}, {}, {})", i, v.hex.q, v.hex.r, dir)
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "BOARD:\n{board_ascii}\n\n\
         SETUP PHASE — Round {round}\n\
         You are Player {player_id}. Place your settlement.\n\
         {round_hint}\n\n\
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
}
