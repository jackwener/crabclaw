use std::path::Path;

use serde::Serialize;

use crate::core::command::{CommandKind, ParsedArgs, detect_command};
use crate::tape::store::TapeStore;
use crate::tools::registry::{ToolRegistry, builtin_registry};
use crate::tools::skills;

/// Routing outcome for user input.
#[derive(Debug, Clone)]
pub struct UserRouteResult {
    /// Whether the input should be sent to the model.
    pub enter_model: bool,
    /// The prompt to send to the model (if enter_model is true).
    pub model_prompt: String,
    /// Immediate output to display to the user.
    pub immediate_output: String,
    /// Whether the user requested to exit.
    pub exit_requested: bool,
}

/// Routing outcome for assistant (model) output.
///
/// When the model outputs comma-prefixed commands, they are automatically executed.
/// Results are captured as structured `<command>` blocks to feed back to the model.
#[derive(Debug, Clone, Default)]
pub struct AssistantRouteResult {
    /// Text from the assistant that is NOT a command — shown to user.
    pub visible_text: String,
    /// Command execution results as structured XML blocks.
    /// If non-empty, these should be fed back to the model as context.
    pub command_blocks: Vec<String>,
    /// Whether a quit/exit was requested.
    pub exit_requested: bool,
}

impl AssistantRouteResult {
    /// Whether any commands were detected and executed.
    pub fn has_commands(&self) -> bool {
        !self.command_blocks.is_empty()
    }

    /// Combined command blocks for feeding back to the model.
    pub fn next_prompt(&self) -> String {
        self.command_blocks.join("\n")
    }
}
/// Route user input to the appropriate handler.
///
/// Logic (aligned with bub's `InputRouter.route_user`):
/// 1. Empty input → ignored
/// 2. `,` prefix → parse as command, execute internally
/// 3. Successful command → return output directly
/// 4. Unknown command → fallback to model with context
/// 5. Natural language → send to model
pub fn route_user(input: &str, tape: &mut TapeStore, workspace: &Path) -> UserRouteResult {
    let stripped = input.trim();

    if stripped.is_empty() {
        return UserRouteResult {
            enter_model: false,
            model_prompt: String::new(),
            immediate_output: String::new(),
            exit_requested: false,
        };
    }

    let Some(command) = detect_command(stripped) else {
        // Natural language → route to model
        tape.append_event(
            "route",
            serde_json::json!({"kind": "model", "input": stripped}),
        )
        .ok();
        return UserRouteResult {
            enter_model: true,
            model_prompt: stripped.to_string(),
            immediate_output: String::new(),
            exit_requested: false,
        };
    };

    // Execute internal command
    match command.kind {
        CommandKind::Internal => {
            let registry = builtin_registry();
            let result = execute_internal(&command.name, tape, &command.args, workspace, &registry);

            tape.append_event(
                "command",
                serde_json::json!({
                    "origin": "human",
                    "kind": "internal",
                    "name": command.name,
                    "status": if result.success { "ok" } else { "error" },
                    "output": result.output,
                }),
            )
            .ok();

            if result.exit_requested {
                return UserRouteResult {
                    enter_model: false,
                    model_prompt: String::new(),
                    immediate_output: String::new(),
                    exit_requested: true,
                };
            }

            if result.success {
                UserRouteResult {
                    enter_model: false,
                    model_prompt: String::new(),
                    immediate_output: result.output,
                    exit_requested: false,
                }
            } else {
                // Failed command falls back to model with context
                let context = format!(
                    "<command name=\"{}\" status=\"error\">\n{}\n</command>",
                    command.name, result.output
                );
                UserRouteResult {
                    enter_model: true,
                    model_prompt: context,
                    immediate_output: result.output.clone(),
                    exit_requested: false,
                }
            }
        }
        CommandKind::Shell => {
            use crate::core::shell;

            let shell_result = shell::execute_shell(&command.raw, workspace);
            let display_output = shell::format_shell_output(&shell_result);

            tape.append_event(
                "command",
                serde_json::json!({
                    "origin": "human",
                    "kind": "shell",
                    "cmd": command.raw,
                    "exit_code": shell_result.exit_code,
                    "timed_out": shell_result.timed_out,
                    "stdout": shell_result.stdout,
                    "stderr": shell_result.stderr,
                }),
            )
            .ok();

            if shell_result.exit_code == 0 && !shell_result.timed_out {
                // Success → return output directly, do not enter model.
                UserRouteResult {
                    enter_model: false,
                    model_prompt: String::new(),
                    immediate_output: display_output,
                    exit_requested: false,
                }
            } else {
                // Failure → structured context for LLM self-correction.
                let context = shell::wrap_failure_context(&command.raw, &shell_result);
                UserRouteResult {
                    enter_model: true,
                    model_prompt: context,
                    immediate_output: display_output,
                    exit_requested: false,
                }
            }
        }
    }
}

/// Route assistant (model) output through command detection.
///
/// Scans each line of the assistant's output for comma-prefixed commands.
/// Commands are executed immediately:
/// - Successful commands: result captured as `<command>` block
/// - Failed commands: result captured similarly for self-correction
///
/// Lines that are NOT commands are kept as `visible_text`.
/// If any commands were found, their results become `command_blocks`
/// which should be fed back to the model in the next turn.
pub fn route_assistant(text: &str, tape: &mut TapeStore, workspace: &Path) -> AssistantRouteResult {
    let mut visible_lines = Vec::new();
    let mut command_blocks = Vec::new();
    let mut exit_requested = false;
    let mut in_fence = false;

    for line in text.lines() {
        let stripped = line.trim();

        // Track code fence boundaries
        if stripped.starts_with("```") {
            in_fence = !in_fence;
            if !command_blocks.is_empty() {
                // Don't add fence markers to visible output when executing commands
                continue;
            }
            visible_lines.push(line.to_string());
            continue;
        }

        // Detect comma-prefixed commands
        let command = detect_command(stripped);

        if command.is_none() {
            visible_lines.push(line.to_string());
            continue;
        }

        let command = command.unwrap();

        match command.kind {
            CommandKind::Shell => {
                use crate::core::shell;

                let shell_result = shell::execute_shell(&command.raw, workspace);

                tape.append_event(
                    "command",
                    serde_json::json!({
                        "origin": "assistant",
                        "kind": "shell",
                        "cmd": command.raw,
                        "exit_code": shell_result.exit_code,
                        "timed_out": shell_result.timed_out,
                        "stdout": shell_result.stdout,
                        "stderr": shell_result.stderr,
                    }),
                )
                .ok();

                let block = if shell_result.exit_code == 0 && !shell_result.timed_out {
                    let output = shell::format_shell_output(&shell_result);
                    format!(
                        "<command name=\"{}\" status=\"ok\">\n{}\n</command>",
                        command.raw, output
                    )
                } else {
                    shell::wrap_failure_context(&command.raw, &shell_result)
                };
                command_blocks.push(block);
            }
            CommandKind::Internal => {
                // Skip quit from assistant — model shouldn't be able to quit
                if command.name == "quit" {
                    visible_lines.push(line.to_string());
                    continue;
                }

                let registry = builtin_registry();
                let result =
                    execute_internal(&command.name, tape, &command.args, workspace, &registry);

                tape.append_event(
                    "command",
                    serde_json::json!({
                        "origin": "assistant",
                        "kind": "internal",
                        "name": command.name,
                        "status": if result.success { "ok" } else { "error" },
                        "output": result.output,
                    }),
                )
                .ok();

                let status = if result.success { "ok" } else { "error" };
                let block = format!(
                    "<command name=\"{}\" status=\"{}\">\n{}\n</command>",
                    command.name, status, result.output
                );
                command_blocks.push(block);

                if result.exit_requested {
                    exit_requested = true;
                }
            }
        }
    }

    // Build visible text from non-command lines
    let visible_text = if command_blocks.is_empty() {
        text.to_string() // No commands found, return original text
    } else {
        visible_lines.join("\n").trim().to_string()
    };

    AssistantRouteResult {
        visible_text,
        command_blocks,
        exit_requested,
    }
}

#[derive(Debug)]
struct CommandResult {
    success: bool,
    output: String,
    exit_requested: bool,
}

fn execute_internal(
    name: &str,
    tape: &mut TapeStore,
    args: &ParsedArgs,
    workspace: &Path,
    registry: &ToolRegistry,
) -> CommandResult {
    match name {
        "help" => execute_help(),
        "quit" => CommandResult {
            success: true,
            output: "exit".to_string(),
            exit_requested: true,
        },
        "tape.info" | "tape" => execute_tape_info(tape),
        "tape.reset" => execute_tape_reset(tape, args.has_flag("archive")),
        "tape.search" => {
            let query = args.positional.join(" ");
            if query.is_empty() {
                return CommandResult {
                    success: false,
                    output: "Usage: ,tape.search <query>".to_string(),
                    exit_requested: false,
                };
            }
            let results = tape.search(&query);
            if results.is_empty() {
                CommandResult {
                    success: true,
                    output: format!("No entries matching '{query}'."),
                    exit_requested: false,
                }
            } else {
                let lines: Vec<String> = results
                    .iter()
                    .map(|e| {
                        let preview = serde_json::to_string(&e.payload)
                            .unwrap_or_default()
                            .chars()
                            .take(80)
                            .collect::<String>();
                        format!("  [{}] {} #{}: {}", e.timestamp, e.kind, e.id, preview)
                    })
                    .collect();
                CommandResult {
                    success: true,
                    output: format!(
                        "Found {} match(es) for '{query}':\n{}",
                        results.len(),
                        lines.join("\n")
                    ),
                    exit_requested: false,
                }
            }
        }
        "anchors" => {
            let anchors = tape.anchor_entries();
            if anchors.is_empty() {
                CommandResult {
                    success: true,
                    output: "No anchors in tape.".to_string(),
                    exit_requested: false,
                }
            } else {
                let lines: Vec<String> = anchors
                    .iter()
                    .map(|a| {
                        let name = a
                            .payload
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unnamed");
                        format!("  #{} [{}] {}", a.id, a.timestamp, name)
                    })
                    .collect();
                CommandResult {
                    success: true,
                    output: format!("Anchors ({}):\n{}", anchors.len(), lines.join("\n")),
                    exit_requested: false,
                }
            }
        }
        "handoff" => {
            let anchor_name = if args.positional.is_empty() {
                "handoff".to_string()
            } else {
                args.positional.join(" ")
            };
            let info = tape.info();
            match tape.anchor(
                &anchor_name,
                serde_json::json!({
                    "owner": "human",
                    "type": "handoff",
                    "entries_before": info.entries,
                    "previous_anchor": info.last_anchor,
                }),
            ) {
                Ok(_) => CommandResult {
                    success: true,
                    output: format!(
                        "Handoff anchor '{}' created. Context window reset ({} entries before).",
                        anchor_name, info.entries
                    ),
                    exit_requested: false,
                },
                Err(e) => CommandResult {
                    success: false,
                    output: format!("Failed to create anchor: {e}"),
                    exit_requested: false,
                },
            }
        }
        "tools" => execute_tools(registry),
        "tool.describe" => {
            let name = if args.positional.is_empty() {
                return CommandResult {
                    success: false,
                    output: "Usage: ,tool.describe <tool_name>".to_string(),
                    exit_requested: false,
                };
            } else {
                args.positional[0].clone()
            };
            match registry.get(&name) {
                Some(tool) => {
                    let params = crate::tools::registry::tool_parameters(&name);
                    let params_str = serde_json::to_string_pretty(&params).unwrap_or_default();
                    CommandResult {
                        success: true,
                        output: format!(
                            "Tool: {}\nDescription: {}\nSource: {}\nParameters:\n{}",
                            tool.name, tool.description, tool.source, params_str
                        ),
                        exit_requested: false,
                    }
                }
                None => CommandResult {
                    success: false,
                    output: format!("Tool not found: {name}"),
                    exit_requested: false,
                },
            }
        }
        "skills" => execute_skills(workspace),
        "skills.describe" => execute_skills_describe(args, workspace),
        _ => CommandResult {
            success: false,
            output: format!("unknown internal command: {name}"),
            exit_requested: false,
        },
    }
}

fn execute_help() -> CommandResult {
    let help = "\
Available commands:
  ,help               — Show this help
  ,quit               — Exit the session
  ,tape               — Show tape session info
  ,tape.info          — Show tape session info (alias)
  ,tape.reset         — Reset the tape (--archive to keep backup)
  ,tape.search <q>    — Search tape entries by content
  ,anchors            — List all anchors in the tape
  ,handoff [name]     — Create a handoff anchor (resets context window)
  ,tools              — List all registered tools
  ,tool.describe <n>  — Show tool details and parameter schema
  ,skills             — List discovered skills
  ,skills.describe <n>— Show full body of a skill
  ,<shell command>    — Execute a shell command (e.g. ,ls, ,git status)";

    CommandResult {
        success: true,
        output: help.to_string(),
        exit_requested: false,
    }
}

fn execute_tape_info(tape: &TapeStore) -> CommandResult {
    let info = tape.info();
    let output = serde_json::to_string_pretty(&TapeInfoDisplay {
        name: &info.name,
        entries: info.entries,
        anchors: info.anchors,
        last_anchor: info.last_anchor.as_deref(),
        entries_since_last_anchor: info.entries_since_last_anchor,
    })
    .unwrap_or_else(|_| format!("{info:?}"));

    CommandResult {
        success: true,
        output,
        exit_requested: false,
    }
}

#[derive(Serialize)]
struct TapeInfoDisplay<'a> {
    name: &'a str,
    entries: usize,
    anchors: usize,
    last_anchor: Option<&'a str>,
    entries_since_last_anchor: usize,
}

fn execute_tape_reset(tape: &mut TapeStore, archive: bool) -> CommandResult {
    match tape.reset(archive) {
        Ok(archive_path) => {
            let msg = if let Some(path) = archive_path {
                format!("Tape reset. Archived: {}", path.display())
            } else {
                "Tape reset.".to_string()
            };
            CommandResult {
                success: true,
                output: msg,
                exit_requested: false,
            }
        }
        Err(e) => CommandResult {
            success: false,
            output: format!("failed to reset tape: {e}"),
            exit_requested: false,
        },
    }
}

fn execute_tools(registry: &ToolRegistry) -> CommandResult {
    let rows = registry.compact_rows();
    if rows.is_empty() {
        return CommandResult {
            success: true,
            output: "No tools registered.".to_string(),
            exit_requested: false,
        };
    }
    let mut lines = vec![format!("Registered tools ({}):", rows.len())];
    for row in rows {
        lines.push(format!("  {row}"));
    }
    CommandResult {
        success: true,
        output: lines.join("\n"),
        exit_requested: false,
    }
}

fn execute_skills(workspace: &Path) -> CommandResult {
    let discovered = skills::discover_skills(workspace);
    if discovered.is_empty() {
        return CommandResult {
            success: true,
            output: "No skills discovered.".to_string(),
            exit_requested: false,
        };
    }
    let mut lines = vec![format!("Discovered skills ({}):", discovered.len())];
    for skill in &discovered {
        lines.push(format!(
            "  {}: {} [{}]",
            skill.name, skill.description, skill.source
        ));
    }
    CommandResult {
        success: true,
        output: lines.join("\n"),
        exit_requested: false,
    }
}

fn execute_skills_describe(args: &ParsedArgs, workspace: &Path) -> CommandResult {
    let name = match args.positional.first() {
        Some(n) => n,
        None => {
            return CommandResult {
                success: false,
                output: "usage: ,skills.describe <name>".to_string(),
                exit_requested: false,
            };
        }
    };

    match skills::load_skill_body(name, workspace) {
        Some(body) => CommandResult {
            success: true,
            output: body,
            exit_requested: false,
        },
        None => CommandResult {
            success: false,
            output: format!("skill not found: {name}"),
            exit_requested: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_tape() -> (tempfile::TempDir, TapeStore) {
        let dir = tempdir().unwrap();
        let tape = TapeStore::open(dir.path(), "router-test").unwrap();
        (dir, tape)
    }

    fn workspace() -> tempfile::TempDir {
        tempdir().unwrap()
    }

    #[test]
    fn empty_input_does_nothing() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user("", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(!result.exit_requested);
        assert!(result.immediate_output.is_empty());
    }

    #[test]
    fn natural_language_routes_to_model() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user("What is Rust?", &mut tape, ws.path());
        assert!(result.enter_model);
        assert_eq!(result.model_prompt, "What is Rust?");
        assert!(!result.exit_requested);
    }

    #[test]
    fn help_command_returns_immediately() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",help", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("Available commands"));
        assert!(result.immediate_output.contains("help"));
        assert!(!result.exit_requested);
    }

    #[test]
    fn quit_command_sets_exit() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",quit", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.exit_requested);
    }

    #[test]
    fn tape_info_returns_stats() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        tape.ensure_bootstrap_anchor().unwrap();
        let result = route_user(",tape.info", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("router-test"));
    }

    #[test]
    fn tape_reset_resets_and_reports() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        tape.append_message("user", "hello").unwrap();
        let result = route_user(",tape.reset", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("Tape reset"));
        // After reset, bootstrap anchor + the command event recording the reset itself
        assert_eq!(tape.entries().len(), 2);
        assert_eq!(tape.entries()[0].kind, "anchor");
        assert_eq!(tape.entries()[1].kind, "command");
    }

    #[test]
    fn shell_command_executes_echo() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",echo hello_shell", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("hello_shell"));
        assert!(!result.exit_requested);
    }

    #[test]
    fn shell_command_failure_wraps_to_model() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",exit 1", &mut tape, ws.path());
        // Failed shell command should enter model with structured context.
        assert!(result.enter_model);
        assert!(result.model_prompt.contains("<command cmd="));
        assert!(result.model_prompt.contains("exit_code=\"1\""));
        assert!(result.model_prompt.contains("</command>"));
    }

    #[test]
    fn shell_command_captures_stderr() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",echo oops >&2", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("oops"));
    }

    #[test]
    fn shell_command_records_tape_event() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        route_user(",echo tape_test", &mut tape, ws.path());
        let entries = tape.entries();
        let shell_events: Vec<_> = entries.iter().filter(|e| e.kind == "command").collect();
        assert!(!shell_events.is_empty());
        let last = shell_events.last().unwrap();
        let data = &last.payload;
        assert_eq!(data["kind"], "shell");
        assert_eq!(data["exit_code"], 0);
    }

    #[test]
    fn shell_command_git_status_type() {
        // ,git status should be detected as Shell, not Internal.
        let cmd = crate::core::command::detect_command(",git status").unwrap();
        assert_eq!(cmd.kind, crate::core::command::CommandKind::Shell);
        assert_eq!(cmd.name, "git");
    }

    #[test]
    fn internal_command_help_type() {
        let cmd = crate::core::command::detect_command(",help").unwrap();
        assert_eq!(cmd.kind, crate::core::command::CommandKind::Internal);
    }

    #[test]
    fn tape_alias_works() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        tape.ensure_bootstrap_anchor().unwrap();
        let result = route_user(",tape", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("router-test"));
    }

    #[test]
    fn tools_command_lists_builtins() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",tools", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("Registered tools"));
        assert!(result.immediate_output.contains("tape.info"));
    }

    #[test]
    fn skills_command_empty_workspace() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",skills", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("No skills discovered"));
    }

    #[test]
    fn skills_describe_missing_name() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",skills.describe", &mut tape, ws.path());
        // Failed command (no name provided) falls back to model
        assert!(result.enter_model);
        assert!(result.model_prompt.contains("usage"));
    }
    #[test]
    fn skills_describe_not_found() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",skills.describe nonexistent", &mut tape, ws.path());
        // Falls back to model on error
        assert!(result.enter_model);
        assert!(result.model_prompt.contains("skill not found"));
    }

    #[test]
    fn tape_search_finds_messages() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        tape.append_message("user", "hello world").unwrap();
        tape.append_message("assistant", "greetings").unwrap();

        let result = route_user(",tape.search world", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("1 match"));
    }

    #[test]
    fn tape_search_no_query_shows_usage() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",tape.search", &mut tape, ws.path());
        // Missing query is a failed command → model gets context
        assert!(result.enter_model);
        assert!(result.model_prompt.contains("Usage"));
    }

    #[test]
    fn anchors_command_lists_anchors() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        tape.anchor("test-anchor", serde_json::json!({})).unwrap();

        let result = route_user(",anchors", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("test-anchor"));
        assert!(result.immediate_output.contains("Anchors ("));
    }

    #[test]
    fn handoff_creates_anchor() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        tape.append_message("user", "old msg").unwrap();

        let result = route_user(",handoff checkpoint-1", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("checkpoint-1"));
        assert!(result.immediate_output.contains("created"));

        // Verify anchor was actually created
        let anchors = tape.anchor_entries();
        let last = anchors.last().unwrap();
        assert_eq!(last.payload["name"], "checkpoint-1");
    }

    #[test]
    fn tool_describe_shows_params() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",tool.describe shell.exec", &mut tape, ws.path());
        assert!(!result.enter_model);
        assert!(result.immediate_output.contains("shell.exec"));
        assert!(result.immediate_output.contains("command"));
    }

    #[test]
    fn tool_describe_unknown_tool() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",tool.describe nonexistent", &mut tape, ws.path());
        // Unknown tool fails and falls to model
        assert!(result.enter_model);
    }

    // ── route_assistant tests ──────────────────────────────────────

    #[test]
    fn assistant_no_commands_passthrough() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_assistant(
            "Here is a normal response from the model.",
            &mut tape,
            ws.path(),
        );
        assert!(!result.has_commands());
        assert_eq!(
            result.visible_text,
            "Here is a normal response from the model."
        );
        assert!(!result.exit_requested);
    }

    #[test]
    fn assistant_shell_command_detected() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_assistant("Let me check:\n,echo hello world", &mut tape, ws.path());
        assert!(result.has_commands());
        assert_eq!(result.command_blocks.len(), 1);
        assert!(result.command_blocks[0].contains("hello world"));
        assert!(result.command_blocks[0].contains("<command"));
        // Visible text should have the preamble but not the command
        assert!(result.visible_text.contains("Let me check:"));
    }

    #[test]
    fn assistant_internal_command_detected() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_assistant("Checking tools:\n,help", &mut tape, ws.path());
        assert!(result.has_commands());
        assert_eq!(result.command_blocks.len(), 1);
        assert!(result.command_blocks[0].contains("help"));
    }

    #[test]
    fn assistant_quit_blocked() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_assistant(",quit", &mut tape, ws.path());
        // Quit from assistant should be blocked
        assert!(!result.has_commands());
        assert!(!result.exit_requested);
    }

    #[test]
    fn assistant_mixed_text_and_commands() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_assistant(
            "Starting work.\n,echo step1\nDone with step 1.\n,echo step2\nAll done.",
            &mut tape,
            ws.path(),
        );
        assert!(result.has_commands());
        assert_eq!(result.command_blocks.len(), 2);
        assert!(result.visible_text.contains("Starting work."));
        assert!(result.visible_text.contains("All done."));
    }

    #[test]
    fn assistant_command_in_fence() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_assistant(
            "Run this:\n```\n,echo inside_fence\n```",
            &mut tape,
            ws.path(),
        );
        assert!(result.has_commands());
        assert_eq!(result.command_blocks.len(), 1);
        assert!(result.command_blocks[0].contains("inside_fence"));
    }

    #[test]
    fn assistant_next_prompt() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_assistant(",echo hello\n,echo world", &mut tape, ws.path());
        let prompt = result.next_prompt();
        assert!(prompt.contains("hello"));
        assert!(prompt.contains("world"));
    }
}
