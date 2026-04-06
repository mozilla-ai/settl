use std::collections::{HashMap, HashSet};

use crate::game::actions::{Action, DevCard, DevCardAction, PlayerId};
use crate::game::board::{
    adjacent_edges, adjacent_vertices, board_hex_coords, edge_neighbors, edge_vertices,
    vertex_neighbors, Board, EdgeCoord, HexCoord, PortType, Resource, VertexCoord,
};
use crate::game::dice::{total_in_circulation, BANK_SUPPLY_PER_RESOURCE};
use crate::game::state::{Building, GamePhase, GameState, VP_TO_WIN};

/// Errors returned by `apply_action` when a rule is violated.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleError {
    NotYourTurn,
    MustRollFirst,
    AlreadyRolled,
    InsufficientResources,
    NoPiecesLeft,
    InvalidPlacement(String),
    NoBuildingToUpgrade,
    NoDevCards,
    AlreadyPlayedDevCard,
    InvalidPhase(String),
    EmptyDevDeck,
    InvalidRobberPlacement,
    InvalidStealTarget,
    InvalidDiscard(String),
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleError::NotYourTurn => write!(f, "Not your turn"),
            RuleError::MustRollFirst => write!(f, "Must roll dice first"),
            RuleError::AlreadyRolled => write!(f, "Already rolled this turn"),
            RuleError::InsufficientResources => write!(f, "Insufficient resources"),
            RuleError::NoPiecesLeft => write!(f, "No pieces of this type remaining"),
            RuleError::InvalidPlacement(s) => write!(f, "Invalid placement: {}", s),
            RuleError::NoBuildingToUpgrade => write!(f, "No settlement to upgrade"),
            RuleError::NoDevCards => write!(f, "No dev cards in hand"),
            RuleError::AlreadyPlayedDevCard => write!(f, "Already played a dev card this turn"),
            RuleError::InvalidPhase(s) => write!(f, "Invalid phase: {}", s),
            RuleError::EmptyDevDeck => write!(f, "Dev card deck is empty"),
            RuleError::InvalidRobberPlacement => write!(f, "Invalid robber placement"),
            RuleError::InvalidStealTarget => write!(f, "Invalid steal target"),
            RuleError::InvalidDiscard(s) => write!(f, "Invalid discard: {}", s),
        }
    }
}

// -- Building costs --

/// Cost to build a road: 1 Brick + 1 Wood.
const ROAD_COST: [(Resource, u32); 2] = [(Resource::Brick, 1), (Resource::Wood, 1)];
/// Cost to build a settlement: 1 Brick + 1 Wood + 1 Wheat + 1 Sheep.
const SETTLEMENT_COST: [(Resource, u32); 4] = [
    (Resource::Brick, 1),
    (Resource::Wood, 1),
    (Resource::Wheat, 1),
    (Resource::Sheep, 1),
];
/// Cost to build a city: 2 Wheat + 3 Ore.
const CITY_COST: [(Resource, u32); 2] = [(Resource::Wheat, 2), (Resource::Ore, 3)];
/// Cost to buy a development card: 1 Wheat + 1 Sheep + 1 Ore.
const DEV_CARD_COST: [(Resource, u32); 3] = [
    (Resource::Wheat, 1),
    (Resource::Sheep, 1),
    (Resource::Ore, 1),
];

// -- Placement validation --

/// Check the "distance rule": no settlement within one edge of another.
fn satisfies_distance_rule(state: &GameState, vertex: VertexCoord) -> bool {
    for adj in adjacent_vertices(vertex) {
        if state.buildings.contains_key(&adj) {
            return false;
        }
    }
    true
}

/// Check whether a vertex is on the board (adjacent to at least one board hex).
fn vertex_on_board(board: &Board, vertex: VertexCoord) -> bool {
    vertex_neighbors(vertex).iter().any(|h| board.has_hex(*h))
}

/// Check whether an edge is on the board (adjacent to at least one board hex).
fn edge_on_board(board: &Board, edge: EdgeCoord) -> bool {
    edge_neighbors(edge).iter().any(|h| board.has_hex(*h))
}

/// Valid settlement placements during normal play.
///
/// Requirements:
/// - Vertex is on the board.
/// - No existing building at this vertex.
/// - Satisfies the distance rule (no building within one edge).
/// - Connected to one of the player's existing roads.
pub fn legal_settlement_vertices(state: &GameState, player: PlayerId) -> Vec<VertexCoord> {
    let board = &state.board;
    let mut result = Vec::new();

    for hex in &board.hexes {
        for v in hex.coord.vertices() {
            if state.buildings.contains_key(&v) {
                continue;
            }
            if !satisfies_distance_rule(state, v) {
                continue;
            }
            // Must be connected to one of the player's roads.
            let has_road = adjacent_edges(v)
                .iter()
                .any(|e| state.roads.get(e) == Some(&player));
            if !has_road {
                continue;
            }
            if !result.contains(&v) {
                result.push(v);
            }
        }
    }
    result
}

/// Valid settlement placements during initial setup.
///
/// Same as normal but without the road connection requirement.
pub fn legal_setup_vertices(state: &GameState) -> Vec<VertexCoord> {
    let board = &state.board;
    let mut result = Vec::new();

    for hex in &board.hexes {
        for v in hex.coord.vertices() {
            if state.buildings.contains_key(&v) {
                continue;
            }
            if !satisfies_distance_rule(state, v) {
                continue;
            }
            if !result.contains(&v) {
                result.push(v);
            }
        }
    }
    result
}

/// Valid road placements adjacent to a given settlement vertex during setup.
pub fn legal_setup_roads(state: &GameState, settlement: VertexCoord) -> Vec<EdgeCoord> {
    adjacent_edges(settlement)
        .iter()
        .filter(|e| !state.roads.contains_key(e) && edge_on_board(&state.board, **e))
        .copied()
        .collect()
}

/// Valid road placements during normal play.
///
/// A road must:
/// - Be on the board.
/// - Not already have a road.
/// - Be adjacent to one of the player's existing roads or buildings.
pub fn legal_road_edges(state: &GameState, player: PlayerId) -> Vec<EdgeCoord> {
    let board = &state.board;
    let mut result = Vec::new();

    for hex in &board.hexes {
        for e in hex.coord.edges() {
            if state.roads.contains_key(&e) {
                continue;
            }
            if !edge_on_board(board, e) {
                continue;
            }
            // Check if connected to player's network.
            let (v1, v2) = edge_vertices(e);
            let connected = [v1, v2].iter().any(|v| {
                // Connected if the player has a building here...
                if let Some(b) = state.buildings.get(v) {
                    return matches!(b, Building::Settlement(p) | Building::City(p) if *p == player);
                }
                // ...or a road that shares this vertex.
                adjacent_edges(*v)
                    .iter()
                    .any(|adj_e| *adj_e != e && state.roads.get(adj_e) == Some(&player))
            });
            if connected && !result.contains(&e) {
                result.push(e);
            }
        }
    }
    result
}

/// Vertices where the player can upgrade a settlement to a city.
pub fn legal_city_vertices(state: &GameState, player: PlayerId) -> Vec<VertexCoord> {
    state
        .buildings
        .iter()
        .filter_map(|(v, b)| match b {
            Building::Settlement(p) if *p == player => Some(*v),
            _ => None,
        })
        .collect()
}

// -- Bank / port trading --

/// The exchange rate for a given resource, considering port access.
///
/// Returns 4 (default), 3 (generic port), or 2 (specific 2:1 port).
pub fn trade_rate(state: &GameState, player: PlayerId, resource: Resource) -> u32 {
    let mut rate = 4u32;

    for port in &state.board.ports {
        let (v1, v2) = &port.vertices;
        let player_at_port = [v1, v2].iter().any(|v| {
            matches!(state.buildings.get(v), Some(Building::Settlement(p) | Building::City(p)) if *p == player)
        });
        if player_at_port {
            match port.port_type {
                PortType::Generic => {
                    rate = rate.min(3);
                }
                PortType::Specific(r) if r == resource => {
                    rate = rate.min(2);
                }
                PortType::Specific(_) => {}
            }
        }
    }
    rate
}

// -- Longest Road (DFS) --

/// Compute the length of the longest contiguous road for a player.
///
/// Uses DFS from every road segment, tracking visited edges to avoid cycles.
pub fn longest_road_length(state: &GameState, player: PlayerId) -> u8 {
    let player_roads: Vec<EdgeCoord> = state
        .roads
        .iter()
        .filter(|(_, p)| **p == player)
        .map(|(e, _)| *e)
        .collect();

    if player_roads.is_empty() {
        return 0;
    }

    let player_road_set: HashSet<EdgeCoord> = player_roads.iter().copied().collect();
    let mut max_length = 0u8;

    for &start in &player_roads {
        let mut visited = HashSet::new();
        visited.insert(start);
        let length = dfs_road(state, player, &player_road_set, start, &mut visited);
        max_length = max_length.max(length);
    }

    max_length
}

fn dfs_road(
    state: &GameState,
    player: PlayerId,
    road_set: &HashSet<EdgeCoord>,
    edge: EdgeCoord,
    visited: &mut HashSet<EdgeCoord>,
) -> u8 {
    let mut max = visited.len() as u8;

    let (v1, v2) = edge_vertices(edge);
    for v in [v1, v2] {
        // Road is broken if another player's building sits on this vertex.
        if let Some(b) = state.buildings.get(&v) {
            match b {
                Building::Settlement(p) | Building::City(p) if *p != player => continue,
                _ => {}
            }
        }

        for next_edge in adjacent_edges(v) {
            if !visited.contains(&next_edge) && road_set.contains(&next_edge) {
                visited.insert(next_edge);
                let length = dfs_road(state, player, road_set, next_edge, visited);
                max = max.max(length);
                visited.remove(&next_edge);
            }
        }
    }

    max
}

/// Update longest road tracking.  Called after a road is built or a
/// settlement breaks a road chain.
///
/// Rules for longest road updates:
/// - If the current holder is beaten by another player, the other player takes it.
/// - If the current holder's road is broken and they tie with someone, they keep it.
/// - If no single player has a clear longest road of 5+, the card is set aside.
pub fn update_longest_road(state: &mut GameState) {
    // Compute all road lengths.
    let lengths: Vec<u8> = (0..state.num_players)
        .map(|p| longest_road_length(state, p))
        .collect();

    // Find the maximum length (must be >= 5).
    let max_len = *lengths.iter().max().unwrap_or(&0);
    if max_len < 5 {
        state.longest_road_player = None;
        state.longest_road_length = 0;
        return;
    }

    // Count how many players share the maximum length.
    let players_at_max: Vec<PlayerId> = (0..state.num_players)
        .filter(|&p| lengths[p] == max_len)
        .collect();

    if players_at_max.len() == 1 {
        // Clear winner.
        let winner = players_at_max[0];
        state.longest_road_player = Some(winner);
        state.longest_road_length = max_len;
    } else {
        // Tie: current holder keeps it if they're among the tied players.
        // Otherwise nobody holds it.
        if let Some(current) = state.longest_road_player {
            if players_at_max.contains(&current) {
                state.longest_road_length = max_len;
            } else {
                state.longest_road_player = None;
                state.longest_road_length = 0;
            }
        } else {
            // No current holder and a tie: nobody gets it.
            state.longest_road_player = None;
            state.longest_road_length = 0;
        }
    }
}

/// Update largest army tracking.  Called after a Knight is played.
pub fn update_largest_army(state: &mut GameState, player: PlayerId) {
    let knights = state.players[player].knights_played;
    if knights >= 3 {
        match state.largest_army_player {
            Some(current) if current == player => {
                state.largest_army_size = knights;
            }
            Some(current) => {
                if knights > state.players[current].knights_played {
                    state.largest_army_player = Some(player);
                    state.largest_army_size = knights;
                }
            }
            None => {
                state.largest_army_player = Some(player);
                state.largest_army_size = knights;
            }
        }
    }
}

// -- Legal actions --

/// Returns all legal actions for the current player in the Playing phase.
pub fn legal_actions(state: &GameState) -> Vec<Action> {
    let phase = match &state.phase {
        GamePhase::Playing {
            current_player,
            has_rolled,
        } => (*current_player, *has_rolled),
        _ => return vec![],
    };

    let (player, has_rolled) = phase;
    let ps = &state.players[player];
    let mut actions = Vec::new();

    if !has_rolled {
        // Before rolling, the only action available is to play a dev card
        // (Knight only, pre-roll).
        if !ps.has_played_dev_card_this_turn && ps.dev_cards.contains(&DevCard::Knight) {
            // We don't enumerate specific knight targets here; the player
            // chooses those when playing the card via the Player trait methods.
        }
        // The roll is implicit in the game loop.
        return actions;
    }

    // -- Build settlement --
    if ps.has_resources(&SETTLEMENT_COST) && ps.settlements_remaining > 0 {
        for v in legal_settlement_vertices(state, player) {
            actions.push(Action::BuildSettlement(v));
        }
    }

    // -- Build city --
    if ps.has_resources(&CITY_COST) && ps.cities_remaining > 0 {
        for v in legal_city_vertices(state, player) {
            actions.push(Action::BuildCity(v));
        }
    }

    // -- Build road --
    if ps.has_resources(&ROAD_COST) && ps.roads_remaining > 0 {
        for e in legal_road_edges(state, player) {
            actions.push(Action::BuildRoad(e));
        }
    }

    // -- Buy dev card --
    if ps.has_resources(&DEV_CARD_COST) && !state.dev_card_deck.is_empty() {
        actions.push(Action::BuyDevCard);
    }

    // Dev card plays are handled by the orchestrator's build_choices() method,
    // which adds PlayerChoice variants that trigger multi-step interactions.

    // -- Bank trade --
    for &give_res in Resource::all() {
        let rate = trade_rate(state, player, give_res);
        if ps.resource_count(give_res) >= rate {
            for &get_res in Resource::all() {
                if give_res != get_res {
                    let in_circulation = total_in_circulation(state, get_res);
                    let bank_has = BANK_SUPPLY_PER_RESOURCE.saturating_sub(in_circulation);
                    if bank_has > 0 {
                        actions.push(Action::BankTrade {
                            give: give_res,
                            get: get_res,
                        });
                    }
                }
            }
        }
    }

    // -- End turn --
    actions.push(Action::EndTurn);

    actions
}

// -- Apply actions --

/// Apply an action and mutate the game state.
///
/// Returns `Ok(())` if the action was successfully applied, or an error
/// describing why it was rejected.
pub fn apply_action(state: &mut GameState, action: &Action) -> Result<(), RuleError> {
    match action {
        Action::BuildSettlement(v) => apply_build_settlement(state, *v),
        Action::BuildCity(v) => apply_build_city(state, *v),
        Action::BuildRoad(e) => apply_build_road(state, *e),
        Action::BuyDevCard => apply_buy_dev_card(state),
        Action::PlayDevCard(card, dev_action) => {
            apply_play_dev_card(state, card.clone(), dev_action.clone())
        }
        Action::BankTrade { give, get } => apply_bank_trade(state, *give, *get),
        Action::EndTurn => apply_end_turn(state),
        Action::ProposeTrade => {
            // Trade proposal is handled by the game orchestrator, not here.
            Ok(())
        }
    }
}

fn current_player_playing(state: &GameState) -> Result<PlayerId, RuleError> {
    match &state.phase {
        GamePhase::Playing {
            current_player,
            has_rolled: true,
        } => Ok(*current_player),
        GamePhase::Playing {
            has_rolled: false, ..
        } => Err(RuleError::MustRollFirst),
        _ => Err(RuleError::InvalidPhase(format!("{:?}", state.phase))),
    }
}

fn apply_build_settlement(state: &mut GameState, vertex: VertexCoord) -> Result<(), RuleError> {
    let player = current_player_playing(state)?;
    let ps = &state.players[player];

    if !ps.has_resources(&SETTLEMENT_COST) {
        return Err(RuleError::InsufficientResources);
    }
    if ps.settlements_remaining == 0 {
        return Err(RuleError::NoPiecesLeft);
    }
    if state.buildings.contains_key(&vertex) {
        return Err(RuleError::InvalidPlacement(
            "Vertex already occupied".into(),
        ));
    }
    if !satisfies_distance_rule(state, vertex) {
        return Err(RuleError::InvalidPlacement(
            "Too close to another building".into(),
        ));
    }
    if !vertex_on_board(&state.board, vertex) {
        return Err(RuleError::InvalidPlacement("Vertex not on board".into()));
    }
    // Must be connected to a road.
    let has_road = adjacent_edges(vertex)
        .iter()
        .any(|e| state.roads.get(e) == Some(&player));
    if !has_road {
        return Err(RuleError::InvalidPlacement(
            "Not connected to your road network".into(),
        ));
    }

    // Deduct resources.
    for (r, c) in &SETTLEMENT_COST {
        state.players[player].remove_resource(*r, *c);
    }
    state.players[player].settlements_remaining -= 1;
    state.buildings.insert(vertex, Building::Settlement(player));

    // A new settlement might break someone's longest road.
    update_longest_road(state);
    check_victory_inline(state);

    Ok(())
}

fn apply_build_city(state: &mut GameState, vertex: VertexCoord) -> Result<(), RuleError> {
    let player = current_player_playing(state)?;
    let ps = &state.players[player];

    if !ps.has_resources(&CITY_COST) {
        return Err(RuleError::InsufficientResources);
    }
    if ps.cities_remaining == 0 {
        return Err(RuleError::NoPiecesLeft);
    }

    match state.buildings.get(&vertex) {
        Some(Building::Settlement(p)) if *p == player => {}
        _ => return Err(RuleError::NoBuildingToUpgrade),
    }

    for (r, c) in &CITY_COST {
        state.players[player].remove_resource(*r, *c);
    }
    state.players[player].cities_remaining -= 1;
    state.players[player].settlements_remaining += 1; // Settlement piece returned.
    state.buildings.insert(vertex, Building::City(player));

    check_victory_inline(state);
    Ok(())
}

fn apply_build_road(state: &mut GameState, edge: EdgeCoord) -> Result<(), RuleError> {
    let player = current_player_playing(state)?;
    let ps = &state.players[player];

    if !ps.has_resources(&ROAD_COST) {
        return Err(RuleError::InsufficientResources);
    }
    if ps.roads_remaining == 0 {
        return Err(RuleError::NoPiecesLeft);
    }
    if state.roads.contains_key(&edge) {
        return Err(RuleError::InvalidPlacement(
            "Edge already has a road".into(),
        ));
    }
    if !edge_on_board(&state.board, edge) {
        return Err(RuleError::InvalidPlacement("Edge not on board".into()));
    }

    // Must be connected to player's network.
    let (v1, v2) = edge_vertices(edge);
    let connected = [v1, v2].iter().any(|v| {
        if let Some(b) = state.buildings.get(v) {
            return matches!(b, Building::Settlement(p) | Building::City(p) if *p == player);
        }
        adjacent_edges(*v)
            .iter()
            .any(|adj_e| *adj_e != edge && state.roads.get(adj_e) == Some(&player))
    });
    if !connected {
        return Err(RuleError::InvalidPlacement(
            "Not connected to your network".into(),
        ));
    }

    for (r, c) in &ROAD_COST {
        state.players[player].remove_resource(*r, *c);
    }
    state.players[player].roads_remaining -= 1;
    state.roads.insert(edge, player);

    update_longest_road(state);
    check_victory_inline(state);
    Ok(())
}

fn apply_buy_dev_card(state: &mut GameState) -> Result<(), RuleError> {
    let player = current_player_playing(state)?;
    let ps = &state.players[player];

    if !ps.has_resources(&DEV_CARD_COST) {
        return Err(RuleError::InsufficientResources);
    }
    if state.dev_card_deck.is_empty() {
        return Err(RuleError::EmptyDevDeck);
    }

    for (r, c) in &DEV_CARD_COST {
        state.players[player].remove_resource(*r, *c);
    }
    let card = state.dev_card_deck.pop().unwrap();
    state.players[player].dev_cards.push(card);
    state.players[player].dev_cards_bought_this_turn += 1;

    // VP cards auto-reveal; check victory.
    check_victory_inline(state);
    Ok(())
}

fn apply_play_dev_card(
    state: &mut GameState,
    card: DevCard,
    action: DevCardAction,
) -> Result<(), RuleError> {
    let player = match &state.phase {
        GamePhase::Playing { current_player, .. } => *current_player,
        _ => return Err(RuleError::InvalidPhase("Not in Playing phase".into())),
    };

    let ps = &state.players[player];
    if ps.has_played_dev_card_this_turn {
        return Err(RuleError::AlreadyPlayedDevCard);
    }

    // Find the card in hand, but not one bought this turn.
    // Cards bought this turn are the last N entries in dev_cards.
    let playable_count = ps
        .dev_cards
        .len()
        .saturating_sub(ps.dev_cards_bought_this_turn);
    let idx = ps.dev_cards[..playable_count]
        .iter()
        .position(|c| *c == card)
        .ok_or(RuleError::NoDevCards)?;

    // Validate the action BEFORE consuming the card, so validation failures
    // don't eat the card.
    match &action {
        DevCardAction::Knight {
            robber_to,
            steal_from: _,
        } => {
            if *robber_to == state.robber_hex {
                return Err(RuleError::InvalidRobberPlacement);
            }
            if !state.board.has_hex(*robber_to) {
                return Err(RuleError::InvalidRobberPlacement);
            }
            if friendly_robber_blocks(state, *robber_to, player) {
                return Err(RuleError::InvalidRobberPlacement);
            }
        }
        DevCardAction::YearOfPlenty(r1, r2) => {
            // Bank must have the requested resources available.
            let mut needed: HashMap<Resource, u32> = HashMap::new();
            *needed.entry(*r1).or_insert(0) += 1;
            *needed.entry(*r2).or_insert(0) += 1;
            for (resource, count) in &needed {
                let in_circulation = total_in_circulation(state, *resource);
                let available = BANK_SUPPLY_PER_RESOURCE.saturating_sub(in_circulation);
                if *count > available {
                    return Err(RuleError::InsufficientResources);
                }
            }
        }
        DevCardAction::Monopoly(_) => {}
        DevCardAction::RoadBuilding(e1, e2) => {
            if state.roads.contains_key(e1) {
                return Err(RuleError::InvalidPlacement(
                    "First edge already has a road".into(),
                ));
            }
            if state.roads.contains_key(e2) {
                return Err(RuleError::InvalidPlacement(
                    "Second edge already has a road".into(),
                ));
            }
            if !edge_on_board(&state.board, *e1) {
                return Err(RuleError::InvalidPlacement(
                    "First edge not on board".into(),
                ));
            }
            if !edge_on_board(&state.board, *e2) {
                return Err(RuleError::InvalidPlacement(
                    "Second edge not on board".into(),
                ));
            }
            if state.players[player].roads_remaining < 2 {
                return Err(RuleError::NoPiecesLeft);
            }
            // First road must connect to player's existing network.
            let (v1, v2) = edge_vertices(*e1);
            let connected_1 = [v1, v2].iter().any(|v| {
                if let Some(b) = state.buildings.get(v) {
                    return matches!(b, Building::Settlement(p) | Building::City(p) if *p == player);
                }
                adjacent_edges(*v)
                    .iter()
                    .any(|adj_e| *adj_e != *e1 && state.roads.get(adj_e) == Some(&player))
            });
            if !connected_1 {
                return Err(RuleError::InvalidPlacement(
                    "First road not connected to your network".into(),
                ));
            }
            // Second road must connect to player's network INCLUDING the first road.
            let (v3, v4) = edge_vertices(*e2);
            let connected_2 = [v3, v4].iter().any(|v| {
                if let Some(b) = state.buildings.get(v) {
                    return matches!(b, Building::Settlement(p) | Building::City(p) if *p == player);
                }
                adjacent_edges(*v).iter().any(|adj_e| {
                    *adj_e != *e2 && (*adj_e == *e1 || state.roads.get(adj_e) == Some(&player))
                })
            });
            if !connected_2 {
                return Err(RuleError::InvalidPlacement(
                    "Second road not connected to your network".into(),
                ));
            }
        }
    }

    // Consume the card now that validation has passed.
    state.players[player].dev_cards.remove(idx);
    state.players[player].has_played_dev_card_this_turn = true;

    // Apply the action.
    match action {
        DevCardAction::Knight {
            robber_to,
            steal_from,
        } => {
            state.robber_hex = robber_to;
            state.players[player].knights_played += 1;
            update_largest_army(state, player);

            if let Some(target) = steal_from {
                steal_random_resource(state, player, target);
            }
        }
        DevCardAction::YearOfPlenty(r1, r2) => {
            state.players[player].add_resource(r1, 1);
            state.players[player].add_resource(r2, 1);
        }
        DevCardAction::Monopoly(resource) => {
            let mut total = 0u32;
            for p in 0..state.num_players {
                if p != player {
                    let count = state.players[p].resource_count(resource);
                    if count > 0 {
                        state.players[p].remove_resource(resource, count);
                        total += count;
                    }
                }
            }
            state.players[player].add_resource(resource, total);
        }
        DevCardAction::RoadBuilding(e1, e2) => {
            state.roads.insert(e1, player);
            state.roads.insert(e2, player);
            state.players[player].roads_remaining -= 2;
            update_longest_road(state);
        }
    }

    check_victory_inline(state);
    Ok(())
}

fn apply_bank_trade(state: &mut GameState, give: Resource, get: Resource) -> Result<(), RuleError> {
    let player = current_player_playing(state)?;
    let rate = trade_rate(state, player, give);

    if state.players[player].resource_count(give) < rate {
        return Err(RuleError::InsufficientResources);
    }

    // Bank must have the requested resource available.
    let in_circulation = total_in_circulation(state, get);
    let bank_has = BANK_SUPPLY_PER_RESOURCE.saturating_sub(in_circulation);
    if bank_has == 0 {
        return Err(RuleError::InsufficientResources);
    }

    state.players[player].remove_resource(give, rate);
    state.players[player].add_resource(get, 1);
    Ok(())
}

fn apply_end_turn(state: &mut GameState) -> Result<(), RuleError> {
    let player = current_player_playing(state)?;
    state.players[player].has_played_dev_card_this_turn = false;
    state.players[player].dev_cards_bought_this_turn = 0;

    let next_player = (player + 1) % state.num_players;
    state.phase = GamePhase::Playing {
        current_player: next_player,
        has_rolled: false,
    };
    state.turn_number += 1;

    Ok(())
}

// -- Setup phase --

/// Place an initial settlement during setup.
pub fn apply_setup_settlement(state: &mut GameState, vertex: VertexCoord) -> Result<(), RuleError> {
    let player = match &state.phase {
        GamePhase::Setup { player_index, .. } => state.setup_order[*player_index],
        _ => return Err(RuleError::InvalidPhase("Not in Setup phase".into())),
    };

    if state.buildings.contains_key(&vertex) {
        return Err(RuleError::InvalidPlacement(
            "Vertex already occupied".into(),
        ));
    }
    if !satisfies_distance_rule(state, vertex) {
        return Err(RuleError::InvalidPlacement(
            "Too close to another building".into(),
        ));
    }
    if !vertex_on_board(&state.board, vertex) {
        return Err(RuleError::InvalidPlacement("Vertex not on board".into()));
    }

    state.buildings.insert(vertex, Building::Settlement(player));
    state.players[player].settlements_remaining -= 1;

    Ok(())
}

/// Place an initial road during setup (must be adjacent to the just-placed settlement).
pub fn apply_setup_road(
    state: &mut GameState,
    settlement: VertexCoord,
    edge: EdgeCoord,
) -> Result<(), RuleError> {
    let (player, round, player_index) = match &state.phase {
        GamePhase::Setup {
            round,
            player_index,
        } => (state.setup_order[*player_index], *round, *player_index),
        _ => return Err(RuleError::InvalidPhase("Not in Setup phase".into())),
    };

    if state.roads.contains_key(&edge) {
        return Err(RuleError::InvalidPlacement(
            "Edge already has a road".into(),
        ));
    }
    if !edge_on_board(&state.board, edge) {
        return Err(RuleError::InvalidPlacement("Edge not on board".into()));
    }
    // Must be adjacent to the settlement just placed.
    if !adjacent_edges(settlement).contains(&edge) {
        return Err(RuleError::InvalidPlacement(
            "Road must be adjacent to settlement just placed".into(),
        ));
    }

    state.roads.insert(edge, player);
    state.players[player].roads_remaining -= 1;

    // Second round: grant one of each adjacent resource.
    if round == 2 {
        for hex_coord in vertex_neighbors(settlement) {
            if let Some(hex) = state.board.hexes.iter().find(|h| h.coord == hex_coord) {
                if let Some(resource) = hex.terrain.resource() {
                    state.players[player].add_resource(resource, 1);
                }
            }
        }
    }

    // Advance to next player in setup order.
    let next_index = player_index + 1;
    if next_index >= state.setup_order.len() {
        // Setup is complete — start the game.
        state.phase = GamePhase::Playing {
            current_player: 0,
            has_rolled: false,
        };
    } else {
        state.phase = GamePhase::Setup {
            round: if next_index >= state.num_players {
                2
            } else {
                1
            },
            player_index: next_index,
        };
    }

    Ok(())
}

// -- Robber / discard --

/// Check whether the friendly robber rule forbids placing on this hex.
///
/// Returns true if the hex is blocked: all adjacent players (other than the
/// placing player) have 2 or fewer VP.  An empty hex is never blocked.
fn friendly_robber_blocks(state: &GameState, hex: HexCoord, player: PlayerId) -> bool {
    if !state.friendly_robber {
        return false;
    }
    let mut found_any = false;
    for v in hex.vertices() {
        if let Some(b) = state.buildings.get(&v) {
            let owner = match b {
                Building::Settlement(p) | Building::City(p) => *p,
            };
            if owner == player {
                continue;
            }
            found_any = true;
            if state.victory_points(owner) > 2 {
                return false;
            }
        }
    }
    found_any
}

/// Return the hexes where `player` may legally place the robber.
///
/// Excludes the current robber position and any hex blocked by the
/// friendly robber rule.
pub fn legal_robber_hexes(state: &GameState, player: PlayerId) -> Vec<HexCoord> {
    board_hex_coords()
        .into_iter()
        .filter(|&h| h != state.robber_hex && !friendly_robber_blocks(state, h, player))
        .collect()
}

/// Move the robber to a new hex.
pub fn apply_move_robber(state: &mut GameState, hex: HexCoord) -> Result<(), RuleError> {
    let player = match &state.phase {
        GamePhase::PlacingRobber { current_player } => *current_player,
        _ => return Err(RuleError::InvalidPhase("Not in PlacingRobber phase".into())),
    };

    if hex == state.robber_hex {
        return Err(RuleError::InvalidRobberPlacement);
    }
    if !state.board.has_hex(hex) {
        return Err(RuleError::InvalidRobberPlacement);
    }
    if friendly_robber_blocks(state, hex, player) {
        return Err(RuleError::InvalidRobberPlacement);
    }

    state.robber_hex = hex;

    // Find players with buildings adjacent to the new robber hex.
    let targets = steal_targets(state, hex, player);
    if targets.is_empty() {
        // No one to steal from — go back to Playing.
        state.phase = GamePhase::Playing {
            current_player: player,
            has_rolled: true,
        };
    } else {
        state.phase = GamePhase::Stealing {
            current_player: player,
            target_hex: hex,
        };
    }

    Ok(())
}

/// Players adjacent to the robber hex that can be stolen from (not self).
pub fn steal_targets(state: &GameState, hex: HexCoord, player: PlayerId) -> Vec<PlayerId> {
    let mut targets = Vec::new();
    for v in hex.vertices() {
        if let Some(b) = state.buildings.get(&v) {
            let owner = match b {
                Building::Settlement(p) | Building::City(p) => *p,
            };
            if owner != player
                && !targets.contains(&owner)
                && state.players[owner].total_resources() > 0
            {
                targets.push(owner);
            }
        }
    }
    targets
}

/// Steal a random resource from the target player.
pub fn steal_random_resource(state: &mut GameState, thief: PlayerId, victim: PlayerId) {
    let total = state.players[victim].total_resources();
    if total == 0 {
        return;
    }

    // Build a list of all resources the victim has.
    let mut pool: Vec<Resource> = Vec::new();
    for &r in Resource::all() {
        let count = state.players[victim].resource_count(r);
        for _ in 0..count {
            pool.push(r);
        }
    }

    // Pick one at random.
    use rand::seq::IndexedRandom;
    let mut rng = rand::rng();
    if let Some(&stolen) = pool.choose(&mut rng) {
        state.players[victim].remove_resource(stolen, 1);
        state.players[thief].add_resource(stolen, 1);
    }
}

/// Apply a steal action.
pub fn apply_steal(state: &mut GameState, target: PlayerId) -> Result<(), RuleError> {
    let player = match &state.phase {
        GamePhase::Stealing {
            current_player,
            target_hex,
        } => {
            let targets = steal_targets(state, *target_hex, *current_player);
            if !targets.contains(&target) {
                return Err(RuleError::InvalidStealTarget);
            }
            *current_player
        }
        _ => return Err(RuleError::InvalidPhase("Not in Stealing phase".into())),
    };

    steal_random_resource(state, player, target);

    state.phase = GamePhase::Playing {
        current_player: player,
        has_rolled: true,
    };

    Ok(())
}

/// Discard cards when a 7 is rolled and a player has more than 7 cards.
pub fn apply_discard(
    state: &mut GameState,
    player: PlayerId,
    cards: &[Resource],
) -> Result<(), RuleError> {
    match &state.phase {
        GamePhase::Discarding {
            players_needing_discard,
            ..
        } => {
            if !players_needing_discard.contains(&player) {
                return Err(RuleError::InvalidDiscard(
                    "Player does not need to discard".into(),
                ));
            }
        }
        _ => return Err(RuleError::InvalidPhase("Not in Discarding phase".into())),
    }

    let total = state.players[player].total_resources();
    let expected = total / 2;
    if cards.len() as u32 != expected {
        return Err(RuleError::InvalidDiscard(format!(
            "Must discard exactly {} cards (have {}), got {}",
            expected,
            total,
            cards.len()
        )));
    }

    // Validate the player actually has these cards.
    let mut counts: HashMap<Resource, u32> = HashMap::new();
    for &r in cards {
        *counts.entry(r).or_insert(0) += 1;
    }
    for (&r, &count) in &counts {
        if state.players[player].resource_count(r) < count {
            return Err(RuleError::InvalidDiscard(format!(
                "Don't have {} {} to discard",
                count, r
            )));
        }
    }

    // Remove the cards.
    for (&r, &count) in &counts {
        state.players[player].remove_resource(r, count);
    }

    // Remove this player from the discard list.
    if let GamePhase::Discarding {
        current_player,
        players_needing_discard,
    } = &mut state.phase
    {
        players_needing_discard.retain(|p| *p != player);
        if players_needing_discard.is_empty() {
            let cp = *current_player;
            state.phase = GamePhase::PlacingRobber { current_player: cp };
        }
    }

    Ok(())
}

// -- Victory check --

/// Check if the current player has won (10+ VP on their own turn).
///
/// A player can only win during their own turn.
/// If you reach 10 VP during another player's turn (e.g. from Longest Road
/// shifting), you must wait until your own turn to claim victory.
pub fn check_victory(state: &GameState) -> Option<PlayerId> {
    let current = match &state.phase {
        GamePhase::Playing { current_player, .. } => *current_player,
        _ => return None,
    };
    if state.victory_points(current) >= VP_TO_WIN {
        Some(current)
    } else {
        None
    }
}

/// Check victory and update the phase if someone won.
fn check_victory_inline(state: &mut GameState) {
    if let Some(winner) = check_victory(state) {
        state.phase = GamePhase::GameOver { winner };
    }
}

// -- Tests --

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::{Board, EdgeDirection, VertexDirection};

    fn make_state(num_players: usize) -> GameState {
        GameState::new(Board::default_board(), num_players)
    }

    /// Helper: put the game into Playing phase with has_rolled = true.
    fn set_playing(state: &mut GameState, player: PlayerId) {
        state.phase = GamePhase::Playing {
            current_player: player,
            has_rolled: true,
        };
    }

    /// Helper: give a player resources.
    fn give_resources(state: &mut GameState, player: PlayerId, resources: &[(Resource, u32)]) {
        for &(r, c) in resources {
            state.players[player].add_resource(r, c);
        }
    }

    /// Helper: place a settlement directly (bypassing rules).
    fn place_settlement(state: &mut GameState, player: PlayerId, vertex: VertexCoord) {
        state.buildings.insert(vertex, Building::Settlement(player));
    }

    /// Helper: place a road directly.
    fn place_road(state: &mut GameState, player: PlayerId, edge: EdgeCoord) {
        state.roads.insert(edge, player);
    }

    // -- Distance rule --

    #[test]
    fn distance_rule_allows_isolated_vertex() {
        let state = make_state(4);
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        assert!(satisfies_distance_rule(&state, v));
    }

    #[test]
    fn distance_rule_blocks_adjacent_vertex() {
        let mut state = make_state(4);
        let v1 = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        place_settlement(&mut state, 0, v1);

        // All three adjacent vertices should be blocked.
        for adj in adjacent_vertices(v1) {
            assert!(
                !satisfies_distance_rule(&state, adj),
                "Vertex {:?} should be blocked",
                adj
            );
        }
    }

    // -- Setup placement --

    #[test]
    fn setup_settlement_placement() {
        let mut state = make_state(4);
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        assert!(apply_setup_settlement(&mut state, v).is_ok());
        assert_eq!(state.buildings.get(&v), Some(&Building::Settlement(0)));
    }

    #[test]
    fn setup_rejects_duplicate_vertex() {
        let mut state = make_state(4);
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        apply_setup_settlement(&mut state, v).unwrap();
        assert!(apply_setup_settlement(&mut state, v).is_err());
    }

    #[test]
    fn setup_road_advances_player() {
        let mut state = make_state(4);
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        apply_setup_settlement(&mut state, v).unwrap();

        let edges = legal_setup_roads(&state, v);
        assert!(!edges.is_empty());
        apply_setup_road(&mut state, v, edges[0]).unwrap();

        // Should advance to player 1.
        match &state.phase {
            GamePhase::Setup {
                player_index,
                round,
            } => {
                assert_eq!(*player_index, 1);
                assert_eq!(*round, 1);
            }
            p => panic!("Expected Setup phase, got {:?}", p),
        }
    }

    #[test]
    fn setup_second_round_grants_resources() {
        let mut state = make_state(2);
        // Setup order for 2 players: [0, 1, 1, 0]
        assert_eq!(state.setup_order, vec![0, 1, 1, 0]);

        // Round 1: player 0
        let v0 = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        apply_setup_settlement(&mut state, v0).unwrap();
        let e0 = legal_setup_roads(&state, v0)[0];
        apply_setup_road(&mut state, v0, e0).unwrap();
        assert_eq!(
            state.players[0].total_resources(),
            0,
            "Round 1 shouldn't grant resources"
        );

        // Round 1: player 1
        let v1 = VertexCoord::new(HexCoord::new(2, -2), VertexDirection::North);
        apply_setup_settlement(&mut state, v1).unwrap();
        let e1 = legal_setup_roads(&state, v1)[0];
        apply_setup_road(&mut state, v1, e1).unwrap();
        assert_eq!(state.players[1].total_resources(), 0);

        // Round 2: player 1
        let v1b = VertexCoord::new(HexCoord::new(-2, 2), VertexDirection::South);
        apply_setup_settlement(&mut state, v1b).unwrap();
        let e1b = legal_setup_roads(&state, v1b)[0];
        apply_setup_road(&mut state, v1b, e1b).unwrap();
        // Second round grants resources based on adjacent hexes.
        assert!(
            state.players[1].total_resources() > 0,
            "Round 2 should grant resources"
        );

        // Round 2: player 0 — should transition to Playing.
        let v0b = VertexCoord::new(HexCoord::new(-2, 0), VertexDirection::North);
        apply_setup_settlement(&mut state, v0b).unwrap();
        let e0b = legal_setup_roads(&state, v0b)[0];
        apply_setup_road(&mut state, v0b, e0b).unwrap();
        assert!(state.players[0].total_resources() > 0);

        // Should now be in Playing phase.
        match &state.phase {
            GamePhase::Playing {
                current_player,
                has_rolled,
            } => {
                assert_eq!(*current_player, 0);
                assert!(!has_rolled);
            }
            p => panic!("Expected Playing phase, got {:?}", p),
        }
    }

    // -- Build settlement (normal play) --

    #[test]
    fn build_settlement_deducts_resources() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        give_resources(&mut state, 0, &SETTLEMENT_COST);

        // Place a road first to connect.
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        let e = adjacent_edges(v)[0];
        place_road(&mut state, 0, e);

        apply_build_settlement(&mut state, v).unwrap();
        assert_eq!(state.players[0].total_resources(), 0);
        assert_eq!(state.buildings.get(&v), Some(&Building::Settlement(0)));
    }

    #[test]
    fn build_settlement_requires_road_connection() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        give_resources(&mut state, 0, &SETTLEMENT_COST);

        // No road placed — should fail.
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        assert!(apply_build_settlement(&mut state, v).is_err());
    }

    // -- Build city --

    #[test]
    fn build_city_upgrades_settlement() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);

        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        place_settlement(&mut state, 0, v);
        state.players[0].settlements_remaining -= 1;

        give_resources(&mut state, 0, &CITY_COST);
        apply_build_city(&mut state, v).unwrap();

        assert_eq!(state.buildings.get(&v), Some(&Building::City(0)));
        assert_eq!(state.players[0].total_resources(), 0);
        // Settlement piece is returned.
        assert_eq!(state.players[0].settlements_remaining, 5);
    }

    #[test]
    fn build_city_requires_own_settlement() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);

        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        place_settlement(&mut state, 1, v); // Player 1's settlement

        give_resources(&mut state, 0, &CITY_COST);
        assert!(apply_build_city(&mut state, v).is_err());
    }

    // -- Build road --

    #[test]
    fn build_road_deducts_resources() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);

        // Place a settlement for connectivity.
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        place_settlement(&mut state, 0, v);

        give_resources(&mut state, 0, &ROAD_COST);
        let e = adjacent_edges(v)[0];
        apply_build_road(&mut state, e).unwrap();

        assert_eq!(state.players[0].total_resources(), 0);
        assert_eq!(state.roads.get(&e), Some(&0));
    }

    // -- Buy dev card --

    #[test]
    fn buy_dev_card_works() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        give_resources(&mut state, 0, &DEV_CARD_COST);

        let deck_size = state.dev_card_deck.len();
        apply_buy_dev_card(&mut state).unwrap();

        assert_eq!(state.dev_card_deck.len(), deck_size - 1);
        assert_eq!(state.players[0].dev_cards.len(), 1);
        assert_eq!(state.players[0].total_resources(), 0);
    }

    #[test]
    fn buy_dev_card_rejects_empty_deck() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.dev_card_deck.clear();
        give_resources(&mut state, 0, &DEV_CARD_COST);
        assert_eq!(apply_buy_dev_card(&mut state), Err(RuleError::EmptyDevDeck));
    }

    // -- Dev cards --

    #[test]
    fn play_year_of_plenty() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::YearOfPlenty);

        apply_play_dev_card(
            &mut state,
            DevCard::YearOfPlenty,
            DevCardAction::YearOfPlenty(Resource::Ore, Resource::Wheat),
        )
        .unwrap();

        assert_eq!(state.players[0].resource_count(Resource::Ore), 1);
        assert_eq!(state.players[0].resource_count(Resource::Wheat), 1);
        assert!(state.players[0].has_played_dev_card_this_turn);
    }

    #[test]
    fn play_monopoly() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::Monopoly);

        // Give other players some ore.
        state.players[1].add_resource(Resource::Ore, 3);
        state.players[2].add_resource(Resource::Ore, 2);
        state.players[3].add_resource(Resource::Ore, 0);

        apply_play_dev_card(
            &mut state,
            DevCard::Monopoly,
            DevCardAction::Monopoly(Resource::Ore),
        )
        .unwrap();

        assert_eq!(state.players[0].resource_count(Resource::Ore), 5);
        assert_eq!(state.players[1].resource_count(Resource::Ore), 0);
        assert_eq!(state.players[2].resource_count(Resource::Ore), 0);
    }

    #[test]
    fn play_knight_moves_robber() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::Knight);

        let new_hex = HexCoord::new(1, 0);
        assert_ne!(state.robber_hex, new_hex);

        apply_play_dev_card(
            &mut state,
            DevCard::Knight,
            DevCardAction::Knight {
                robber_to: new_hex,
                steal_from: None,
            },
        )
        .unwrap();

        assert_eq!(state.robber_hex, new_hex);
        assert_eq!(state.players[0].knights_played, 1);
    }

    #[test]
    fn cannot_play_two_dev_cards_per_turn() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::YearOfPlenty);
        state.players[0].dev_cards.push(DevCard::Monopoly);

        apply_play_dev_card(
            &mut state,
            DevCard::YearOfPlenty,
            DevCardAction::YearOfPlenty(Resource::Ore, Resource::Ore),
        )
        .unwrap();

        let result = apply_play_dev_card(
            &mut state,
            DevCard::Monopoly,
            DevCardAction::Monopoly(Resource::Ore),
        );
        assert_eq!(result, Err(RuleError::AlreadyPlayedDevCard));
    }

    #[test]
    fn cannot_play_dev_card_bought_this_turn() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);

        // Buy a dev card (push a known card and mark it as bought this turn).
        state.players[0].dev_cards.push(DevCard::YearOfPlenty);
        state.players[0].dev_cards_bought_this_turn = 1;

        // Try to play it -- should fail because it was bought this turn.
        let result = apply_play_dev_card(
            &mut state,
            DevCard::YearOfPlenty,
            DevCardAction::YearOfPlenty(Resource::Ore, Resource::Ore),
        );
        assert_eq!(result, Err(RuleError::NoDevCards));
    }

    #[test]
    fn can_play_dev_card_from_previous_turn() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);

        // Card from a previous turn (not counted in bought_this_turn).
        state.players[0].dev_cards.push(DevCard::YearOfPlenty);
        // Buy a new card this turn (it goes at the end).
        state.players[0].dev_cards.push(DevCard::Monopoly);
        state.players[0].dev_cards_bought_this_turn = 1;

        // Playing the old card (index 0) should succeed.
        apply_play_dev_card(
            &mut state,
            DevCard::YearOfPlenty,
            DevCardAction::YearOfPlenty(Resource::Ore, Resource::Ore),
        )
        .unwrap();
    }

    // -- Bank trade --

    #[test]
    fn bank_trade_4_to_1() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        give_resources(&mut state, 0, &[(Resource::Brick, 4)]);

        apply_bank_trade(&mut state, Resource::Brick, Resource::Ore).unwrap();
        assert_eq!(state.players[0].resource_count(Resource::Brick), 0);
        assert_eq!(state.players[0].resource_count(Resource::Ore), 1);
    }

    #[test]
    fn bank_trade_insufficient_resources() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        give_resources(&mut state, 0, &[(Resource::Brick, 3)]);

        assert!(apply_bank_trade(&mut state, Resource::Brick, Resource::Ore).is_err());
    }

    #[test]
    fn bank_trade_rejected_when_bank_empty() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        // Player 0 has 4 Brick to trade.
        give_resources(&mut state, 0, &[(Resource::Brick, 4)]);
        // Distribute all 19 Ore among other players so the bank has none.
        give_resources(&mut state, 1, &[(Resource::Ore, 10)]);
        give_resources(&mut state, 2, &[(Resource::Ore, 9)]);

        assert_eq!(
            apply_bank_trade(&mut state, Resource::Brick, Resource::Ore),
            Err(RuleError::InsufficientResources),
        );
        // Player's Brick should be untouched.
        assert_eq!(state.players[0].resource_count(Resource::Brick), 4);
    }

    #[test]
    fn bank_trade_excluded_from_legal_actions_when_bank_empty() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        give_resources(&mut state, 0, &[(Resource::Brick, 4)]);
        // Exhaust all Ore in the bank.
        give_resources(&mut state, 1, &[(Resource::Ore, 10)]);
        give_resources(&mut state, 2, &[(Resource::Ore, 9)]);

        let actions = legal_actions(&state);
        let ore_trade = actions.iter().any(|a| {
            matches!(
                a,
                Action::BankTrade {
                    get: Resource::Ore,
                    ..
                }
            )
        });
        assert!(
            !ore_trade,
            "Should not offer bank trade for depleted resource"
        );
    }

    // -- End turn --

    #[test]
    fn end_turn_advances_player() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);

        apply_end_turn(&mut state).unwrap();

        match &state.phase {
            GamePhase::Playing {
                current_player,
                has_rolled,
            } => {
                assert_eq!(*current_player, 1);
                assert!(!has_rolled);
            }
            _ => panic!("Expected Playing phase"),
        }
    }

    #[test]
    fn end_turn_wraps_around() {
        let mut state = make_state(4);
        set_playing(&mut state, 3);

        apply_end_turn(&mut state).unwrap();

        match &state.phase {
            GamePhase::Playing { current_player, .. } => {
                assert_eq!(*current_player, 0);
            }
            _ => panic!("Expected Playing phase"),
        }
    }

    #[test]
    fn end_turn_resets_dev_card_flag() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.players[0].has_played_dev_card_this_turn = true;

        apply_end_turn(&mut state).unwrap();
        assert!(!state.players[0].has_played_dev_card_this_turn);
    }

    // -- Longest road --

    #[test]
    fn longest_road_empty_is_zero() {
        let state = make_state(4);
        assert_eq!(longest_road_length(&state, 0), 0);
    }

    #[test]
    fn longest_road_single_road() {
        let mut state = make_state(4);
        let e = EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::NorthEast);
        place_road(&mut state, 0, e);
        assert_eq!(longest_road_length(&state, 0), 1);
    }

    #[test]
    fn longest_road_chain_of_three() {
        let mut state = make_state(4);
        // Build a chain: (0,0,NE) -> (0,0,E) -> (0,0,SE)
        // These share vertices and form a connected path.
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::East),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::SouthEast),
        );

        assert_eq!(longest_road_length(&state, 0), 3);
    }

    #[test]
    fn longest_road_broken_by_opponent_settlement() {
        let mut state = make_state(4);

        // Build a chain of 3 roads for player 0.
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::East),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::SouthEast),
        );

        // Edge NE(0,0) has vertices: North(0,0) and South(1,-1)
        // Edge E(0,0) has vertices: South(1,-1) and North(0,1)
        // They share vertex South(1,-1).
        // Place opponent's settlement at South(1,-1) to break the chain.
        place_settlement(
            &mut state,
            1,
            VertexCoord::new(HexCoord::new(1, -1), VertexDirection::South),
        );

        // Now the chain is broken at South(1,-1).
        assert!(longest_road_length(&state, 0) < 3);
    }

    #[test]
    fn longest_road_tie_nobody_holds() {
        let mut state = make_state(4);

        // Player 0: 5-road chain NE(0,0)->E(0,0)->NE(0,1)->SE(1,0)->NE(1,1)
        // NE(0,0): N(0,0)-S(1,-1) -> E(0,0): S(1,-1)-N(0,1) -> NE(0,1): N(0,1)-S(1,0)
        // -> SE(1,0): S(1,0)-N(1,1) -> NE(1,1): N(1,1)-S(2,0)
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::East),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 1), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(1, 0), EdgeDirection::SouthEast),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(1, 1), EdgeDirection::NorthEast),
        );
        assert_eq!(longest_road_length(&state, 0), 5);

        // Player 1: 5-road chain on the opposite side of the board.
        // NE(-2,1)->E(-2,1)->NE(-2,2)->SE(-1,1)->NE(-1,2)
        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-2, 1), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-2, 1), EdgeDirection::East),
        );
        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-2, 2), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-1, 1), EdgeDirection::SouthEast),
        );
        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-1, 2), EdgeDirection::NorthEast),
        );
        assert_eq!(longest_road_length(&state, 1), 5);

        // With no current holder and a tie, nobody gets it.
        update_longest_road(&mut state);
        assert_eq!(
            state.longest_road_player, None,
            "Tie with no holder means nobody gets it"
        );
    }

    #[test]
    fn longest_road_holder_keeps_on_tie() {
        let mut state = make_state(4);
        // Player 0 is the current holder.
        state.longest_road_player = Some(0);
        state.longest_road_length = 5;

        // Same road layout as above.
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::East),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(0, 1), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(1, 0), EdgeDirection::SouthEast),
        );
        place_road(
            &mut state,
            0,
            EdgeCoord::new(HexCoord::new(1, 1), EdgeDirection::NorthEast),
        );

        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-2, 1), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-2, 1), EdgeDirection::East),
        );
        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-2, 2), EdgeDirection::NorthEast),
        );
        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-1, 1), EdgeDirection::SouthEast),
        );
        place_road(
            &mut state,
            1,
            EdgeCoord::new(HexCoord::new(-1, 2), EdgeDirection::NorthEast),
        );

        // Current holder keeps it in a tie.
        update_longest_road(&mut state);
        assert_eq!(
            state.longest_road_player,
            Some(0),
            "Current holder keeps it in a tie"
        );
    }

    // -- Largest army --

    #[test]
    fn largest_army_requires_three_knights() {
        let mut state = make_state(4);
        state.players[0].knights_played = 2;
        update_largest_army(&mut state, 0);
        assert_eq!(state.largest_army_player, None);

        state.players[0].knights_played = 3;
        update_largest_army(&mut state, 0);
        assert_eq!(state.largest_army_player, Some(0));
        assert_eq!(state.largest_army_size, 3);
    }

    #[test]
    fn largest_army_requires_beating_current() {
        let mut state = make_state(4);
        state.players[0].knights_played = 3;
        update_largest_army(&mut state, 0);

        state.players[1].knights_played = 3;
        update_largest_army(&mut state, 1);
        // Player 1 doesn't take it — must be strictly more.
        assert_eq!(state.largest_army_player, Some(0));

        state.players[1].knights_played = 4;
        update_largest_army(&mut state, 1);
        assert_eq!(state.largest_army_player, Some(1));
    }

    // -- Victory --

    #[test]
    fn victory_at_ten_points() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);

        // Give player 0: 5 settlements (5 VP) + longest road (2) + largest army (2) + 1 VP card = 10
        for i in 0..5 {
            let v = VertexCoord::new(HexCoord::new(i * 3, 0), VertexDirection::North);
            place_settlement(&mut state, 0, v);
        }
        state.longest_road_player = Some(0);
        state.largest_army_player = Some(0);
        state.players[0].dev_cards.push(DevCard::VictoryPoint);

        assert_eq!(state.victory_points(0), 10);
        assert_eq!(check_victory(&state), Some(0));
    }

    #[test]
    fn victory_not_on_other_players_turn() {
        let mut state = make_state(4);
        // It's player 1's turn, but player 0 has 10 VP.
        set_playing(&mut state, 1);

        for i in 0..5 {
            let v = VertexCoord::new(HexCoord::new(i * 3, 0), VertexDirection::North);
            place_settlement(&mut state, 0, v);
        }
        state.longest_road_player = Some(0);
        state.largest_army_player = Some(0);
        state.players[0].dev_cards.push(DevCard::VictoryPoint);

        assert_eq!(state.victory_points(0), 10);
        // Player 0 has 10 VP but it's player 1's turn -- no winner yet.
        assert_eq!(check_victory(&state), None);
    }

    #[test]
    fn no_victory_at_nine_points() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);

        for i in 0..5 {
            let v = VertexCoord::new(HexCoord::new(i * 3, 0), VertexDirection::North);
            place_settlement(&mut state, 0, v);
        }
        state.longest_road_player = Some(0);
        state.largest_army_player = Some(0);

        assert_eq!(state.victory_points(0), 9);
        assert_eq!(check_victory(&state), None);
    }

    // -- Discard --

    #[test]
    fn discard_removes_correct_cards() {
        let mut state = make_state(4);
        state.phase = GamePhase::Discarding {
            current_player: 0,
            players_needing_discard: vec![0],
        };
        // Give player 0 eight cards — must discard 4.
        give_resources(&mut state, 0, &[(Resource::Brick, 4), (Resource::Ore, 4)]);

        apply_discard(
            &mut state,
            0,
            &[
                Resource::Brick,
                Resource::Brick,
                Resource::Ore,
                Resource::Ore,
            ],
        )
        .unwrap();

        assert_eq!(state.players[0].resource_count(Resource::Brick), 2);
        assert_eq!(state.players[0].resource_count(Resource::Ore), 2);
        assert_eq!(state.players[0].total_resources(), 4);
    }

    #[test]
    fn discard_wrong_count_fails() {
        let mut state = make_state(4);
        state.phase = GamePhase::Discarding {
            current_player: 0,
            players_needing_discard: vec![0],
        };
        give_resources(&mut state, 0, &[(Resource::Brick, 8)]);

        // Must discard 4, but we try 3.
        let result = apply_discard(
            &mut state,
            0,
            &[Resource::Brick, Resource::Brick, Resource::Brick],
        );
        assert!(result.is_err());
    }

    #[test]
    fn discard_transitions_to_placing_robber() {
        let mut state = make_state(4);
        state.phase = GamePhase::Discarding {
            current_player: 0,
            players_needing_discard: vec![1],
        };
        give_resources(&mut state, 1, &[(Resource::Brick, 8)]);

        apply_discard(&mut state, 1, &[Resource::Brick; 4]).unwrap();

        match &state.phase {
            GamePhase::PlacingRobber { current_player } => {
                assert_eq!(*current_player, 0);
            }
            p => panic!("Expected PlacingRobber, got {:?}", p),
        }
    }

    // -- Robber --

    #[test]
    fn move_robber_rejects_same_hex() {
        let mut state = make_state(4);
        let current_robber = state.robber_hex;
        state.phase = GamePhase::PlacingRobber { current_player: 0 };

        assert_eq!(
            apply_move_robber(&mut state, current_robber),
            Err(RuleError::InvalidRobberPlacement)
        );
    }

    #[test]
    fn move_robber_to_valid_hex() {
        let mut state = make_state(4);
        state.phase = GamePhase::PlacingRobber { current_player: 0 };
        let target = HexCoord::new(1, 0);

        apply_move_robber(&mut state, target).unwrap();
        assert_eq!(state.robber_hex, target);

        // No buildings adjacent -> should go straight to Playing.
        match &state.phase {
            GamePhase::Playing {
                current_player,
                has_rolled,
            } => {
                assert_eq!(*current_player, 0);
                assert!(*has_rolled);
            }
            p => panic!("Expected Playing, got {:?}", p),
        }
    }

    // -- Friendly Robber --

    fn make_friendly_robber_state(num_players: usize) -> GameState {
        let mut state = GameState::new(Board::default_board(), num_players);
        state.friendly_robber = true;
        state
    }

    #[test]
    fn friendly_robber_blocks_hex_with_low_vp_player() {
        let mut state = make_friendly_robber_state(4);
        state.phase = GamePhase::PlacingRobber { current_player: 0 };

        // Place a settlement for player 1 adjacent to hex (1,0).
        let target_hex = HexCoord::new(1, 0);
        let vertex = target_hex.vertices()[0];
        state.buildings.insert(vertex, Building::Settlement(1));

        // Player 1 has only 1 VP (the settlement), so friendly robber blocks.
        assert_eq!(state.victory_points(1), 1);
        assert_eq!(
            apply_move_robber(&mut state, target_hex),
            Err(RuleError::InvalidRobberPlacement)
        );
    }

    #[test]
    fn friendly_robber_allows_hex_with_high_vp_player() {
        let mut state = make_friendly_robber_state(4);
        state.phase = GamePhase::PlacingRobber { current_player: 0 };

        let target_hex = HexCoord::new(1, 0);
        let vertices = target_hex.vertices();

        // Give player 1 three settlements (3 VP) adjacent to the target hex.
        state.buildings.insert(vertices[0], Building::Settlement(1));
        state.buildings.insert(vertices[1], Building::Settlement(1));
        state.buildings.insert(vertices[2], Building::Settlement(1));

        assert_eq!(state.victory_points(1), 3);
        apply_move_robber(&mut state, target_hex).unwrap();
        assert_eq!(state.robber_hex, target_hex);
    }

    #[test]
    fn friendly_robber_allows_empty_hex() {
        let mut state = make_friendly_robber_state(4);
        state.phase = GamePhase::PlacingRobber { current_player: 0 };

        // No buildings adjacent to this hex.
        let target_hex = HexCoord::new(1, 0);
        apply_move_robber(&mut state, target_hex).unwrap();
        assert_eq!(state.robber_hex, target_hex);
    }

    #[test]
    fn friendly_robber_ignores_own_buildings() {
        let mut state = make_friendly_robber_state(4);
        state.phase = GamePhase::PlacingRobber { current_player: 0 };

        // Only the placing player's own building is adjacent.
        let target_hex = HexCoord::new(1, 0);
        let vertex = target_hex.vertices()[0];
        state.buildings.insert(vertex, Building::Settlement(0));

        // Own buildings with low VP don't trigger the block.
        apply_move_robber(&mut state, target_hex).unwrap();
        assert_eq!(state.robber_hex, target_hex);
    }

    #[test]
    fn friendly_robber_off_allows_low_vp_target() {
        let mut state = make_state(4);
        state.phase = GamePhase::PlacingRobber { current_player: 0 };

        let target_hex = HexCoord::new(1, 0);
        let vertex = target_hex.vertices()[0];
        state.buildings.insert(vertex, Building::Settlement(1));

        // friendly_robber is false, so 1 VP player can be targeted.
        assert!(!state.friendly_robber);
        apply_move_robber(&mut state, target_hex).unwrap();
    }

    #[test]
    fn friendly_robber_blocks_knight_against_low_vp() {
        let mut state = make_friendly_robber_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::Knight);

        let target_hex = HexCoord::new(1, 0);
        let vertex = target_hex.vertices()[0];
        state.buildings.insert(vertex, Building::Settlement(1));

        // Player 1 has 1 VP, friendly robber should block knight placement.
        let result = apply_play_dev_card(
            &mut state,
            DevCard::Knight,
            DevCardAction::Knight {
                robber_to: target_hex,
                steal_from: None,
            },
        );
        assert_eq!(result, Err(RuleError::InvalidRobberPlacement));
    }

    #[test]
    fn friendly_robber_allows_knight_against_high_vp() {
        let mut state = make_friendly_robber_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::Knight);

        let target_hex = HexCoord::new(1, 0);
        let vertices = target_hex.vertices();

        // Give player 1 a city (2 VP) + a settlement (1 VP) = 3 VP.
        state.buildings.insert(vertices[0], Building::City(1));
        state.buildings.insert(vertices[1], Building::Settlement(1));

        assert_eq!(state.victory_points(1), 3);
        apply_play_dev_card(
            &mut state,
            DevCard::Knight,
            DevCardAction::Knight {
                robber_to: target_hex,
                steal_from: None,
            },
        )
        .unwrap();
        assert_eq!(state.robber_hex, target_hex);
    }

    #[test]
    fn legal_robber_hexes_excludes_current_position() {
        let state = make_state(3);
        let hexes = legal_robber_hexes(&state, 0);
        assert!(!hexes.contains(&state.robber_hex));
    }

    #[test]
    fn legal_robber_hexes_excludes_friendly_robber_blocked() {
        let mut state = make_friendly_robber_state(3);
        // Place a low-VP opponent settlement adjacent to hex (1,0).
        let target_hex = HexCoord::new(1, 0);
        let vertex = target_hex.vertices()[0];
        state.buildings.insert(vertex, Building::Settlement(1));
        assert_eq!(state.victory_points(1), 1);

        let hexes = legal_robber_hexes(&state, 0);
        // Target hex should be excluded because player 1 has <= 2 VP.
        assert!(
            !hexes.contains(&target_hex),
            "legal_robber_hexes should exclude hex blocked by friendly robber"
        );
    }

    // -- Legal actions --

    #[test]
    fn legal_actions_always_includes_end_turn() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);

        let actions = legal_actions(&state);
        assert!(actions.contains(&Action::EndTurn));
    }

    #[test]
    fn legal_actions_includes_bank_trade_when_affordable() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        give_resources(&mut state, 0, &[(Resource::Brick, 4)]);

        let actions = legal_actions(&state);
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::BankTrade {
                give: Resource::Brick,
                ..
            }
        )));
    }

    #[test]
    fn legal_actions_no_bank_trade_when_poor() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        give_resources(&mut state, 0, &[(Resource::Brick, 3)]);

        let actions = legal_actions(&state);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, Action::BankTrade { .. })));
    }

    #[test]
    fn legal_actions_empty_before_roll() {
        let mut state = make_state(4);
        state.phase = GamePhase::Playing {
            current_player: 0,
            has_rolled: false,
        };

        let actions = legal_actions(&state);
        assert!(actions.is_empty(), "No actions before rolling");
    }

    // -- Trade rate --

    #[test]
    fn default_trade_rate_is_4() {
        let state = make_state(4);
        assert_eq!(trade_rate(&state, 0, Resource::Brick), 4);
    }

    #[test]
    fn generic_port_reduces_trade_rate_to_3() {
        let mut state = make_state(4);

        // Find a generic port and place a settlement on it.
        let generic_port = state
            .board
            .ports
            .iter()
            .find(|p| p.port_type == PortType::Generic)
            .expect("Should have a generic port");
        let v = generic_port.vertices.0;
        place_settlement(&mut state, 0, v);

        // Generic port gives 3:1 for all resources.
        for &r in Resource::all() {
            assert_eq!(trade_rate(&state, 0, r), 3);
        }
    }

    #[test]
    fn specific_port_reduces_trade_rate_to_2() {
        let mut state = make_state(4);

        // Find the Wheat 2:1 port and place a settlement on it.
        let wheat_port = state
            .board
            .ports
            .iter()
            .find(|p| p.port_type == PortType::Specific(Resource::Wheat))
            .expect("Should have a wheat port");
        let v = wheat_port.vertices.0;
        place_settlement(&mut state, 0, v);

        // 2:1 rate for the matching resource.
        assert_eq!(trade_rate(&state, 0, Resource::Wheat), 2);
        // Other resources still at 4:1 (no generic port here).
        assert_eq!(trade_rate(&state, 0, Resource::Brick), 4);
    }

    #[test]
    fn both_port_vertices_grant_trade_rate() {
        let state = make_state(4);
        // Every port vertex should grant the correct rate when a settlement is placed.
        for port in &state.board.ports {
            for v in [port.vertices.0, port.vertices.1] {
                let mut s = make_state(4);
                place_settlement(&mut s, 0, v);
                match port.port_type {
                    PortType::Generic => {
                        for &r in Resource::all() {
                            assert_eq!(
                                trade_rate(&s, 0, r),
                                3,
                                "Generic port vertex {:?} should give 3:1",
                                v
                            );
                        }
                    }
                    PortType::Specific(res) => {
                        assert_eq!(
                            trade_rate(&s, 0, res),
                            2,
                            "Specific {:?} port vertex {:?} should give 2:1",
                            res,
                            v
                        );
                    }
                }
            }
        }
    }

    // -- Dev card validation-before-consume --

    #[test]
    fn failed_dev_card_validation_preserves_card() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::Knight);

        // Try to play knight with robber staying on same hex (invalid).
        let same_hex = state.robber_hex;
        let result = apply_play_dev_card(
            &mut state,
            DevCard::Knight,
            DevCardAction::Knight {
                robber_to: same_hex,
                steal_from: None,
            },
        );
        assert_eq!(result, Err(RuleError::InvalidRobberPlacement));

        // Card should still be in hand.
        assert_eq!(state.players[0].dev_cards.len(), 1);
        assert!(!state.players[0].has_played_dev_card_this_turn);
    }

    #[test]
    fn year_of_plenty_rejects_when_bank_empty() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::YearOfPlenty);

        // Exhaust the bank's ore supply (19 total) by giving it all to player 1.
        state.players[1].add_resource(Resource::Ore, 19);

        let result = apply_play_dev_card(
            &mut state,
            DevCard::YearOfPlenty,
            DevCardAction::YearOfPlenty(Resource::Ore, Resource::Wheat),
        );
        assert_eq!(result, Err(RuleError::InsufficientResources));
        // Card preserved.
        assert_eq!(state.players[0].dev_cards.len(), 1);
    }

    #[test]
    fn road_building_rejects_disconnected_first_road() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::RoadBuilding);

        // Player 0 has a settlement at (0,0) North.
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        place_settlement(&mut state, 0, v);

        // Pick edges far from the settlement -- disconnected.
        let far_e1 = EdgeCoord::new(HexCoord::new(2, -2), EdgeDirection::East);
        let far_e2 = EdgeCoord::new(HexCoord::new(2, -2), EdgeDirection::SouthEast);

        let result = apply_play_dev_card(
            &mut state,
            DevCard::RoadBuilding,
            DevCardAction::RoadBuilding(far_e1, far_e2),
        );
        assert_eq!(
            result,
            Err(RuleError::InvalidPlacement(
                "First road not connected to your network".into()
            ))
        );
        // Card preserved, no roads placed.
        assert_eq!(state.players[0].dev_cards.len(), 1);
        assert!(state.roads.is_empty());
    }

    #[test]
    fn road_building_rejects_disconnected_second_road() {
        let mut state = make_state(4);
        set_playing(&mut state, 0);
        state.players[0].dev_cards.push(DevCard::RoadBuilding);

        // Settlement at (0,0) North so first road can connect.
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        place_settlement(&mut state, 0, v);

        // First road adjacent to settlement, second road disconnected.
        let e1 = EdgeCoord::new(HexCoord::new(0, 0), EdgeDirection::NorthEast);
        let far_e2 = EdgeCoord::new(HexCoord::new(2, -2), EdgeDirection::East);

        let result = apply_play_dev_card(
            &mut state,
            DevCard::RoadBuilding,
            DevCardAction::RoadBuilding(e1, far_e2),
        );
        assert_eq!(
            result,
            Err(RuleError::InvalidPlacement(
                "Second road not connected to your network".into()
            ))
        );
        assert_eq!(state.players[0].dev_cards.len(), 1);
        assert!(state.roads.is_empty());
    }
}
