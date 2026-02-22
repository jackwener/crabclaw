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
        "tools" => execute_tools(registry),
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
    let registry = builtin_registry();
    let rows = registry.compact_rows();
    let mut lines = vec!["Available commands:".to_string()];
    for row in rows {
        lines.push(format!("  ,{row}"));
    }
    CommandResult {
        success: true,
        output: lines.join("\n"),
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
    fn unknown_command_falls_back_to_model() {
        let (_dir, mut tape) = make_tape();
        let ws = workspace();
        let result = route_user(",nonexistent", &mut tape, ws.path());
        assert!(result.enter_model);
        assert!(result.model_prompt.contains("unknown internal command"));
        assert!(!result.exit_requested);
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
}
