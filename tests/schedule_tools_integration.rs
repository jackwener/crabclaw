//! End-to-end integration tests for schedule.add, schedule.list, and schedule.remove tools.
//!
//! Tests verify the full pipeline: mock LLM issues a tool call →
//! `process_message` dispatches to `execute_tool` → schedule tool runs →
//! result flows back through the agent loop.

mod support;

use crabclaw::channels::telegram::process_message;
use support::assertions::assert_ok_reply;
use support::builders::openai_config;
use support::responses::{text_response, tool_call_response};
use tempfile::TempDir;

#[tokio::test]
async fn e2e_schedule_add_one_shot() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "schedule.add",
            "call_sched",
            r#"{"message":"time to stretch","after_seconds":300}"#,
        ))
        .create_async()
        .await;

    let final_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response(
            "Done! I've set a reminder to stretch in 5 minutes.",
        ))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "remind me to stretch in 5 minutes",
        &config,
        workspace.path(),
        "test:schedule_add",
        None,
        None,
    )
    .await;

    final_mock.assert_async().await;
    assert_ok_reply(
        &response,
        "Done! I've set a reminder to stretch in 5 minutes.",
    );
}

#[tokio::test]
async fn e2e_schedule_list_empty() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response("schedule.list", "call_list", "{}"))
        .create_async()
        .await;

    let final_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("You have no scheduled reminders."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "show my reminders",
        &config,
        workspace.path(),
        "test:schedule_list",
        None,
        None,
    )
    .await;

    final_mock.assert_async().await;
    assert_ok_reply(&response, "You have no scheduled reminders.");
}

#[tokio::test]
async fn e2e_schedule_add_missing_message_error() {
    let mut server = mockito::Server::new_async().await;
    // LLM calls schedule.add without message
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "schedule.add",
            "call_no_msg",
            r#"{"after_seconds":60}"#,
        ))
        .create_async()
        .await;

    let final_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("I need a message for the reminder."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "set a reminder",
        &config,
        workspace.path(),
        "test:schedule_no_msg",
        None,
        None,
    )
    .await;

    final_mock.assert_async().await;
    assert_ok_reply(&response, "I need a message for the reminder.");
}

#[tokio::test]
async fn e2e_schedule_remove_nonexistent() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(tool_call_response(
            "schedule.remove",
            "call_rm",
            r#"{"job_id":"fakeid123"}"#,
        ))
        .create_async()
        .await;

    let final_mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(text_response("That reminder doesn't exist."))
        .create_async()
        .await;

    let config = openai_config(&server.url());
    let workspace = TempDir::new().unwrap();
    let response = process_message(
        "cancel reminder fakeid123",
        &config,
        workspace.path(),
        "test:schedule_rm",
        None,
        None,
    )
    .await;

    final_mock.assert_async().await;
    assert_ok_reply(&response, "That reminder doesn't exist.");
}
