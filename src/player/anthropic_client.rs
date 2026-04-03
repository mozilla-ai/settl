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
    /// Explicitly disable streaming (llamafile may default to streaming otherwise).
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
            .timeout(std::time::Duration::from_secs(45))
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
                        // Also check for reasoning inside the tool input itself.
                        let tool_reasoning = input
                            .get("reasoning")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if !tool_reasoning.is_empty() {
                            if !reasoning.is_empty() {
                                reasoning.push('\n');
                            }
                            reasoning.push_str(tool_reasoning);
                        }
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
                    text: "I think option 2 is best.".into(),
                },
                ContentBlock::ToolUse {
                    id: "tu-1".into(),
                    name: "choose_index".into(),
                    input: json!({"reasoning": "Good resources", "index": 2}),
                },
            ],
            model: "test".into(),
            stop_reason: Some("tool_use".into()),
            usage: None,
        };

        let (args, reasoning) = AnthropicClient::extract_tool_call(&response, "choose_index")
            .expect("should find tool call");
        assert_eq!(args["index"], 2);
        assert!(reasoning.contains("I think option 2 is best."));
        assert!(reasoning.contains("Good resources"));
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
}
