//! Trade offer validation and utilities.

use crate::game::actions::TradeOffer;
use crate::game::board::Resource;
use crate::game::state::PlayerState;

/// Errors that can occur during trade validation.
#[derive(Debug, Clone, PartialEq)]
pub enum TradeError {
    /// The proposer doesn't have enough resources to give.
    InsufficientOfferingResources {
        resource: Resource,
        have: u32,
        need: u32,
    },
    /// The acceptor doesn't have enough resources to fulfill the request.
    InsufficientRequestedResources {
        resource: Resource,
        have: u32,
        need: u32,
    },
    /// The offer is empty (nothing offered or nothing requested).
    EmptyOffer,
    /// Trading the same resource for itself.
    SelfTrade { resource: Resource },
}

impl std::fmt::Display for TradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TradeError::InsufficientOfferingResources {
                resource,
                have,
                need,
            } => {
                write!(
                    f,
                    "Need {} {} to offer but only have {}",
                    need, resource, have
                )
            }
            TradeError::InsufficientRequestedResources {
                resource,
                have,
                need,
            } => {
                write!(
                    f,
                    "Acceptor needs {} {} but only has {}",
                    need, resource, have
                )
            }
            TradeError::EmptyOffer => write!(f, "Trade offer is empty"),
            TradeError::SelfTrade { resource } => {
                write!(f, "Cannot trade {} for {}", resource, resource)
            }
        }
    }
}

/// Validate that a trade offer is well-formed and the proposer has the resources.
pub fn validate_offer(offer: &TradeOffer, proposer: &PlayerState) -> Result<(), TradeError> {
    if offer.offering.is_empty() || offer.requesting.is_empty() {
        return Err(TradeError::EmptyOffer);
    }

    // Check for self-trades (offering and requesting the same resource).
    for &(give_r, _) in &offer.offering {
        for &(get_r, _) in &offer.requesting {
            if give_r == get_r {
                return Err(TradeError::SelfTrade { resource: give_r });
            }
        }
    }

    // Check proposer has enough of each offered resource.
    for &(resource, count) in &offer.offering {
        let have = proposer.resource_count(resource);
        if have < count {
            return Err(TradeError::InsufficientOfferingResources {
                resource,
                have,
                need: count,
            });
        }
    }

    Ok(())
}

/// Check if a player can fulfill the requesting side of a trade.
pub fn can_fulfill(offer: &TradeOffer, acceptor: &PlayerState) -> Result<(), TradeError> {
    for &(resource, count) in &offer.requesting {
        let have = acceptor.resource_count(resource);
        if have < count {
            return Err(TradeError::InsufficientRequestedResources {
                resource,
                have,
                need: count,
            });
        }
    }
    Ok(())
}

/// Execute a trade between two players, transferring resources.
///
/// Returns an error if either side lacks resources (should be validated first).
pub fn execute_trade(
    proposer: &mut PlayerState,
    acceptor: &mut PlayerState,
    offer: &TradeOffer,
) -> Result<(), TradeError> {
    // Validate both sides.
    validate_offer(offer, proposer)?;
    can_fulfill(offer, acceptor)?;

    // Transfer: proposer gives, acceptor receives.
    for &(resource, count) in &offer.offering {
        proposer.remove_resource(resource, count);
        acceptor.add_resource(resource, count);
    }

    // Transfer: acceptor gives, proposer receives.
    for &(resource, count) in &offer.requesting {
        acceptor.remove_resource(resource, count);
        proposer.add_resource(resource, count);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::Resource;

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
    fn validate_valid_offer() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 3);
        let offer = make_offer(0, Resource::Wood, 2, Resource::Ore, 1);
        assert!(validate_offer(&offer, &ps).is_ok());
    }

    #[test]
    fn validate_insufficient_resources() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 1);
        let offer = make_offer(0, Resource::Wood, 2, Resource::Ore, 1);
        assert!(matches!(
            validate_offer(&offer, &ps),
            Err(TradeError::InsufficientOfferingResources { .. })
        ));
    }

    #[test]
    fn validate_empty_offer() {
        let ps = PlayerState::new();
        let offer = TradeOffer {
            from: 0,
            offering: vec![],
            requesting: vec![(Resource::Ore, 1)],
            message: String::new(),
        };
        assert!(matches!(
            validate_offer(&offer, &ps),
            Err(TradeError::EmptyOffer)
        ));
    }

    #[test]
    fn validate_self_trade() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 3);
        let offer = make_offer(0, Resource::Wood, 1, Resource::Wood, 1);
        assert!(matches!(
            validate_offer(&offer, &ps),
            Err(TradeError::SelfTrade { .. })
        ));
    }

    #[test]
    fn can_fulfill_sufficient() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Ore, 2);
        let offer = make_offer(0, Resource::Wood, 1, Resource::Ore, 2);
        assert!(can_fulfill(&offer, &ps).is_ok());
    }

    #[test]
    fn can_fulfill_insufficient() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Ore, 1);
        let offer = make_offer(0, Resource::Wood, 1, Resource::Ore, 2);
        assert!(matches!(
            can_fulfill(&offer, &ps),
            Err(TradeError::InsufficientRequestedResources { .. })
        ));
    }

    #[test]
    fn execute_trade_transfers_resources() {
        let mut proposer = PlayerState::new();
        proposer.add_resource(Resource::Wood, 3);
        proposer.add_resource(Resource::Brick, 1);

        let mut acceptor = PlayerState::new();
        acceptor.add_resource(Resource::Ore, 2);

        let offer = make_offer(0, Resource::Wood, 2, Resource::Ore, 1);
        execute_trade(&mut proposer, &mut acceptor, &offer).unwrap();

        assert_eq!(proposer.resource_count(Resource::Wood), 1);
        assert_eq!(proposer.resource_count(Resource::Ore), 1);
        assert_eq!(acceptor.resource_count(Resource::Wood), 2);
        assert_eq!(acceptor.resource_count(Resource::Ore), 1);
    }

    #[test]
    fn execute_trade_fails_if_proposer_short() {
        let mut proposer = PlayerState::new();
        proposer.add_resource(Resource::Wood, 1);

        let mut acceptor = PlayerState::new();
        acceptor.add_resource(Resource::Ore, 2);

        let offer = make_offer(0, Resource::Wood, 2, Resource::Ore, 1);
        assert!(execute_trade(&mut proposer, &mut acceptor, &offer).is_err());
        // Resources should not have changed.
        assert_eq!(proposer.resource_count(Resource::Wood), 1);
        assert_eq!(acceptor.resource_count(Resource::Ore), 2);
    }

    #[test]
    fn execute_multi_resource_trade() {
        let mut proposer = PlayerState::new();
        proposer.add_resource(Resource::Wood, 2);
        proposer.add_resource(Resource::Brick, 1);

        let mut acceptor = PlayerState::new();
        acceptor.add_resource(Resource::Ore, 1);
        acceptor.add_resource(Resource::Wheat, 1);

        let offer = TradeOffer {
            from: 0,
            offering: vec![(Resource::Wood, 2), (Resource::Brick, 1)],
            requesting: vec![(Resource::Ore, 1), (Resource::Wheat, 1)],
            message: String::new(),
        };

        execute_trade(&mut proposer, &mut acceptor, &offer).unwrap();

        assert_eq!(proposer.resource_count(Resource::Wood), 0);
        assert_eq!(proposer.resource_count(Resource::Brick), 0);
        assert_eq!(proposer.resource_count(Resource::Ore), 1);
        assert_eq!(proposer.resource_count(Resource::Wheat), 1);
        assert_eq!(acceptor.resource_count(Resource::Wood), 2);
        assert_eq!(acceptor.resource_count(Resource::Brick), 1);
        assert_eq!(acceptor.resource_count(Resource::Ore), 0);
        assert_eq!(acceptor.resource_count(Resource::Wheat), 0);
    }
}
