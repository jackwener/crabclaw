mod support;

use crabclaw::channels::telegram::process_message;
use support::assertions::{assert_has_error, assert_ok_reply};
use support::builders::openai_config;
use support::responses::{text_response, tool_call_response};
use tempfile::TempDir;

#[tokio::test]
async fn routes_to_openai_model_and_returns_reply() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("Hello from OpenAI-compatible mock!"))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message("hi", &config, workspace.path(), "test:openai", None).await;

    mock.assert_async().await;
    assert_ok_reply(&response, "Hello from OpenAI-compatible mock!");
}

#[tokio::test]
async fn openai_tool_call_then_final_reply() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("tools", "call_01", "{}"))
        .create_async()
        .await;

    let final_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response(
            "Here are your tools: shell.exec, file.read, etc.",
        ))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "what tools?",
        &config,
        workspace.path(),
        "test:openai_tool",
        None,
    )
    .await;

    final_mock.assert_async().await;
    assert!(response.error.is_none());
    assert!(
        response
            .assistant_output
            .as_deref()
            .unwrap_or("")
            .contains("Here are your tools")
    );
}

#[tokio::test]
async fn openai_error_during_tool_loop_is_propagated() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "shell.exec",
            "call_err",
            r#"{"command":"echo ok"}"#,
        ))
        .create_async()
        .await;
    server
        .mock("POST", "/chat/completions")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Internal server error","type":"server_error"}}"#)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "run shell",
        &config,
        workspace.path(),
        "test:openai_err",
        None,
    )
    .await;
    assert_has_error(&response);
}

#[tokio::test]
async fn openai_429_rate_limit_propagated() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Rate limit exceeded","type":"rate_limit"}}"#)
        .expect_at_least(1)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "trigger rate",
        &config,
        workspace.path(),
        "test:openai_rate",
        None,
    )
    .await;
    assert_has_error(&response);
}

#[tokio::test]
async fn openai_system_prompt_includes_workspace_override() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_body(mockito::Matcher::Regex(
            "CUSTOM_WORKSPACE_PROMPT_OPENAI".to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("OK"))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let agent_dir = workspace.path().join(".agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::write(
        agent_dir.join("system-prompt.md"),
        "CUSTOM_WORKSPACE_PROMPT_OPENAI",
    )
    .unwrap();

    let _ = process_message(
        "hi",
        &config,
        workspace.path(),
        "test:ws_prompt_openai",
        None,
    )
    .await;
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_multi_turn_keeps_context() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("I am CrabClaw."))
        .create_async()
        .await;

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("You asked about my name."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let r1 = process_message(
        "name?",
        &config,
        workspace.path(),
        "test:openai_multi",
        None,
    )
    .await;
    let r2 = process_message(
        "what did I ask?",
        &config,
        workspace.path(),
        "test:openai_multi",
        None,
    )
    .await;

    assert!(r1.error.is_none());
    assert!(r2.error.is_none());
    assert_eq!(
        r2.assistant_output.as_deref(),
        Some("You asked about my name.")
    );
}
