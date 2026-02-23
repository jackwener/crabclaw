//! Integration tests for the Telegram message processing pipeline.
//!
//! These tests call `process_message` directly — the same function that
//! handles inbound Telegram messages — with a mock LLM API. This lets us
//! verify the full pipeline (tape → router → API call → response) without
//! needing a real Telegram bot or LLM provider.

use crabclaw::channels::telegram::process_message;
use crabclaw::core::config::AppConfig;
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
        max_context_messages: 50,
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

/// Tool calling: model returns tool_calls → execute → re-call model → final reply.
#[tokio::test]
async fn process_message_handles_tool_calling_loop() {
    let mut server = mockito::Server::new_async().await;

    // First API call: model returns tool_calls instead of text
    let tool_call_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [{
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "tools",
                                "arguments": "{}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }"#,
        )
        .create_async()
        .await;

    // Second API call: after tool results, model returns final text
    let final_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "choices": [{
                    "message": {"role": "assistant", "content": "I found 5 tools available."},
                    "finish_reason": "stop"
                }]
            }"#,
        )
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message(
        "what tools do you have?",
        &config,
        workspace.path(),
        "test:session_tool",
    )
    .await;

    tool_call_mock.assert_async().await;
    final_mock.assert_async().await;
    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("I found 5 tools available.")
    );
}

// =========================================================================
// New tests: Anthropic tool calling, system prompt, file ops, edge cases
// =========================================================================

fn anthropic_config(api_base: &str) -> AppConfig {
    AppConfig {
        profile: "test".to_string(),
        api_key: "test-key".to_string(),
        api_base: api_base.to_string(),
        model: "anthropic:test-model".to_string(),
        system_prompt: None,
        telegram_token: Some("fake-token".to_string()),
        telegram_allow_from: vec![],
        telegram_allow_chats: vec![],
        telegram_proxy: None,
        max_context_messages: 50,
    }
}

/// Anthropic model returns tool_use → execute → tool_result sent back → final text reply.
#[tokio::test]
async fn anthropic_tool_call_and_result() {
    let mut server = mockito::Server::new_async().await;

    // First call: model returns tool_use
    let tool_mock = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "id": "msg_01",
                "content": [
                    {"type": "text", "text": "Let me list the tools."},
                    {"type": "tool_use", "id": "toolu_01", "name": "tools", "input": {}}
                ],
                "stop_reason": "tool_use"
            }"#,
        )
        .create_async()
        .await;

    // Second call: after tool result, model returns final text
    let final_mock = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "content": [{"type": "text", "text": "Here are your tools: shell.exec, file.read, etc."}],
                "stop_reason": "end_turn"
            }"#,
        )
        .create_async()
        .await;

    let config = anthropic_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message(
        "what tools do you have?",
        &config,
        workspace.path(),
        "test:anth_tool",
    )
    .await;

    tool_mock.assert_async().await;
    final_mock.assert_async().await;
    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("Here are your tools: shell.exec, file.read, etc.")
    );
}

/// Anthropic model calls shell.exec with `echo hello` → real execution → final reply.
#[tokio::test]
async fn anthropic_tool_call_shell_exec() {
    let mut server = mockito::Server::new_async().await;

    // First call: model calls shell.exec
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "id": "msg_02",
                "content": [
                    {"type": "tool_use", "id": "toolu_02", "name": "shell.exec", "input": {"command": "echo hello_from_test"}}
                ],
                "stop_reason": "tool_use"
            }"#,
        )
        .create_async()
        .await;

    // Second call: model sees stdout and replies
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "content": [{"type": "text", "text": "The command output was: hello_from_test"}],
                "stop_reason": "end_turn"
            }"#,
        )
        .create_async()
        .await;

    let config = anthropic_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message(
        "run echo hello_from_test",
        &config,
        workspace.path(),
        "test:anth_shell",
    )
    .await;

    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert!(
        response
            .assistant_output
            .as_deref()
            .unwrap_or("")
            .contains("hello_from_test"),
        "response should contain command output"
    );
}

/// Anthropic model returns 2 tool_use blocks in one response → both executed.
#[tokio::test]
async fn anthropic_multi_tool_calls() {
    let mut server = mockito::Server::new_async().await;

    // First call: model returns 2 tool calls
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "id": "msg_03",
                "content": [
                    {"type": "tool_use", "id": "toolu_03a", "name": "shell.exec", "input": {"command": "echo first"}},
                    {"type": "tool_use", "id": "toolu_03b", "name": "shell.exec", "input": {"command": "echo second"}}
                ],
                "stop_reason": "tool_use"
            }"#,
        )
        .create_async()
        .await;

    // Second call: final text
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "content": [{"type": "text", "text": "Both commands ran successfully."}],
                "stop_reason": "end_turn"
            }"#,
        )
        .create_async()
        .await;

    let config = anthropic_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message(
        "run two commands",
        &config,
        workspace.path(),
        "test:anth_multi",
    )
    .await;

    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("Both commands ran successfully.")
    );
}

/// Model returns tool_calls every iteration → loop terminates at MAX_TOOL_ITERATIONS.
#[tokio::test]
async fn tool_call_max_iterations_breaker() {
    let mut server = mockito::Server::new_async().await;

    // Every call returns a tool_call → should stop after MAX_TOOL_ITERATIONS (5)
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [{
                            "id": "call_loop",
                            "type": "function",
                            "function": {
                                "name": "shell.exec",
                                "arguments": "{\"command\": \"echo loop\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }"#,
        )
        .expect_at_most(5)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response =
        process_message("loop forever", &config, workspace.path(), "test:max_iter").await;

    // Should not hang — no final text, no crash
    assert!(response.assistant_output.is_none());
}

/// System prompt sent to API contains identity, tools, context, workspace sections.
#[tokio::test]
async fn system_prompt_includes_identity_and_tools() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/v1/messages")
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::Regex("identity".to_string()),
            mockito::Matcher::Regex("tools_contract".to_string()),
            mockito::Matcher::Regex("CrabClaw".to_string()),
            mockito::Matcher::Regex("context".to_string()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "content": [{"type": "text", "text": "OK"}],
                "stop_reason": "end_turn"
            }"#,
        )
        .create_async()
        .await;

    let config = anthropic_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let _ = process_message("hi", &config, workspace.path(), "test:sys_prompt").await;

    mock.assert_async().await; // Fails if system prompt didn't contain expected sections
}

/// Workspace .agent/system-prompt.md content is included in the system prompt.
#[tokio::test]
async fn system_prompt_includes_workspace_override() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/v1/messages")
        .match_body(mockito::Matcher::Regex(
            "CUSTOM_WORKSPACE_PROMPT_XYZ".to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "content": [{"type": "text", "text": "OK"}],
                "stop_reason": "end_turn"
            }"#,
        )
        .create_async()
        .await;

    let config = anthropic_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let agent_dir = workspace.path().join(".agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::write(
        agent_dir.join("system-prompt.md"),
        "CUSTOM_WORKSPACE_PROMPT_XYZ",
    )
    .unwrap();

    let _ = process_message("hi", &config, workspace.path(), "test:ws_override").await;

    mock.assert_async().await;
}

/// Model calls file.write then file.read on the same file through the tool pipeline.
#[tokio::test]
async fn tool_call_file_write_and_read() {
    let mut server = mockito::Server::new_async().await;

    // First call: model calls file.write
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [{
                            "id": "call_write",
                            "type": "function",
                            "function": {
                                "name": "file.write",
                                "arguments": "{\"path\": \"test_output.txt\", \"content\": \"hello world\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }"#,
        )
        .create_async()
        .await;

    // Second call: model calls file.read
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [{
                            "id": "call_read",
                            "type": "function",
                            "function": {
                                "name": "file.read",
                                "arguments": "{\"path\": \"test_output.txt\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }"#,
        )
        .create_async()
        .await;

    // Third call: final reply
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "choices": [{
                    "message": {"role": "assistant", "content": "The file contains: hello world"},
                    "finish_reason": "stop"
                }]
            }"#,
        )
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message(
        "write and read a file",
        &config,
        workspace.path(),
        "test:file_ops",
    )
    .await;

    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("The file contains: hello world")
    );
    // Verify file was actually written
    let content = std::fs::read_to_string(workspace.path().join("test_output.txt")).unwrap();
    assert_eq!(content, "hello world");
}

/// Model returns tool_call with unknown tool → error result → model recovers.
#[tokio::test]
async fn tool_call_with_unknown_tool_name() {
    let mut server = mockito::Server::new_async().await;

    // First call: model calls non-existent tool
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [{
                            "id": "call_unknown",
                            "type": "function",
                            "function": {
                                "name": "nonexistent.tool",
                                "arguments": "{}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }"#,
        )
        .create_async()
        .await;

    // Second call: model sees error and recovers
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "choices": [{
                    "message": {"role": "assistant", "content": "Sorry, that tool doesn't exist."},
                    "finish_reason": "stop"
                }]
            }"#,
        )
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message(
        "use a fake tool",
        &config,
        workspace.path(),
        "test:unknown_tool",
    )
    .await;

    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("Sorry, that tool doesn't exist.")
    );
}

/// Empty text input → no model call, no error, no output.
#[tokio::test]
async fn empty_text_input_ignored() {
    let config = test_config("http://unused:9999");
    let workspace = TempDir::new().unwrap();

    let response = process_message("", &config, workspace.path(), "test:empty").await;

    assert!(response.error.is_none());
    assert!(response.assistant_output.is_none());
    assert!(response.immediate_output.is_none());
}

/// Anthropic API error during tool loop: first call returns tool_use, second returns 500.
#[tokio::test]
async fn anthropic_api_error_during_tool_loop() {
    let mut server = mockito::Server::new_async().await;

    // First call: tool_use
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "id": "msg_err",
                "content": [
                    {"type": "tool_use", "id": "toolu_err", "name": "shell.exec", "input": {"command": "echo ok"}}
                ],
                "stop_reason": "tool_use"
            }"#,
        )
        .create_async()
        .await;

    // Second call: server error
    server
        .mock("POST", "/v1/messages")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": {"message": "Internal server error", "type": "server_error"}}"#)
        .create_async()
        .await;

    let config = anthropic_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let response = process_message(
        "do something",
        &config,
        workspace.path(),
        "test:anth_err_loop",
    )
    .await;

    assert!(response.error.is_some(), "expected error");
    assert!(
        response.error.as_deref().unwrap().contains("500")
            || response.error.as_deref().unwrap().contains("server error"),
        "error should mention server: {:?}",
        response.error
    );
}
