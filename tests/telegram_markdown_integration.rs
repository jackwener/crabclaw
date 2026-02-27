//! End-to-end integration tests for Telegram Markdown → HTML rendering.
//!
//! These tests verify the full pipeline: the LLM returns markdown-formatted
//! text → `process_message()` captures it → `markdown_to_telegram_html()`
//! converts it to Telegram-compatible HTML ready for `parse_mode=Html`.

mod support;

use crabclaw::channels::telegram::{markdown_to_telegram_html, process_message};
use support::builders::openai_config;
use tempfile::TempDir;

/// Helper: drive `process_message` with a mock LLM that returns the given text,
/// then convert the assistant reply through the same HTML pipeline that
/// `handle_message` uses before sending to Telegram.
async fn render_via_pipeline(markdown: &str) -> String {
    // Build the JSON response body using serde_json so that newlines,
    // quotes, etc. are properly escaped — text_response() uses format!
    // which doesn't escape control characters.
    let body = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": markdown
            },
            "finish_reason": "stop"
        }]
    });

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body.to_string())
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message("test input", &config, workspace.path(), "test:md_render").await;

    let reply = response
        .to_reply()
        .expect("expected a reply from process_message");

    markdown_to_telegram_html(&reply)
}

// ---------------------------------------------------------------------------
// E2E pipeline tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_bold_and_italic_render_as_html() {
    let html = render_via_pipeline("This is **bold** and *italic* text").await;
    assert!(html.contains("<b>bold</b>"), "bold not rendered: {html}");
    assert!(
        html.contains("<i>italic</i>"),
        "italic not rendered: {html}"
    );
}

#[tokio::test]
async fn e2e_headers_render_as_bold() {
    let html = render_via_pipeline("## Summary").await;
    assert!(
        html.contains("<b>Summary</b>"),
        "header not rendered: {html}"
    );
}

#[tokio::test]
async fn e2e_inline_code_renders_as_code_tag() {
    let html = render_via_pipeline("Use `cargo test` to run tests").await;
    assert!(
        html.contains("<code>cargo test</code>"),
        "inline code not rendered: {html}"
    );
}

#[tokio::test]
async fn e2e_code_block_renders_as_pre() {
    let html = render_via_pipeline("```rust\nfn main() {}\n```").await;
    assert!(
        html.contains("<pre><code>"),
        "code block not rendered: {html}"
    );
    assert!(
        html.contains("fn main() {}"),
        "code content missing: {html}"
    );
}

#[tokio::test]
async fn e2e_link_renders_as_anchor() {
    let html = render_via_pipeline("Visit [Rust](https://rust-lang.org) for more").await;
    assert!(
        html.contains("<a href=\"https://rust-lang.org\">Rust</a>"),
        "link not rendered: {html}"
    );
}

#[tokio::test]
async fn e2e_strikethrough_renders_as_s_tag() {
    let html = render_via_pipeline("This is ~~deleted~~ text").await;
    assert!(
        html.contains("<s>deleted</s>"),
        "strikethrough not rendered: {html}"
    );
}

#[tokio::test]
async fn e2e_html_entities_are_escaped() {
    let html = render_via_pipeline("Compare: a < b && c > d").await;
    assert!(
        html.contains("&lt;") && html.contains("&gt;") && html.contains("&amp;"),
        "HTML entities not escaped: {html}"
    );
}

#[tokio::test]
async fn e2e_mixed_markdown_renders_correctly() {
    let md = "## Results\n\nFound **3** issues in `main.rs`:\n- Use ~~old_api~~ new API\n- See [docs](https://example.com)";
    let html = render_via_pipeline(md).await;

    assert!(html.contains("<b>Results</b>"), "header: {html}");
    assert!(html.contains("<b>3</b>"), "bold: {html}");
    assert!(html.contains("<code>main.rs</code>"), "code: {html}");
    assert!(html.contains("<s>old_api</s>"), "strikethrough: {html}");
    assert!(
        html.contains("<a href=\"https://example.com\">docs</a>"),
        "link: {html}"
    );
}

#[tokio::test]
async fn e2e_plain_text_passes_through_unchanged() {
    let html = render_via_pipeline("Hello, world!").await;
    assert_eq!(html, "Hello, world!");
}
