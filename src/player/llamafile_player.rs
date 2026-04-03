//! Llamafile player implementation using the `genai` crate with a local llamafile backend.
//!
//! Uses tool/function calling for structured responses.

use async_trait::async_trait;
use genai::chat::{ChatMessage, ChatRequest, Tool};
use genai::Client;

use crate::game::actions::{PlayerId, TradeOffer, TradeResponse};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};
use crate::game::state::GameState;
use crate::player::personality::Personality;
use crate::player::prompt;
use crate::player::{Player, PlayerChoice};

/// An LLM-powered player using genai for multi-provider support.
pub struct LlamafilePlayer {
    /// Display name (e.g. "Alice", "Bob").
    name: String,
    /// Model identifier for genai (e.g. "openai::bonsai").
    model: String,
    /// genai client.
    client: Client,
    /// Personality for system prompt injection.
    personality: Personality,
    /// Maximum retries on parse failure before falling back to random.
    max_retries: usize,
    /// Extra game context (recent history) injected by the orchestrator.
    extra_context: tokio::sync::Mutex<String>,
}

impl LlamafilePlayer {
    /// Create an LLM player backed by a pre-configured genai Client.
    pub fn with_client(
        name: String,
        model: String,
        personality: Personality,
        client: Client,
    ) -> Self {
        Self {
            name,
            model,
            client,
            personality,
            max_retries: 2,
            extra_context: tokio::sync::Mutex::new(String::new()),
        }
    }

    /// Build the compact system prompt for local models.
    fn system_prompt(&self) -> String {
        let personality = self.personality.to_system_prompt();
        prompt::system_prompt_compact(&self.name, &personality)
    }

    /// Build the choose_index tool definition (used for most selection prompts).
    fn index_tool(max_index: usize) -> Tool {
        Tool::new("choose_index")
            .with_description(
                "Choose an option by its index number. Explain your reasoning first.",
            )
            .with_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "reasoning": {
                        "type": "string",
                        "description": "Your strategic reasoning for this choice (2-3 sentences)"
                    },
                    "index": {
                        "type": "integer",
                        "description": format!("Index of your choice (0 to {})", max_index.saturating_sub(1)),
                        "minimum": 0,
                        "maximum": max_index.saturating_sub(1)
                    }
                },
                "required": ["reasoning", "index"]
            }))
    }

    /// Build the choose_resource tool definition.
    fn resource_tool() -> Tool {
        Tool::new("choose_resource")
            .with_description("Choose a resource type.")
            .with_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "reasoning": {
                        "type": "string",
                        "description": "Your reasoning for choosing this resource"
                    },
                    "resource": {
                        "type": "string",
                        "enum": ["Wood", "Brick", "Sheep", "Wheat", "Ore"],
                        "description": "The resource to choose"
                    }
                },
                "required": ["reasoning", "resource"]
            }))
    }

    /// Build the discard tool definition.
    fn discard_tool(count: usize) -> Tool {
        Tool::new("choose_discard")
            .with_description(format!(
                "Choose exactly {} resource cards to discard.",
                count
            ))
            .with_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "reasoning": {
                        "type": "string",
                        "description": "Why you chose to discard these cards"
                    },
                    "cards": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["Wood", "Brick", "Sheep", "Wheat", "Ore"]
                        },
                        "minItems": count,
                        "maxItems": count,
                        "description": format!("Exactly {} resource names to discard", count)
                    }
                },
                "required": ["reasoning", "cards"]
            }))
    }

    /// Build the propose_trade tool definition.
    fn propose_trade_tool() -> Tool {
        Tool::new("propose_trade")
            .with_description(
                "Propose a trade to other players. Specify what you want to give and receive.",
            )
            .with_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "reasoning": {
                        "type": "string",
                        "description": "Your strategic reasoning for this trade (2-3 sentences)"
                    },
                    "give_resource": {
                        "type": "string",
                        "enum": ["Wood", "Brick", "Sheep", "Wheat", "Ore"],
                        "description": "Resource you are offering"
                    },
                    "give_count": {
                        "type": "integer",
                        "description": "How many of that resource to offer",
                        "minimum": 1,
                        "maximum": 10
                    },
                    "want_resource": {
                        "type": "string",
                        "enum": ["Wood", "Brick", "Sheep", "Wheat", "Ore"],
                        "description": "Resource you want in return"
                    },
                    "want_count": {
                        "type": "integer",
                        "description": "How many of that resource you want",
                        "minimum": 1,
                        "maximum": 10
                    },
                    "message": {
                        "type": "string",
                        "description": "A short message to other players about this trade"
                    }
                },
                "required": ["reasoning", "give_resource", "give_count", "want_resource", "want_count"]
            }))
    }

    /// Build the trade_response tool definition.
    fn trade_response_tool() -> Tool {
        Tool::new("respond_to_trade")
            .with_description("Respond to a trade offer: accept, reject, or counter.")
            .with_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "reasoning": {
                        "type": "string",
                        "description": "Why you chose this response"
                    },
                    "response": {
                        "type": "string",
                        "enum": ["accept", "reject"],
                        "description": "Accept or reject the trade"
                    },
                    "reject_reason": {
                        "type": "string",
                        "description": "If rejecting, why (optional)"
                    }
                },
                "required": ["reasoning", "response"]
            }))
    }

    /// Make an LLM call with retry logic.
    ///
    /// Returns `(tool_call_arguments, reasoning)` on success.
    /// Falls back to a default on repeated failure.
    async fn call_with_retry(
        &self,
        system_msg: &str,
        user_msg: &str,
        tool: Tool,
    ) -> Result<(serde_json::Value, String), String> {
        let tool_name = match &tool.name {
            genai::chat::ToolName::Custom(s) => s.clone(),
            _ => "unknown".to_string(),
        };

        log::debug!(
            "[{}] LLM call: tool={} model={}",
            self.name,
            tool_name,
            self.model,
        );
        log::debug!(
            "[{}] system prompt ({} chars):\n{}",
            self.name,
            system_msg.len(),
            system_msg
        );
        log::debug!(
            "[{}] user prompt ({} chars):\n{}",
            self.name,
            user_msg.len(),
            user_msg
        );

        for attempt in 0..=self.max_retries {
            let mut messages = vec![ChatMessage::user(user_msg)];
            if attempt > 0 {
                messages.push(ChatMessage::user(
                    "Your previous response didn't use the tool correctly. \
                     Please call the tool with valid arguments.",
                ));
            }

            let chat_req = ChatRequest::new(messages)
                .with_system(system_msg)
                .with_tools(vec![tool.clone()]);

            match self.client.exec_chat(&self.model, chat_req, None).await {
                Ok(res) => {
                    // Log raw response content.
                    if let Some(content) = res.first_text() {
                        log::debug!("[{}] response text: {}", self.name, content);
                    }

                    let tool_calls = res.into_tool_calls();
                    log::debug!(
                        "[{}] tool calls received: {}",
                        self.name,
                        tool_calls
                            .iter()
                            .map(|tc| format!("{}({})", tc.fn_name, tc.fn_arguments))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );

                    if let Some(tc) = tool_calls.into_iter().find(|tc| tc.fn_name == tool_name) {
                        let reasoning = tc
                            .fn_arguments
                            .get("reasoning")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        log::debug!(
                            "[{}] success: tool={} args={} reasoning={}",
                            self.name,
                            tool_name,
                            tc.fn_arguments,
                            reasoning,
                        );
                        return Ok((tc.fn_arguments, reasoning));
                    }
                    // No matching tool call -- retry.
                    log::warn!(
                        "[{}] attempt {}: no {} tool call in response",
                        self.name,
                        attempt + 1,
                        tool_name,
                    );
                }
                Err(e) => {
                    log::warn!("[{}] attempt {}: API error: {}", self.name, attempt + 1, e,);
                    if attempt < self.max_retries {
                        // Exponential backoff.
                        let delay = std::time::Duration::from_secs(1 << attempt);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(format!(
            "Failed after {} attempts for {}",
            self.max_retries + 1,
            tool_name
        ))
    }

    /// Extract an index from tool call arguments, clamped to valid range.
    fn extract_index(args: &serde_json::Value, max: usize) -> usize {
        args.get("index")
            .and_then(|v| v.as_u64())
            .map(|i| (i as usize).min(max.saturating_sub(1)))
            .unwrap_or(0)
    }

    /// Parse a resource name string into a Resource enum.
    fn parse_resource(s: &str) -> Resource {
        match s.to_lowercase().as_str() {
            "wood" => Resource::Wood,
            "brick" => Resource::Brick,
            "sheep" => Resource::Sheep,
            "wheat" => Resource::Wheat,
            "ore" => Resource::Ore,
            _ => Resource::Wood, // fallback
        }
    }
}

#[async_trait]
impl Player for LlamafilePlayer {
    fn name(&self) -> &str {
        &self.name
    }

    async fn set_game_context(&self, context: &str) {
        *self.extra_context.lock().await = context.to_string();
    }

    async fn choose_action(
        &self,
        state: &GameState,
        player_id: PlayerId,
        choices: &[PlayerChoice],
    ) -> (usize, String) {
        let system = self.system_prompt();
        let extra = self.extra_context.lock().await;
        let user = if extra.is_empty() {
            prompt::turn_prompt(state, player_id, choices)
        } else {
            let board_ascii = prompt::ascii_board(&state.board);
            let state_json = prompt::game_state_json(state, player_id);
            format!(
                "BOARD:\n{board_ascii}\n\n\
                 GAME STATE:\n{state_json}\n\n\
                 {extra}\n\n\
                 You are Player {player_id}.\n\n\
                 LEGAL ACTIONS:\n{choices}\n\n\
                 Choose your action by calling the choose_action tool.",
                choices = prompt::format_choices(choices),
            )
        };
        let tool = Self::index_tool(choices.len());

        match self.call_with_retry(&system, &user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, choices.len());
                (idx, reasoning)
            }
            Err(_) => {
                // Random fallback.
                use rand::Rng;
                let idx = rand::rng().random_range(0..choices.len());
                (idx, "[AI was confused and acted randomly]".into())
            }
        }
    }

    async fn choose_settlement(
        &self,
        state: &GameState,
        player_id: PlayerId,
        legal_vertices: &[VertexCoord],
        round: u8,
    ) -> (usize, String) {
        let system = self.system_prompt();
        let strategy = self.personality.setup_strategy_text();
        let names: Vec<String> = (0..state.num_players).map(|i| format!("P{}", i)).collect();
        let user = prompt::setup_settlement_prompt(state, player_id, round, legal_vertices, &names);
        let user = format!("SETUP STRATEGY:\n{strategy}\n\n{user}",);
        let tool = Self::index_tool(legal_vertices.len());

        match self.call_with_retry(&system, &user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, legal_vertices.len());
                (idx, reasoning)
            }
            Err(_) => {
                use rand::Rng;
                let idx = rand::rng().random_range(0..legal_vertices.len());
                (idx, "[AI was confused and acted randomly]".into())
            }
        }
    }

    async fn choose_road(
        &self,
        state: &GameState,
        player_id: PlayerId,
        legal_edges: &[EdgeCoord],
    ) -> (usize, String) {
        let system = self.system_prompt();
        let user = prompt::setup_road_prompt(state, player_id, legal_edges);
        let tool = Self::index_tool(legal_edges.len());

        match self.call_with_retry(&system, &user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, legal_edges.len());
                (idx, reasoning)
            }
            Err(_) => {
                use rand::Rng;
                let idx = rand::rng().random_range(0..legal_edges.len());
                (idx, "[AI was confused and acted randomly]".into())
            }
        }
    }

    async fn choose_robber_hex(
        &self,
        state: &GameState,
        player_id: PlayerId,
        legal_hexes: &[HexCoord],
    ) -> (usize, String) {
        let system = self.system_prompt();
        let state_json = prompt::game_state_json(state, player_id);
        let hex_list = prompt::format_hex_options(legal_hexes);
        let user = format!(
            "GAME STATE:\n{state_json}\n\n\
             You must move the robber. Choose a hex:\n{hex_list}\n\n\
             Choose by calling the choose_index tool."
        );
        let tool = Self::index_tool(legal_hexes.len());

        match self.call_with_retry(&system, &user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, legal_hexes.len());
                (idx, reasoning)
            }
            Err(_) => {
                use rand::Rng;
                let idx = rand::rng().random_range(0..legal_hexes.len());
                (idx, "[AI was confused and acted randomly]".into())
            }
        }
    }

    async fn choose_steal_target(
        &self,
        state: &GameState,
        _player_id: PlayerId,
        targets: &[PlayerId],
    ) -> (usize, String) {
        let system = self.system_prompt();
        let target_list: String = targets
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                format!(
                    "  {}. Player {} ({} resource cards)",
                    i,
                    p,
                    state.players[p].total_resources()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let user = format!(
            "Choose a player to steal from:\n{target_list}\n\n\
             Choose by calling the choose_index tool."
        );
        let tool = Self::index_tool(targets.len());

        match self.call_with_retry(&system, &user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, targets.len());
                (idx, reasoning)
            }
            Err(_) => {
                use rand::Rng;
                let idx = rand::rng().random_range(0..targets.len());
                (idx, "[AI was confused and acted randomly]".into())
            }
        }
    }

    async fn choose_discard(
        &self,
        state: &GameState,
        player_id: PlayerId,
        count: usize,
    ) -> (Vec<Resource>, String) {
        let system = self.system_prompt();
        let ps = &state.players[player_id];
        let hand: String = Resource::all()
            .iter()
            .map(|&r| format!("{}: {}", r, ps.resource_count(r)))
            .collect::<Vec<_>>()
            .join(", ");
        let user = format!(
            "You have {} total cards: {}\n\
             You must discard exactly {} cards.\n\n\
             Choose by calling the choose_discard tool.",
            ps.total_resources(),
            hand,
            count,
        );
        let tool = Self::discard_tool(count);

        match self.call_with_retry(&system, &user, tool).await {
            Ok((args, reasoning)) => {
                if let Some(cards) = args.get("cards").and_then(|v| v.as_array()) {
                    let resources: Vec<Resource> = cards
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(Self::parse_resource)
                        .collect();
                    if resources.len() == count {
                        return (resources, reasoning);
                    }
                }
                // Fallback: discard the first `count` resources available.
                let fallback = fallback_discard(ps, count);
                (fallback, "[AI discard was invalid, using fallback]".into())
            }
            Err(_) => {
                let fallback = fallback_discard(ps, count);
                (fallback, "[AI was confused, discarding randomly]".into())
            }
        }
    }

    async fn choose_resource(
        &self,
        state: &GameState,
        player_id: PlayerId,
        context: &str,
    ) -> (Resource, String) {
        let system = self.system_prompt();
        let state_json = prompt::game_state_json(state, player_id);
        let user = format!(
            "GAME STATE:\n{state_json}\n\n\
             {context}\n\n\
             Choose by calling the choose_resource tool."
        );
        let tool = Self::resource_tool();

        match self.call_with_retry(&system, &user, tool).await {
            Ok((args, reasoning)) => {
                let resource = args
                    .get("resource")
                    .and_then(|v| v.as_str())
                    .map(Self::parse_resource)
                    .unwrap_or(Resource::Wheat);
                (resource, reasoning)
            }
            Err(_) => (
                Resource::Wheat,
                "[AI was confused and chose randomly]".into(),
            ),
        }
    }

    async fn propose_trade(
        &self,
        state: &GameState,
        player_id: PlayerId,
    ) -> Option<(TradeOffer, String)> {
        let system = self.system_prompt();
        let state_json = prompt::game_state_json(state, player_id);
        let ps = &state.players[player_id];
        let hand: String = Resource::all()
            .iter()
            .map(|&r| format!("{}: {}", r, ps.resource_count(r)))
            .collect::<Vec<_>>()
            .join(", ");
        let user = format!(
            "GAME STATE:\n{state_json}\n\n\
             Your resources: {hand}\n\n\
             You may propose a trade to other players.\n\
             Consider what you need for your strategy (settlements need Wood+Brick+Sheep+Wheat, \
             cities need Wheat+Wheat+Ore+Ore+Ore, roads need Wood+Brick, dev cards need Sheep+Wheat+Ore).\n\
             Think about what you have in excess and what you lack.\n\n\
             Call the propose_trade tool to make an offer."
        );
        let tool = Self::propose_trade_tool();

        match self.call_with_retry(&system, &user, tool).await {
            Ok((args, reasoning)) => {
                let give_resource = args
                    .get("give_resource")
                    .and_then(|v| v.as_str())
                    .map(Self::parse_resource)
                    .unwrap_or(Resource::Wood);
                let give_count =
                    args.get("give_count").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                let want_resource = args
                    .get("want_resource")
                    .and_then(|v| v.as_str())
                    .map(Self::parse_resource)
                    .unwrap_or(Resource::Wheat);
                let want_count =
                    args.get("want_count").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                let message = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                // Validate the player actually has enough to offer.
                if ps.resource_count(give_resource) < give_count {
                    return None;
                }

                Some((
                    TradeOffer {
                        from: player_id,
                        offering: vec![(give_resource, give_count)],
                        requesting: vec![(want_resource, want_count)],
                        message,
                    },
                    reasoning,
                ))
            }
            Err(_) => None,
        }
    }

    async fn respond_to_trade(
        &self,
        state: &GameState,
        player_id: PlayerId,
        offer: &TradeOffer,
    ) -> (TradeResponse, String) {
        let system = self.system_prompt();
        let state_json = prompt::game_state_json(state, player_id);
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
        let user = format!(
            "GAME STATE:\n{state_json}\n\n\
             Player {} offers a trade:\n\
             They give: {}\n\
             They want: {}\n\
             Message: \"{}\"\n\n\
             Choose by calling the respond_to_trade tool.",
            offer.from, offering, requesting, offer.message,
        );
        let tool = Self::trade_response_tool();

        match self.call_with_retry(&system, &user, tool).await {
            Ok((args, reasoning)) => {
                let response = match args.get("response").and_then(|v| v.as_str()) {
                    Some("accept") => TradeResponse::Accept,
                    _ => TradeResponse::Reject {
                        reason: args
                            .get("reject_reason")
                            .and_then(|v| v.as_str())
                            .unwrap_or("No thanks")
                            .to_string(),
                    },
                };
                (response, reasoning)
            }
            Err(_) => (
                TradeResponse::Reject {
                    reason: "Unable to process trade".into(),
                },
                "[AI was confused and declined]".into(),
            ),
        }
    }
}

/// The model name used for llamafile players. The `openai::` prefix forces
/// genai's OpenAI adapter, which speaks the right protocol for llamafile's
/// OpenAI-compatible API.
pub const LLAMAFILE_MODEL: &str = "openai::bonsai";

/// Build a genai `Client` that routes all requests to a local llamafile server.
pub fn llamafile_client(port: u16) -> Client {
    use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
    use genai::ServiceTarget;

    let endpoint_url: String = format!("http://127.0.0.1:{}/v1/", port);

    let target_resolver =
        ServiceTargetResolver::from_resolver_fn(move |service_target: ServiceTarget| {
            let ServiceTarget { model, .. } = service_target;
            Ok(ServiceTarget {
                endpoint: Endpoint::from_owned(endpoint_url.clone()),
                auth: AuthData::from_single("no-key"),
                model,
            })
        });

    Client::builder()
        .with_service_target_resolver(target_resolver)
        .build()
}

/// Fallback discard: drop the most abundant resources first.
pub(crate) fn fallback_discard(
    ps: &crate::game::state::PlayerState,
    count: usize,
) -> Vec<Resource> {
    let mut pool: Vec<(Resource, u32)> = Resource::all()
        .iter()
        .map(|&r| (r, ps.resource_count(r)))
        .filter(|(_, c)| *c > 0)
        .collect();
    // Sort by count descending to discard the most abundant first.
    pool.sort_by(|a, b| b.1.cmp(&a.1));

    let mut result = Vec::with_capacity(count);
    for (r, c) in &pool {
        let take = (*c as usize).min(count - result.len());
        for _ in 0..take {
            result.push(*r);
        }
        if result.len() >= count {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::PlayerState;
    use serde_json::json;

    #[test]
    fn extract_index_valid() {
        let args = json!({"index": 3});
        assert_eq!(LlamafilePlayer::extract_index(&args, 5), 3);
    }

    #[test]
    fn extract_index_clamps_to_max() {
        let args = json!({"index": 10});
        assert_eq!(LlamafilePlayer::extract_index(&args, 5), 4);
    }

    #[test]
    fn extract_index_missing_defaults_to_zero() {
        let args = json!({"reasoning": "test"});
        assert_eq!(LlamafilePlayer::extract_index(&args, 5), 0);
    }

    #[test]
    fn parse_resource_valid() {
        assert_eq!(LlamafilePlayer::parse_resource("Wood"), Resource::Wood);
        assert_eq!(LlamafilePlayer::parse_resource("brick"), Resource::Brick);
        assert_eq!(LlamafilePlayer::parse_resource("SHEEP"), Resource::Sheep);
        assert_eq!(LlamafilePlayer::parse_resource("Wheat"), Resource::Wheat);
        assert_eq!(LlamafilePlayer::parse_resource("ore"), Resource::Ore);
    }

    #[test]
    fn parse_resource_fallback() {
        assert_eq!(LlamafilePlayer::parse_resource("invalid"), Resource::Wood);
    }

    #[test]
    fn fallback_discard_takes_most_abundant_first() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 1);
        ps.add_resource(Resource::Brick, 3);
        ps.add_resource(Resource::Ore, 2);

        let discards = fallback_discard(&ps, 3);
        assert_eq!(discards.len(), 3);
        // Should start with Brick (most abundant).
        assert_eq!(discards[0], Resource::Brick);
    }

    #[test]
    fn fallback_discard_handles_exact_count() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 2);
        ps.add_resource(Resource::Brick, 2);

        let discards = fallback_discard(&ps, 4);
        assert_eq!(discards.len(), 4);
    }

    #[test]
    fn fallback_discard_handles_zero() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 5);

        let discards = fallback_discard(&ps, 0);
        assert!(discards.is_empty());
    }

    #[test]
    fn index_tool_has_correct_schema() {
        let tool = LlamafilePlayer::index_tool(5);
        match &tool.name {
            genai::chat::ToolName::Custom(s) => assert_eq!(s, "choose_index"),
            _ => panic!("Expected custom tool name"),
        }
    }

    #[test]
    fn resource_tool_has_correct_schema() {
        let tool = LlamafilePlayer::resource_tool();
        match &tool.name {
            genai::chat::ToolName::Custom(s) => assert_eq!(s, "choose_resource"),
            _ => panic!("Expected custom tool name"),
        }
    }

    #[test]
    fn discard_tool_has_correct_schema() {
        let tool = LlamafilePlayer::discard_tool(3);
        match &tool.name {
            genai::chat::ToolName::Custom(s) => assert_eq!(s, "choose_discard"),
            _ => panic!("Expected custom tool name"),
        }
    }

    #[test]
    fn trade_response_tool_has_correct_schema() {
        let tool = LlamafilePlayer::trade_response_tool();
        match &tool.name {
            genai::chat::ToolName::Custom(s) => assert_eq!(s, "respond_to_trade"),
            _ => panic!("Expected custom tool name"),
        }
    }

    #[test]
    fn propose_trade_tool_has_correct_schema() {
        let tool = LlamafilePlayer::propose_trade_tool();
        match &tool.name {
            genai::chat::ToolName::Custom(s) => assert_eq!(s, "propose_trade"),
            _ => panic!("Expected custom tool name"),
        }
    }

    #[test]
    fn llamafile_client_builds_without_panic() {
        let _client = llamafile_client(8080);
    }

    #[test]
    fn with_client_constructor_sets_fields() {
        let client = llamafile_client(8080);
        let player = LlamafilePlayer::with_client(
            "Test".into(),
            LLAMAFILE_MODEL.into(),
            Personality::default(),
            client,
        );
        assert_eq!(player.name, "Test");
        assert_eq!(player.model, LLAMAFILE_MODEL);
    }
}
