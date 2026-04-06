//! Event hook system -- runs shell commands when game events fire.
//!
//! Hooks are configured in `~/.settl/config.toml`:
//! ```toml
//! [[hooks]]
//! event = "GameWon"
//! command = "curl -X POST localhost:8080/event"
//!
//! [[hooks]]
//! event = "*"
//! command = "cat >> /tmp/settl-events.jsonl"
//! ```
//!
//! When a matching event fires, the command is spawned as a child process
//! with the event serialized as JSON on stdin. Hooks are fire-and-forget:
//! they run asynchronously and never block the game.

use crate::config::HookConfig;
use crate::game::event::GameEvent;

/// Determine the event name string for a `GameEvent` variant (e.g. "DiceRolled").
fn event_name(event: &GameEvent) -> &'static str {
    match event {
        GameEvent::InitialSettlementPlaced { .. } => "InitialSettlementPlaced",
        GameEvent::InitialRoadPlaced { .. } => "InitialRoadPlaced",
        GameEvent::DiceRolled { .. } => "DiceRolled",
        GameEvent::ResourcesDistributed { .. } => "ResourcesDistributed",
        GameEvent::SettlementBuilt { .. } => "SettlementBuilt",
        GameEvent::CityUpgraded { .. } => "CityUpgraded",
        GameEvent::RoadBuilt { .. } => "RoadBuilt",
        GameEvent::TradeProposed { .. } => "TradeProposed",
        GameEvent::TradeAccepted { .. } => "TradeAccepted",
        GameEvent::TradeRejected { .. } => "TradeRejected",
        GameEvent::TradeWithdrawn { .. } => "TradeWithdrawn",
        GameEvent::PlayerTradeExecuted { .. } => "PlayerTradeExecuted",
        GameEvent::BankTradeExecuted { .. } => "BankTradeExecuted",
        GameEvent::DevCardBought { .. } => "DevCardBought",
        GameEvent::DevCardPlayed { .. } => "DevCardPlayed",
        GameEvent::RobberMoved { .. } => "RobberMoved",
        GameEvent::CardsDiscarded { .. } => "CardsDiscarded",
        GameEvent::GameWon { .. } => "GameWon",
    }
}

/// Fire all matching hooks for a game event.
///
/// Hooks run as background tasks (fire-and-forget). The event is serialized
/// to JSON and piped to each matching command's stdin.
pub fn fire(hooks: &[HookConfig], event: &GameEvent, player_names: &[String]) {
    if hooks.is_empty() {
        return;
    }

    let name = event_name(event);

    // Build JSON payload: { "event": "<name>", "data": <event>, "player_names": [...] }
    let payload = match serde_json::to_string(&serde_json::json!({
        "event": name,
        "data": event,
        "player_names": player_names,
    })) {
        Ok(json) => json,
        Err(e) => {
            log::warn!("Failed to serialize hook event: {e}");
            return;
        }
    };

    for hook in hooks {
        if hook.event != "*" && hook.event != name {
            continue;
        }

        let command = hook.command.clone();
        let payload = payload.clone();

        // Fire-and-forget: spawn in a background task.
        tokio::spawn(async move {
            match run_hook_command(&command, &payload).await {
                Ok(()) => {}
                Err(e) => {
                    log::warn!("Hook command failed: {command}: {e}");
                }
            }
        });
    }
}

/// Run a single hook command with the payload on stdin.
async fn run_hook_command(command: &str, payload: &str) -> Result<(), String> {
    use tokio::process::Command;

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(payload.as_bytes()).await;
        let _ = stdin.write_all(b"\n").await;
        // Drop stdin to close it, signaling EOF to the child.
    }

    // Don't wait for the child -- fire and forget.
    // The tokio runtime will reap it when it exits.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HookConfig;

    #[test]
    fn event_name_covers_all_variants() {
        // Verify every variant has a name by constructing one of each.
        let events = vec![
            GameEvent::InitialSettlementPlaced {
                player: 0,
                vertex: dummy_vertex(),
            },
            GameEvent::InitialRoadPlaced {
                player: 0,
                edge: dummy_edge(),
            },
            GameEvent::DiceRolled {
                player: 0,
                values: (3, 4),
                total: 7,
            },
            GameEvent::ResourcesDistributed {
                distributions: vec![],
            },
            GameEvent::SettlementBuilt {
                player: 0,
                vertex: dummy_vertex(),
                reasoning: String::new(),
            },
            GameEvent::CityUpgraded {
                player: 0,
                vertex: dummy_vertex(),
                reasoning: String::new(),
            },
            GameEvent::RoadBuilt {
                player: 0,
                edge: dummy_edge(),
                reasoning: String::new(),
            },
            GameEvent::TradeProposed {
                from: 0,
                offer: dummy_offer(),
                reasoning: String::new(),
            },
            GameEvent::TradeAccepted {
                by: 0,
                reasoning: String::new(),
            },
            GameEvent::TradeRejected {
                by: 0,
                reasoning: String::new(),
            },
            GameEvent::TradeWithdrawn { by: 0 },
            GameEvent::PlayerTradeExecuted {
                proposer: 0,
                acceptor: 1,
                gave: vec![(crate::game::board::Resource::Wood, 1)],
                got: vec![(crate::game::board::Resource::Brick, 1)],
            },
            GameEvent::BankTradeExecuted {
                player: 0,
                gave: (crate::game::board::Resource::Wood, 4),
                got: (crate::game::board::Resource::Brick, 1),
            },
            GameEvent::DevCardBought { player: 0 },
            GameEvent::DevCardPlayed {
                player: 0,
                card: crate::game::actions::DevCard::Knight,
                reasoning: String::new(),
            },
            GameEvent::RobberMoved {
                player: 0,
                to: crate::game::board::HexCoord { q: 0, r: 0 },
                stole_from: None,
            },
            GameEvent::CardsDiscarded {
                player: 0,
                cards: vec![],
            },
            GameEvent::GameWon {
                player: 0,
                final_vp: 10,
            },
        ];

        let names: Vec<&str> = events.iter().map(event_name).collect();
        assert_eq!(names.len(), 18);
        // All names should be non-empty and unique.
        for name in &names {
            assert!(!name.is_empty());
        }
        let unique: std::collections::HashSet<&&str> = names.iter().collect();
        assert_eq!(unique.len(), 18, "all event names should be unique");
    }

    #[test]
    fn fire_skips_non_matching_hooks() {
        // This just verifies fire() doesn't panic with no matches.
        let hooks = vec![HookConfig {
            event: "GameWon".into(),
            command: "echo test".into(),
        }];
        let event = GameEvent::DiceRolled {
            player: 0,
            values: (3, 4),
            total: 7,
        };
        fire(&hooks, &event, &["Alice".into()]);
    }

    #[test]
    fn fire_with_empty_hooks_is_noop() {
        let event = GameEvent::DiceRolled {
            player: 0,
            values: (3, 4),
            total: 7,
        };
        fire(&[], &event, &["Alice".into()]);
    }

    fn dummy_vertex() -> crate::game::board::VertexCoord {
        crate::game::board::VertexCoord {
            hex: crate::game::board::HexCoord { q: 0, r: 0 },
            dir: crate::game::board::VertexDirection::North,
        }
    }

    fn dummy_edge() -> crate::game::board::EdgeCoord {
        crate::game::board::EdgeCoord {
            hex: crate::game::board::HexCoord { q: 0, r: 0 },
            dir: crate::game::board::EdgeDirection::NorthEast,
        }
    }

    fn dummy_offer() -> crate::game::actions::TradeOffer {
        crate::game::actions::TradeOffer {
            from: 0,
            offering: vec![],
            requesting: vec![],
            message: String::new(),
        }
    }
}
