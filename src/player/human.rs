//! Human player that reads choices from stdin.

use async_trait::async_trait;
use std::io::{self, Write};

use crate::game::actions::{PlayerId, TradeOffer, TradeResponse};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord, VertexDirection};
use crate::game::state::GameState;
use crate::player::prompt;
use crate::player::{Player, PlayerChoice};

/// A human player who makes decisions via terminal input.
#[allow(dead_code)]
pub struct HumanPlayer {
    name: String,
}

#[allow(dead_code)]
impl HumanPlayer {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    /// Read a valid index from stdin, re-prompting on invalid input.
    fn read_index(max: usize) -> usize {
        loop {
            print!("Your choice (0-{}): ", max.saturating_sub(1));
            io::stdout().flush().ok();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                println!("Error reading input, try again.");
                continue;
            }

            match input.trim().parse::<usize>() {
                Ok(n) if n < max => return n,
                _ => println!("Invalid choice. Enter a number from 0 to {}.", max - 1),
            }
        }
    }

    /// Read a resource name from stdin.
    fn read_resource() -> Resource {
        loop {
            print!("Resource (Wood/Brick/Sheep/Wheat/Ore): ");
            io::stdout().flush().ok();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                continue;
            }

            match input.trim().to_lowercase().as_str() {
                "wood" | "w" => return Resource::Wood,
                "brick" | "b" => return Resource::Brick,
                "sheep" | "s" => return Resource::Sheep,
                "wheat" | "wh" => return Resource::Wheat,
                "ore" | "o" => return Resource::Ore,
                _ => println!("Invalid resource. Try: Wood, Brick, Sheep, Wheat, Ore"),
            }
        }
    }
}

#[async_trait]
impl Player for HumanPlayer {
    fn name(&self) -> &str {
        &self.name
    }

    async fn choose_action(
        &self,
        state: &GameState,
        player_id: PlayerId,
        choices: &[PlayerChoice],
    ) -> (usize, String) {
        println!("\n{}", prompt::ascii_board(&state.board));
        println!("\n--- Your Turn (Player {}: {}) ---", player_id, self.name);

        let ps = &state.players[player_id];
        println!(
            "Resources: Wood:{} Brick:{} Sheep:{} Wheat:{} Ore:{}",
            ps.resource_count(Resource::Wood),
            ps.resource_count(Resource::Brick),
            ps.resource_count(Resource::Sheep),
            ps.resource_count(Resource::Wheat),
            ps.resource_count(Resource::Ore),
        );
        println!("VP: {}", state.victory_points(player_id));
        println!("\nLegal actions:");
        println!("{}", prompt::format_choices(choices));

        let idx = Self::read_index(choices.len());
        (idx, String::new())
    }

    async fn choose_settlement(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        legal_vertices: &[VertexCoord],
        _round: u8,
    ) -> (usize, String) {
        println!(
            "\n--- Place Settlement (Player {}: {}) ---",
            player_id, self.name
        );
        println!("Legal locations:");
        for (i, v) in legal_vertices.iter().enumerate() {
            let dir = match v.dir {
                VertexDirection::North => "N",
                VertexDirection::South => "S",
            };
            println!("  {}. ({}, {}, {})", i, v.hex.q, v.hex.r, dir);
        }

        let idx = Self::read_index(legal_vertices.len());
        (idx, String::new())
    }

    async fn choose_road(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        legal_edges: &[EdgeCoord],
    ) -> (usize, String) {
        println!("\n--- Place Road (Player {}: {}) ---", player_id, self.name);
        println!("Legal locations:");
        for (i, e) in legal_edges.iter().enumerate() {
            println!("  {}. {}", i, e);
        }

        let idx = Self::read_index(legal_edges.len());
        (idx, String::new())
    }

    async fn choose_robber_hex(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        legal_hexes: &[HexCoord],
    ) -> (usize, String) {
        println!(
            "\n--- Move Robber (Player {}: {}) ---",
            player_id, self.name
        );
        println!("{}", prompt::format_hex_options(legal_hexes));

        let idx = Self::read_index(legal_hexes.len());
        (idx, String::new())
    }

    async fn choose_steal_target(
        &self,
        state: &GameState,
        player_id: PlayerId,
        targets: &[PlayerId],
    ) -> (usize, String) {
        println!("\n--- Steal From (Player {}: {}) ---", player_id, self.name);
        for (i, &p) in targets.iter().enumerate() {
            println!(
                "  {}. Player {} ({} cards)",
                i,
                p,
                state.players[p].total_resources()
            );
        }

        let idx = Self::read_index(targets.len());
        (idx, String::new())
    }

    async fn choose_discard(
        &self,
        state: &GameState,
        player_id: PlayerId,
        count: usize,
    ) -> (Vec<Resource>, String) {
        let ps = &state.players[player_id];
        println!(
            "\n--- Discard {} Cards (Player {}: {}) ---",
            count, player_id, self.name
        );
        println!(
            "Your hand: Wood:{} Brick:{} Sheep:{} Wheat:{} Ore:{}",
            ps.resource_count(Resource::Wood),
            ps.resource_count(Resource::Brick),
            ps.resource_count(Resource::Sheep),
            ps.resource_count(Resource::Wheat),
            ps.resource_count(Resource::Ore),
        );

        let mut discards = Vec::with_capacity(count);
        for i in 0..count {
            println!("Card {} of {} to discard:", i + 1, count);
            discards.push(Self::read_resource());
        }
        (discards, String::new())
    }

    async fn choose_resource(
        &self,
        _state: &GameState,
        _player_id: PlayerId,
        context: &str,
    ) -> (Resource, String) {
        println!("\n{}", context);
        let r = Self::read_resource();
        (r, String::new())
    }

    async fn propose_trade(
        &self,
        state: &GameState,
        player_id: PlayerId,
    ) -> Option<(TradeOffer, String)> {
        let ps = &state.players[player_id];
        println!(
            "\n--- Propose Trade (Player {}: {}) ---",
            player_id, self.name
        );
        println!(
            "Your hand: Wood:{} Brick:{} Sheep:{} Wheat:{} Ore:{}",
            ps.resource_count(Resource::Wood),
            ps.resource_count(Resource::Brick),
            ps.resource_count(Resource::Sheep),
            ps.resource_count(Resource::Wheat),
            ps.resource_count(Resource::Ore),
        );
        println!("What do you want to GIVE?");
        let give_resource = Self::read_resource();
        print!("How many? ");
        io::stdout().flush().ok();
        let give_count = {
            let mut input = String::new();
            io::stdin().read_line(&mut input).ok();
            input.trim().parse::<u32>().unwrap_or(1).max(1)
        };
        println!("What do you want to GET?");
        let get_resource = Self::read_resource();
        print!("How many? ");
        io::stdout().flush().ok();
        let get_count = {
            let mut input = String::new();
            io::stdin().read_line(&mut input).ok();
            input.trim().parse::<u32>().unwrap_or(1).max(1)
        };

        Some((
            TradeOffer {
                from: player_id,
                offering: vec![(give_resource, give_count)],
                requesting: vec![(get_resource, get_count)],
                message: String::new(),
            },
            String::new(),
        ))
    }

    async fn respond_to_trade(
        &self,
        _state: &GameState,
        player_id: PlayerId,
        offer: &TradeOffer,
    ) -> (TradeResponse, String) {
        let offering: String = offer
            .offering
            .iter()
            .map(|(r, n)| format!("{} {}", n, r))
            .collect::<Vec<_>>()
            .join(", ");
        let requesting: String = offer
            .requesting
            .iter()
            .map(|(r, n)| format!("{} {}", n, r))
            .collect::<Vec<_>>()
            .join(", ");

        println!(
            "\n--- Trade Offer (Player {}: {}) ---",
            player_id, self.name
        );
        println!("Player {} offers: {}", offer.from, offering);
        println!("Player {} wants: {}", offer.from, requesting);
        if !offer.message.is_empty() {
            println!("Message: \"{}\"", offer.message);
        }
        println!("  0. Accept");
        println!("  1. Reject");

        let idx = Self::read_index(2);
        if idx == 0 {
            (TradeResponse::Accept, String::new())
        } else {
            (
                TradeResponse::Reject {
                    reason: "No thanks".into(),
                },
                String::new(),
            )
        }
    }
}
