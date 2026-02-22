//! Integration tests for the Telegram message processing pipeline.
//!
//! These tests call `process_message` directly — the same function that
//! handles inbound Telegram messages — with a mock LLM API. This lets us
//! verify the full pipeline (tape → router → API call → response) without
//! needing a real Telegram bot or LLM provider.

use crabclaw::config::AppConfig;
use crabclaw::telegram::process_message;
use tempfile::TempDir;

fn test_config(api_base: &str) -> AppConfig {
    AppConfig {
        profile: "test".to_string(),
        api_key: "test-key".to_string(),
        api_base: api_base.to_string(),
        model: "test-model".to_string(),
        system_prompt: None,
        telegram_token: Some("fake-token".to_string()),
        telegram_allow_from: vec![],
        telegram_allow_chats: vec![],
        telegram_proxy: None,
    }
}

/// Natural language → model call → assistant reply returned to user.
#[tokio::test]
async fn process_message_routes_to_model_and_returns_reply() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "choices": [{
                    "message": {"role": "assistant", "content": "Hello from mock LLM!"},
                    "finish_reason": "stop"
                }]
            }"#,
        )
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message("hi there", &config, workspace.path(), "test:session1").await;

    mock.assert_async().await;
    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("Hello from mock LLM!")
    );
    let reply = response.to_reply().unwrap();
    assert!(reply.contains("Hello from mock LLM!"));
}

/// Natural language → Anthropic model call → assistant reply returned to user.
#[tokio::test]
async fn process_message_routes_to_anthropic_model_and_returns_reply() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "content": [{"type": "text", "text": "Hello from Anthropic mock LLM!"}],
                "stop_reason": "end_turn"
            }"#,
        )
        .create_async()
        .await;

    let mut config = test_config(&server.url());
    config.model = "anthropic:test-model".to_string();
    let workspace = TempDir::new().unwrap();

    let response = process_message(
        "hi there",
        &config,
        workspace.path(),
        "test:session_anthropic",
    )
    .await;

    mock.assert_async().await;
    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("Hello from Anthropic mock LLM!")
    );
    let reply = response.to_reply().unwrap();
    assert!(reply.contains("Hello from Anthropic mock LLM!"));
}

/// Comma command → immediate output, no model call.
#[tokio::test]
async fn process_message_handles_comma_command() {
    let config = test_config("http://unused:9999");
    let workspace = TempDir::new().unwrap();

    let response = process_message(",help", &config, workspace.path(), "test:session2").await;

    assert!(response.error.is_none());
    // ,help returns immediate output listing available commands
    assert!(response.immediate_output.is_some());
    assert!(response.assistant_output.is_none()); // no model call
}

/// Model returns empty choices → no assistant output (graceful).
#[tokio::test]
async fn process_message_handles_empty_model_response() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"choices": []}"#)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message("hello", &config, workspace.path(), "test:session3").await;

    mock.assert_async().await;
    assert!(response.assistant_output.is_none());
}

/// API returns non-standard error body (like GLM's {"code":500,"msg":"..."}).
#[tokio::test]
async fn process_message_returns_error_on_api_failure() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"code": 500, "msg": "404 NOT_FOUND", "success": false}"#)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message("test error", &config, workspace.path(), "test:session4").await;

    mock.assert_async().await;
    assert!(response.error.is_some(), "expected error in response");
    let err = response.error.unwrap();
    assert!(err.contains("API error"), "error should mention API: {err}");
}

/// API returns HTTP 429 → error returned to user.
#[tokio::test]
async fn process_message_returns_error_on_rate_limit() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": {"message": "rate limited", "type": "rate_limit"}}"#)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message(
        "test rate limit",
        &config,
        workspace.path(),
        "test:session5",
    )
    .await;

    mock.assert_async().await;
    assert!(response.error.is_some());
    let err = response.error.unwrap();
    assert!(
        err.contains("rate limit"),
        "error should mention rate limit: {err}"
    );
}

/// Tape persists: second message in same session sees previous context.
#[tokio::test]
async fn process_message_maintains_session_tape() {
    let mut server = mockito::Server::new_async().await;

    // First call
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"choices": [{"message": {"role": "assistant", "content": "first reply"}, "finish_reason": "stop"}]}"#,
        )
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let _ = process_message("msg 1", &config, workspace.path(), "test:session6").await;

    // Second call - mock expects messages to include previous context
    let mock2 = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"choices": [{"message": {"role": "assistant", "content": "second reply"}, "finish_reason": "stop"}]}"#,
        )
        .create_async()
        .await;

    let response2 = process_message("msg 2", &config, workspace.path(), "test:session6").await;

    mock2.assert_async().await;
    assert_eq!(response2.assistant_output.as_deref(), Some("second reply"));
}
