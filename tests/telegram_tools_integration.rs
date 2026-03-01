mod support;

use crabclaw::channels::telegram::process_message;
use support::assertions::assert_ok_reply;
use support::builders::openai_config;
use support::responses::{text_response, tool_call_response};
use tempfile::TempDir;

#[tokio::test]
async fn openai_tool_calling_loop_returns_final_reply() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("tools", "call_123", "{}"))
        .create_async()
        .await;

    let final_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("I found 5 tools available."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "what tools do you have?",
        &config,
        workspace.path(),
        "test:tools",
        None,
        None,
    )
    .await;

    final_mock.assert_async().await;
    assert_ok_reply(&response, "I found 5 tools available.");
}

#[tokio::test]
async fn file_write_then_read_tool_sequence() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "file.write",
            "call_write",
            r#"{"path":"test_output.txt","content":"hello world"}"#,
        ))
        .create_async()
        .await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "file.read",
            "call_read",
            r#"{"path":"test_output.txt"}"#,
        ))
        .create_async()
        .await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("The file contains: hello world"))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "write and read a file",
        &config,
        workspace.path(),
        "test:file_ops",
        None,
        None,
    )
    .await;

    assert!(response.error.is_none());
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("The file contains: hello world")
    );
    let content = std::fs::read_to_string(workspace.path().join("test_output.txt")).unwrap();
    assert_eq!(content, "hello world");
}

#[tokio::test]
async fn unknown_tool_name_recovery() {
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
    let response = process_message(
        "use a fake tool",
        &config,
        workspace.path(),
        "test:unknown_tool",
        None,
        None,
    )
    .await;

    assert!(response.error.is_none());
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("Sorry, that tool doesn't exist.")
    );
}

#[tokio::test]
async fn tool_loop_breaks_after_max_iterations() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "shell.exec",
            "call_loop",
            r#"{"command":"echo loop"}"#,
        ))
        .expect_at_most(5)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "loop forever",
        &config,
        workspace.path(),
        "test:max_iter",
        None,
        None,
    )
    .await;

    assert!(
        response
            .error
            .as_deref()
            .is_some_and(|e| e.contains("tool iteration limit reached"))
    );
    assert!(response.assistant_output.is_none());
}

#[tokio::test]
async fn malformed_file_write_args_model_recovers() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "file.write",
            "call_bad_args",
            r#"{"content":"x"}"#,
        ))
        .create_async()
        .await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("I need a valid path to write the file."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "create a file",
        &config,
        workspace.path(),
        "test:bad_args",
        None,
        None,
    )
    .await;

    assert!(response.error.is_none());
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("I need a valid path to write the file.")
    );
}
