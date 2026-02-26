//! OpenAI Codex Responses API provider.
//!
//! Codex models use `https://chatgpt.com/backend-api/codex/responses`
//! with OAuth tokens from ChatGPT subscription (not API keys).
//! This is a completely different format from Chat Completions.

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::core::auth;
use crate::core::error::{CrabClawError, Result};
use crate::llm::api_types::ChatRequest;
use crate::llm::api_types::Message;

const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const DEFAULT_INSTRUCTIONS: &str = "You are CrabClaw, a concise and helpful coding assistant.";

// ---------------------------------------------------------------------------
// Request types (Responses API)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ResponsesRequest {
    model: String,
    input: Vec<ResponsesInput>,
    instructions: String,
    store: bool,
    stream: bool,
    text: TextOptions,
    reasoning: ReasoningOptions,
    include: Vec<String>,
    tool_choice: String,
    parallel_tool_calls: bool,
}

#[derive(Debug, Serialize)]
struct ResponsesInput {
    role: String,
    content: Vec<InputContent>,
}

#[derive(Debug, Serialize)]
struct InputContent {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct TextOptions {
    verbosity: String,
}

#[derive(Debug, Serialize)]
struct ReasoningOptions {
    effort: String,
    summary: String,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    output: Vec<ResponsesOutput>,
    #[serde(default)]
    output_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutput {
    #[serde(default)]
    content: Vec<ResponsesContent>,
}

#[derive(Debug, Deserialize)]
struct ResponsesContent {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

/// Send a Codex Responses API request using OAuth tokens.
pub async fn send_codex_request(
    model: &str,
    request: &ChatRequest,
    system_prompt: Option<&str>,
) -> Result<crate::llm::api_types::ChatResponse> {
    // Get OAuth token
    let tokens = auth::load_tokens().ok_or_else(|| {
        CrabClawError::Auth(
            "Codex models require OAuth login. Run `crabclaw auth login` first.".to_string(),
        )
    })?;

    let access_token = if tokens.is_expired() {
        let refreshed = auth::refresh_access_token(&tokens).await?;
        refreshed.access_token
    } else {
        tokens.access_token.clone()
    };

    // Extract account_id from id_token JWT
    let account_id = tokens
        .id_token
        .as_ref()
        .and_then(|t| extract_account_id_from_jwt(t))
        .ok_or_else(|| {
            CrabClawError::Auth(
                "No account_id in OAuth token. Run `crabclaw auth login` again.".to_string(),
            )
        })?;

    // Build Responses API input
    let instructions = system_prompt
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| DEFAULT_INSTRUCTIONS.to_string());

    let input = build_responses_input(&request.messages);
    let effort = resolve_reasoning_effort(model);

    let body = ResponsesRequest {
        model: model.to_string(),
        input,
        instructions,
        store: false,
        stream: true,
        text: TextOptions {
            verbosity: "medium".to_string(),
        },
        reasoning: ReasoningOptions {
            effort,
            summary: "auto".to_string(),
        },
        include: vec!["reasoning.encrypted_content".to_string()],
        tool_choice: "auto".to_string(),
        parallel_tool_calls: true,
    };

    info!(
        "codex.request model={model} input_count={} instructions_len={}",
        body.input.len(),
        body.instructions.len()
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| CrabClawError::Network(format!("failed to build HTTP client: {e}")))?;

    let response = client
        .post(CODEX_RESPONSES_URL)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("chatgpt-account-id", &account_id)
        .header("OpenAI-Beta", "responses=experimental")
        .header("originator", "pi")
        .header("accept", "text/event-stream")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| CrabClawError::Network(format!("codex request failed: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(CrabClawError::Api(format!(
            "Codex API error (HTTP {status}): {body}"
        )));
    }

    // Parse SSE response
    let body_text = response
        .text()
        .await
        .map_err(|e| CrabClawError::Network(format!("failed to read codex response: {e}")))?;

    let content = parse_sse_text(&body_text)?;

    info!(
        "codex.response content_preview={}",
        content.chars().take(60).collect::<String>()
    );

    Ok(crate::llm::api_types::ChatResponse {
        id: None,
        choices: vec![crate::llm::api_types::Choice {
            index: 0,
            message: Message {
                role: "assistant".to_string(),
                content,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: None,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_responses_input(messages: &[Message]) -> Vec<ResponsesInput> {
    let mut input = Vec::new();
    for msg in messages {
        let content_text = &msg.content;
        match msg.role.as_str() {
            "user" => {
                input.push(ResponsesInput {
                    role: "user".to_string(),
                    content: vec![InputContent {
                        kind: "input_text".to_string(),
                        text: content_text.to_string(),
                    }],
                });
            }
            "assistant" => {
                input.push(ResponsesInput {
                    role: "assistant".to_string(),
                    content: vec![InputContent {
                        kind: "output_text".to_string(),
                        text: content_text.to_string(),
                    }],
                });
            }
            // system messages → handled via instructions field
            // tool messages → skipped (Codex doesn't support tool calling via this API)
            _ => {}
        }
    }
    input
}

fn resolve_reasoning_effort(model: &str) -> String {
    let effort = std::env::var("CODEX_REASONING_EFFORT")
        .unwrap_or_else(|_| "high".to_string())
        .to_ascii_lowercase();

    // Clamp for specific models
    match model {
        "gpt-5-codex" => match effort.as_str() {
            "low" | "medium" | "high" => effort,
            "minimal" => "low".to_string(),
            _ => "high".to_string(),
        },
        m if m.starts_with("gpt-5.1-codex-mini") => {
            if effort == "high" || effort == "xhigh" {
                "high".to_string()
            } else {
                "medium".to_string()
            }
        }
        m if (m.starts_with("gpt-5.2") || m.starts_with("gpt-5.3")) && effort == "minimal" => {
            "low".to_string()
        }
        _ => effort,
    }
}

/// Extract account_id from a JWT id_token payload.
pub fn extract_account_id_from_jwt(token: &str) -> Option<String> {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;

    for key in [
        "account_id",
        "accountId",
        "acct",
        "sub",
        "https://api.openai.com/account_id",
    ] {
        if let Some(value) = claims.get(key).and_then(|v| v.as_str()) {
            if !value.trim().is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn extract_responses_text(response: &ResponsesResponse) -> Option<String> {
    // Try output_text first
    if let Some(text) = &response.output_text {
        if !text.trim().is_empty() {
            return Some(text.clone());
        }
    }
    // Try nested output content
    for item in &response.output {
        for content in &item.content {
            if content.kind.as_deref() == Some("output_text") {
                if let Some(text) = &content.text {
                    if !text.trim().is_empty() {
                        return Some(text.clone());
                    }
                }
            }
        }
    }
    // Try any text content
    for item in &response.output {
        for content in &item.content {
            if let Some(text) = &content.text {
                if !text.trim().is_empty() {
                    return Some(text.clone());
                }
            }
        }
    }
    None
}

/// Parse SSE event stream to extract text response.
fn parse_sse_text(body: &str) -> Result<String> {
    let mut saw_delta = false;
    let mut delta_buf = String::new();
    let mut fallback_text = None;

    for chunk in body.split("\n\n") {
        for line in chunk.lines() {
            let data = match line.strip_prefix("data:") {
                Some(d) => d.trim(),
                None => continue,
            };
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let event: serde_json::Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // Check for errors
            let event_type = event.get("type").and_then(|v| v.as_str());
            if event_type == Some("error") || event_type == Some("response.failed") {
                let msg = event
                    .get("message")
                    .or_else(|| {
                        event
                            .get("response")
                            .and_then(|r| r.get("error"))
                            .and_then(|e| e.get("message"))
                    })
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                return Err(CrabClawError::Api(format!("Codex stream error: {msg}")));
            }

            match event_type {
                Some("response.output_text.delta") => {
                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                        saw_delta = true;
                        delta_buf.push_str(delta);
                    }
                }
                Some("response.output_text.done") if !saw_delta => {
                    if let Some(text) = event.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            fallback_text = Some(text.to_string());
                        }
                    }
                }
                Some("response.completed" | "response.done") => {
                    if let Some(resp) = event.get("response") {
                        if let Ok(parsed) =
                            serde_json::from_value::<ResponsesResponse>(resp.clone())
                        {
                            if let Some(text) = extract_responses_text(&parsed) {
                                if fallback_text.is_none() {
                                    fallback_text = Some(text);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if saw_delta && !delta_buf.is_empty() {
        return Ok(delta_buf);
    }
    if let Some(text) = fallback_text {
        return Ok(text);
    }

    // Try parsing as plain JSON (non-SSE response)
    if let Ok(parsed) = serde_json::from_str::<ResponsesResponse>(body.trim()) {
        if let Some(text) = extract_responses_text(&parsed) {
            return Ok(text);
        }
    }

    Err(CrabClawError::Api(
        "No text content in Codex response".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_account_id_from_valid_jwt() {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        let header = URL_SAFE_NO_PAD.encode("{}");
        let payload = URL_SAFE_NO_PAD.encode(r#"{"account_id":"acct_123"}"#);
        let token = format!("{header}.{payload}.sig");
        assert_eq!(
            extract_account_id_from_jwt(&token).as_deref(),
            Some("acct_123")
        );
    }

    #[test]
    fn extract_account_id_from_sub_claim() {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        let header = URL_SAFE_NO_PAD.encode("{}");
        let payload = URL_SAFE_NO_PAD.encode(r#"{"sub":"user_456"}"#);
        let token = format!("{header}.{payload}.sig");
        assert_eq!(
            extract_account_id_from_jwt(&token).as_deref(),
            Some("user_456")
        );
    }

    #[test]
    fn extract_account_id_missing() {
        assert_eq!(extract_account_id_from_jwt("invalid"), None);
        assert_eq!(extract_account_id_from_jwt("a.b.c"), None);
    }

    #[test]
    fn parse_sse_delta_events() {
        let payload = concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"r1\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(parse_sse_text(payload).unwrap(), "Hello world");
    }

    #[test]
    fn parse_sse_completed_fallback() {
        let payload = "data: {\"type\":\"response.completed\",\"response\":{\"output_text\":\"Done\"}}\ndata: [DONE]\n";
        assert_eq!(parse_sse_text(payload).unwrap(), "Done");
    }

    #[test]
    fn parse_sse_error_event() {
        let payload = "data: {\"type\":\"error\",\"message\":\"rate limited\"}\n\n";
        let err = parse_sse_text(payload).unwrap_err();
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn reasoning_effort_clamping() {
        assert_eq!(resolve_reasoning_effort("gpt-5-codex"), "high");
        assert_eq!(resolve_reasoning_effort("gpt-5.3-codex"), "high");
    }

    #[test]
    fn build_input_from_messages() {
        let messages = vec![
            Message::system("Be helpful"),
            Message::user("hi"),
            Message {
                role: "assistant".to_string(),
                content: "hello!".to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let input = build_responses_input(&messages);
        // system message is excluded (goes into instructions)
        assert_eq!(input.len(), 2);
        assert_eq!(input[0].role, "user");
        assert_eq!(input[0].content[0].kind, "input_text");
        assert_eq!(input[1].role, "assistant");
        assert_eq!(input[1].content[0].kind, "output_text");
    }
}
