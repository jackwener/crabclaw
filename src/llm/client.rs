use std::sync::OnceLock;
use std::time::Duration;

use tracing::{debug, info, instrument, warn};

use crate::core::config::AppConfig;
use crate::core::error::{CrabClawError, Result};
use crate::llm::api_types::{
    AnthropicRequest, ApiErrorBody, ChatRequest, ChatResponse, StreamChunk,
};
use futures_util::StreamExt;
use tokio::sync::mpsc;

/// Global shared HTTP client for connection pooling across all requests.
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn get_http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .pool_max_idle_per_host(5)
            .build()
            .expect("failed to build HTTP client")
    })
}

/// Non-standard error response (e.g. GLM returns HTTP 200 with error JSON).
#[derive(Debug, serde::Deserialize)]
struct NonStandardError {
    code: Option<i32>,
    msg: Option<String>,
    success: Option<bool>,
}

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_RETRIES: usize = 3;
const INITIAL_RETRY_DELAY_MS: u64 = 1000;

/// Send a chat completion request, automatically choosing the provider SDK
/// based on the model prefix (`provider:model`).
///
/// Retries on 429 (rate limit) and network errors with exponential backoff.
#[instrument(skip_all, fields(model = %request.model))]
pub async fn send_chat_request(config: &AppConfig, request: &ChatRequest) -> Result<ChatResponse> {
    let mut delay_ms = INITIAL_RETRY_DELAY_MS;

    for attempt in 0..=MAX_RETRIES {
        let result = if let Some(codex_model) = request.model.strip_prefix("codex:") {
            crate::llm::codex::send_codex_request(codex_model, request, None).await
        } else if let Some(anthropic_model) = request.model.strip_prefix("anthropic:") {
            send_anthropic_request(config, request, anthropic_model).await
        } else if request.model.strip_prefix("openai:").is_some() {
            send_openai_request(config, request).await
        } else {
            return Err(CrabClawError::Config(format!(
                "MODEL '{}' must have a provider prefix: openai:<model>, anthropic:<model>, or codex:<model>",
                request.model
            )));
        };

        match &result {
            Err(CrabClawError::RateLimit(_)) if attempt < MAX_RETRIES => {
                warn!(attempt = attempt + 1, delay_ms, "rate limited, retrying");
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms *= 2;
                continue;
            }
            Err(CrabClawError::Network(_)) if attempt < MAX_RETRIES => {
                warn!(attempt = attempt + 1, delay_ms, "network error, retrying");
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms *= 2;
                continue;
            }
            _ => return result,
        }
    }

    unreachable!()
}

/// Send a chat completion request as a stream.
///
/// Retries on 429 (rate limit) and network errors with exponential backoff.
#[instrument(skip_all, fields(model = %request.model))]
pub async fn send_chat_request_stream(
    config: &AppConfig,
    request: &ChatRequest,
) -> Result<mpsc::UnboundedReceiver<Result<StreamChunk>>> {
    let mut delay_ms = INITIAL_RETRY_DELAY_MS;

    for attempt in 0..=MAX_RETRIES {
        // Codex models use the Responses API; wrap in a non-streaming adapter
        if let Some(codex_model) = request.model.strip_prefix("codex:") {
            let result = crate::llm::codex::send_codex_request(codex_model, request, None).await;
            let (tx, rx) = mpsc::unbounded_channel();
            match result {
                Ok(resp) => {
                    let text = resp
                        .choices
                        .first()
                        .map(|c| c.message.content.clone())
                        .unwrap_or_default();
                    let _ = tx.send(Ok(StreamChunk::Content(text)));
                    let _ = tx.send(Ok(StreamChunk::Done));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
            return Ok(rx);
        }

        let result = if let Some(anthropic_model) = request.model.strip_prefix("anthropic:") {
            send_anthropic_request_stream(config, request, anthropic_model).await
        } else if request.model.strip_prefix("openai:").is_some() {
            send_openai_request_stream(config, request).await
        } else {
            return Err(CrabClawError::Config(format!(
                "MODEL '{}' must have a provider prefix: openai:<model>, anthropic:<model>, or codex:<model>",
                request.model
            )));
        };

        match &result {
            Err(CrabClawError::RateLimit(_)) if attempt < MAX_RETRIES => {
                warn!(attempt = attempt + 1, delay_ms, "rate limited, retrying");
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms *= 2;
                continue;
            }
            Err(CrabClawError::Network(_)) if attempt < MAX_RETRIES => {
                warn!(attempt = attempt + 1, delay_ms, "network error, retrying");
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms *= 2;
                continue;
            }
            _ => return result,
        }
    }

    unreachable!()
}

async fn send_anthropic_request(
    config: &AppConfig,
    request: &ChatRequest,
    model: &str,
) -> Result<ChatResponse> {
    let url = format!("{}/v1/messages", config.api_base.trim_end_matches('/'));
    debug!(url = %url, model = %model, "sending anthropic chat request");

    let mut system_text = String::new();
    for msg in &request.messages {
        if msg.role == "system" {
            if !system_text.is_empty() {
                system_text.push('\n');
            }
            system_text.push_str(&msg.content);
        }
    }

    let messages = crate::llm::api_types::convert_messages_for_anthropic(&request.messages);

    let tools = request.tools.as_ref().map(|ts| {
        ts.iter()
            .map(crate::llm::api_types::AnthropicToolDefinition::from)
            .collect::<Vec<_>>()
    });

    let anth_req = AnthropicRequest {
        model: model.to_string(),
        messages,
        max_tokens: request.max_tokens.unwrap_or(4096),
        system: if system_text.is_empty() {
            None
        } else {
            Some(system_text)
        },
        tools,
    };

    info!(
        model = %anth_req.model,
        message_count = anth_req.messages.len(),
        tool_count = anth_req.tools.as_ref().map(|t| t.len()).unwrap_or(0),
        has_system = anth_req.system.is_some(),
        "anthropic.request"
    );

    let client = get_http_client();

    let response = client
        .post(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&anth_req)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                CrabClawError::Network(format!("request timed out after {DEFAULT_TIMEOUT_SECS}s"))
            } else if e.is_connect() {
                CrabClawError::Network(format!("connection failed: {e}"))
            } else {
                CrabClawError::Network(format!("request failed: {e}"))
            }
        })?;

    let status = response.status();
    debug!(status = %status, "received anthropic response");

    let body = response
        .text()
        .await
        .map_err(|e| CrabClawError::Network(format!("failed to read response body: {e}")))?;

    debug!(body = %body, "raw response body");

    if status.is_success() {
        let anth_resp: crate::llm::api_types::AnthropicResponse = serde_json::from_str(&body)?;
        let chat_resp = anth_resp.into_chat_response();
        let has_tool_calls = chat_resp.tool_calls().is_some();
        let content_preview = chat_resp
            .assistant_content()
            .map(|c| crate::core::utils::safe_truncate(c, 100));
        info!(
            has_tool_calls = has_tool_calls,
            content_preview = ?content_preview,
            "anthropic.response"
        );
        return Ok(chat_resp);
    }

    handle_error_response(status, &body)
}

async fn send_anthropic_request_stream(
    config: &AppConfig,
    request: &ChatRequest,
    model: &str,
) -> Result<mpsc::UnboundedReceiver<Result<StreamChunk>>> {
    let url = format!("{}/v1/messages", config.api_base.trim_end_matches('/'));
    debug!(url = %url, model = %model, "sending anthropic chat streaming request");

    let mut system_text = String::new();
    for msg in &request.messages {
        if msg.role == "system" {
            if !system_text.is_empty() {
                system_text.push('\n');
            }
            system_text.push_str(&msg.content);
        }
    }

    let messages = crate::llm::api_types::convert_messages_for_anthropic(&request.messages);

    let tools = request.tools.as_ref().map(|ts| {
        ts.iter()
            .map(crate::llm::api_types::AnthropicToolDefinition::from)
            .collect::<Vec<_>>()
    });

    let anth_req = AnthropicRequest {
        model: model.to_string(),
        messages,
        max_tokens: request.max_tokens.unwrap_or(4096),
        system: if system_text.is_empty() {
            None
        } else {
            Some(system_text)
        },
        tools,
    };

    let client = get_http_client();

    let mut json_val = serde_json::to_value(&anth_req).map_err(CrabClawError::from)?;
    if let Some(obj) = json_val.as_object_mut() {
        obj.insert("stream".to_string(), serde_json::Value::Bool(true));
    }

    let response = client
        .post(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&json_val)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                CrabClawError::Network(format!("request timed out after {DEFAULT_TIMEOUT_SECS}s"))
            } else if e.is_connect() {
                CrabClawError::Network(format!("connection failed: {e}"))
            } else {
                CrabClawError::Network(format!("request failed: {e}"))
            }
        })?;

    let status = response.status();
    debug!(status = %status, "received anthropic stream response headers");

    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error body".to_string());
        return Err(handle_error_response(status, &body).unwrap_err());
    }

    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        use crate::llm::api_types::{
            AnthropicStreamBlock, AnthropicStreamDelta, AnthropicStreamEvent,
        };

        while let Some(chunk_res) = stream.next().await {
            match chunk_res {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    while let Some(pos) = buffer.find("\n\n") {
                        let event_str = buffer[..pos].to_string();
                        buffer.drain(..pos + 2);

                        // Anthropic sends:
                        // event: message_start
                        // data: {"type": ...}
                        // We can just look for "data: " and parse it.
                        for line in event_str.lines() {
                            let line = line.trim();
                            if let Some(data) = line.strip_prefix("data: ") {
                                let data = data.trim();
                                if data == "[DONE]" {
                                    continue; // Anthropic usually doesn't send this, but just in case
                                }

                                match serde_json::from_str::<AnthropicStreamEvent>(data) {
                                    Ok(event) => match event {
                                        AnthropicStreamEvent::ContentBlockStart {
                                            index,
                                            content_block,
                                        } => match content_block {
                                            AnthropicStreamBlock::Text { text } => {
                                                if !text.is_empty() {
                                                    let _ = tx.send(Ok(StreamChunk::Content(text)));
                                                }
                                            }
                                            AnthropicStreamBlock::ToolUse { id, name } => {
                                                let _ = tx.send(Ok(StreamChunk::ToolCallStart {
                                                    index,
                                                    id,
                                                    name,
                                                }));
                                            }
                                        },
                                        AnthropicStreamEvent::ContentBlockDelta {
                                            index,
                                            delta,
                                        } => match delta {
                                            AnthropicStreamDelta::TextDelta { text } => {
                                                if !text.is_empty() {
                                                    let _ = tx.send(Ok(StreamChunk::Content(text)));
                                                }
                                            }
                                            AnthropicStreamDelta::InputJsonDelta {
                                                partial_json,
                                            } => {
                                                let _ =
                                                    tx.send(Ok(StreamChunk::ToolCallArgument {
                                                        index,
                                                        text: partial_json,
                                                    }));
                                            }
                                        },
                                        AnthropicStreamEvent::MessageStop => {
                                            let _ = tx.send(Ok(StreamChunk::Done));
                                            return;
                                        }
                                        AnthropicStreamEvent::Error { error } => {
                                            let _ = tx.send(Err(CrabClawError::Api(format!(
                                                "anthropic stream error: {}",
                                                error.message
                                            ))));
                                            return;
                                        }
                                        _ => {} // Ignore MessageStart, Ping, etc.
                                    },
                                    Err(e) => {
                                        debug!(error = %e, data = %data, "failed to parse anthropic SSE chunk");
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(CrabClawError::Network(format!("stream error: {e}"))));
                    return;
                }
            }
        }
        let _ = tx.send(Ok(StreamChunk::Done));
    });

    Ok(rx)
}

async fn send_openai_request(config: &AppConfig, request: &ChatRequest) -> Result<ChatResponse> {
    let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));
    let model = request
        .model
        .strip_prefix("openai:")
        .unwrap_or(&request.model);
    debug!(url = %url, model = %model, "sending openai chat request");

    let mut api_request = request.clone();
    api_request.model = model.to_string();

    let client = get_http_client();

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&api_request)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                CrabClawError::Network(format!("request timed out after {DEFAULT_TIMEOUT_SECS}s"))
            } else if e.is_connect() {
                CrabClawError::Network(format!("connection failed: {e}"))
            } else {
                CrabClawError::Network(format!("request failed: {e}"))
            }
        })?;

    let status = response.status();
    debug!(status = %status, "received openai response");

    let body = response
        .text()
        .await
        .map_err(|e| CrabClawError::Network(format!("failed to read response body: {e}")))?;
    debug!(body = %body, "raw response body");

    if status.is_success() {
        #[allow(clippy::collapsible_if)]
        if let Ok(ns_err) = serde_json::from_str::<NonStandardError>(&body) {
            if ns_err.success == Some(false) || ns_err.code.is_some_and(|c| c >= 400) {
                let msg = ns_err
                    .msg
                    .unwrap_or_else(|| "unknown API error".to_string());
                let code = ns_err.code.unwrap_or(0);
                warn!(code = code, msg = %msg, "non-standard API error in 200 response");
                return Err(CrabClawError::Api(format!(
                    "API error (code {code}): {msg}"
                )));
            }
        }

        let chat_response: ChatResponse = serde_json::from_str(&body)?;
        return Ok(chat_response);
    }

    handle_error_response(status, &body)
}

async fn send_openai_request_stream(
    config: &AppConfig,
    request: &ChatRequest,
) -> Result<mpsc::UnboundedReceiver<Result<StreamChunk>>> {
    let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));
    let model = request
        .model
        .strip_prefix("openai:")
        .unwrap_or(&request.model);
    debug!(url = %url, model = %model, "sending openai chat streaming request");

    let client = get_http_client();

    let mut api_request = request.clone();
    api_request.model = model.to_string();
    let mut json_val = serde_json::to_value(&api_request).map_err(CrabClawError::from)?;
    if let Some(obj) = json_val.as_object_mut() {
        obj.insert("stream".to_string(), serde_json::Value::Bool(true));
    }

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&json_val)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                CrabClawError::Network(format!("request timed out after {DEFAULT_TIMEOUT_SECS}s"))
            } else if e.is_connect() {
                CrabClawError::Network(format!("connection failed: {e}"))
            } else {
                CrabClawError::Network(format!("request failed: {e}"))
            }
        })?;

    let status = response.status();
    debug!(status = %status, "received openai stream response headers");

    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error body".to_string());
        return Err(handle_error_response(status, &body).unwrap_err());
    }

    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk_res) = stream.next().await {
            match chunk_res {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    // Process double-newline separated SSE events
                    while let Some(pos) = buffer.find("\n\n") {
                        let event = buffer[..pos].to_string();
                        buffer.drain(..pos + 2);

                        for line in event.lines() {
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }
                            if let Some(data) = line.strip_prefix("data: ") {
                                let data = data.trim();
                                if data == "[DONE]" {
                                    let _ = tx.send(Ok(StreamChunk::Done));
                                    return;
                                }

                                match serde_json::from_str::<crate::llm::api_types::ChatStreamChunk>(
                                    data,
                                ) {
                                    Ok(parsed) => {
                                        if let Some(choice) = parsed.choices.first() {
                                            if let Some(content) = choice
                                                .delta
                                                .content
                                                .as_ref()
                                                .filter(|c| !c.is_empty())
                                            {
                                                let _ = tx.send(Ok(StreamChunk::Content(
                                                    content.clone(),
                                                )));
                                            }
                                            if let Some(tool_calls) = &choice.delta.tool_calls {
                                                for tc in tool_calls {
                                                    if let Some(id) = &tc.id {
                                                        // It's the start of a tool call
                                                        if let Some(name) = tc
                                                            .function
                                                            .as_ref()
                                                            .and_then(|f| f.name.as_ref())
                                                        {
                                                            let _ = tx.send(Ok(
                                                                StreamChunk::ToolCallStart {
                                                                    index: tc.index,
                                                                    id: id.clone(),
                                                                    name: name.clone(),
                                                                },
                                                            ));
                                                        }
                                                    }
                                                    if let Some(args) = tc
                                                        .function
                                                        .as_ref()
                                                        .and_then(|f| f.arguments.as_ref())
                                                    {
                                                        if !args.is_empty() {
                                                            let _ = tx.send(Ok(
                                                                StreamChunk::ToolCallArgument {
                                                                    index: tc.index,
                                                                    text: args.clone(),
                                                                },
                                                            ));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // Some providers send weird pings or format differently, optionally warn
                                        debug!(error = %e, data = %data, "failed to parse SSE chunk");
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(CrabClawError::Network(format!("stream error: {e}"))));
                    return;
                }
            }
        }
        let _ = tx.send(Ok(StreamChunk::Done));
    });

    Ok(rx)
}

fn handle_error_response(status: reqwest::StatusCode, body_text: &str) -> Result<ChatResponse> {
    let detail = serde_json::from_str::<ApiErrorBody>(body_text)
        .ok()
        .and_then(|b| b.error)
        .map(|e| e.message)
        .unwrap_or_else(|| body_text.to_string());

    match status.as_u16() {
        401 | 403 => {
            warn!(status = %status, "authentication failure");
            Err(CrabClawError::Auth(format!("HTTP {status}: {detail}")))
        }
        429 => {
            warn!(status = %status, "rate limited");
            Err(CrabClawError::RateLimit(format!("HTTP {status}: {detail}")))
        }
        s if (500..600).contains(&s) => {
            warn!(status = %status, "server error");
            Err(CrabClawError::Api(format!(
                "server error (HTTP {status}): {detail}"
            )))
        }
        _ => {
            warn!(status = %status, "unexpected status");
            Err(CrabClawError::Api(format!("HTTP {status}: {detail}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::AppConfig;
    use crate::llm::api_types::{ChatRequest, Message, StreamChunk};
    use tokio::sync::mpsc;

    fn test_config(api_base: &str) -> AppConfig {
        AppConfig {
            profile: "test".to_string(),
            api_key: "test-key".to_string(),
            api_base: api_base.to_string(),
            model: "openai:test-model".to_string(),
            system_prompt: None,
            telegram_token: None,
            telegram_allow_from: vec![],
            telegram_allow_chats: vec![],
            telegram_proxy: None,
            max_context_messages: 50,
        }
    }

    async fn collect_stream_chunks(
        mut rx: mpsc::UnboundedReceiver<Result<StreamChunk>>,
    ) -> Vec<StreamChunk> {
        let mut out = Vec::new();
        while let Some(item) = rx.recv().await {
            match item {
                Ok(chunk) => {
                    let done = matches!(chunk, StreamChunk::Done);
                    out.push(chunk);
                    if done {
                        break;
                    }
                }
                Err(e) => panic!("stream returned error: {e}"),
            }
        }
        out
    }

    // TP-008: HTTP 401 response → Auth error
    #[tokio::test]
    async fn http_401_returns_auth_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(401)
            .with_body(r#"{"error": {"message": "Invalid API key", "type": "auth_error"}}"#)
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let err = send_chat_request(&config, &request).await.unwrap_err();
        match err {
            CrabClawError::Auth(msg) => assert!(msg.contains("401"), "msg: {msg}"),
            other => panic!("expected Auth error, got: {other}"),
        }
        mock.assert_async().await;
    }

    // TP-009: HTTP 500 response → Api error
    #[tokio::test]
    async fn http_500_returns_api_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(500)
            .with_body(r#"{"error": {"message": "Internal server error"}}"#)
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let err = send_chat_request(&config, &request).await.unwrap_err();
        match err {
            CrabClawError::Api(msg) => assert!(msg.contains("500"), "msg: {msg}"),
            other => panic!("expected Api error, got: {other}"),
        }
        mock.assert_async().await;
    }

    // TP-009: HTTP 429 → rate limit error after retries
    #[tokio::test]
    async fn http_429_returns_rate_limit_error() {
        let mut server = mockito::Server::new_async().await;
        // With retry logic: 1 initial + 3 retries = 4 total requests
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(429)
            .with_body(r#"{"error": {"message": "Rate limit exceeded"}}"#)
            .expect(4)
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let err = send_chat_request(&config, &request).await.unwrap_err();
        match err {
            CrabClawError::RateLimit(msg) => assert!(msg.contains("429"), "msg: {msg}"),
            other => panic!("expected RateLimit error, got: {other}"),
        }
        mock.assert_async().await;
    }

    // Successful response
    #[tokio::test]
    async fn successful_response_returns_chat_response() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "id": "chatcmpl-test",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "Hello there!"},
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 5,
                        "completion_tokens": 3,
                        "total_tokens": 8
                    }
                }"#,
            )
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: Some(100),
            tools: None,
        };

        let resp = send_chat_request(&config, &request)
            .await
            .expect("should succeed");
        assert_eq!(resp.assistant_content(), Some("Hello there!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_403_returns_auth_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(403)
            .with_body(r#"{"error": {"message": "Forbidden"}}"#)
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let err = send_chat_request(&config, &request).await.unwrap_err();
        match err {
            CrabClawError::Auth(msg) => assert!(msg.contains("403"), "msg: {msg}"),
            other => panic!("expected Auth error, got: {other}"),
        }
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_418_returns_api_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(418)
            .with_body("I'm a teapot")
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let err = send_chat_request(&config, &request).await.unwrap_err();
        match err {
            CrabClawError::Api(msg) => assert!(msg.contains("418"), "msg: {msg}"),
            other => panic!("expected Api error, got: {other}"),
        }
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn malformed_json_body_returns_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not valid json")
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let err = send_chat_request(&config, &request).await.unwrap_err();
        match err {
            CrabClawError::Serialization(_) => {} // expected
            other => panic!("expected Serialization error, got: {other}"),
        }
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn empty_error_body_handled() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(500)
            .with_body("")
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let err = send_chat_request(&config, &request).await.unwrap_err();
        match err {
            CrabClawError::Api(msg) => assert!(msg.contains("500"), "msg: {msg}"),
            other => panic!("expected Api error, got: {other}"),
        }
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn openai_stream_content_and_done() {
        let mut server = mockito::Server::new_async().await;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n"
        );

        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test-model".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let rx = send_chat_request_stream(&config, &request)
            .await
            .expect("stream request should succeed");
        let chunks = collect_stream_chunks(rx).await;

        assert_eq!(
            chunks,
            vec![
                StreamChunk::Content("Hello".to_string()),
                StreamChunk::Content(" world".to_string()),
                StreamChunk::Done
            ]
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn openai_stream_tool_calls_and_arguments() {
        let mut server = mockito::Server::new_async().await;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"file.write\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\\\"a.txt\\\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\",\\\"content\\\":\\\"x\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n"
        );

        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test-model".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let rx = send_chat_request_stream(&config, &request)
            .await
            .expect("stream request should succeed");
        let chunks = collect_stream_chunks(rx).await;

        assert_eq!(
            chunks,
            vec![
                StreamChunk::ToolCallStart {
                    index: 0,
                    id: "call_1".to_string(),
                    name: "file.write".to_string()
                },
                StreamChunk::ToolCallArgument {
                    index: 0,
                    text: "{\"path\":\"a.txt\"".to_string()
                },
                StreamChunk::ToolCallArgument {
                    index: 0,
                    text: ",\"content\":\"x\"}".to_string()
                },
                StreamChunk::Done
            ]
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn openai_stream_ignores_malformed_events_and_recovers() {
        let mut server = mockito::Server::new_async().await;
        let body = concat!(
            "data: not json at all\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n"
        );

        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "openai:test-model".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let rx = send_chat_request_stream(&config, &request)
            .await
            .expect("stream request should succeed");
        let chunks = collect_stream_chunks(rx).await;

        assert_eq!(
            chunks,
            vec![StreamChunk::Content("ok".to_string()), StreamChunk::Done]
        );
        mock.assert_async().await;
    }
}
