use std::time::Duration;

use tracing::{debug, warn};

use crate::api_types::{ApiErrorBody, ChatRequest, ChatResponse};
use crate::config::AppConfig;
use crate::error::{CrabClawError, Result};

const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Send a chat completion request to an OpenAI-compatible endpoint.
pub async fn send_chat_request(config: &AppConfig, request: &ChatRequest) -> Result<ChatResponse> {
    let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));
    debug!(url = %url, model = %request.model, "sending chat request");

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
    debug!(status = %status, "received response");

    if status.is_success() {
        let body = response
            .text()
            .await
            .map_err(|e| CrabClawError::Network(format!("failed to read response body: {e}")))?;
        debug!(body = %body, "raw response body");
        let chat_response: ChatResponse = serde_json::from_str(&body)?;
        return Ok(chat_response);
    }

    // Try to parse the error body for a structured message.
    let body_text = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<ApiErrorBody>(&body_text)
        .ok()
        .and_then(|b| b.error)
        .map(|e| e.message)
        .unwrap_or_else(|| body_text.clone());

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
    use crate::api_types::{ChatRequest, Message};
    use crate::config::AppConfig;

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
        };

        let err = send_chat_request(&config, &request).await.unwrap_err();
        match err {
            CrabClawError::Api(msg) => assert!(msg.contains("500"), "msg: {msg}"),
            other => panic!("expected Api error, got: {other}"),
        }
        mock.assert_async().await;
    }
}
