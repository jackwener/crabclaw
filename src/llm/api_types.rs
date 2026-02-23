use serde::{Deserialize, Serialize};

/// A single message in the chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// Tool calls requested by the assistant (only present when role=assistant).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// ID of the tool call this message is responding to (only when role=tool).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    /// Create an assistant message carrying tool calls (no text content).
    pub fn assistant_with_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: String::new(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool / Function Calling types
// ---------------------------------------------------------------------------

/// A tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

fn default_tool_type() -> String {
    "function".to_string()
}

/// The function inside a tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// Tool definition sent to the API to describe available functions.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// Function metadata within a tool definition.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Request body for the chat completions endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
}

/// A single choice returned by the API.
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

impl Choice {
    /// Check if this choice has tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.message
            .tool_calls
            .as_ref()
            .is_some_and(|tc| !tc.is_empty())
    }
}

/// Token usage statistics.
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
}

/// Response body from the chat completions endpoint.
///
/// All fields are optional or defaulted to handle non-standard API providers
/// (e.g. GLM) that may omit OpenAI-standard fields like `id` or `choices`.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

impl ChatResponse {
    /// Extract the assistant's reply text from the first choice.
    pub fn assistant_content(&self) -> Option<&str> {
        self.choices
            .first()
            .map(|c| c.message.content.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Check if the response contains tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.choices.first().is_some_and(|c| c.has_tool_calls())
    }

    /// Extract tool calls from the first choice.
    pub fn tool_calls(&self) -> Option<&[ToolCall]> {
        self.choices
            .first()
            .and_then(|c| c.message.tool_calls.as_deref())
    }
}

/// Error body returned by the API on failure.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiErrorBody {
    pub error: Option<ApiErrorDetail>,
}

/// Detail inside an API error response.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiErrorDetail {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub code: Option<String>,
}

// ---------------------------------------------------------------------------
// Streaming types (Unified + OpenAI)
// ---------------------------------------------------------------------------

/// Unified stream chunk for cross-provider streaming (OpenAI & Anthropic)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamChunk {
    /// Incremental text output
    Content(String),
    /// A tool call started: name and ID provided
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    /// A chunk of JSON arguments for an ongoing tool call
    ToolCallArgument { index: usize, text: String },
    /// The stream has finished normally
    Done,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamChunk {
    #[serde(default)]
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    #[serde(default)]
    pub index: u32,
    pub delta: StreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamDelta {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamToolCall {
    pub index: usize,
    pub id: Option<String>,
    pub function: Option<StreamToolCallFunction>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamToolCallFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

// ---------------------------------------------------------------------------
// Anthropic API types (POST /v1/messages)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct AnthropicToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl From<&ToolDefinition> for AnthropicToolDefinition {
    fn from(tool: &ToolDefinition) -> Self {
        Self {
            name: tool.function.name.clone(),
            description: tool.function.description.clone(),
            input_schema: tool.function.parameters.clone(),
        }
    }
}

/// Request body for the Anthropic messages endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicToolDefinition>>,
}

/// A message in Anthropic format with structured content blocks.
#[derive(Debug, Clone, Serialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

/// Content can be a plain string or structured blocks.
#[derive(Debug, Clone)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentItem>),
}

impl Serialize for AnthropicContent {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match self {
            AnthropicContent::Text(text) => serializer.serialize_str(text),
            AnthropicContent::Blocks(blocks) => blocks.serialize(serializer),
        }
    }
}

/// A content block item for Anthropic messages.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AnthropicContentItem {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

/// Convert CrabClaw's unified messages to Anthropic format.
///
/// Key conversions:
/// - `system` messages → filtered out (handled via AnthropicRequest.system)
/// - `user`/`assistant` plain text → kept as-is
/// - `assistant` with tool_calls → content blocks with tool_use items
/// - `tool` messages → `user` message with tool_result content blocks
pub fn convert_messages_for_anthropic(messages: &[Message]) -> Vec<AnthropicMessage> {
    let mut result = Vec::new();

    let mut i = 0;
    while i < messages.len() {
        let msg = &messages[i];

        if msg.role == "system" {
            i += 1;
            continue;
        }

        if msg.role == "assistant" {
            if let Some(tool_calls) = &msg.tool_calls {
                // Assistant message with tool calls → structured content blocks
                let mut blocks = Vec::new();
                if !msg.content.is_empty() {
                    blocks.push(AnthropicContentItem::Text {
                        text: msg.content.clone(),
                    });
                }
                for tc in tool_calls {
                    let input: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::json!({}));
                    blocks.push(AnthropicContentItem::ToolUse {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        input,
                    });
                }
                result.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content: AnthropicContent::Blocks(blocks),
                });
                i += 1;
                continue;
            }

            // Plain assistant message
            result.push(AnthropicMessage {
                role: "assistant".to_string(),
                content: AnthropicContent::Text(msg.content.clone()),
            });
            i += 1;
            continue;
        }

        if msg.role == "tool" {
            // Collect consecutive tool messages into a single user message
            let mut blocks = Vec::new();
            while i < messages.len() && messages[i].role == "tool" {
                let tool_msg = &messages[i];
                if let Some(tool_call_id) = &tool_msg.tool_call_id {
                    blocks.push(AnthropicContentItem::ToolResult {
                        tool_use_id: tool_call_id.clone(),
                        content: tool_msg.content.clone(),
                    });
                }
                i += 1;
            }
            result.push(AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Blocks(blocks),
            });
            continue;
        }

        // user or other roles
        result.push(AnthropicMessage {
            role: msg.role.clone(),
            content: AnthropicContent::Text(msg.content.clone()),
        });
        i += 1;
    }

    result
}

/// A single content block in an Anthropic response.
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

/// Response body from the Anthropic messages endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub content: Vec<AnthropicContentBlock>,
    pub stop_reason: Option<String>,
    pub usage: Option<AnthropicUsage>,
}

/// Anthropic token usage.
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicStreamMessage },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: AnthropicStreamBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: usize,
        delta: AnthropicStreamDelta,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: AnthropicMessageDelta,
        usage: AnthropicUsage,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "error")]
    Error { error: AnthropicStreamError },
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicStreamMessage {
    pub id: String,
    pub role: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicStreamBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicStreamDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicMessageDelta {
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicStreamError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

impl AnthropicResponse {
    /// Convert to unified ChatResponse for downstream processing.
    pub fn into_chat_response(self) -> ChatResponse {
        let text: String = self
            .content
            .iter()
            .filter(|b| b.block_type == "text")
            .filter_map(|b| b.text.as_deref())
            .collect::<Vec<_>>()
            .join("");

        let mut tool_calls = Vec::new();
        for block in &self.content {
            #[allow(clippy::collapsible_if)]
            if block.block_type == "tool_use" {
                if let (Some(id), Some(name), Some(input)) = (&block.id, &block.name, &block.input)
                {
                    let arguments = serde_json::to_string(input).unwrap_or_default();
                    tool_calls.push(ToolCall {
                        id: id.clone(),
                        call_type: "function".to_string(),
                        function: ToolCallFunction {
                            name: name.clone(),
                            arguments,
                        },
                    });
                }
            }
        }

        let mut message = Message {
            role: "assistant".to_string(),
            content: text,
            tool_calls: None,
            tool_call_id: None,
        };

        if !tool_calls.is_empty() {
            message.tool_calls = Some(tool_calls);
        }

        let choices = if message.content.is_empty() && message.tool_calls.is_none() {
            vec![]
        } else {
            vec![Choice {
                index: 0,
                message,
                finish_reason: self.stop_reason,
            }]
        };

        let usage = self.usage.map(|u| Usage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            total_tokens: u.input_tokens + u.output_tokens,
        });

        ChatResponse {
            id: self.id,
            choices,
            usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TP-006: Serialize request model
    #[test]
    fn chat_request_serializes_to_expected_shape() {
        let req = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::system("You are helpful."), Message::user("Hello")],
            max_tokens: Some(1024),
            tools: None,
        };

        let json = serde_json::to_value(&req).expect("serialize");
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["max_tokens"], 1024);
        assert_eq!(json["messages"].as_array().unwrap().len(), 2);
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(json["messages"][0]["content"], "You are helpful.");
        assert_eq!(json["messages"][1]["role"], "user");
        assert_eq!(json["messages"][1]["content"], "Hello");
    }

    // TP-006: max_tokens omitted when None
    #[test]
    fn chat_request_omits_null_max_tokens() {
        let req = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("Hi")],
            max_tokens: None,
            tools: None,
        };
        let json = serde_json::to_value(&req).expect("serialize");
        assert!(json.get("max_tokens").is_none());
    }

    // TP-007: Deserialize success response
    #[test]
    fn chat_response_deserializes_from_json() {
        let raw = r#"{
            "id": "chatcmpl-abc123",
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hello!"},
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }"#;

        let resp: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        assert_eq!(resp.id.as_deref(), Some("chatcmpl-abc123"));
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.role, "assistant");
        assert_eq!(resp.choices[0].message.content, "Hello!");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.assistant_content(), Some("Hello!"));

        let usage = resp.usage.expect("usage present");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    // TP-007: Deserialize response without usage
    #[test]
    fn chat_response_handles_missing_usage() {
        let raw = r#"{
            "id": "chatcmpl-xyz",
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hi"},
                    "finish_reason": null
                }
            ]
        }"#;

        let resp: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        assert!(resp.usage.is_none());
        assert!(resp.choices[0].finish_reason.is_none());
    }

    #[test]
    fn assistant_content_empty_choices() {
        let raw = r#"{
            "id": "chatcmpl-empty",
            "choices": []
        }"#;
        let resp: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        assert!(resp.assistant_content().is_none());
    }

    #[test]
    fn message_constructors() {
        let u = Message::user("hi");
        assert_eq!(u.role, "user");
        assert_eq!(u.content, "hi");

        let s = Message::system("be concise");
        assert_eq!(s.role, "system");
        assert_eq!(s.content, "be concise");
    }

    #[test]
    fn api_error_body_deserialization() {
        let raw =
            r#"{"error": {"message": "Rate limit", "type": "rate_limit_error", "code": "429"}}"#;
        let body: ApiErrorBody = serde_json::from_str(raw).expect("deserialize");
        let detail = body.error.expect("error present");
        assert_eq!(detail.message, "Rate limit");
        assert_eq!(detail.error_type.as_deref(), Some("rate_limit_error"));
        assert_eq!(detail.code.as_deref(), Some("429"));
    }

    #[test]
    fn response_minimal_no_id_no_choices() {
        // Some API providers (e.g. GLM) return minimal responses
        let raw = r#"{}"#;
        let resp: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        assert!(resp.id.is_none());
        assert!(resp.choices.is_empty());
        assert!(resp.usage.is_none());
        assert!(resp.assistant_content().is_none());
    }

    #[test]
    fn response_content_only_no_id() {
        // GLM-style response: choices but no id
        let raw = r#"{
            "choices": [{
                "message": {"role": "assistant", "content": "Hello from GLM!"},
                "finish_reason": "stop"
            }]
        }"#;
        let resp: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        assert!(resp.id.is_none());
        assert_eq!(resp.assistant_content(), Some("Hello from GLM!"));
    }

    #[test]
    fn response_partial_usage() {
        // Usage with only some fields present
        let raw = r#"{
            "choices": [{
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"
            }],
            "usage": {"total_tokens": 42}
        }"#;
        let resp: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 42);
    }
}
