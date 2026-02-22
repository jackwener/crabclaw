use std::time::Duration;

use tracing::{debug, warn};

use crate::core::config::AppConfig;
use crate::core::error::{CrabClawError, Result};
use crate::llm::api_types::{AnthropicRequest, ApiErrorBody, ChatRequest, ChatResponse};

/// Non-standard error response (e.g. GLM returns HTTP 200 with error JSON).
#[derive(Debug, serde::Deserialize)]
struct NonStandardError {
    code: Option<i32>,
    msg: Option<String>,
    success: Option<bool>,
}

const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Send a chat completion request, automatically choosing the provider SDK
/// based on the model prefix (`provider:model`).
pub async fn send_chat_request(config: &AppConfig, request: &ChatRequest) -> Result<ChatResponse> {
    if let Some(anthropic_model) = request.model.strip_prefix("anthropic:") {
        send_anthropic_request(config, request, anthropic_model).await
    } else {
        // Assume OpenAI compatible by default
        send_openai_request(config, request).await
    }
}

async fn send_anthropic_request(
    config: &AppConfig,
    request: &ChatRequest,
    model: &str,
) -> Result<ChatResponse> {
    let url = format!("{}/v1/messages", config.api_base.trim_end_matches('/'));
    debug!(url = %url, model = %model, "sending anthropic chat request");

    let mut system_text = String::new();
    let mut messages = Vec::new();

    for msg in &request.messages {
        if msg.role == "system" {
            if !system_text.is_empty() {
                system_text.push('\n');
            }
            system_text.push_str(&msg.content);
        } else {
            messages.push(msg.clone());
        }
    }

    let anth_req = AnthropicRequest {
        model: model.to_string(),
        messages,
        max_tokens: request.max_tokens.unwrap_or(4096),
        system: if system_text.is_empty() {
            None
        } else {
            Some(system_text)
        },
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .build()
        .map_err(|e| CrabClawError::Network(format!("failed to build HTTP client: {e}")))?;

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
        return Ok(anth_resp.into_chat_response());
    }

    handle_error_response(status, &body)
}

async fn send_openai_request(config: &AppConfig, request: &ChatRequest) -> Result<ChatResponse> {
    let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));
    debug!(url = %url, model = %request.model, "sending openai chat request");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .build()
        .map_err(|e| CrabClawError::Network(format!("failed to build HTTP client: {e}")))?;

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(request)
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
            Err(CrabClawError::Api(format!(
                "rate limited (HTTP {status}): {detail}"
            )))
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
    use crate::llm::api_types::{ChatRequest, Message};

    fn test_config(api_base: &str) -> AppConfig {
        AppConfig {
            profile: "test".to_string(),
            api_key: "test-key".to_string(),
            api_base: api_base.to_string(),
            model: "test-model".to_string(),
            system_prompt: None,
            telegram_token: None,
            telegram_allow_from: vec![],
            telegram_allow_chats: vec![],
            telegram_proxy: None,
        }
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
            model: "test".to_string(),
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
            model: "test".to_string(),
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

    // TP-009: HTTP 429 → Api error (rate limit)
    #[tokio::test]
    async fn http_429_returns_rate_limit_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(429)
            .with_body(r#"{"error": {"message": "Rate limit exceeded"}}"#)
            .create_async()
            .await;

        let config = test_config(&server.url());
        let request = ChatRequest {
            model: "test".to_string(),
            messages: vec![Message::user("hello")],
            max_tokens: None,
            tools: None,
        };

        let err = send_chat_request(&config, &request).await.unwrap_err();
        match err {
            CrabClawError::Api(msg) => assert!(msg.contains("rate limited"), "msg: {msg}"),
            other => panic!("expected Api error, got: {other}"),
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
            model: "test".to_string(),
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
            model: "test".to_string(),
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
            model: "test".to_string(),
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
            model: "test".to_string(),
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
            model: "test".to_string(),
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
}
