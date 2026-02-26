mod support;

use crabclaw::core::agent_loop::AgentLoop;
use support::builders::openai_config;
use support::responses::{text_response, tool_call_response};
use support::sse::{sse_content_chunk, sse_stream, sse_tool_call_args, sse_tool_call_start};
use tempfile::TempDir;

#[tokio::test]
async fn non_streaming_tool_calling_loop_runs_then_returns_text() {
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("tools", "call_1", "{}"))
        .create_async()
        .await;

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("Found 5 tools."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let mut agent = AgentLoop::open(&config, workspace.path(), "test_tools").unwrap();

    let result = agent.handle_input("what tools?").await;
    assert!(result.error.is_none());
    assert_eq!(result.tool_rounds, 1);
    assert_eq!(result.assistant_output.as_deref(), Some("Found 5 tools."));
}

#[tokio::test]
async fn streaming_delivers_tokens_and_handles_tool_round() {
    let mut server = mockito::Server::new_async().await;

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

    let text_stream = sse_stream(&[&sse_content_chunk("Tool "), &sse_content_chunk("result!")]);
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(text_stream)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let mut agent = AgentLoop::open(&config, workspace.path(), "test_stream_tool").unwrap();

    let mut tokens = Vec::<String>::new();
    let result = agent
        .handle_input_stream("list tools", |token| tokens.push(token.to_string()))
        .await;

    assert!(result.error.is_none());
    assert_eq!(tokens, vec!["Tool ", "result!"]);
    assert_eq!(result.assistant_output.as_deref(), Some("Tool result!"));
}

#[tokio::test]
async fn unknown_tool_call_recovery_path() {
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("nonexistent.tool", "call_unknown", "{}"))
        .create_async()
        .await;

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("Sorry, that tool doesn't exist."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let mut agent = AgentLoop::open(&config, workspace.path(), "test_unknown_tool").unwrap();

    let result = agent.handle_input("use fake tool").await;
    assert!(result.error.is_none());
    assert_eq!(
        result.assistant_output.as_deref(),
        Some("Sorry, that tool doesn't exist.")
    );
}

#[tokio::test]
async fn file_edit_tool_call_modifies_file() {
    let mut server = mockito::Server::new_async().await;
    let workspace = TempDir::new().unwrap();
    std::fs::write(workspace.path().join("test.txt"), "hello world").unwrap();

    let args = r#"{"path": "test.txt", "old": "hello", "new": "goodbye"}"#;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("file.edit", "edit_1", args))
        .create_async()
        .await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("File updated successfully."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let mut agent = AgentLoop::open(&config, workspace.path(), "test_edit").unwrap();
    let result = agent.handle_input("replace hello").await;

    assert!(result.error.is_none());
    let content = std::fs::read_to_string(workspace.path().join("test.txt")).unwrap();
    assert_eq!(content, "goodbye world");
}

#[tokio::test]
async fn tool_loop_breaks_after_max_iterations_without_hanging() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("tools", "call_loop", "{}"))
        .expect_at_most(5)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let mut agent = AgentLoop::open(&config, workspace.path(), "test_max_iter").unwrap();

    let result = agent.handle_input("loop forever?").await;
    assert!(result.error.is_none());
    assert_eq!(result.tool_rounds, 5);
    assert!(result.assistant_output.is_none() || result.assistant_output.as_deref() == Some(""));
}
