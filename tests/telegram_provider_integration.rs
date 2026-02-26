mod support;

use crabclaw::channels::telegram::process_message;
use support::assertions::{assert_has_error, assert_ok_reply};
use support::builders::{anthropic_config, openai_config};
use support::responses::text_response;
use tempfile::TempDir;

#[tokio::test]
async fn routes_to_anthropic_model_and_returns_reply() {
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

    let config = anthropic_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message("hi", &config, workspace.path(), "test:anthropic").await;

    mock.assert_async().await;
    assert_ok_reply(&response, "Hello from Anthropic mock LLM!");
}

#[tokio::test]
async fn anthropic_tool_call_then_final_reply() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "id": "msg_01",
                "content": [
                    {"type": "tool_use", "id": "toolu_01", "name": "tools", "input": {}}
                ],
                "stop_reason": "tool_use"
            }"#,
        )
        .create_async()
        .await;

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
    let response =
        process_message("what tools?", &config, workspace.path(), "test:anth_tool").await;

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
async fn anthropic_error_during_tool_loop_is_propagated() {
    let mut server = mockito::Server::new_async().await;
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
    server
        .mock("POST", "/v1/messages")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Internal server error","type":"server_error"}}"#)
        .create_async()
        .await;

    let config = anthropic_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message("run shell", &config, workspace.path(), "test:anth_err").await;
    assert_has_error(&response);
}

#[tokio::test]
async fn system_prompt_includes_workspace_override_for_anthropic() {
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
                "content": [{"type":"text","text":"OK"}],
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

    let _ = process_message("hi", &config, workspace.path(), "test:ws_prompt").await;
    mock.assert_async().await;
}

#[tokio::test]
async fn openai_system_prompt_contains_identity_contract() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .match_body(mockito::Matcher::Regex("runtime_contract".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("ok"))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let _ = process_message("ping", &config, workspace.path(), "test:prompt_openai").await;
    mock.assert_async().await;
}
