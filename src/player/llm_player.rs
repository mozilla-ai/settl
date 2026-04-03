//! LLM player implementation using the Anthropic Messages API.
//!
//! Maintains per-player conversation history for KV cache efficiency and
//! uses tool/function calling for structured responses.

use std::sync::Arc;

use async_trait::async_trait;

use crate::game::actions::{PlayerId, TradeOffer, TradeResponse};
use crate::game::board::{EdgeCoord, HexCoord, Resource, VertexCoord};
use crate::game::state::GameState;
use crate::player::anthropic_client::{
    AnthropicClient, ContentBlock, Message, MessagesRequest, ToolDef,
};
use crate::player::personality::Personality;
use crate::player::prompt;
use crate::player::{Player, PlayerChoice};

/// Maximum conversation history pairs before trimming.
const MAX_HISTORY_PAIRS: usize = 30;

/// Default model name for llamafile (Bonsai-8B).
pub const LLAMAFILE_MODEL: &str = "bonsai";

/// Per-player conversation state.
struct Conversation {
    /// System prompt -- set once at construction, never changes.
    system_prompt: String,
    /// Growing message history (user/assistant pairs).
    messages: Vec<Message>,
    /// Maximum user/assistant pairs to retain.
    max_history_pairs: usize,
}

impl Conversation {
    fn new(system_prompt: String) -> Self {
        Self {
            system_prompt,
            messages: Vec::new(),
            max_history_pairs: MAX_HISTORY_PAIRS,
        }
    }

    /// Append a user message and assistant response to history.
    fn record_exchange(&mut self, user_msg: Message, assistant_msg: Message) {
        self.messages.push(user_msg);
        self.messages.push(assistant_msg);
        self.trim();
    }

    /// Trim oldest pairs if we exceed the cap. Keep the first pair for context.
    fn trim(&mut self) {
        let pair_count = self.messages.len() / 2;
        if pair_count > self.max_history_pairs {
            let to_remove = (pair_count - self.max_history_pairs) * 2;
            // Keep the first 2 messages (first exchange), remove from index 2.
            if self.messages.len() > 2 + to_remove {
                self.messages.drain(2..2 + to_remove);
            } else {
                // If somehow tiny, just keep last max pairs.
                let keep = self.max_history_pairs * 2;
                if self.messages.len() > keep {
                    let start = self.messages.len() - keep;
                    self.messages = self.messages.split_off(start);
                }
            }
        }
    }

    /// Build the full message list for a new request: history + current user message.
    fn build_messages(&self, current_user: &Message) -> Vec<Message> {
        let mut msgs = self.messages.clone();
        msgs.push(current_user.clone());
        msgs
    }
}

/// An LLM-powered player using the Anthropic Messages API.
pub struct LlmPlayer {
    /// Display name (e.g. "Alice", "Bob").
    name: String,
    /// Shared Anthropic client.
    client: Arc<AnthropicClient>,
    /// Personality for system prompt injection.
    personality: Personality,
    /// KV cache slot ID for llamafile (None for cloud API).
    slot_id: Option<usize>,
    /// Maximum retries on parse failure before falling back to random.
    max_retries: usize,
    /// Per-player conversation history.
    conversation: tokio::sync::Mutex<Conversation>,
    /// Extra game context (recent history) injected by the orchestrator.
    extra_context: tokio::sync::Mutex<String>,
    /// Optional channel to stream reasoning text chunks to the UI in real-time.
    reasoning_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

impl LlmPlayer {
    /// Create an LLM player backed by an Anthropic client.
    pub fn new(
        name: String,
        client: Arc<AnthropicClient>,
        personality: Personality,
        slot_id: Option<usize>,
    ) -> Self {
        let system_prompt = prompt::system_prompt_compact(&name, &personality.to_system_prompt());
        Self {
            name,
            client,
            personality,
            slot_id,
            max_retries: 2,
            conversation: tokio::sync::Mutex::new(Conversation::new(system_prompt)),
            extra_context: tokio::sync::Mutex::new(String::new()),
            reasoning_tx: None,
        }
    }

    /// Set the streaming reasoning sender. Called before the game starts.
    pub fn set_reasoning_sender(&mut self, tx: tokio::sync::mpsc::UnboundedSender<String>) {
        self.reasoning_tx = Some(tx);
    }

    // -- Tool definitions (same schemas as before, new ToolDef type) --

    fn index_tool(max_index: usize) -> ToolDef {
        ToolDef {
            name: "choose_index".into(),
            description: "Choose an option by its index number.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "index": {
                        "type": "integer",
                        "description": format!("Index of your choice (0 to {})", max_index.saturating_sub(1)),
                        "minimum": 0,
                        "maximum": max_index.saturating_sub(1)
                    }
                },
                "required": ["index"]
            }),
        }
    }

    fn resource_tool() -> ToolDef {
        ToolDef {
            name: "choose_resource".into(),
            description: "Choose a resource type.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "resource": {
                        "type": "string",
                        "enum": ["Wood", "Brick", "Sheep", "Wheat", "Ore"],
                        "description": "The resource to choose"
                    }
                },
                "required": ["resource"]
            }),
        }
    }

    fn discard_tool(count: usize) -> ToolDef {
        ToolDef {
            name: "choose_discard".into(),
            description: format!("Choose exactly {} resource cards to discard.", count),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
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
                "required": ["cards"]
            }),
        }
    }

    fn propose_trade_tool() -> ToolDef {
        ToolDef {
            name: "propose_trade".into(),
            description:
                "Propose a trade to other players. Specify what you want to give and receive."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
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
                "required": ["give_resource", "give_count", "want_resource", "want_count"]
            }),
        }
    }

    fn trade_response_tool() -> ToolDef {
        ToolDef {
            name: "respond_to_trade".into(),
            description: "Respond to a trade offer: accept or reject.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
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
                "required": ["response"]
            }),
        }
    }

    /// Make an LLM call with retry logic and conversation history.
    ///
    /// Returns `(tool_call_arguments, reasoning)` on success.
    async fn call_with_retry(
        &self,
        user_msg: &str,
        tool: ToolDef,
    ) -> Result<(serde_json::Value, String), String> {
        let tool_name = tool.name.clone();

        log::debug!(
            "[{}] LLM call: tool={} model={}",
            self.name,
            tool_name,
            self.client.model(),
        );
        log::debug!(
            "[{}] user prompt ({} chars):\n{}",
            self.name,
            user_msg.len(),
            user_msg
        );

        for attempt in 0..=self.max_retries {
            let current_user = if attempt == 0 {
                Message::user(user_msg)
            } else {
                Message::user(format!(
                    "{}\n\nYour previous response didn't use the tool correctly. \
                     Please call the {} tool with valid arguments.",
                    user_msg, tool_name,
                ))
            };

            let conversation = self.conversation.lock().await;
            let messages = conversation.build_messages(&current_user);
            let system_prompt = conversation.system_prompt.clone();
            drop(conversation);

            let mut request = MessagesRequest::new(self.client.model(), 4_096);
            request.system = Some(system_prompt);
            request.messages = messages;
            request.tools = vec![tool.clone()];
            request.id_slot = self.slot_id;
            request.cache_prompt = Some(true);
            request.stream = self.reasoning_tx.is_some();

            // On retry, notify the UI that we're retrying.
            if attempt > 0 {
                if let Some(tx) = &self.reasoning_tx {
                    let _ = tx.send("\n[Retrying...]\n".to_string());
                }
            }

            let result = if request.stream {
                self.client
                    .send_message_streaming(&request, self.reasoning_tx.clone())
                    .await
            } else {
                self.client.send_message(&request).await
            };

            match result {
                Ok(response) => {
                    // Log raw response content.
                    for block in &response.content {
                        if let ContentBlock::Text { text } = block {
                            log::debug!("[{}] response text: {}", self.name, text);
                        }
                    }

                    if let Some((args, reasoning)) =
                        AnthropicClient::extract_tool_call(&response, &tool_name)
                    {
                        log::debug!(
                            "[{}] success: tool={} args={} reasoning={}",
                            self.name,
                            tool_name,
                            args,
                            reasoning,
                        );

                        // Record this exchange in conversation history.
                        // Store a compact assistant message with just the tool call.
                        let assistant_msg = Message::assistant_tool_use(
                            format!("call-{}", attempt),
                            &tool_name,
                            args.clone(),
                        );
                        self.conversation
                            .lock()
                            .await
                            .record_exchange(current_user, assistant_msg);

                        return Ok((args, reasoning));
                    }

                    log::warn!(
                        "[{}] attempt {}: no {} tool call in response",
                        self.name,
                        attempt + 1,
                        tool_name,
                    );
                }
                Err(e) => {
                    log::warn!("[{}] attempt {}: API error: {}", self.name, attempt + 1, e);
                    if attempt < self.max_retries {
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
        match args.get("index").and_then(|v| v.as_u64()) {
            Some(i) => (i as usize).min(max.saturating_sub(1)),
            None => {
                log::warn!(
                    "LLM returned non-integer index: {:?}, defaulting to 0",
                    args.get("index")
                );
                0
            }
        }
    }

    /// Parse a resource name string into a Resource enum.
    fn parse_resource(s: &str) -> Resource {
        match s.to_lowercase().as_str() {
            "wood" | "lumber" => Resource::Wood,
            "brick" => Resource::Brick,
            "sheep" | "wool" => Resource::Sheep,
            "wheat" | "grain" => Resource::Wheat,
            "ore" => Resource::Ore,
            _ => {
                log::warn!("LLM returned unknown resource '{}', defaulting to Wood", s);
                Resource::Wood
            }
        }
    }
}

#[async_trait]
impl Player for LlmPlayer {
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
        let extra = self.extra_context.lock().await;
        let user = if extra.is_empty() {
            prompt::turn_prompt(state, player_id, choices, &self.name)
        } else {
            let board_ascii = prompt::ascii_board(&state.board);
            let state_json = prompt::game_state_json(state, player_id);
            format!(
                "BOARD:\n{board_ascii}\n\n\
                 GAME STATE:\n{state_json}\n\n\
                 {extra}\n\n\
                 You are {player_name}.\n\n\
                 LEGAL ACTIONS:\n{choices}\n\n\
                 Choose your action by calling the choose_index tool.",
                player_name = self.name,
                choices = prompt::format_choices(choices),
            )
        };
        let tool = Self::index_tool(choices.len());

        match self.call_with_retry(&user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, choices.len());
                (idx, reasoning)
            }
            Err(_) => {
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
        player_names: &[String],
    ) -> (usize, String) {
        let strategy = self.personality.setup_strategy_text();
        let user =
            prompt::setup_settlement_prompt(state, player_id, round, legal_vertices, player_names);
        let user = format!("SETUP STRATEGY:\n{strategy}\n\n{user}");
        let tool = Self::index_tool(legal_vertices.len());

        match self.call_with_retry(&user, tool).await {
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
        player_names: &[String],
    ) -> (usize, String) {
        let user = prompt::setup_road_prompt(state, player_id, legal_edges, player_names);
        let tool = Self::index_tool(legal_edges.len());

        match self.call_with_retry(&user, tool).await {
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
        let state_json = prompt::game_state_json(state, player_id);
        let hex_list = prompt::format_hex_options(legal_hexes);
        let user = format!(
            "GAME STATE:\n{state_json}\n\n\
             You must move the robber. Choose a hex:\n{hex_list}\n\n\
             Choose by calling the choose_index tool."
        );
        let tool = Self::index_tool(legal_hexes.len());

        match self.call_with_retry(&user, tool).await {
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
        player_names: &[String],
    ) -> (usize, String) {
        let name =
            |p: PlayerId| -> &str { player_names.get(p).map(|s| s.as_str()).unwrap_or("???") };
        let target_list: String = targets
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                format!(
                    "  {}. {} ({} resource cards)",
                    i,
                    name(p),
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

        match self.call_with_retry(&user, tool).await {
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

        match self.call_with_retry(&user, tool).await {
            Ok((args, reasoning)) => {
                if let Some(cards) = args.get("cards").and_then(|v| v.as_array()) {
                    let resources: Vec<Resource> = cards
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(Self::parse_resource)
                        .collect();
                    if resources.len() == count && validate_discard(ps, &resources) {
                        return (resources, reasoning);
                    }
                }
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
        let state_json = prompt::game_state_json(state, player_id);
        let user = format!(
            "GAME STATE:\n{state_json}\n\n\
             {context}\n\n\
             Choose by calling the choose_resource tool."
        );
        let tool = Self::resource_tool();

        match self.call_with_retry(&user, tool).await {
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

        match self.call_with_retry(&user, tool).await {
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
        player_names: &[String],
    ) -> (TradeResponse, String) {
        let from_name = player_names
            .get(offer.from)
            .map(|s| s.as_str())
            .unwrap_or("???");
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
             {from_name} offers a trade:\n\
             They give: {}\n\
             They want: {}\n\
             Message: \"{}\"\n\n\
             Choose by calling the respond_to_trade tool.",
            offering, requesting, offer.message,
        );
        let tool = Self::trade_response_tool();

        match self.call_with_retry(&user, tool).await {
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

/// Validate that the player actually holds the resources being discarded.
fn validate_discard(ps: &crate::game::state::PlayerState, resources: &[Resource]) -> bool {
    let mut counts = std::collections::HashMap::new();
    for &r in resources {
        *counts.entry(r).or_insert(0u32) += 1;
    }
    counts
        .iter()
        .all(|(r, &needed)| ps.resource_count(*r) >= needed)
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
        assert_eq!(LlmPlayer::extract_index(&args, 5), 3);
    }

    #[test]
    fn extract_index_clamps_to_max() {
        let args = json!({"index": 10});
        assert_eq!(LlmPlayer::extract_index(&args, 5), 4);
    }

    #[test]
    fn extract_index_missing_defaults_to_zero() {
        let args = json!({"reasoning": "test"});
        assert_eq!(LlmPlayer::extract_index(&args, 5), 0);
    }

    #[test]
    fn parse_resource_valid() {
        assert_eq!(LlmPlayer::parse_resource("Wood"), Resource::Wood);
        assert_eq!(LlmPlayer::parse_resource("brick"), Resource::Brick);
        assert_eq!(LlmPlayer::parse_resource("SHEEP"), Resource::Sheep);
        assert_eq!(LlmPlayer::parse_resource("Wheat"), Resource::Wheat);
        assert_eq!(LlmPlayer::parse_resource("ore"), Resource::Ore);
    }

    #[test]
    fn parse_resource_synonyms() {
        assert_eq!(LlmPlayer::parse_resource("lumber"), Resource::Wood);
        assert_eq!(LlmPlayer::parse_resource("Wool"), Resource::Sheep);
        assert_eq!(LlmPlayer::parse_resource("grain"), Resource::Wheat);
    }

    #[test]
    fn parse_resource_fallback() {
        assert_eq!(LlmPlayer::parse_resource("invalid"), Resource::Wood);
    }

    #[test]
    fn validate_discard_accepts_valid() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 2);
        ps.add_resource(Resource::Brick, 3);
        assert!(validate_discard(
            &ps,
            &[Resource::Wood, Resource::Brick, Resource::Brick]
        ));
    }

    #[test]
    fn validate_discard_rejects_insufficient() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 1);
        ps.add_resource(Resource::Ore, 1);
        // Trying to discard 2 Wood when we only have 1.
        assert!(!validate_discard(&ps, &[Resource::Wood, Resource::Wood]));
    }

    #[test]
    fn validate_discard_rejects_missing_resource() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 3);
        // Trying to discard Ore when we have none.
        assert!(!validate_discard(&ps, &[Resource::Wood, Resource::Ore]));
    }

    #[test]
    fn fallback_discard_takes_most_abundant_first() {
        let mut ps = PlayerState::new();
        ps.add_resource(Resource::Wood, 1);
        ps.add_resource(Resource::Brick, 3);
        ps.add_resource(Resource::Ore, 2);

        let discards = fallback_discard(&ps, 3);
        assert_eq!(discards.len(), 3);
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
    fn conversation_trimming() {
        let mut conv = Conversation::new("system".into());
        conv.max_history_pairs = 3;

        // Add 5 exchanges (10 messages).
        for i in 0..5 {
            conv.record_exchange(
                Message::user(format!("turn {}", i)),
                Message::assistant_tool_use(
                    format!("call-{}", i),
                    "choose_index",
                    json!({"index": i}),
                ),
            );
        }

        // Should retain first pair + last 2 pairs = 3 pairs = 6 messages.
        assert_eq!(conv.messages.len(), 6);

        // First message should be from turn 0.
        if let ContentBlock::Text { text } = &conv.messages[0].content[0] {
            assert_eq!(text, "turn 0");
        }

        // Last user message should be from turn 4.
        if let ContentBlock::Text { text } = &conv.messages[4].content[0] {
            assert_eq!(text, "turn 4");
        }
    }

    #[test]
    fn conversation_build_messages_appends_current() {
        let mut conv = Conversation::new("system".into());
        conv.record_exchange(
            Message::user("past"),
            Message::assistant_tool_use("id", "tool", json!({})),
        );
        let current = Message::user("now");
        let msgs = conv.build_messages(&current);
        assert_eq!(msgs.len(), 3); // 2 history + 1 current
    }

    #[test]
    fn index_tool_schema_valid() {
        let tool = LlmPlayer::index_tool(5);
        assert_eq!(tool.name, "choose_index");
        assert!(tool.input_schema["properties"]["index"].is_object());
    }

    #[test]
    fn resource_tool_schema_valid() {
        let tool = LlmPlayer::resource_tool();
        assert_eq!(tool.name, "choose_resource");
    }

    #[test]
    fn discard_tool_schema_valid() {
        let tool = LlmPlayer::discard_tool(3);
        assert_eq!(tool.name, "choose_discard");
    }

    #[test]
    fn trade_response_tool_schema_valid() {
        let tool = LlmPlayer::trade_response_tool();
        assert_eq!(tool.name, "respond_to_trade");
    }

    #[test]
    fn propose_trade_tool_schema_valid() {
        let tool = LlmPlayer::propose_trade_tool();
        assert_eq!(tool.name, "propose_trade");
    }
}
