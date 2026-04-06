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

/// Maximum conversation history pairs before trimming (large context models).
const MAX_HISTORY_PAIRS: usize = 30;

/// Reduced history limit for small local models (e.g. Bonsai 1.7B with 8K context).
const MAX_HISTORY_PAIRS_SMALL: usize = 3;

/// Model name sent to the llamafile API (works for all local models).
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
    /// Optional reasoning effort level (e.g. "low", "medium", "high", "max").
    effort: Option<String>,
    /// When true, inject an `analysis` field into tool schemas so the model
    /// must write reasoning before its answer. Used for small models (1B)
    /// that skip text content blocks entirely.
    force_tool_reasoning: bool,
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
        // Local llamafile models have limited context per KV cache slot
        // (typically 8K tokens with --parallel 4). Use a small history
        // window to avoid context overflow errors.
        let history_limit = if slot_id.is_some() {
            MAX_HISTORY_PAIRS_SMALL
        } else {
            MAX_HISTORY_PAIRS
        };
        let mut conversation = Conversation::new(system_prompt);
        conversation.max_history_pairs = history_limit;
        Self {
            name,
            client,
            personality,
            slot_id,
            max_retries: 2,
            conversation: tokio::sync::Mutex::new(conversation),
            extra_context: tokio::sync::Mutex::new(String::new()),
            reasoning_tx: None,
            effort: None,
            force_tool_reasoning: false,
        }
    }

    /// Set the streaming reasoning sender. Called before the game starts.
    pub fn set_reasoning_sender(&mut self, tx: tokio::sync::mpsc::UnboundedSender<String>) {
        self.reasoning_tx = Some(tx);
    }

    /// Set the reasoning effort level. Called before the game starts.
    pub fn set_effort(&mut self, effort: String) {
        self.effort = Some(effort);
    }

    /// Force the model to include reasoning inside tool call arguments.
    /// Used for small models (1B) that skip text content blocks.
    pub fn set_force_tool_reasoning(&mut self, force: bool) {
        self.force_tool_reasoning = force;
    }

    // -- Tool definitions --
    //
    // When `force_reasoning` is true an `analysis` field is prepended so the
    // model must write its reasoning before the answer. This is essential for
    // small models (1B) that skip text content blocks entirely.

    fn index_tool(max_index: usize, force_reasoning: bool) -> ToolDef {
        let mut props = serde_json::Map::new();
        let mut required = Vec::new();
        if force_reasoning {
            props.insert(
                "analysis".into(),
                serde_json::json!({
                    "type": "string",
                    "description": "Brief strategic reasoning for your choice (1-2 sentences)"
                }),
            );
            required.push(serde_json::Value::String("analysis".into()));
        }
        props.insert(
            "index".into(),
            serde_json::json!({
                "type": "integer",
                "description": format!("Index of your choice (0 to {})", max_index.saturating_sub(1)),
                "minimum": 0,
                "maximum": max_index.saturating_sub(1)
            }),
        );
        required.push(serde_json::Value::String("index".into()));

        ToolDef {
            name: "choose_index".into(),
            description: "Choose an option by its index number.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": serde_json::Value::Object(props),
                "required": required,
            }),
        }
    }

    fn resource_tool(force_reasoning: bool) -> ToolDef {
        let mut props = serde_json::Map::new();
        let mut required = Vec::new();
        if force_reasoning {
            props.insert(
                "analysis".into(),
                serde_json::json!({
                    "type": "string",
                    "description": "Brief reasoning for choosing this resource (1-2 sentences)"
                }),
            );
            required.push(serde_json::Value::String("analysis".into()));
        }
        props.insert(
            "resource".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["Wood", "Brick", "Sheep", "Wheat", "Ore"],
                "description": "The resource to choose"
            }),
        );
        required.push(serde_json::Value::String("resource".into()));

        ToolDef {
            name: "choose_resource".into(),
            description: "Choose a resource type.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": serde_json::Value::Object(props),
                "required": required,
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

    fn propose_trade_tool(force_reasoning: bool) -> ToolDef {
        let mut props = serde_json::Map::new();
        let mut required = Vec::new();
        if force_reasoning {
            props.insert(
                "analysis".into(),
                serde_json::json!({
                    "type": "string",
                    "description": "Why this trade helps your strategy (1-2 sentences)"
                }),
            );
            required.push(serde_json::Value::String("analysis".into()));
        }
        props.insert(
            "give_resource".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["Wood", "Brick", "Sheep", "Wheat", "Ore"],
                "description": "Resource you are offering"
            }),
        );
        props.insert(
            "give_count".into(),
            serde_json::json!({
                "type": "integer",
                "description": "How many of that resource to offer",
                "minimum": 1,
                "maximum": 10
            }),
        );
        props.insert(
            "want_resource".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["Wood", "Brick", "Sheep", "Wheat", "Ore"],
                "description": "Resource you want in return"
            }),
        );
        props.insert(
            "want_count".into(),
            serde_json::json!({
                "type": "integer",
                "description": "How many of that resource you want",
                "minimum": 1,
                "maximum": 10
            }),
        );
        props.insert(
            "message".into(),
            serde_json::json!({
                "type": "string",
                "description": "A short message to other players about this trade"
            }),
        );
        required.extend([
            serde_json::Value::String("give_resource".into()),
            serde_json::Value::String("give_count".into()),
            serde_json::Value::String("want_resource".into()),
            serde_json::Value::String("want_count".into()),
        ]);

        ToolDef {
            name: "propose_trade".into(),
            description:
                "Propose a trade to other players. Specify what you want to give and receive."
                    .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": serde_json::Value::Object(props),
                "required": required,
            }),
        }
    }

    fn trade_response_tool(force_reasoning: bool) -> ToolDef {
        let mut props = serde_json::Map::new();
        let mut required = Vec::new();
        if force_reasoning {
            props.insert(
                "analysis".into(),
                serde_json::json!({
                    "type": "string",
                    "description": "Why you accept or reject this trade (1-2 sentences)"
                }),
            );
        }
        props.insert(
            "response".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["accept", "reject"],
                "description": "Accept or reject the trade"
            }),
        );
        props.insert(
            "reject_reason".into(),
            serde_json::json!({
                "type": "string",
                "description": "If rejecting, why (optional)"
            }),
        );
        required.push(serde_json::Value::String("response".into()));

        ToolDef {
            name: "respond_to_trade".into(),
            description: "Respond to a trade offer: accept or reject.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": serde_json::Value::Object(props),
                "required": required,
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
            if let Some(ref effort) = self.effort {
                request.output_config = Some(crate::player::anthropic_client::OutputConfig {
                    effort: effort.clone(),
                });
            }

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

                    if let Some((mut args, mut reasoning)) =
                        AnthropicClient::extract_tool_call(&response, &tool_name)
                    {
                        // When force_tool_reasoning is enabled, the model writes
                        // its reasoning inside the tool args as `analysis`. Pull
                        // it out so it shows in the UI and doesn't bloat history.
                        if self.force_tool_reasoning {
                            if let Some(analysis) = args
                                .get("analysis")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                            {
                                if !analysis.is_empty() {
                                    reasoning = analysis;
                                }
                            }
                            // Strip `analysis` from args before recording in
                            // history (saves context tokens).
                            if let Some(obj) = args.as_object_mut() {
                                obj.remove("analysis");
                            }
                        }

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

                    // If the model produced text but no tool call, try to extract
                    // an index from the text itself (small models sometimes write
                    // "I choose option 2" instead of calling the tool).
                    if tool_name == "choose_index" {
                        if let Some(idx) = extract_index_from_text(&response) {
                            let reasoning = response
                                .content
                                .iter()
                                .filter_map(|b| {
                                    if let ContentBlock::Text { text } = b {
                                        Some(text.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            log::debug!(
                                "[{}] extracted index {} from text response",
                                self.name,
                                idx,
                            );
                            let args = serde_json::json!({"index": idx});
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
                    }

                    log::warn!(
                        "[{}] attempt {}: no {} tool call in response",
                        self.name,
                        attempt + 1,
                        tool_name,
                    );
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    log::warn!(
                        "[{}] attempt {}: API error: {}",
                        self.name,
                        attempt + 1,
                        err_msg
                    );

                    // Context overflow: trim history aggressively and retry
                    // immediately rather than waiting.
                    if err_msg.contains("context")
                        || err_msg.contains("too many tokens")
                        || err_msg.contains("exceed")
                    {
                        let mut conv = self.conversation.lock().await;
                        let before = conv.messages.len();
                        // Drop all but the most recent pair (or clear entirely).
                        if conv.messages.len() > 2 {
                            let keep = 2.min(conv.messages.len());
                            let start = conv.messages.len() - keep;
                            conv.messages = conv.messages.split_off(start);
                        } else {
                            conv.messages.clear();
                        }
                        // Also reduce the limit to prevent future overflow.
                        conv.max_history_pairs =
                            conv.max_history_pairs.min(MAX_HISTORY_PAIRS_SMALL);
                        log::info!(
                            "[{}] context overflow: trimmed history {}->{}, limit now {}",
                            self.name,
                            before,
                            conv.messages.len(),
                            conv.max_history_pairs,
                        );
                        drop(conv);
                        // Retry immediately -- no backoff needed for context overflow.
                        continue;
                    }

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

    /// Pre-filter vertices to the top `max` candidates by heuristic score.
    ///
    /// Returns the filtered vertex list and a mapping from filtered indices
    /// back to original indices in `legal_vertices`.
    fn filter_top_vertices(
        state: &GameState,
        legal_vertices: &[VertexCoord],
        max: usize,
    ) -> (Vec<VertexCoord>, Vec<usize>) {
        if legal_vertices.len() <= max {
            let map: Vec<usize> = (0..legal_vertices.len()).collect();
            return (legal_vertices.to_vec(), map);
        }

        // Score each vertex and sort by score descending.
        let mut scored: Vec<(usize, i32)> = legal_vertices
            .iter()
            .enumerate()
            .map(|(i, v)| (i, prompt::score_vertex(v, state)))
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.truncate(max);

        // Preserve original ordering within the top set so indices feel natural.
        scored.sort_by_key(|(i, _)| *i);

        let filtered: Vec<VertexCoord> = scored.iter().map(|(i, _)| legal_vertices[*i]).collect();
        let index_map: Vec<usize> = scored.iter().map(|(i, _)| *i).collect();

        (filtered, index_map)
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
        let tool = Self::index_tool(choices.len(), self.force_tool_reasoning);

        match self.call_with_retry(&user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, choices.len());
                (idx, reasoning)
            }
            Err(_) => {
                use rand::RngExt;
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
        // Pre-filter to top candidates by heuristic score.
        // Small models choke on 50+ options; showing only the best ones
        // makes the choice tractable.
        const MAX_OPTIONS: usize = 8;
        let (filtered_vertices, index_map) =
            Self::filter_top_vertices(state, legal_vertices, MAX_OPTIONS);

        let strategy = self.personality.setup_strategy_text();
        let user = prompt::setup_settlement_prompt(
            state,
            player_id,
            round,
            &filtered_vertices,
            player_names,
        );
        let user = format!("SETUP STRATEGY:\n{strategy}\n\n{user}");
        let tool = Self::index_tool(filtered_vertices.len(), self.force_tool_reasoning);

        match self.call_with_retry(&user, tool).await {
            Ok((args, reasoning)) => {
                let filtered_idx = Self::extract_index(&args, filtered_vertices.len());
                let original_idx = index_map[filtered_idx];
                (original_idx, reasoning)
            }
            Err(_) => {
                // Random fallback still picks from the filtered (good) set.
                use rand::RngExt;
                let filtered_idx = rand::rng().random_range(0..filtered_vertices.len());
                let original_idx = index_map[filtered_idx];
                (original_idx, "[AI was confused and acted randomly]".into())
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
        let tool = Self::index_tool(legal_edges.len(), self.force_tool_reasoning);

        match self.call_with_retry(&user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, legal_edges.len());
                (idx, reasoning)
            }
            Err(_) => {
                use rand::RngExt;
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
        let tool = Self::index_tool(legal_hexes.len(), self.force_tool_reasoning);

        match self.call_with_retry(&user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, legal_hexes.len());
                (idx, reasoning)
            }
            Err(_) => {
                use rand::RngExt;
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
        let tool = Self::index_tool(targets.len(), self.force_tool_reasoning);

        match self.call_with_retry(&user, tool).await {
            Ok((args, reasoning)) => {
                let idx = Self::extract_index(&args, targets.len());
                (idx, reasoning)
            }
            Err(_) => {
                use rand::RngExt;
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
        let tool = Self::resource_tool(self.force_tool_reasoning);

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
        let tool = Self::propose_trade_tool(self.force_tool_reasoning);

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
        let tool = Self::trade_response_tool(self.force_tool_reasoning);

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

/// Try to extract a chosen index from free-text response content.
///
/// Small models sometimes write "I choose option 5" or "Vertex 5" instead of
/// calling the tool. This lets us recover a valid answer from the text.
fn extract_index_from_text(
    response: &crate::player::anthropic_client::MessagesResponse,
) -> Option<usize> {
    let text: String = response
        .content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if text.is_empty() {
        return None;
    }

    let lower = text.to_lowercase();

    // Look for patterns like "index": 5, index: 5, index = 5
    for keyword in &["index", "choose", "option", "vertex", "pick", "select"] {
        if let Some(pos) = lower.find(keyword) {
            let after = &text[pos + keyword.len()..];
            // Skip non-digit chars (whitespace, quotes, colons, equals)
            if let Some(num) = extract_first_number(after) {
                return Some(num);
            }
        }
    }

    // Last resort: look for the last standalone number in the text.
    // This catches "Final Decision: Alice should place at **Vertex 5**."
    let mut last_num = None;
    for word in text.split(|c: char| !c.is_ascii_digit()) {
        if let Ok(n) = word.parse::<usize>() {
            if n < 100 {
                last_num = Some(n);
            }
        }
    }
    last_num
}

/// Extract the first number from a string, skipping leading punctuation/whitespace.
fn extract_first_number(s: &str) -> Option<usize> {
    let mut skipped = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() {
            // Found start of number, collect consecutive digits.
            let num_str: String = s[i..]
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            return num_str.parse().ok();
        }
        skipped += 1;
        if skipped > 10 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::{HexCoord, VertexCoord, VertexDirection};
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
        let tool = LlmPlayer::index_tool(5, false);
        assert_eq!(tool.name, "choose_index");
        assert!(tool.input_schema["properties"]["index"].is_object());
        // Without force_reasoning, no analysis field.
        assert!(tool.input_schema["properties"]["analysis"].is_null());
    }

    #[test]
    fn index_tool_schema_with_analysis() {
        let tool = LlmPlayer::index_tool(5, true);
        assert_eq!(tool.name, "choose_index");
        assert!(tool.input_schema["properties"]["analysis"].is_object());
        assert!(tool.input_schema["properties"]["index"].is_object());
        // analysis should come before index in the serialized JSON.
        let json = serde_json::to_string(&tool.input_schema).unwrap();
        let analysis_pos = json.find("analysis").unwrap();
        let index_pos = json.find("\"index\"").unwrap();
        assert!(
            analysis_pos < index_pos,
            "analysis ({}) should appear before index ({}) in JSON: {}",
            analysis_pos,
            index_pos,
            json,
        );
        // analysis should be required.
        let required = tool.input_schema["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("analysis")));
    }

    #[test]
    fn resource_tool_schema_valid() {
        let tool = LlmPlayer::resource_tool(false);
        assert_eq!(tool.name, "choose_resource");
    }

    #[test]
    fn discard_tool_schema_valid() {
        let tool = LlmPlayer::discard_tool(3);
        assert_eq!(tool.name, "choose_discard");
    }

    #[test]
    fn trade_response_tool_schema_valid() {
        let tool = LlmPlayer::trade_response_tool(false);
        assert_eq!(tool.name, "respond_to_trade");
    }

    #[test]
    fn propose_trade_tool_schema_valid() {
        let tool = LlmPlayer::propose_trade_tool(false);
        assert_eq!(tool.name, "propose_trade");
    }

    #[test]
    fn filter_top_vertices_returns_all_when_under_max() {
        let board = crate::game::board::Board::default_board();
        let state = GameState::new(board, 2);
        let vertices: Vec<VertexCoord> = vec![
            VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North),
            VertexCoord::new(HexCoord::new(1, 0), VertexDirection::South),
        ];
        let (filtered, map) = LlmPlayer::filter_top_vertices(&state, &vertices, 10);
        assert_eq!(filtered.len(), 2);
        assert_eq!(map, vec![0, 1]);
    }

    #[test]
    fn filter_top_vertices_selects_best() {
        let board = crate::game::board::Board::default_board();
        let state = GameState::new(board, 2);
        let all_vertices = crate::game::rules::legal_setup_vertices(&state);
        assert!(
            all_vertices.len() > 10,
            "should have many vertices on empty board"
        );

        let (filtered, map) = LlmPlayer::filter_top_vertices(&state, &all_vertices, 5);
        assert_eq!(filtered.len(), 5);
        assert_eq!(map.len(), 5);

        // All returned indices should be valid.
        for &idx in &map {
            assert!(idx < all_vertices.len());
        }

        // Filtered vertices should all be high quality (score >= median).
        let scores: Vec<i32> = all_vertices
            .iter()
            .map(|v| prompt::score_vertex(v, &state))
            .collect();
        let mut sorted_scores = scores.clone();
        sorted_scores.sort();
        let median = sorted_scores[sorted_scores.len() / 2];

        for &idx in &map {
            assert!(
                scores[idx] >= median,
                "filtered vertex {} (score {}) should be above median ({})",
                idx,
                scores[idx],
                median,
            );
        }
    }

    #[test]
    fn extract_first_number_basic() {
        assert_eq!(extract_first_number(": 5"), Some(5));
        assert_eq!(extract_first_number("= 3}"), Some(3));
        assert_eq!(extract_first_number(" 12"), Some(12));
        assert_eq!(extract_first_number("\"7\""), Some(7));
    }

    #[test]
    fn extract_first_number_skips_too_far() {
        assert_eq!(extract_first_number("a very long string before 5"), None);
    }

    #[test]
    fn extract_first_number_empty() {
        assert_eq!(extract_first_number(""), None);
        assert_eq!(extract_first_number("no numbers"), None);
    }

    #[test]
    fn extract_index_from_text_with_keyword() {
        use crate::player::anthropic_client::{ContentBlock, MessagesResponse};

        let response = MessagesResponse {
            id: String::new(),
            content: vec![ContentBlock::Text {
                text: "I choose option 5 because it has the best pips.".into(),
            }],
            model: String::new(),
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        assert_eq!(extract_index_from_text(&response), Some(5));
    }

    #[test]
    fn extract_index_from_text_with_index_keyword() {
        use crate::player::anthropic_client::{ContentBlock, MessagesResponse};

        let response = MessagesResponse {
            id: String::new(),
            content: vec![ContentBlock::Text {
                text: "After analysis, index: 3 is the best.".into(),
            }],
            model: String::new(),
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        assert_eq!(extract_index_from_text(&response), Some(3));
    }

    #[test]
    fn extract_index_from_text_vertex() {
        use crate::player::anthropic_client::{ContentBlock, MessagesResponse};

        let response = MessagesResponse {
            id: String::new(),
            content: vec![ContentBlock::Text {
                text: "Alice should place at **Vertex 5**.".into(),
            }],
            model: String::new(),
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        assert_eq!(extract_index_from_text(&response), Some(5));
    }

    #[test]
    fn extract_index_from_text_empty() {
        use crate::player::anthropic_client::{ContentBlock, MessagesResponse};

        let response = MessagesResponse {
            id: String::new(),
            content: vec![],
            model: String::new(),
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        assert_eq!(extract_index_from_text(&response), None);
    }

    #[test]
    fn extract_index_from_text_last_number_fallback() {
        use crate::player::anthropic_client::{ContentBlock, MessagesResponse};

        let response = MessagesResponse {
            id: String::new(),
            content: vec![ContentBlock::Text {
                text: "The best location with 12 pips and 3 resources is number 5".into(),
            }],
            model: String::new(),
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        assert_eq!(extract_index_from_text(&response), Some(5));
    }

    #[test]
    fn small_context_history_limit() {
        let mut conv = Conversation::new("system".into());
        conv.max_history_pairs = MAX_HISTORY_PAIRS_SMALL;

        for i in 0..10 {
            conv.record_exchange(
                Message::user(format!("turn {}", i)),
                Message::assistant_tool_use(
                    format!("call-{}", i),
                    "choose_index",
                    json!({"index": i}),
                ),
            );
        }

        let pair_count = conv.messages.len() / 2;
        assert!(
            pair_count <= MAX_HISTORY_PAIRS_SMALL,
            "should have at most {} pairs, got {}",
            MAX_HISTORY_PAIRS_SMALL,
            pair_count,
        );
    }
}
