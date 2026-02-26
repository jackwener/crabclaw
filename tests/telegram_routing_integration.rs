mod support;

use support::assertions::assert_has_error;
use support::builders::openai_config;
use support::responses::text_response;
use crabclaw::channels::telegram::process_message;
use tempfile::TempDir;

#[tokio::test]
async fn routes_to_openai_model_and_returns_reply() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("Hello from mock LLM!"))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message("hi there", &config, workspace.path(), "test:session1").await;

    mock.assert_async().await;
    assert!(response.error.is_none());
    assert_eq!(
        response.assistant_output.as_deref(),
        Some("Hello from mock LLM!")
    );
}

#[tokio::test]
async fn comma_command_returns_immediate_output_without_model_call() {
    let config = openai_config("http://unused:9999");
    let workspace = TempDir::new().unwrap();
    let response = process_message(",help", &config, workspace.path(), "test:session2").await;

    assert!(response.error.is_none());
    assert!(response.immediate_output.is_some());
    assert!(response.assistant_output.is_none());
}

#[tokio::test]
async fn empty_model_response_is_graceful() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"choices":[]}"#)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message("hello", &config, workspace.path(), "test:empty").await;

    mock.assert_async().await;
    assert!(response.error.is_none());
    assert!(response.assistant_output.is_none());
}

#[tokio::test]
async fn nonstandard_200_error_body_returns_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"code":500,"msg":"404 NOT_FOUND","success":false}"#)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message("test error", &config, workspace.path(), "test:ns_err").await;

    mock.assert_async().await;
    assert_has_error(&response);
}

#[tokio::test]
async fn http_429_is_reported_as_rate_limit_error() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"rate limited","type":"rate_limit"}}"#)
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response =
        process_message("test rate limit", &config, workspace.path(), "test:session5").await;

    mock.assert_async().await;
    assert_has_error(&response);
}

#[tokio::test]
async fn session_tape_persists_between_calls() {
    let mut server = mockito::Server::new_async().await;

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("first reply"))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let _ = process_message("msg 1", &config, workspace.path(), "test:session6").await;

    let mock2 = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("second reply"))
        .create_async()
        .await;

    let response2 = process_message("msg 2", &config, workspace.path(), "test:session6").await;
    mock2.assert_async().await;
    assert_eq!(response2.assistant_output.as_deref(), Some("second reply"));
}
