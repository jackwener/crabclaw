use std::path::Path;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use tracing::debug;

use crate::core::config::AppConfig;
use crate::core::context::build_messages;
use crate::core::error::{CrabClawError, Result};
use crate::core::router::route_user;
use crate::llm::api_types::ChatRequest;
use crate::llm::client::send_chat_request;
use crate::tape::store::TapeStore;

/// Run an interactive REPL session.
///
/// Aligned with bub's `InteractiveCli._run()`:
/// - Read input from user
/// - Route through command router
/// - If model needed, build messages from tape + send
/// - Record responses to tape
/// - Loop until exit
pub fn run_interactive(config: &AppConfig, workspace: &Path) -> Result<()> {
    let tape_dir = workspace.join(".crabclaw");
    let mut tape = TapeStore::open(&tape_dir, "default").map_err(CrabClawError::Io)?;
    tape.ensure_bootstrap_anchor().map_err(CrabClawError::Io)?;

    let mut editor = DefaultEditor::new()
        .map_err(|e| CrabClawError::Config(format!("failed to init editor: {e}")))?;

    // Load history from workspace
    let history_path = tape_dir.join("history.txt");
    let _ = editor.load_history(&history_path);

    println!("CrabClaw interactive mode");
    println!("  model: {}", config.model);
    println!("  workspace: {}", workspace.display());
    println!("  Type ,help for commands, ,quit to exit.\n");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CrabClawError::Network(format!("failed to start runtime: {e}")))?;

    // Build tool definitions once (builtins + skills)
    let mut registry = crate::tools::registry::builtin_registry();
    crate::tools::registry::register_skills(&mut registry, workspace);
    let tool_defs = crate::tools::registry::to_tool_definitions(&registry);
    let tools = if tool_defs.is_empty() {
        None
    } else {
        Some(tool_defs)
    };

    loop {
        let cwd_name = workspace
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("crabclaw");

        let readline = editor.readline(&format!("{cwd_name} > "));

        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let _ = editor.add_history_entry(trimmed);

                let route = route_user(trimmed, &mut tape, workspace);

                if route.exit_requested {
                    break;
                }

                if !route.immediate_output.is_empty() {
                    println!("{}", route.immediate_output);
                }

                if !route.enter_model {
                    continue;
                }

                // Record user message to tape
                tape.append_message("user", &route.model_prompt)
                    .map_err(CrabClawError::Io)?;

                // Build multi-turn messages from tape
                let system_prompt = crate::core::context::build_system_prompt(
                    config.system_prompt.as_deref(),
                    workspace,
                );
                let mut messages = build_messages(&tape, Some(&system_prompt));

                debug!(message_count = messages.len(), "sending multi-turn request");

                // Tool calling loop (up to 5 iterations)
                const MAX_TOOL_ITERATIONS: usize = 5;

                for iteration in 0..MAX_TOOL_ITERATIONS {
                    let request = ChatRequest {
                        model: config.model.clone(),
                        messages: messages.clone(),
                        max_tokens: None,
                        tools: tools.clone(),
                    };

                    match rt.block_on(send_chat_request(config, &request)) {
                        Ok(response) => {
                            // Check if model wants to call tools
                            if response.has_tool_calls() {
                                if let Some(tool_calls) = response.tool_calls() {
                                    debug!(
                                        iteration = iteration,
                                        tool_count = tool_calls.len(),
                                        "repl.tool_calls"
                                    );

                                    // Append the assistant message with tool_calls
                                    messages.push(
                                        crate::llm::api_types::Message::assistant_with_tool_calls(
                                            tool_calls.to_vec(),
                                        ),
                                    );

                                    // Execute each tool and append results
                                    for tc in tool_calls {
                                        let result = crate::tools::registry::execute_tool(
                                            &tc.function.name,
                                            &tc.function.arguments,
                                            &tape,
                                            workspace,
                                        );
                                        debug!(
                                            tool = %tc.function.name,
                                            result_len = result.len(),
                                            "repl.tool_result"
                                        );
                                        println!(
                                            "  [tool] {} → {} chars",
                                            tc.function.name,
                                            result.len()
                                        );
                                        messages.push(crate::llm::api_types::Message::tool(
                                            &tc.id, &result,
                                        ));
                                    }
                                    // Continue loop — re-call model with tool results
                                    continue;
                                }
                            }

                            // No tool calls — we have the final response
                            if let Some(content) = response.assistant_content() {
                                tape.append_message("assistant", content)
                                    .map_err(CrabClawError::Io)?;
                                println!("\n{content}\n");
                            } else {
                                eprintln!("warning: no response content from model");
                            }
                            break;
                        }
                        Err(e) => {
                            eprintln!("error: {e}");
                            break;
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("Interrupted. Use ,quit to exit.");
                continue;
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                eprintln!("readline error: {err}");
                break;
            }
        }
    }

    // Save history
    let _ = editor.save_history(&history_path);
    println!("Bye.");
    Ok(())
}
