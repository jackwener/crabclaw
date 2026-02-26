//! Integration tests for the AgentLoop abstraction.
//!
//! These tests verify the full AgentLoop pipeline (route → model → tool → tape)
//! using mock LLM servers. They cover both non-streaming (`handle_input`) and
//! streaming (`handle_input_stream`) paths.
//!
//! Run with:
//!   cargo test --test agent_loop_integration

use crabclaw::core::agent_loop::AgentLoop;
use crabclaw::core::config::AppConfig;
use tempfile::TempDir;

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
        max_context_messages: 50,
    }
}

// ---------------------------------------------------------------------------
// Helper: build mock response bodies
// ---------------------------------------------------------------------------

fn text_response(content: &str) -> String {
    format!(
        r#"{{"choices":[{{"message":{{"role":"assistant","content":"{content}"}},"finish_reason":"stop"}}]}}"#
    )
}

fn tool_call_response(tool_name: &str, call_id: &str, arguments: &str) -> String {
    let args_escaped = arguments.replace('"', "\\\"");
    format!(
        r#"{{"choices":[{{"message":{{"role":"assistant","content":"","tool_calls":[{{"id":"{call_id}","type":"function","function":{{"name":"{tool_name}","arguments":"{args_escaped}"}}}}]}},"finish_reason":"tool_calls"}}]}}"#
    )
}

fn sse_stream(chunks: &[&str]) -> String {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str(&format!("data: {chunk}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body
}

fn sse_content_chunk(text: &str) -> String {
    format!(r#"{{"choices":[{{"delta":{{"content":"{text}"}},"finish_reason":null}}]}}"#)
}

fn sse_tool_call_start(index: usize, id: &str, name: &str) -> String {
    format!(
        r#"{{"choices":[{{"delta":{{"tool_calls":[{{"index":{index},"id":"{id}","function":{{"name":"{name}","arguments":""}}}}]}},"finish_reason":null}}]}}"#
    )
}

fn sse_tool_call_args(index: usize, args: &str) -> String {
    let args_escaped = args.replace('"', "\\\"");
    format!(
        r#"{{"choices":[{{"delta":{{"tool_calls":[{{"index":{index},"function":{{"arguments":"{args_escaped}"}}}}]}},"finish_reason":null}}]}}"#
    )
}

// ============================================================================
// Test 1: Natural language → model call → assistant reply
// ============================================================================

#[tokio::test]
async fn agent_loop_routes_to_model_and_returns_reply() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("Hello from AgentLoop!"))
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_session").unwrap();
    let result = agent.handle_input("hello").await;

    mock.assert_async().await;
    assert!(
        result.error.is_none(),
        "unexpected error: {:?}",
        result.error
    );
    assert_eq!(
        result.assistant_output.as_deref(),
        Some("Hello from AgentLoop!")
    );
    assert!(!result.exit_requested);
    assert_eq!(result.tool_rounds, 0);
}

// ============================================================================
// Test 2: Streaming — tokens delivered via callback
// ============================================================================

#[tokio::test]
async fn agent_loop_streams_tokens_via_callback() {
    let mut server = mockito::Server::new_async().await;

    let stream_body = sse_stream(&[
        &sse_content_chunk("Hello"),
        &sse_content_chunk(", "),
        &sse_content_chunk("world!"),
    ]);

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(stream_body)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_stream").unwrap();
    let mut tokens: Vec<String> = Vec::new();
    let result = agent
        .handle_input_stream("hi", |token| {
            tokens.push(token.to_string());
        })
        .await;

    assert!(
        result.error.is_none(),
        "unexpected error: {:?}",
        result.error
    );
    assert_eq!(tokens, vec!["Hello", ", ", "world!"]);
    assert_eq!(result.assistant_output.as_deref(), Some("Hello, world!"));
}

// ============================================================================
// Test 3: Tool calling loop (non-streaming)
// ============================================================================

#[tokio::test]
async fn agent_loop_tool_calling_loop() {
    let mut server = mockito::Server::new_async().await;

    // First call: model requests a tool call
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("tools", "call_1", "{}"))
        .create_async()
        .await;

    // Second call: model returns final text after getting tool result
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("Found 5 tools."))
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_tools").unwrap();
    let result = agent.handle_input("what tools?").await;

    assert!(
        result.error.is_none(),
        "unexpected error: {:?}",
        result.error
    );
    assert_eq!(result.assistant_output.as_deref(), Some("Found 5 tools."));
    assert_eq!(result.tool_rounds, 1);
}

// ============================================================================
// Test 4: Tool calling max iterations breaker
// ============================================================================

#[tokio::test]
async fn agent_loop_tool_max_iterations() {
    let mut server = mockito::Server::new_async().await;

    // Every call returns a tool call → loop should stop at max iterations (5)
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("tools", "call_loop", "{}"))
        .expect_at_most(5)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_max_iter").unwrap();
    let result = agent.handle_input("infinite loop?").await;

    // Should terminate after 5 iterations, tool_rounds = 5
    assert_eq!(result.tool_rounds, 5);
    // The final response will be empty since model never returned text
    assert!(result.assistant_output.is_none() || result.assistant_output.as_deref() == Some(""));
}

// ============================================================================
// Test 5: Tape records conversation
// ============================================================================

#[tokio::test]
async fn agent_loop_tape_records_conversation() {
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("I remember everything."))
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_tape").unwrap();
    let _ = agent.handle_input("remember this").await;

    // Verify tape contains both user and assistant messages
    let entries = agent.tape().entries();
    let messages: Vec<_> = entries.iter().filter(|e| e.kind == "message").collect();

    // Should have at least: user + assistant
    assert!(
        messages.len() >= 2,
        "expected at least 2 messages in tape, got {}",
        messages.len()
    );

    // Check user message exists
    let has_user = messages.iter().any(|e| {
        e.payload.get("role").and_then(|v| v.as_str()) == Some("user")
            && e.payload
                .get("content")
                .and_then(|v| v.as_str())
                .map(|c| c.contains("remember this"))
                .unwrap_or(false)
    });
    assert!(has_user, "user message not found in tape");

    // Check assistant message exists
    let has_assistant = messages.iter().any(|e| {
        e.payload.get("role").and_then(|v| v.as_str()) == Some("assistant")
            && e.payload
                .get("content")
                .and_then(|v| v.as_str())
                .map(|c| c.contains("I remember everything"))
                .unwrap_or(false)
    });
    assert!(has_assistant, "assistant message not found in tape");
}

// ============================================================================
// Test 6: Command routing — ,help doesn't call model
// ============================================================================

#[tokio::test]
async fn agent_loop_command_routing_no_model() {
    let mut server = mockito::Server::new_async().await;

    // If this mock is hit, the test should fail — ,help shouldn't call model
    let mock = server
        .mock("POST", "/chat/completions")
        .expect(0)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_cmd").unwrap();
    let result = agent.handle_input(",help").await;

    mock.assert_async().await;
    assert!(result.immediate_output.is_some());
    assert!(
        result
            .immediate_output
            .as_ref()
            .unwrap()
            .contains("Available commands")
    );
    assert!(result.assistant_output.is_none());
    assert!(!result.exit_requested);
}

// ============================================================================
// Test 7: Multi-turn session preserves context
// ============================================================================

#[tokio::test]
async fn agent_loop_multi_turn_session() {
    let mut server = mockito::Server::new_async().await;

    // First turn
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("My name is CrabClaw."))
        .create_async()
        .await;

    // Second turn
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("You asked about my name earlier."))
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_multi").unwrap();

    // First turn
    let r1 = agent.handle_input("What is your name?").await;
    assert!(r1.error.is_none());
    assert_eq!(r1.assistant_output.as_deref(), Some("My name is CrabClaw."));

    // Second turn (same agent instance = same session)
    let r2 = agent.handle_input("What did I ask you?").await;
    assert!(r2.error.is_none());
    assert_eq!(
        r2.assistant_output.as_deref(),
        Some("You asked about my name earlier.")
    );

    // Tape should have all messages
    let entries = agent.tape().entries();
    let msg_count = entries.iter().filter(|e| e.kind == "message").count();
    assert!(
        msg_count >= 4,
        "expected at least 4 messages (2 user + 2 assistant), got {}",
        msg_count
    );
}

// ============================================================================
// Test 8: Error propagation from API
// ============================================================================

#[tokio::test]
async fn agent_loop_error_propagation() {
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/chat/completions")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Internal server error","type":"server_error"}}"#)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_error").unwrap();
    let result = agent.handle_input("trigger error").await;

    assert!(result.error.is_some(), "expected error but got none");
    assert!(result.assistant_output.is_none());
}

// ============================================================================
// Test 9: Streaming + tool calling loop combined
// ============================================================================

#[tokio::test]
async fn agent_loop_stream_tool_calling_loop() {
    let mut server = mockito::Server::new_async().await;

    // First streaming call: returns a tool call
    let tool_stream = sse_stream(&[
        &sse_tool_call_start(0, "call_stream_1", "tools"),
        &sse_tool_call_args(0, "{}"),
    ]);

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(tool_stream)
        .create_async()
        .await;

    // Second streaming call: returns final text
    let text_stream = sse_stream(&[&sse_content_chunk("Tool "), &sse_content_chunk("result!")]);

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(text_stream)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_stream_tool").unwrap();
    let mut tokens: Vec<String> = Vec::new();
    let result = agent
        .handle_input_stream("list tools", |token| {
            tokens.push(token.to_string());
        })
        .await;

    assert!(
        result.error.is_none(),
        "unexpected error: {:?}",
        result.error
    );
    // Tokens should be from the second (final) call only
    assert_eq!(tokens, vec!["Tool ", "result!"]);
    assert_eq!(result.assistant_output.as_deref(), Some("Tool result!"));
    assert_eq!(result.tool_rounds, 1);
}

// ============================================================================
// Test 10: ,quit sets exit_requested
// ============================================================================

#[tokio::test]
async fn agent_loop_quit_exits() {
    let config = test_config("http://unused");
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_quit").unwrap();
    let result = agent.handle_input(",quit").await;

    assert!(result.exit_requested);
    assert!(result.assistant_output.is_none());
    assert!(result.immediate_output.is_none());
}

// ============================================================================
// Test 11: Empty input returns nothing
// ============================================================================

#[tokio::test]
async fn agent_loop_empty_input() {
    let config = test_config("http://unused");
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_empty").unwrap();
    let result = agent.handle_input("").await;

    assert!(!result.exit_requested);
    assert!(result.assistant_output.is_none());
    assert!(result.immediate_output.is_none());
    assert!(result.error.is_none());
}

// ============================================================================
// Test 12: Rate limit error (429)
// ============================================================================

#[tokio::test]
async fn agent_loop_rate_limit_error() {
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/chat/completions")
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Rate limit exceeded","type":"rate_limit"}}"#)
        .create_async()
        .await;

    let config = test_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_ratelimit").unwrap();
    let result = agent.handle_input("hello").await;

    assert!(result.error.is_some());
    let err = result.error.unwrap();
    assert!(
        err.to_lowercase().contains("rate") || err.to_lowercase().contains("429"),
        "expected rate limit error, got: {err}"
    );
}
