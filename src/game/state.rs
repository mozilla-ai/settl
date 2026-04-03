use std::collections::HashMap;

use rand::seq::SliceRandom as _;
use serde::{Deserialize, Serialize};

use crate::game::actions::{DevCard, PlayerId};
use crate::game::board::{Board, EdgeCoord, HexCoord, Resource, VertexCoord};

/// Maximum number of settlements a player can build.
pub const MAX_SETTLEMENTS: usize = 5;
/// Maximum number of cities a player can build.
pub const MAX_CITIES: usize = 4;
/// Maximum number of roads a player can build.
pub const MAX_ROADS: usize = 15;
/// Victory points needed to win.
pub const VP_TO_WIN: u8 = 10;

/// A building placed on the board at a vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Building {
    Settlement(PlayerId),
    City(PlayerId),
}

/// Tracks a single player's state during the game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    /// Resource cards in hand, keyed by resource type.
    #[serde(with = "resources_serde")]
    pub resources: HashMap<Resource, u32>,
    /// Development cards in hand (not yet played).
    pub dev_cards: Vec<DevCard>,
    /// Number of dev cards bought this turn (can't be played until next turn).
    pub dev_cards_bought_this_turn: usize,
    /// Number of Knight cards played (for Largest Army).
    pub knights_played: u32,
    /// Whether the player has already played a dev card this turn.
    pub has_played_dev_card_this_turn: bool,
    /// Remaining settlement pieces.
    pub settlements_remaining: usize,
    /// Remaining city pieces.
    pub cities_remaining: usize,
    /// Remaining road pieces.
    pub roads_remaining: usize,
}

impl PlayerState {
    /// Create a new player with starting piece counts and zero resources.
    pub fn new() -> Self {
        let mut resources = HashMap::new();
        for &r in Resource::all() {
            resources.insert(r, 0);
        }
        PlayerState {
            resources,
            dev_cards: Vec::new(),
            dev_cards_bought_this_turn: 0,
            knights_played: 0,
            has_played_dev_card_this_turn: false,
            settlements_remaining: MAX_SETTLEMENTS,
            cities_remaining: MAX_CITIES,
            roads_remaining: MAX_ROADS,
        }
    }

    /// Get the count of a specific resource in hand.
    pub fn resource_count(&self, r: Resource) -> u32 {
        *self.resources.get(&r).unwrap_or(&0)
    }

    /// Check whether the player has at least the specified resources.
    pub fn has_resources(&self, costs: &[(Resource, u32)]) -> bool {
        costs
            .iter()
            .all(|(resource, amount)| self.resource_count(*resource) >= *amount)
    }

    /// Add resources to the player's hand.
    pub fn add_resource(&mut self, r: Resource, count: u32) {
        *self.resources.entry(r).or_insert(0) += count;
    }

    /// Remove resources from the player's hand.
    ///
    /// # Panics
    ///
    /// Panics if the player does not have enough of the specified resource.
    pub fn remove_resource(&mut self, r: Resource, count: u32) {
        let entry = self.resources.entry(r).or_insert(0);
        assert!(
            *entry >= count,
            "Cannot remove {} {} — player only has {}",
            count,
            r,
            *entry
        );
        *entry -= count;
    }

    /// Total number of resource cards in hand.
    pub fn total_resources(&self) -> u32 {
        self.resources.values().sum()
    }
}

impl Default for PlayerState {
    fn default() -> Self {
        Self::new()
    }
}

/// The current phase of the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GamePhase {
    /// Initial setup: players place settlements and roads in snake-draft order.
    Setup { round: u8, player_index: usize },
    /// Normal play: the current player may roll, build, trade, etc.
    Playing {
        current_player: PlayerId,
        has_rolled: bool,
    },
    /// A 7 was rolled and some players must discard half their cards.
    Discarding {
        current_player: PlayerId,
        players_needing_discard: Vec<PlayerId>,
    },
    /// The current player must move the robber to a new hex.
    PlacingRobber { current_player: PlayerId },
    /// The current player may steal from a player adjacent to the robber hex.
    Stealing {
        current_player: PlayerId,
        target_hex: HexCoord,
    },
    /// The game is over.
    GameOver { winner: PlayerId },
}

/// The complete state of a game in progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    /// The board layout (hex tiles).
    pub board: Board,
    /// Per-player state (indexed by PlayerId).
    pub players: Vec<PlayerState>,
    /// Number of players in this game.
    pub num_players: usize,
    /// Buildings placed on the board, keyed by vertex.
    #[serde(with = "buildings_serde")]
    pub buildings: HashMap<VertexCoord, Building>,
    /// Roads placed on the board, keyed by edge.
    #[serde(with = "roads_serde")]
    pub roads: HashMap<EdgeCoord, PlayerId>,
    /// Current location of the robber.
    pub robber_hex: HexCoord,
    /// Remaining development cards to be purchased.
    pub dev_card_deck: Vec<DevCard>,
    /// Player who currently holds Longest Road (if any).
    pub longest_road_player: Option<PlayerId>,
    /// Length of the current longest road.
    pub longest_road_length: u8,
    /// Player who currently holds Largest Army (if any).
    pub largest_army_player: Option<PlayerId>,
    /// Size of the current largest army.
    pub largest_army_size: u32,
    /// Current game phase.
    pub phase: GamePhase,
    /// Number of full turns that have been completed.
    pub turn_number: u32,
    /// Snake-draft order for setup phase.
    pub setup_order: Vec<PlayerId>,
    /// When true, the robber cannot target hexes where all adjacent players
    /// have 2 or fewer victory points.
    pub friendly_robber: bool,
}

impl GameState {
    /// Create a new game with the given board and number of players.
    ///
    /// Initializes player states, shuffles the dev card deck, places the robber
    /// on the desert, and sets up the snake-draft order.
    pub fn new(board: Board, num_players: usize) -> Self {
        assert!(
            (2..=4).contains(&num_players),
            "settl supports 2-4 players, got {}",
            num_players
        );

        // Create player states.
        let players: Vec<PlayerState> = (0..num_players).map(|_| PlayerState::new()).collect();

        // Build and shuffle the development card deck.
        let mut dev_card_deck = Vec::with_capacity(25);
        for _ in 0..14 {
            dev_card_deck.push(DevCard::Knight);
        }
        for _ in 0..5 {
            dev_card_deck.push(DevCard::VictoryPoint);
        }
        for _ in 0..2 {
            dev_card_deck.push(DevCard::RoadBuilding);
        }
        for _ in 0..2 {
            dev_card_deck.push(DevCard::YearOfPlenty);
        }
        for _ in 0..2 {
            dev_card_deck.push(DevCard::Monopoly);
        }
        {
            let mut rng = rand::rng();
            dev_card_deck.shuffle(&mut rng);
        }

        // Find the desert hex for the robber's starting position.
        let robber_hex = board.desert_hex().expect("Board must contain a desert hex");

        // Snake-draft setup order: [0,1,...,n-1, n-1,...,1,0]
        let mut setup_order: Vec<PlayerId> = (0..num_players).collect();
        let reverse: Vec<PlayerId> = (0..num_players).rev().collect();
        setup_order.extend(reverse);

        GameState {
            board,
            players,
            num_players,
            buildings: HashMap::new(),
            roads: HashMap::new(),
            robber_hex,
            dev_card_deck,
            longest_road_player: None,
            longest_road_length: 0,
            largest_army_player: None,
            largest_army_size: 0,
            phase: GamePhase::Setup {
                round: 1,
                player_index: 0,
            },
            turn_number: 0,
            setup_order,
            friendly_robber: false,
        }
    }

    /// Returns the PlayerId of whoever should act right now.
    pub fn current_player(&self) -> PlayerId {
        match &self.phase {
            GamePhase::Setup { player_index, .. } => self.setup_order[*player_index],
            GamePhase::Playing { current_player, .. }
            | GamePhase::Discarding { current_player, .. }
            | GamePhase::PlacingRobber { current_player }
            | GamePhase::Stealing { current_player, .. } => *current_player,
            GamePhase::GameOver { winner } => *winner,
        }
    }

    /// Calculate the victory points for a given player.
    ///
    /// Counts:
    /// - 1 VP per settlement on the board
    /// - 2 VP per city on the board
    /// - 2 VP for holding Longest Road
    /// - 2 VP for holding Largest Army
    /// - 1 VP per Victory Point development card in hand
    pub fn victory_points(&self, player: PlayerId) -> u8 {
        let mut vp: u8 = 0;

        // Count buildings on the board.
        for building in self.buildings.values() {
            match building {
                Building::Settlement(owner) if *owner == player => vp += 1,
                Building::City(owner) if *owner == player => vp += 2,
                _ => {}
            }
        }

        // Longest Road bonus.
        if self.longest_road_player == Some(player) {
            vp += 2;
        }

        // Largest Army bonus.
        if self.largest_army_player == Some(player) {
            vp += 2;
        }

        // Victory Point dev cards in hand.
        let vp_cards = self.players[player]
            .dev_cards
            .iter()
            .filter(|c| **c == DevCard::VictoryPoint)
            .count() as u8;
        vp += vp_cards;

        vp
    }
}

/// Serialize HashMap<VertexCoord, Building> as a Vec of pairs.
mod buildings_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        map: &HashMap<VertexCoord, Building>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let vec: Vec<(VertexCoord, Building)> = map.iter().map(|(k, v)| (*k, *v)).collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HashMap<VertexCoord, Building>, D::Error> {
        let vec: Vec<(VertexCoord, Building)> = Vec::deserialize(deserializer)?;
        Ok(vec.into_iter().collect())
    }
}

/// Serialize HashMap<EdgeCoord, PlayerId> as a Vec of pairs.
mod roads_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        map: &HashMap<EdgeCoord, PlayerId>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let vec: Vec<(EdgeCoord, PlayerId)> = map.iter().map(|(k, v)| (*k, *v)).collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HashMap<EdgeCoord, PlayerId>, D::Error> {
        let vec: Vec<(EdgeCoord, PlayerId)> = Vec::deserialize(deserializer)?;
        Ok(vec.into_iter().collect())
    }
}

/// Serialize HashMap<Resource, u32> as a Vec of pairs.
mod resources_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        map: &HashMap<Resource, u32>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let vec: Vec<(Resource, u32)> = map.iter().map(|(k, v)| (*k, *v)).collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HashMap<Resource, u32>, D::Error> {
        let vec: Vec<(Resource, u32)> = Vec::deserialize(deserializer)?;
        Ok(vec.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::{Board, VertexDirection};

    fn make_board() -> Board {
        Board::default_board()
    }

    #[test]
    fn new_player_has_correct_piece_counts() {
        let p = PlayerState::new();
        assert_eq!(p.settlements_remaining, MAX_SETTLEMENTS);
        assert_eq!(p.cities_remaining, MAX_CITIES);
        assert_eq!(p.roads_remaining, MAX_ROADS);
        assert_eq!(p.total_resources(), 0);
        assert_eq!(p.knights_played, 0);
        assert!(!p.has_played_dev_card_this_turn);
        assert!(p.dev_cards.is_empty());
    }

    #[test]
    fn dev_card_deck_has_correct_size() {
        let state = GameState::new(make_board(), 4);
        assert_eq!(state.dev_card_deck.len(), 25);

        // Verify composition: count each type.
        let knights = state
            .dev_card_deck
            .iter()
            .filter(|c| **c == DevCard::Knight)
            .count();
        let vps = state
            .dev_card_deck
            .iter()
            .filter(|c| **c == DevCard::VictoryPoint)
            .count();
        let roads = state
            .dev_card_deck
            .iter()
            .filter(|c| **c == DevCard::RoadBuilding)
            .count();
        let yops = state
            .dev_card_deck
            .iter()
            .filter(|c| **c == DevCard::YearOfPlenty)
            .count();
        let monos = state
            .dev_card_deck
            .iter()
            .filter(|c| **c == DevCard::Monopoly)
            .count();

        assert_eq!(knights, 14);
        assert_eq!(vps, 5);
        assert_eq!(roads, 2);
        assert_eq!(yops, 2);
        assert_eq!(monos, 2);
    }

    #[test]
    fn setup_order_correct_for_4_players() {
        let state = GameState::new(make_board(), 4);
        assert_eq!(state.setup_order, vec![0, 1, 2, 3, 3, 2, 1, 0]);
    }

    #[test]
    fn setup_order_correct_for_3_players() {
        let state = GameState::new(make_board(), 3);
        assert_eq!(state.setup_order, vec![0, 1, 2, 2, 1, 0]);
    }

    #[test]
    fn victory_points_includes_all_sources() {
        let mut state = GameState::new(make_board(), 4);
        let player: PlayerId = 0;

        // Initially zero VP.
        assert_eq!(state.victory_points(player), 0);

        // Add a settlement: +1 VP.
        let v1 = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        state.buildings.insert(v1, Building::Settlement(player));
        assert_eq!(state.victory_points(player), 1);

        // Add a city: +2 VP (total 3).
        let v2 = VertexCoord::new(HexCoord::new(1, 0), VertexDirection::South);
        state.buildings.insert(v2, Building::City(player));
        assert_eq!(state.victory_points(player), 3);

        // Grant Longest Road: +2 VP (total 5).
        state.longest_road_player = Some(player);
        assert_eq!(state.victory_points(player), 5);

        // Grant Largest Army: +2 VP (total 7).
        state.largest_army_player = Some(player);
        assert_eq!(state.victory_points(player), 7);

        // Add a VP dev card: +1 VP (total 8).
        state.players[player].dev_cards.push(DevCard::VictoryPoint);
        assert_eq!(state.victory_points(player), 8);

        // Add another VP dev card: +1 VP (total 9).
        state.players[player].dev_cards.push(DevCard::VictoryPoint);
        assert_eq!(state.victory_points(player), 9);

        // Non-VP dev cards should not count.
        state.players[player].dev_cards.push(DevCard::Knight);
        assert_eq!(state.victory_points(player), 9);

        // Another player's buildings should not count for player 0.
        let v3 = VertexCoord::new(HexCoord::new(2, -1), VertexDirection::North);
        state.buildings.insert(v3, Building::Settlement(1));
        assert_eq!(state.victory_points(player), 9);
        assert_eq!(state.victory_points(1), 1);
    }

    #[test]
    fn resource_add_remove_works() {
        let mut p = PlayerState::new();

        // Start with zero.
        assert_eq!(p.resource_count(Resource::Brick), 0);
        assert_eq!(p.total_resources(), 0);

        // Add some resources.
        p.add_resource(Resource::Brick, 3);
        p.add_resource(Resource::Ore, 2);
        assert_eq!(p.resource_count(Resource::Brick), 3);
        assert_eq!(p.resource_count(Resource::Ore), 2);
        assert_eq!(p.total_resources(), 5);

        // Remove some.
        p.remove_resource(Resource::Brick, 1);
        assert_eq!(p.resource_count(Resource::Brick), 2);
        assert_eq!(p.total_resources(), 4);

        // has_resources checks.
        assert!(p.has_resources(&[(Resource::Brick, 2)]));
        assert!(p.has_resources(&[(Resource::Brick, 1), (Resource::Ore, 2)]));
        assert!(!p.has_resources(&[(Resource::Brick, 3)]));
        assert!(!p.has_resources(&[(Resource::Wood, 1)]));
    }

    #[test]
    #[should_panic(expected = "Cannot remove")]
    fn remove_resource_panics_on_insufficient() {
        let mut p = PlayerState::new();
        p.add_resource(Resource::Wheat, 1);
        p.remove_resource(Resource::Wheat, 2);
    }

    #[test]
    fn current_player_in_setup_phase() {
        let state = GameState::new(make_board(), 4);
        // Phase is Setup { round: 1, player_index: 0 }, setup_order[0] = 0.
        assert_eq!(state.current_player(), 0);
    }

    #[test]
    fn current_player_in_playing_phase() {
        let mut state = GameState::new(make_board(), 4);
        state.phase = GamePhase::Playing {
            current_player: 2,
            has_rolled: false,
        };
        assert_eq!(state.current_player(), 2);
    }

    #[test]
    fn robber_starts_on_desert() {
        let board = make_board();
        let desert = board.desert_hex().unwrap();
        let state = GameState::new(board, 4);
        assert_eq!(state.robber_hex, desert);
    }
}
