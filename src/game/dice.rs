use std::collections::HashMap;

use rand::Rng;

use crate::game::actions::PlayerId;
use crate::game::board::Resource;
use crate::game::state::{Building, GameState};

/// Roll two six-sided dice, returning each die's value.
pub fn roll_dice(rng: &mut impl Rng) -> (u8, u8) {
    (rng.gen_range(1..=6), rng.gen_range(1..=6))
}

/// Calculate resource distribution for a given dice roll.
///
/// For each hex whose number token matches `roll`:
///   - Skip the hex if the robber is on it.
///   - For each vertex of the hex that has a building:
///     - Settlement: the owning player receives 1 of the hex's resource.
///     - City: the owning player receives 2 of the hex's resource.
///
/// Returns a map from player to a list of `(resource, count)` pairs.
pub fn distribute_resources(
    state: &GameState,
    roll: u8,
) -> HashMap<PlayerId, Vec<(Resource, u32)>> {
    // Intermediate: player -> resource -> count
    let mut totals: HashMap<PlayerId, HashMap<Resource, u32>> = HashMap::new();

    for hex in &state.board.hexes {
        // Only consider hexes whose number token matches the roll.
        let token = match hex.number_token {
            Some(t) => t,
            None => continue,
        };
        if token != roll {
            continue;
        }

        // Skip if the robber is sitting on this hex.
        if hex.coord == state.robber_hex {
            continue;
        }

        // Determine the resource this hex produces.
        let resource = match hex.terrain.resource() {
            Some(r) => r,
            None => continue,
        };

        // Check each vertex of the hex for a building.
        for vertex in hex.coord.vertices() {
            if let Some(building) = state.buildings.get(&vertex) {
                let (player, amount) = match *building {
                    Building::Settlement(p) => (p, 1u32),
                    Building::City(p) => (p, 2u32),
                };
                *totals
                    .entry(player)
                    .or_default()
                    .entry(resource)
                    .or_insert(0) += amount;
            }
        }
    }

    // Convert to the output format: player -> Vec<(Resource, count)>
    totals
        .into_iter()
        .map(|(player, resources)| {
            let list: Vec<(Resource, u32)> = resources.into_iter().collect();
            (player, list)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::{Board, HexCoord};
    use crate::game::state::GameState;
    use rand::SeedableRng;

    fn make_state(num_players: usize) -> GameState {
        GameState::new(Board::default_board(), num_players)
    }

    #[test]
    fn dice_roll_values_in_range() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        for _ in 0..1000 {
            let (d1, d2) = roll_dice(&mut rng);
            assert!((1..=6).contains(&d1), "die 1 out of range: {}", d1);
            assert!((1..=6).contains(&d2), "die 2 out of range: {}", d2);
        }
    }

    #[test]
    fn settlement_gets_one_resource() {
        let mut state = make_state(2);

        // Find a non-desert hex with a number token.
        let hex = state
            .board
            .hexes
            .iter()
            .find(|h| h.number_token.is_some())
            .unwrap()
            .clone();
        let token = hex.number_token.unwrap();
        let resource = hex.terrain.resource().unwrap();

        // Make sure robber is NOT on this hex (move it elsewhere if needed).
        if state.robber_hex == hex.coord {
            state.robber_hex = HexCoord::new(127, 127);
        }

        // Place a settlement at the first vertex of this hex for player 0.
        let vertex = hex.coord.vertices()[0];
        state.buildings.insert(vertex, Building::Settlement(0));

        let dist = distribute_resources(&state, token);
        let player_resources = dist.get(&0).expect("player 0 should receive resources");

        let count = player_resources
            .iter()
            .find(|(r, _)| *r == resource)
            .map(|(_, c)| *c)
            .unwrap_or(0);

        assert_eq!(count, 1, "settlement should produce exactly 1 resource");
    }

    #[test]
    fn city_gets_two_resources() {
        let mut state = make_state(2);

        let hex = state
            .board
            .hexes
            .iter()
            .find(|h| h.number_token.is_some())
            .unwrap()
            .clone();
        let token = hex.number_token.unwrap();
        let resource = hex.terrain.resource().unwrap();

        if state.robber_hex == hex.coord {
            state.robber_hex = HexCoord::new(127, 127);
        }

        let vertex = hex.coord.vertices()[0];
        state.buildings.insert(vertex, Building::City(1));

        let dist = distribute_resources(&state, token);
        let player_resources = dist.get(&1).expect("player 1 should receive resources");

        let count = player_resources
            .iter()
            .find(|(r, _)| *r == resource)
            .map(|(_, c)| *c)
            .unwrap_or(0);

        assert_eq!(count, 2, "city should produce exactly 2 resources");
    }

    #[test]
    fn robber_blocks_production() {
        let mut state = make_state(2);

        let hex = state
            .board
            .hexes
            .iter()
            .find(|h| h.number_token.is_some())
            .unwrap()
            .clone();
        let token = hex.number_token.unwrap();

        // Place the robber on this hex.
        state.robber_hex = hex.coord;

        // Place a settlement on a vertex.
        let vertex = hex.coord.vertices()[0];
        state.buildings.insert(vertex, Building::Settlement(0));

        let dist = distribute_resources(&state, token);

        // Player 0 should get nothing because the robber blocks production.
        assert!(
            dist.get(&0).is_none(),
            "robber hex should produce no resources"
        );
    }

    #[test]
    fn no_buildings_means_no_distribution() {
        let state = make_state(2);
        // Pick any roll; with no buildings, nobody should get anything.
        let dist = distribute_resources(&state, 6);
        assert!(dist.is_empty(), "no buildings means empty distribution");
    }

    #[test]
    fn multiple_players_on_same_hex() {
        let mut state = make_state(3);

        let hex = state
            .board
            .hexes
            .iter()
            .find(|h| h.number_token.is_some())
            .unwrap()
            .clone();
        let token = hex.number_token.unwrap();
        let resource = hex.terrain.resource().unwrap();

        if state.robber_hex == hex.coord {
            state.robber_hex = HexCoord::new(127, 127);
        }

        let vertices = hex.coord.vertices();
        state.buildings.insert(vertices[0], Building::Settlement(0));
        state.buildings.insert(vertices[1], Building::City(1));
        state.buildings.insert(vertices[2], Building::Settlement(2));

        let dist = distribute_resources(&state, token);

        let get_count = |player: PlayerId| -> u32 {
            dist.get(&player)
                .and_then(|rs| rs.iter().find(|(r, _)| *r == resource).map(|(_, c)| *c))
                .unwrap_or(0)
        };

        assert_eq!(get_count(0), 1, "player 0 settlement -> 1");
        assert_eq!(get_count(1), 2, "player 1 city -> 2");
        assert_eq!(get_count(2), 1, "player 2 settlement -> 1");
    }
}
