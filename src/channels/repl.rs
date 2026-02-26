use std::path::Path;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use crate::core::agent_loop::AgentLoop;
use crate::core::config::AppConfig;
use crate::core::error::{CrabClawError, Result};

/// Run an interactive REPL session.
///
/// Delegates to `AgentLoop::handle_input_stream` for each user input,
/// which handles command routing, tool calling, tape recording,
/// and streaming output.
pub fn run_interactive(config: &AppConfig, workspace: &Path) -> Result<()> {
    let mut agent = AgentLoop::open(config, workspace, "default")?;

    let mut editor = DefaultEditor::new()
        .map_err(|e| CrabClawError::Config(format!("failed to init editor: {e}")))?;

    // Load history from workspace
    let history_path = workspace.join(".crabclaw").join("history.txt");
    let _ = editor.load_history(&history_path);

    println!("CrabClaw interactive mode");
    println!("  model: {}", config.model);
    println!("  workspace: {}", workspace.display());
    println!("  Type ,help for commands, ,quit to exit.\n");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CrabClawError::Network(format!("failed to start runtime: {e}")))?;

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

                let mut has_started_text = false;
                let result = rt.block_on(agent.handle_input_stream(trimmed, |token| {
                    if !has_started_text {
                        println!();
                        has_started_text = true;
                    }
                    print!("{token}");
                    std::io::Write::flush(&mut std::io::stdout()).unwrap();
                }));

                if has_started_text {
                    println!();
                }

                if result.exit_requested {
                    break;
                }

                if let Some(output) = &result.immediate_output {
                    println!("{output}");
                }

                if result.tool_rounds > 0 {
                    // Print tool round info for user awareness
                    println!("  ({} tool round(s))", result.tool_rounds);
                }

                if let Some(err) = &result.error {
                    eprintln!("error: {err}");
                }

                if result.assistant_output.is_some() {
                    println!();
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
