#![allow(clippy::collapsible_if)]
//! OpenAI Codex Responses API provider.
//!
//! Codex models use `https://chatgpt.com/backend-api/codex/responses`
//! with OAuth tokens from ChatGPT subscription (not API keys).
//! This is a completely different format from Chat Completions.

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::core::auth;
use crate::core::error::{CrabClawError, Result};
use crate::llm::api_types::{ChatRequest, Message, ToolCall, ToolCallFunction};

const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const DEFAULT_INSTRUCTIONS: &str = "You are CrabClaw, a concise and helpful coding assistant.";

/// Codex API requires tool names matching ^[a-zA-Z0-9_-]+$ (no dots).
/// We convert dots to double-underscores and reverse on response.
fn encode_tool_name(name: &str) -> String {
    name.replace('.', "__")
}

fn decode_tool_name(name: &str) -> String {
    name.replace("__", ".")
}

// ---------------------------------------------------------------------------
// Request types (Responses API)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ResponsesRequest {
    model: String,
    input: Vec<serde_json::Value>,
    instructions: String,
    store: bool,
    stream: bool,
    text: TextOptions,
    reasoning: ReasoningOptions,
    include: Vec<String>,
    tool_choice: String,
    parallel_tool_calls: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<CodexToolDef>,
}

/// Tool definition in the Responses API format.
#[derive(Debug, Clone, Serialize)]
struct CodexToolDef {
    #[serde(rename = "type")]
    tool_type: String,
    name: String,
    description: String,
    parameters: serde_json::Value,
    strict: bool,
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
    output: Vec<serde_json::Value>,
    #[serde(default)]
    output_text: Option<String>,
}

/// Parsed result from SSE stream — may contain text, tool calls, or both.
#[derive(Debug, Default)]
struct ParsedCodexResponse {
    text: String,
    tool_calls: Vec<ToolCall>,
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
    let tools = convert_tools(&request.tools);
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
        tool_choice: if tools.is_empty() {
            "none".to_string()
        } else {
            "auto".to_string()
        },
        parallel_tool_calls: true,
        tools,
    };

    info!(
        "codex.request model={model} input_count={} tools_count={} instructions_len={}",
        body.input.len(),
        body.tools.len(),
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

    let parsed = parse_sse_response(&body_text)?;

    info!(
        "codex.response text_len={} tool_calls={}",
        parsed.text.len(),
        parsed.tool_calls.len()
    );

    let tool_calls = if parsed.tool_calls.is_empty() {
        None
    } else {
        Some(parsed.tool_calls)
    };
    let finish_reason = if tool_calls.is_some() {
        "tool_calls"
    } else {
        "stop"
    };

    Ok(crate::llm::api_types::ChatResponse {
        id: None,
        choices: vec![crate::llm::api_types::Choice {
            index: 0,
            message: Message {
                role: "assistant".to_string(),
                content: parsed.text,
                tool_calls,
                tool_call_id: None,
            },
            finish_reason: Some(finish_reason.to_string()),
        }],
        usage: None,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_responses_input(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut input: Vec<serde_json::Value> = Vec::new();
    for msg in messages {
        match msg.role.as_str() {
            "user" => {
                input.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": msg.content
                    }]
                }));
            }
            "assistant" => {
                // If the assistant message has tool calls, emit function_call items
                if let Some(tool_calls) = &msg.tool_calls {
                    for tc in tool_calls {
                        input.push(serde_json::json!({
                            "type": "function_call",
                            "name": encode_tool_name(&tc.function.name),
                            "arguments": tc.function.arguments,
                            "call_id": tc.id
                        }));
                    }
                }
                // Also emit text content if non-empty
                if !msg.content.trim().is_empty() {
                    input.push(serde_json::json!({
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": msg.content
                        }]
                    }));
                }
            }
            "tool" => {
                // Tool result → function_call_output
                if let Some(call_id) = &msg.tool_call_id {
                    input.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": msg.content
                    }));
                }
            }
            // system messages → handled via instructions field
            _ => {}
        }
    }
    input
}

/// Convert ChatRequest tool definitions to Codex Responses API format.
fn convert_tools(tools: &Option<Vec<crate::llm::api_types::ToolDefinition>>) -> Vec<CodexToolDef> {
    match tools {
        Some(defs) => defs
            .iter()
            .map(|td| CodexToolDef {
                tool_type: "function".to_string(),
                name: encode_tool_name(&td.function.name),
                description: td.function.description.clone(),
                parameters: td.function.parameters.clone(),
                strict: false,
            })
            .collect(),
        None => Vec::new(),
    }
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

/// Extract text and tool calls from a completed ResponsesResponse.
fn extract_from_response(response: &ResponsesResponse) -> ParsedCodexResponse {
    let mut result = ParsedCodexResponse::default();

    // Try output_text first
    if let Some(text) = &response.output_text {
        if !text.trim().is_empty() {
            result.text = text.clone();
        }
    }

    // Scan output items for text content and function calls
    for item in &response.output {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match item_type {
            "function_call" => {
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = item
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                if !name.is_empty() {
                    result.tool_calls.push(ToolCall {
                        id: call_id.to_string(),
                        call_type: "function".to_string(),
                        function: ToolCallFunction {
                            name: decode_tool_name(name),
                            arguments: arguments.to_string(),
                        },
                    });
                }
            }
            "message" => {
                // Nested content array
                if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                    for c in content {
                        if c.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                            if let Some(text) = c.get("text").and_then(|v| v.as_str()) {
                                if !text.trim().is_empty() && result.text.is_empty() {
                                    result.text = text.to_string();
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                // Also check for nested content arrays (older format)
                if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                    for c in content {
                        let ct = c.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if ct == "output_text" {
                            if let Some(text) = c.get("text").and_then(|v| v.as_str()) {
                                if !text.trim().is_empty() && result.text.is_empty() {
                                    result.text = text.to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    result
}

/// Parse SSE event stream to extract text and tool calls.
fn parse_sse_response(body: &str) -> Result<ParsedCodexResponse> {
    let mut text_delta_buf = String::new();
    let mut saw_text_delta = false;
    let mut fallback = ParsedCodexResponse::default();

    // Track function call argument deltas (keyed by output_index)
    let mut fn_call_args: std::collections::HashMap<u64, (String, String, String)> =
        std::collections::HashMap::new(); // index -> (name, call_id, arguments_buf)

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

            let output_index = event
                .get("output_index")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            match event_type {
                // --- Text deltas ---
                Some("response.output_text.delta") => {
                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                        saw_text_delta = true;
                        text_delta_buf.push_str(delta);
                    }
                }
                Some("response.output_text.done") if !saw_text_delta => {
                    if let Some(text) = event.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            fallback.text = text.to_string();
                        }
                    }
                }

                // --- Function call events ---
                Some("response.output_item.added") => {
                    // A new output item is being added; if it's a function_call, track it
                    if let Some(item) = event.get("item") {
                        if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let call_id =
                                item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                            fn_call_args.insert(
                                output_index,
                                (name.to_string(), call_id.to_string(), String::new()),
                            );
                            debug!(
                                "codex: function_call item added idx={output_index} name={name}"
                            );
                        }
                    }
                }
                Some("response.function_call_arguments.delta") => {
                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                        if let Some(entry) = fn_call_args.get_mut(&output_index) {
                            entry.2.push_str(delta);
                        }
                    }
                }
                Some("response.function_call_arguments.done") => {
                    // Finalize this function call
                    let name = event.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let call_id = event.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                    let arguments = event
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}");

                    // Prefer the "done" event's fields, fall back to tracked deltas
                    let final_name = if name.is_empty() {
                        fn_call_args
                            .get(&output_index)
                            .map(|e| e.0.as_str())
                            .unwrap_or("")
                    } else {
                        name
                    };
                    let final_call_id = if call_id.is_empty() {
                        fn_call_args
                            .get(&output_index)
                            .map(|e| e.1.as_str())
                            .unwrap_or("")
                    } else {
                        call_id
                    };
                    let final_args = if arguments != "{}" {
                        arguments.to_string()
                    } else {
                        fn_call_args
                            .get(&output_index)
                            .map(|e| e.2.clone())
                            .unwrap_or_else(|| "{}".to_string())
                    };

                    if !final_name.is_empty() {
                        debug!(
                            "codex: function_call done name={final_name} call_id={final_call_id}"
                        );
                        fallback.tool_calls.push(ToolCall {
                            id: final_call_id.to_string(),
                            call_type: "function".to_string(),
                            function: ToolCallFunction {
                                name: decode_tool_name(final_name),
                                arguments: final_args,
                            },
                        });
                    }
                    fn_call_args.remove(&output_index);
                }

                // --- Completed / done ---
                Some("response.completed" | "response.done") => {
                    if let Some(resp) = event.get("response") {
                        if let Ok(parsed) =
                            serde_json::from_value::<ResponsesResponse>(resp.clone())
                        {
                            let extracted = extract_from_response(&parsed);
                            if fallback.text.is_empty() {
                                fallback.text = extracted.text;
                            }
                            // Merge tool calls from completed event if we didn't get them via SSE
                            if fallback.tool_calls.is_empty() && !extracted.tool_calls.is_empty() {
                                fallback.tool_calls = extracted.tool_calls;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Build final result
    let mut result = ParsedCodexResponse::default();

    if saw_text_delta && !text_delta_buf.is_empty() {
        result.text = text_delta_buf;
    } else if !fallback.text.is_empty() {
        result.text = fallback.text.clone();
    }

    // Merge tool calls from deltas and from completed event
    if !fallback.tool_calls.is_empty() {
        result.tool_calls = fallback.tool_calls;
    }

    // If we have nothing at all, try plain JSON parse
    if result.text.is_empty() && result.tool_calls.is_empty() {
        if let Ok(parsed) = serde_json::from_str::<ResponsesResponse>(body.trim()) {
            let extracted = extract_from_response(&parsed);
            result.text = extracted.text;
            result.tool_calls = extracted.tool_calls;
        }
    }

    if result.text.is_empty() && result.tool_calls.is_empty() {
        return Err(CrabClawError::Api(
            "No text content or tool calls in Codex response".to_string(),
        ));
    }

    Ok(result)
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
        let parsed = parse_sse_response(payload).unwrap();
        assert_eq!(parsed.text, "Hello world");
        assert!(parsed.tool_calls.is_empty());
    }

    #[test]
    fn parse_sse_completed_fallback() {
        let payload = "data: {\"type\":\"response.completed\",\"response\":{\"output_text\":\"Done\"}}\ndata: [DONE]\n";
        let parsed = parse_sse_response(payload).unwrap();
        assert_eq!(parsed.text, "Done");
    }

    #[test]
    fn parse_sse_error_event() {
        let payload = "data: {\"type\":\"error\",\"message\":\"rate limited\"}\n\n";
        let err = parse_sse_response(payload).unwrap_err();
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn parse_sse_function_call() {
        let payload = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"name\":\"file.write\",\"call_id\":\"call_123\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"delta\":\"{\\\"path\\\": \"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"delta\":\"\\\"test.txt\\\"}\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":0,\"name\":\"file.write\",\"call_id\":\"call_123\",\"arguments\":\"{\\\"path\\\": \\\"test.txt\\\"}\"}\n\n",
            "data: [DONE]\n\n",
        );
        let parsed = parse_sse_response(payload).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].function.name, "file.write");
        assert_eq!(parsed.tool_calls[0].id, "call_123");
        assert!(parsed.tool_calls[0].function.arguments.contains("test.txt"));
    }

    #[test]
    fn parse_sse_function_call_from_completed() {
        // Some models put function_call in the response.completed event
        let payload = concat!(
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"function_call\",\"name\":\"shell.exec\",\"call_id\":\"call_456\",\"arguments\":\"{\\\"command\\\":\\\"echo hi\\\"}\"}]}}\n\n",
            "data: [DONE]\n\n",
        );
        let parsed = parse_sse_response(payload).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].function.name, "shell.exec");
        assert_eq!(parsed.tool_calls[0].id, "call_456");
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
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"][0]["type"], "output_text");
    }

    #[test]
    fn build_input_with_tool_calls() {
        let messages = vec![
            Message::user("create a file"),
            Message::assistant_with_tool_calls(vec![ToolCall {
                id: "call_abc".to_string(),
                call_type: "function".to_string(),
                function: ToolCallFunction {
                    name: "file.write".to_string(),
                    arguments: r#"{"path":"test.txt"}"#.to_string(),
                },
            }]),
            Message::tool("call_abc", "File written successfully"),
        ];
        let input = build_responses_input(&messages);
        assert_eq!(input.len(), 3); // user + function_call + function_call_output
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["name"], "file__write");
        assert_eq!(input[1]["call_id"], "call_abc");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_abc");
        assert_eq!(input[2]["output"], "File written successfully");
    }

    #[test]
    fn convert_tools_formats_correctly() {
        use crate::llm::api_types::{FunctionDefinition, ToolDefinition};
        let tools = vec![ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "file.read".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }];
        let codex_tools = convert_tools(&Some(tools));
        assert_eq!(codex_tools.len(), 1);
        assert_eq!(codex_tools[0].name, "file__read");
        assert_eq!(codex_tools[0].tool_type, "function");
        assert!(!codex_tools[0].strict);
    }

    #[test]
    fn convert_tools_empty() {
        assert!(convert_tools(&None).is_empty());
    }
}
