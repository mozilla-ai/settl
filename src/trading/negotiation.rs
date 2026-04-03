//! Trade negotiation protocol — defines the flow of proposing, responding to,
//! and executing trades between players.

use crate::game::actions::{PlayerId, TradeOffer, TradeResponse};
use crate::game::state::GameState;
use crate::trading::offers::{self, TradeError};

/// The outcome of a trade negotiation round.
#[derive(Debug, Clone)]
pub enum NegotiationResult {
    /// Trade was accepted by a player and executed successfully.
    Executed { acceptor: PlayerId },
    /// No player accepted the trade.
    NoTakers,
    /// The proposer cancelled the trade.
    Cancelled,
    /// Trade validation failed.
    Invalid(TradeError),
}

/// Validate a trade offer against the current game state.
///
/// Returns Ok(()) if the offer is valid, or an error describing the problem.
pub fn validate_trade(state: &GameState, offer: &TradeOffer) -> Result<(), TradeError> {
    let proposer = &state.players[offer.from];
    offers::validate_offer(offer, proposer)
}

/// Check which players can potentially accept a trade offer.
///
/// Returns a list of player IDs who have the requested resources.
pub fn eligible_responders(state: &GameState, offer: &TradeOffer) -> Vec<PlayerId> {
    (0..state.num_players)
        .filter(|&p| p != offer.from)
        .filter(|&p| offers::can_fulfill(offer, &state.players[p]).is_ok())
        .collect()
}

/// Execute a trade between the proposer and acceptor in the game state.
///
/// Validates both sides have the resources before modifying state.
pub fn execute_in_state(
    state: &mut GameState,
    offer: &TradeOffer,
    acceptor: PlayerId,
) -> Result<(), TradeError> {
    // Split borrow: we need mutable access to two different player states.
    let (proposer_ps, acceptor_ps) = if offer.from < acceptor {
        let (left, right) = state.players.split_at_mut(acceptor);
        (&mut left[offer.from], &mut right[0])
    } else {
        let (left, right) = state.players.split_at_mut(offer.from);
        (&mut right[0], &mut left[acceptor])
    };

    offers::execute_trade(proposer_ps, acceptor_ps, offer)
}

/// Evaluate whether a trade is favorable for the responding player.
///
/// Returns a simple heuristic score: positive = good for responder,
/// negative = bad. Used by AI players as one input to their decision.
pub fn trade_value_heuristic(
    offer: &TradeOffer,
    responder: &crate::game::state::PlayerState,
) -> f32 {
    // Simple heuristic: value of resources gained minus resources lost.
    // Resources you have less of are worth more to you.
    let mut score: f32 = 0.0;

    // What the responder would give (the offer's requesting side).
    for &(resource, count) in &offer.requesting {
        let have = responder.resource_count(resource) as f32;
        // Losing a resource hurts more when you have fewer.
        let loss = count as f32 * (1.0 + 2.0 / (have + 1.0));
        score -= loss;
    }

    // What the responder would receive (the offer's offering side).
    for &(resource, count) in &offer.offering {
        let have = responder.resource_count(resource) as f32;
        // Gaining a resource helps more when you have fewer.
        let gain = count as f32 * (1.0 + 2.0 / (have + 1.0));
        score += gain;
    }

    score
}

/// Determine the AI trade response based on heuristic evaluation.
///
/// Returns Accept if the trade has positive value, Reject otherwise.
pub fn heuristic_response(
    offer: &TradeOffer,
    responder: &crate::game::state::PlayerState,
) -> TradeResponse {
    let value = trade_value_heuristic(offer, responder);
    if value > 0.5 {
        TradeResponse::Accept
    } else {
        TradeResponse::Reject {
            reason: format!("Trade value too low ({:.1})", value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::{Board, Resource};
    use crate::game::state::GameState;

    fn setup_game() -> GameState {
        let board = Board::default_board();
        let mut state = GameState::new(board, 3);
        // Give players some resources.
        state.players[0].add_resource(Resource::Wood, 3);
        state.players[0].add_resource(Resource::Brick, 2);
        state.players[1].add_resource(Resource::Ore, 2);
        state.players[1].add_resource(Resource::Wheat, 3);
        state.players[2].add_resource(Resource::Sheep, 4);
        state
    }

    fn make_offer(
        from: usize,
        give: Resource,
        give_n: u32,
        want: Resource,
        want_n: u32,
    ) -> TradeOffer {
        TradeOffer {
            from,
            offering: vec![(give, give_n)],
            requesting: vec![(want, want_n)],
            message: String::new(),
        }
    }

    #[test]
    fn validate_trade_valid() {
        let state = setup_game();
        let offer = make_offer(0, Resource::Wood, 2, Resource::Ore, 1);
        assert!(validate_trade(&state, &offer).is_ok());
    }

    #[test]
    fn validate_trade_insufficient() {
        let state = setup_game();
        let offer = make_offer(0, Resource::Wood, 5, Resource::Ore, 1);
        assert!(validate_trade(&state, &offer).is_err());
    }

    #[test]
    fn eligible_responders_finds_correct_players() {
        let state = setup_game();
        // P0 offers wood for ore — only P1 has ore.
        let offer = make_offer(0, Resource::Wood, 1, Resource::Ore, 1);
        let eligible = eligible_responders(&state, &offer);
        assert_eq!(eligible, vec![1]);
    }

    #[test]
    fn eligible_responders_excludes_proposer() {
        let state = setup_game();
        let offer = make_offer(0, Resource::Wood, 1, Resource::Sheep, 1);
        let eligible = eligible_responders(&state, &offer);
        assert!(!eligible.contains(&0));
        assert!(eligible.contains(&2)); // P2 has sheep
    }

    #[test]
    fn execute_in_state_transfers_resources() {
        let mut state = setup_game();
        let offer = make_offer(0, Resource::Wood, 2, Resource::Ore, 1);
        execute_in_state(&mut state, &offer, 1).unwrap();

        assert_eq!(state.players[0].resource_count(Resource::Wood), 1);
        assert_eq!(state.players[0].resource_count(Resource::Ore), 1);
        assert_eq!(state.players[1].resource_count(Resource::Wood), 2);
        assert_eq!(state.players[1].resource_count(Resource::Ore), 1);
    }

    #[test]
    fn execute_in_state_reversed_indices() {
        // Test when acceptor < proposer (different split_at_mut branch).
        let mut state = setup_game();
        let offer = make_offer(1, Resource::Ore, 1, Resource::Wood, 2);
        execute_in_state(&mut state, &offer, 0).unwrap();

        assert_eq!(state.players[1].resource_count(Resource::Ore), 1);
        assert_eq!(state.players[1].resource_count(Resource::Wood), 2);
        assert_eq!(state.players[0].resource_count(Resource::Wood), 1);
        assert_eq!(state.players[0].resource_count(Resource::Ore), 1);
    }

    #[test]
    fn trade_value_positive_when_gaining_scarce_resource() {
        let mut ps = crate::game::state::PlayerState::new();
        ps.add_resource(Resource::Wood, 5); // lots of wood
        ps.add_resource(Resource::Ore, 0); // no ore

        // Offer asks for wood (abundant), gives ore (scarce) — great deal!
        let offer = make_offer(0, Resource::Ore, 1, Resource::Wood, 1);
        let value = trade_value_heuristic(&offer, &ps);
        assert!(
            value > 0.0,
            "Should be positive when gaining scarce resource, got {}",
            value
        );
    }

    #[test]
    fn trade_value_negative_when_losing_scarce_resource() {
        let mut ps = crate::game::state::PlayerState::new();
        ps.add_resource(Resource::Ore, 1); // scarce ore
        ps.add_resource(Resource::Wood, 5); // lots of wood

        // Offer asks for ore (scarce), gives wood (abundant) — bad deal!
        let offer = make_offer(0, Resource::Wood, 1, Resource::Ore, 1);
        let value = trade_value_heuristic(&offer, &ps);
        assert!(
            value < 0.0,
            "Should be negative when losing scarce resource, got {}",
            value
        );
    }

    #[test]
    fn heuristic_accepts_good_trade() {
        let mut ps = crate::game::state::PlayerState::new();
        ps.add_resource(Resource::Wood, 5);
        let offer = make_offer(0, Resource::Ore, 1, Resource::Wood, 1);
        assert!(matches!(
            heuristic_response(&offer, &ps),
            TradeResponse::Accept
        ));
    }

    #[test]
    fn heuristic_rejects_bad_trade() {
        let mut ps = crate::game::state::PlayerState::new();
        ps.add_resource(Resource::Wood, 5); // already has plenty of wood
        ps.add_resource(Resource::Ore, 1); // scarce ore
                                           // Offer wants their scarce Ore, gives Wood they already have plenty of — bad deal!
        let offer = make_offer(0, Resource::Wood, 1, Resource::Ore, 1);
        assert!(matches!(
            heuristic_response(&offer, &ps),
            TradeResponse::Reject { .. }
        ));
    }
}
