use serde::{Deserialize, Serialize};

/// A single message in the chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
}

/// Request body for the chat completions endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// A single choice returned by the API.
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
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
        self.choices.first().map(|c| c.message.content.as_str())
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
// Anthropic API types (POST /v1/messages)
// ---------------------------------------------------------------------------

/// Request body for the Anthropic messages endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

/// A single content block in an Anthropic response.
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub text: Option<String>,
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

        let choices = if text.is_empty() {
            vec![]
        } else {
            vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".to_string(),
                    content: text,
                },
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
