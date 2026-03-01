//! End-to-end integration tests for web.fetch and web.search tools.
//!
//! Tests verify the full pipeline: mock LLM issues a tool call →
//! `process_message` dispatches to `execute_tool` → web tool runs →
//! result flows back through the agent loop.

mod support;

use crabclaw::channels::telegram::process_message;
use support::assertions::assert_ok_reply;
use support::builders::openai_config;
use support::responses::{text_response, tool_call_response};
use tempfile::TempDir;

#[tokio::test]
async fn e2e_web_fetch_tool_returns_page_content() {
    // Set up a mock "web page" that web.fetch will actually hit
    let mut web_server = mockito::Server::new_async().await;
    let web_mock = web_server
        .mock("GET", "/test-page")
        .with_status(200)
        .with_header("content-type", "text/html; charset=utf-8")
        .with_body("<html><body><h1>Hello CrabClaw</h1><p>This is a test page.</p></body></html>")
        .create_async()
        .await;

    let fetch_url = format!("{}/test-page", web_server.url());

    // Set up the mock LLM that calls web.fetch then summarizes
    let mut llm_server = mockito::Server::new_async().await;
    llm_server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "web.fetch",
            "call_fetch",
            &format!(r#"{{"url":"{}"}}"#, fetch_url),
        ))
        .create_async()
        .await;

    let final_mock = llm_server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("The page says Hello CrabClaw."))
        .create_async()
        .await;

    let config = openai_config(&llm_server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "fetch this page",
        &config,
        workspace.path(),
        "test:web_fetch",
        None,
        None,
    )
    .await;

    web_mock.assert_async().await;
    final_mock.assert_async().await;
    assert_ok_reply(&response, "The page says Hello CrabClaw.");
}

#[tokio::test]
async fn e2e_web_search_tool_returns_search_url() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "web.search",
            "call_search",
            r#"{"query":"rust programming"}"#,
        ))
        .create_async()
        .await;

    let final_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("Here is a DuckDuckGo search link for Rust."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "search for rust",
        &config,
        workspace.path(),
        "test:web_search",
        None,
        None,
    )
    .await;

    final_mock.assert_async().await;
    assert_ok_reply(&response, "Here is a DuckDuckGo search link for Rust.");
}

#[tokio::test]
async fn e2e_web_fetch_missing_url_returns_error() {
    let mut server = mockito::Server::new_async().await;
    // LLM calls web.fetch with empty args (missing url)
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("web.fetch", "call_no_url", "{}"))
        .create_async()
        .await;

    let final_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("I need a URL to fetch."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "fetch something",
        &config,
        workspace.path(),
        "test:web_fetch_no_url",
        None,
        None,
    )
    .await;

    final_mock.assert_async().await;
    assert_ok_reply(&response, "I need a URL to fetch.");
}

#[tokio::test]
async fn e2e_web_fetch_plain_text_passthrough() {
    // Serve plain text (not HTML)
    let mut web_server = mockito::Server::new_async().await;
    let web_mock = web_server
        .mock("GET", "/robots.txt")
        .with_status(200)
        .with_header("content-type", "text/plain")
        .with_body("User-agent: *\nDisallow: /private/")
        .create_async()
        .await;

    let fetch_url = format!("{}/robots.txt", web_server.url());

    let mut llm_server = mockito::Server::new_async().await;
    llm_server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "web.fetch",
            "call_fetch_txt",
            &format!(r#"{{"url":"{}"}}"#, fetch_url),
        ))
        .create_async()
        .await;

    let final_mock = llm_server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("The robots.txt disallows /private/."))
        .create_async()
        .await;

    let config = openai_config(&llm_server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "check robots.txt",
        &config,
        workspace.path(),
        "test:web_fetch_txt",
        None,
        None,
    )
    .await;

    web_mock.assert_async().await;
    final_mock.assert_async().await;
    assert_ok_reply(&response, "The robots.txt disallows /private/.");
}
