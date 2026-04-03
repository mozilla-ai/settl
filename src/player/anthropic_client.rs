//! Thin HTTP client for the Anthropic Messages API.
//!
//! Speaks the same `/v1/messages` protocol that both the real Anthropic API and
//! llamafile/llama.cpp expose. Includes llamafile-specific extensions (`id_slot`,
//! `cache_prompt`) that are skipped when not set.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Anthropic API version header value.
const ANTHROPIC_VERSION: &str = "2023-06-01";

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// A complete Messages API request.
#[derive(Debug, Clone, Serialize)]
pub struct MessagesRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,
    /// Enable streaming (SSE) responses.
    pub stream: bool,
    // -- llamafile extensions (omitted when None) --
    /// Assign this request to a specific KV cache slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_slot: Option<usize>,
    /// Reuse the KV cache from a previous request in this slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_prompt: Option<bool>,
}

impl MessagesRequest {
    pub fn new(model: impl Into<String>, max_tokens: u32) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            system: None,
            messages: Vec::new(),
            tools: Vec::new(),
            stream: false,
            id_slot: None,
            cache_prompt: None,
        }
    }
}

/// A single message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn assistant_tool_use(
        id: impl Into<String>,
        name: impl Into<String>,
        input: serde_json::Value,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: id.into(),
                name: name.into(),
                input,
            }],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// A content block within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

/// A tool definition for the Messages API.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// The response from a Messages API call.
#[derive(Debug, Clone, Deserialize)]
pub struct MessagesResponse {
    #[serde(default)]
    pub id: String,
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
}

/// An error returned by the API.
#[derive(Debug)]
pub struct ApiError {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(status) = self.status {
            write!(f, "API error {}: {}", status, self.message)
        } else {
            write!(f, "API error: {}", self.message)
        }
    }
}

impl std::error::Error for ApiError {}

// ---------------------------------------------------------------------------
// SSE stream processor
// ---------------------------------------------------------------------------

/// Processes Server-Sent Event lines from an Anthropic streaming response,
/// accumulating content blocks and optionally forwarding text deltas.
struct SseProcessor {
    reasoning_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    content_blocks: Vec<ContentBlock>,
    response_id: String,
    response_model: String,
    stop_reason: Option<String>,
    usage: Option<Usage>,
    current_text: String,
    current_tool_id: String,
    current_tool_name: String,
    current_tool_json: String,
    current_block_type: Option<SseBlockType>,
    buf: String,
    current_event_type: String,
}

#[derive(Clone, Copy)]
enum SseBlockType {
    Text,
    ToolUse,
}

impl SseProcessor {
    fn new(reasoning_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>) -> Self {
        Self {
            reasoning_tx,
            content_blocks: Vec::new(),
            response_id: String::new(),
            response_model: String::new(),
            stop_reason: None,
            usage: None,
            current_text: String::new(),
            current_tool_id: String::new(),
            current_tool_name: String::new(),
            current_tool_json: String::new(),
            current_block_type: None,
            buf: String::new(),
            current_event_type: String::new(),
        }
    }

    /// Feed raw SSE text (may contain partial lines).
    fn feed(&mut self, data: &str) {
        self.buf.push_str(data);

        while let Some(line_end) = self.buf.find('\n') {
            let line = self.buf[..line_end].trim_end_matches('\r').to_string();
            self.buf = self.buf[line_end + 1..].to_string();
            self.process_line(&line);
        }
    }

    fn process_line(&mut self, line: &str) {
        if let Some(event_type) = line.strip_prefix("event: ") {
            self.current_event_type = event_type.to_string();
        } else if let Some(data) = line.strip_prefix("data: ") {
            let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
                return;
            };
            self.process_event(&json);
        }
    }

    fn process_event(&mut self, json: &serde_json::Value) {
        match self.current_event_type.as_str() {
            "message_start" => {
                if let Some(msg) = json.get("message") {
                    self.response_id = msg
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    self.response_model = msg
                        .get("model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(u) = msg.get("usage") {
                        self.usage = serde_json::from_value(u.clone()).ok();
                    }
                }
            }
            "content_block_start" => {
                if let Some(cb) = json.get("content_block") {
                    let block_type = cb.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match block_type {
                        "text" => {
                            self.current_block_type = Some(SseBlockType::Text);
                            self.current_text.clear();
                            let initial = cb.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            if !initial.is_empty() {
                                self.current_text.push_str(initial);
                                if let Some(tx) = &self.reasoning_tx {
                                    let _ = tx.send(initial.to_string());
                                }
                            }
                        }
                        "tool_use" => {
                            self.current_block_type = Some(SseBlockType::ToolUse);
                            self.current_tool_id = cb
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            self.current_tool_name = cb
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            self.current_tool_json.clear();
                        }
                        _ => {
                            self.current_block_type = None;
                        }
                    }
                }
            }
            "content_block_delta" => {
                if let Some(delta) = json.get("delta") {
                    let delta_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match delta_type {
                        "text_delta" => {
                            let text = delta.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            if !text.is_empty() {
                                self.current_text.push_str(text);
                                if let Some(tx) = &self.reasoning_tx {
                                    let _ = tx.send(text.to_string());
                                }
                            }
                        }
                        "input_json_delta" => {
                            let partial = delta
                                .get("partial_json")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            self.current_tool_json.push_str(partial);
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => match self.current_block_type {
                Some(SseBlockType::Text) => {
                    self.content_blocks.push(ContentBlock::Text {
                        text: std::mem::take(&mut self.current_text),
                    });
                    self.current_block_type = None;
                }
                Some(SseBlockType::ToolUse) => {
                    let input: serde_json::Value = serde_json::from_str(&self.current_tool_json)
                        .unwrap_or(serde_json::Value::Object(Default::default()));
                    self.content_blocks.push(ContentBlock::ToolUse {
                        id: std::mem::take(&mut self.current_tool_id),
                        name: std::mem::take(&mut self.current_tool_name),
                        input,
                    });
                    self.current_tool_json.clear();
                    self.current_block_type = None;
                }
                None => {}
            },
            "message_delta" => {
                if let Some(delta) = json.get("delta") {
                    self.stop_reason = delta
                        .get("stop_reason")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
                if let Some(u) = json.get("usage") {
                    if let Ok(u) = serde_json::from_value::<Usage>(u.clone()) {
                        if let Some(ref mut existing) = self.usage {
                            existing.output_tokens = u.output_tokens;
                        } else {
                            self.usage = Some(u);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Consume the processor and return the assembled response.
    fn finish(self) -> MessagesResponse {
        MessagesResponse {
            id: self.response_id,
            content: self.content_blocks,
            model: self.response_model,
            stop_reason: self.stop_reason,
            usage: self.usage,
        }
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// A minimal client for the Anthropic Messages API.
///
/// Works with both the real Anthropic API and local llamafile/llama.cpp servers
/// that expose `/v1/messages`.
pub struct AnthropicClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl AnthropicClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("failed to build HTTP client");
        Arc::new(Self {
            http,
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
        })
    }

    /// The model identifier this client uses.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Send a Messages API request and parse the response.
    pub async fn send_message(
        &self,
        request: &MessagesRequest,
    ) -> Result<MessagesResponse, ApiError> {
        let url = format!("{}/v1/messages", self.base_url);

        log::debug!(
            "AnthropicClient::send_message url={} model={} messages={} tools={}",
            url,
            request.model,
            request.messages.len(),
            request.tools.len(),
        );

        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(request)
            .send()
            .await
            .map_err(|e| ApiError {
                status: None,
                message: format!("HTTP request failed: {}", e),
            })?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| ApiError {
            status: Some(status.as_u16()),
            message: format!("Failed to read response body: {}", e),
        })?;

        log::debug!(
            "AnthropicClient::send_message status={} body_len={}",
            status,
            body.len(),
        );

        if !status.is_success() {
            return Err(ApiError {
                status: Some(status.as_u16()),
                message: body,
            });
        }

        serde_json::from_str(&body).map_err(|e| ApiError {
            status: Some(status.as_u16()),
            message: format!("Failed to parse response: {} -- body: {}", e, body),
        })
    }

    /// Send a streaming Messages API request, forwarding text deltas to `reasoning_tx`
    /// as they arrive. Returns the fully assembled response when the stream completes.
    pub async fn send_message_streaming(
        &self,
        request: &MessagesRequest,
        reasoning_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> Result<MessagesResponse, ApiError> {
        let url = format!("{}/v1/messages", self.base_url);

        log::debug!(
            "AnthropicClient::send_message_streaming url={} model={} messages={} tools={}",
            url,
            request.model,
            request.messages.len(),
            request.tools.len(),
        );

        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(request)
            .send()
            .await
            .map_err(|e| ApiError {
                status: None,
                message: format!("HTTP request failed: {}", e),
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError {
                status: Some(status.as_u16()),
                message: body,
            });
        }

        let mut processor = SseProcessor::new(reasoning_tx);
        let mut resp = resp;
        while let Some(chunk) = resp.chunk().await.map_err(|e| ApiError {
            status: None,
            message: format!("Stream read error: {}", e),
        })? {
            processor.feed(&String::from_utf8_lossy(&chunk));
        }

        let response = processor.finish();
        log::debug!(
            "AnthropicClient::send_message_streaming done: id={} blocks={} stop={:?}",
            response.id,
            response.content.len(),
            response.stop_reason,
        );

        Ok(response)
    }

    /// Extract the first tool call matching `name` from a response.
    ///
    /// Returns `(tool_input_args, reasoning_text)` where reasoning is the text
    /// content preceding the tool call (if any).
    pub fn extract_tool_call(
        response: &MessagesResponse,
        name: &str,
    ) -> Option<(serde_json::Value, String)> {
        let mut reasoning = String::new();

        for block in &response.content {
            match block {
                ContentBlock::Text { text } => {
                    if !reasoning.is_empty() {
                        reasoning.push('\n');
                    }
                    reasoning.push_str(text);
                }
                ContentBlock::ToolUse {
                    name: tool_name,
                    input,
                    ..
                } => {
                    if tool_name == name {
                        return Some((input.clone(), reasoning));
                    }
                }
                _ => {}
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_serialization_omits_none_fields() {
        let req = MessagesRequest::new("test-model", 1024);
        let json = serde_json::to_value(&req).unwrap();
        assert!(!json.as_object().unwrap().contains_key("id_slot"));
        assert!(!json.as_object().unwrap().contains_key("cache_prompt"));
        assert!(!json.as_object().unwrap().contains_key("system"));
        assert!(!json.as_object().unwrap().contains_key("tools"));
        assert_eq!(json["stream"], false);
    }

    #[test]
    fn request_serialization_includes_llamafile_fields() {
        let mut req = MessagesRequest::new("test-model", 1024);
        req.id_slot = Some(2);
        req.cache_prompt = Some(true);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["id_slot"], 2);
        assert_eq!(json["cache_prompt"], true);
    }

    #[test]
    fn message_user_creates_text_block() {
        let msg = Message::user("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn message_assistant_tool_use_creates_correct_block() {
        let msg = Message::assistant_tool_use("id-1", "choose_index", json!({"index": 3}));
        assert_eq!(msg.role, Role::Assistant);
        match &msg.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "id-1");
                assert_eq!(name, "choose_index");
                assert_eq!(input["index"], 3);
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn extract_tool_call_finds_matching_tool() {
        let response = MessagesResponse {
            id: "msg-1".into(),
            content: vec![
                ContentBlock::Text {
                    text: "I think option 2 is best because it has good resources.".into(),
                },
                ContentBlock::ToolUse {
                    id: "tu-1".into(),
                    name: "choose_index".into(),
                    input: json!({"index": 2}),
                },
            ],
            model: "test".into(),
            stop_reason: Some("tool_use".into()),
            usage: None,
        };

        let (args, reasoning) = AnthropicClient::extract_tool_call(&response, "choose_index")
            .expect("should find tool call");
        assert_eq!(args["index"], 2);
        assert!(reasoning.contains("I think option 2 is best"));
    }

    #[test]
    fn extract_tool_call_returns_none_for_missing_tool() {
        let response = MessagesResponse {
            id: "msg-1".into(),
            content: vec![ContentBlock::Text {
                text: "No tool here".into(),
            }],
            model: "test".into(),
            stop_reason: Some("end_turn".into()),
            usage: None,
        };

        assert!(AnthropicClient::extract_tool_call(&response, "choose_index").is_none());
    }

    #[test]
    fn response_deserialization() {
        let json = json!({
            "id": "msg-123",
            "content": [
                {"type": "text", "text": "reasoning here"},
                {"type": "tool_use", "id": "tu-1", "name": "choose_index", "input": {"index": 0}}
            ],
            "model": "bonsai",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });

        let resp: MessagesResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.id, "msg-123");
        assert_eq!(resp.content.len(), 2);
        assert_eq!(resp.usage.unwrap().input_tokens, 100);
    }

    #[test]
    fn content_block_text_round_trip() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "hello");

        let parsed: ContentBlock = serde_json::from_value(json).unwrap();
        match parsed {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn content_block_tool_use_round_trip() {
        let block = ContentBlock::ToolUse {
            id: "tu-1".into(),
            name: "test_tool".into(),
            input: json!({"key": "value"}),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "tool_use");
        assert_eq!(json["name"], "test_tool");

        let parsed: ContentBlock = serde_json::from_value(json).unwrap();
        match parsed {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu-1");
                assert_eq!(name, "test_tool");
                assert_eq!(input["key"], "value");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn tool_def_serialization() {
        let tool = ToolDef {
            name: "choose_index".into(),
            description: "Pick an option".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "index": {"type": "integer"}
                },
                "required": ["index"]
            }),
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["name"], "choose_index");
        assert!(json["input_schema"]["properties"]["index"].is_object());
    }

    #[test]
    fn client_constructor() {
        let client = AnthropicClient::new("http://localhost:8080", "test-key", "test-model");
        assert_eq!(client.model(), "test-model");
    }

    #[test]
    fn sse_processor_assembles_text_and_tool_use() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut proc = SseProcessor::new(Some(tx));

        // Simulate a typical streaming response with text reasoning + tool call.
        proc.feed("event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-123\",\"model\":\"claude\",\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n");
        proc.feed("event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n");
        proc.feed("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"I choose \"}}\n\n");
        proc.feed("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"option 2.\"}}\n\n");
        proc.feed(
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        );
        proc.feed("event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu-1\",\"name\":\"choose_index\"}}\n\n");
        proc.feed("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"index\\\"\"}}\n\n");
        proc.feed("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\": 2}\"}}\n\n");
        proc.feed(
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        );
        proc.feed("event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":50}}\n\n");
        proc.feed("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n");

        let response = proc.finish();
        assert_eq!(response.id, "msg-123");
        assert_eq!(response.model, "claude");
        assert_eq!(response.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(response.content.len(), 2);

        // Verify text block.
        match &response.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "I choose option 2."),
            other => panic!("expected Text, got {:?}", other),
        }

        // Verify tool use block.
        match &response.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu-1");
                assert_eq!(name, "choose_index");
                assert_eq!(input["index"], 2);
            }
            other => panic!("expected ToolUse, got {:?}", other),
        }

        // Verify text deltas were sent through the channel.
        let mut chunks = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            chunks.push(chunk);
        }
        assert_eq!(chunks, vec!["I choose ", "option 2."]);
    }

    #[test]
    fn sse_processor_handles_partial_lines() {
        let mut proc = SseProcessor::new(None);

        // Feed data split across chunk boundaries.
        proc.feed("event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_del");
        proc.feed("ta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n");
        proc.feed(
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        );

        let response = proc.finish();
        assert_eq!(response.content.len(), 1);
        match &response.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            other => panic!("expected Text, got {:?}", other),
        }
    }
}
