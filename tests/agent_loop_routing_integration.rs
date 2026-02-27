mod support;

use crabclaw::core::agent_loop::AgentLoop;
use support::builders::openai_config;
use support::responses::text_response;
use tempfile::TempDir;

#[tokio::test]
async fn routes_to_model_and_returns_reply() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("Hello from AgentLoop!"))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_session", None).unwrap();
    let result = agent.handle_input("hello").await;

    mock.assert_async().await;
    assert!(result.error.is_none());
    assert_eq!(
        result.assistant_output.as_deref(),
        Some("Hello from AgentLoop!")
    );
}

#[tokio::test]
async fn command_routing_help_does_not_call_model() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .expect(0)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_cmd", None).unwrap();
    let result = agent.handle_input(",help").await;

    mock.assert_async().await;
    assert!(result.assistant_output.is_none());
    assert!(result.immediate_output.is_some());
}

#[tokio::test]
async fn multi_turn_session_keeps_context_on_same_agent() {
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("My name is CrabClaw."))
        .create_async()
        .await;

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("You asked about my name earlier."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();

    let mut agent = AgentLoop::open(&config, workspace.path(), "test_multi", None).unwrap();
    let r1 = agent.handle_input("What is your name?").await;
    let r2 = agent.handle_input("What did I ask you?").await;

    assert!(r1.error.is_none());
    assert!(r2.error.is_none());
    assert_eq!(
        r2.assistant_output.as_deref(),
        Some("You asked about my name earlier.")
    );
}

#[tokio::test]
async fn api_error_is_propagated() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Internal server error","type":"server_error"}}"#)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let mut agent = AgentLoop::open(&config, workspace.path(), "test_error", None).unwrap();

    let result = agent.handle_input("trigger error").await;
    assert!(result.error.is_some());
}
