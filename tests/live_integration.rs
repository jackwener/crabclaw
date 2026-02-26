//! Live integration tests using real AI models.
//!
//! These tests call `process_message` with a real LLM API (configured via
//! `.env.local` in the project root). They are skipped automatically when
//! no API key is configured.
//!
//! Run with:
//!   cargo test --test live_integration -- --nocapture
//!
//! Configure `.env.local` with:
//!   API_KEY=your-key
//!   BASE_URL=https://your-api-endpoint
//!   MODEL=anthropic:your-model
//!
//! Tests are serialized to avoid hitting rate limits.

use crabclaw::channels::telegram::process_message;
use crabclaw::core::config::{CliConfigOverrides, load_runtime_config};
use serial_test::serial;
use tempfile::TempDir;

/// Load config from `.env.local` in the project root.
/// Returns None if API key is not configured, causing tests to be skipped.
fn try_load_live_config() -> Option<crabclaw::core::config::AppConfig> {
    let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let overrides = CliConfigOverrides {
        api_key: None,
        api_base: None,
        model: None,
        system_prompt: None,
        max_context_messages: None,
    };

    load_runtime_config(&workspace, None, &overrides).ok()
}

/// Helper: send message in a workspace with config.
async fn send_live_in(
    text: &str,
    config: &crabclaw::core::config::AppConfig,
    workspace: &std::path::Path,
    session: &str,
) -> crabclaw::channels::base::ChannelResponse {
    process_message(text, config, workspace, session).await
}

/// Macro to skip test when no live config is available.
macro_rules! require_live_config {
    () => {
        match try_load_live_config() {
            Some(c) => c,
            None => {
                eprintln!("SKIPPED: no .env.local with API_KEY configured");
                return;
            }
        }
    };
}

// ============================================================================
// Test: Basic Chat — model should reply with something non-empty
// ============================================================================

#[tokio::test]
#[serial]
async fn live_model_replies_to_simple_message() {
    let config = require_live_config!();

    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "Say exactly: PONG",
        &config,
        workspace.path(),
        "live_test:chat",
    )
    .await;

    assert!(
        response.error.is_none(),
        "Unexpected error: {:?}",
        response.error
    );
    assert!(
        response.assistant_output.is_some(),
        "Expected a reply from model"
    );
    let output = response.assistant_output.unwrap();
    println!(
        "[live_chat] model replied: {}",
        &output[..output.len().min(200)]
    );
    assert!(!output.is_empty(), "Reply should not be empty");
}

// ============================================================================
// Test: Tool Calling — Create File
// ============================================================================

#[tokio::test]
#[serial]
async fn live_tool_call_creates_file() {
    let config = require_live_config!();
    let workspace = TempDir::new().unwrap();
    let session = "live_test:file_create";

    // Give a very explicit instruction so the model uses the file.write tool.
    let response = send_live_in(
        "Use the file.write tool to create a file called hello.txt with the content 'Hello World'. \
         Do NOT reply with the content, use the tool.",
        &config,
        workspace.path(),
        session,
    )
    .await;

    println!(
        "[live_file_create] assistant_output: {:?}",
        response
            .assistant_output
            .as_deref()
            .map(|s| &s[..s.len().min(300)])
    );
    if let Some(err) = &response.error {
        println!("[live_file_create] error: {}", err);
    }

    // Check if the file was actually created
    let file_path = workspace.path().join("hello.txt");
    assert!(
        file_path.exists(),
        "File hello.txt was not created. Model likely did not use tool calling. \
         Response: {:?}",
        response
            .assistant_output
            .as_deref()
            .map(|s| &s[..s.len().min(200)])
    );

    let content = std::fs::read_to_string(&file_path).unwrap();
    println!("[live_file_create] file content: {}", content);
    assert!(
        content.contains("Hello"),
        "File should contain 'Hello', got: {}",
        content
    );
}

// ============================================================================
// Test: Tool Calling — Read File
// ============================================================================

#[tokio::test]
#[serial]
async fn live_tool_call_reads_file() {
    let config = require_live_config!();
    let workspace = TempDir::new().unwrap();
    let session = "live_test:file_read";

    // Pre-create a file in the workspace.
    std::fs::write(workspace.path().join("secret.txt"), "The answer is 42").unwrap();

    let response = send_live_in(
        "Use the file.read tool to read the file 'secret.txt' and tell me what the answer is.",
        &config,
        workspace.path(),
        session,
    )
    .await;

    assert!(
        response.error.is_none(),
        "Unexpected error: {:?}",
        response.error
    );
    let output = response.assistant_output.unwrap_or_default();
    println!(
        "[live_file_read] model replied: {}",
        &output[..output.len().min(300)]
    );
    assert!(
        output.contains("42"),
        "Model should have read the file and found '42', got: {}",
        &output[..output.len().min(300)]
    );
}

// ============================================================================
// Test: Tool Calling — Shell Execution
// ============================================================================

#[tokio::test]
#[serial]
async fn live_tool_call_shell_exec() {
    let config = require_live_config!();
    let workspace = TempDir::new().unwrap();
    let session = "live_test:shell_exec";

    let response = send_live_in(
        "Use the shell.exec tool to run 'echo CRABCLAW_OK' and tell me the output.",
        &config,
        workspace.path(),
        session,
    )
    .await;

    assert!(
        response.error.is_none(),
        "Unexpected error: {:?}",
        response.error
    );
    let output = response.assistant_output.unwrap_or_default();
    println!(
        "[live_shell] model replied: {}",
        &output[..output.len().min(300)]
    );
    assert!(
        output.contains("CRABCLAW_OK"),
        "Model should have executed echo and returned 'CRABCLAW_OK', got: {}",
        &output[..output.len().min(300)]
    );
}

// ============================================================================
// Test: Multi-turn — Write then Read
// ============================================================================

#[tokio::test]
#[serial]
async fn live_multi_turn_write_then_read() {
    let config = require_live_config!();
    let workspace = TempDir::new().unwrap();
    let session = "live_test:multi_turn";

    // Turn 1: Write a file
    let r1 = send_live_in(
        "Use the file.write tool to create 'notes.txt' with content 'CrabClaw version 1.0'.",
        &config,
        workspace.path(),
        session,
    )
    .await;

    println!(
        "[live_multi_turn_1] reply: {:?}",
        r1.assistant_output
            .as_deref()
            .map(|s| &s[..s.len().min(200)])
    );
    assert!(
        workspace.path().join("notes.txt").exists(),
        "notes.txt should exist after write. Response: {:?}",
        r1.assistant_output
    );

    // Turn 2: Read it back
    let r2 = send_live_in(
        "Now use file.read to read 'notes.txt' and tell me what version is mentioned.",
        &config,
        workspace.path(),
        session,
    )
    .await;

    let output = r2.assistant_output.unwrap_or_default();
    println!(
        "[live_multi_turn_2] reply: {}",
        &output[..output.len().min(300)]
    );
    assert!(
        output.contains("1.0"),
        "Model should have read the file and found version '1.0', got: {}",
        &output[..output.len().min(300)]
    );
}

// ============================================================================
// Diagnostic: Uses REAL project workspace (like Telegram does) to write a file
// in a temp subdir — exactly replicating the TG scenario.
// ============================================================================

#[tokio::test]
#[serial]
async fn live_diagnostic_project_workspace_tool_call() {
    let config = require_live_config!();

    // Use real project dir as workspace — same as TG bot does
    let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Use a unique session to avoid stale tape context
    let session = &format!(
        "diag_test:{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    let target_file = "crabclaw_diag_test.txt";
    let prompt = format!(
        "Use the file.write tool to create a file called '{}' with the content 'DIAG_OK'. \
         You MUST use the file.write tool. Do NOT just reply with text.",
        target_file
    );

    let response = send_live_in(&prompt, &config, &workspace, session).await;

    println!(
        "[diag] assistant_output: {:?}",
        response
            .assistant_output
            .as_deref()
            .map(|s| &s[..s.len().min(300)])
    );
    if let Some(err) = &response.error {
        println!("[diag] error: {}", err);
    }

    let file_path = workspace.join(target_file);
    assert!(
        file_path.exists(),
        "DIAGNOSTIC FAILED: '{}' was not created. Model likely did NOT use tool calling.\n\
         Response: {:?}",
        target_file,
        response.assistant_output
    );

    let content = std::fs::read_to_string(&file_path).unwrap();
    println!("[diag] file content: {}", content);
    // Clean up
    std::fs::remove_file(&file_path).ok();
    assert!(
        content.contains("DIAG_OK"),
        "Expected 'DIAG_OK' in file, got: {}",
        content
    );
}

// ============================================================================
// Live AgentLoop tests — test the AgentLoop abstraction directly
// ============================================================================

/// AgentLoop::handle_input returns a non-empty reply from real model.
#[tokio::test]
#[serial]
async fn live_agent_loop_basic_reply() {
    let config = require_live_config!();
    let workspace = TempDir::new().unwrap();

    let mut agent =
        crabclaw::core::agent_loop::AgentLoop::open(&config, workspace.path(), "live_al_basic")
            .unwrap();

    let result = agent.handle_input("Say exactly: AGENT_LOOP_OK").await;

    assert!(
        result.error.is_none(),
        "Unexpected error: {:?}",
        result.error
    );
    assert!(
        result.assistant_output.is_some(),
        "Expected a reply from model"
    );
    let output = result.assistant_output.unwrap();
    println!(
        "[live_agent_loop_basic] reply: {}",
        &output[..output.len().min(200)]
    );
    assert!(!output.is_empty(), "Reply should not be empty");
    assert!(!result.exit_requested);
}

/// AgentLoop::handle_input_stream delivers tokens via callback.
#[tokio::test]
#[serial]
async fn live_agent_loop_streaming() {
    let config = require_live_config!();
    let workspace = TempDir::new().unwrap();

    let mut agent =
        crabclaw::core::agent_loop::AgentLoop::open(&config, workspace.path(), "live_al_stream")
            .unwrap();

    let mut token_count = 0usize;
    let mut collected = String::new();

    let result = agent
        .handle_input_stream("Say exactly: STREAM_OK", |token| {
            token_count += 1;
            collected.push_str(token);
        })
        .await;

    assert!(
        result.error.is_none(),
        "Unexpected error: {:?}",
        result.error
    );
    println!(
        "[live_agent_loop_stream] {} tokens collected, content: {}",
        token_count,
        &collected[..collected.len().min(200)]
    );
    assert!(token_count > 0, "Expected at least one streaming token");
    assert!(
        result.assistant_output.is_some(),
        "Expected assistant_output to be set"
    );
    let output = result.assistant_output.unwrap();
    assert!(!output.is_empty(), "assistant_output should not be empty");
}

/// AgentLoop::handle_input triggers tool calling (file.write) with real model.
#[tokio::test]
#[serial]
async fn live_agent_loop_tool_call() {
    let config = require_live_config!();
    let workspace = TempDir::new().unwrap();

    let mut agent =
        crabclaw::core::agent_loop::AgentLoop::open(&config, workspace.path(), "live_al_tool")
            .unwrap();

    let result = agent
        .handle_input(
            "Use the file.write tool to create a file called 'agent_test.txt' \
             with the content 'AGENT_TOOL_OK'. You MUST use the file.write tool.",
        )
        .await;

    println!(
        "[live_agent_loop_tool] reply: {:?}, tool_rounds: {}",
        result
            .assistant_output
            .as_deref()
            .map(|s| &s[..s.len().min(300)]),
        result.tool_rounds
    );
    if let Some(err) = &result.error {
        println!("[live_agent_loop_tool] error: {}", err);
    }

    let file_path = workspace.path().join("agent_test.txt");
    assert!(
        file_path.exists(),
        "agent_test.txt was not created. Model likely did not use tool calling. \
         Response: {:?}",
        result
            .assistant_output
            .as_deref()
            .map(|s| &s[..s.len().min(200)])
    );

    let content = std::fs::read_to_string(&file_path).unwrap();
    println!("[live_agent_loop_tool] file content: {}", content);
    assert!(
        content.contains("AGENT_TOOL_OK"),
        "Expected 'AGENT_TOOL_OK' in file, got: {}",
        content
    );
    assert!(
        result.tool_rounds > 0,
        "Expected at least 1 tool round, got: {}",
        result.tool_rounds
    );
}
