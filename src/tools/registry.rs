use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;

use crate::tools::schedule::{AgentRunner, Notifier};

/// Execution context passed to tools during a model turn.
///
/// Carries channel-specific capabilities (e.g. notification delivery,
/// agent execution) so each tool invocation has access to its session context.
/// Inspired by Bub's context-bound callback pattern.
#[derive(Clone, Default)]
pub struct ToolContext {
    /// Optional notification callback for the current session.
    /// When set, schedule jobs capture this to deliver reminders
    /// back to the originating channel (e.g. Telegram chat).
    pub notifier: Option<Notifier>,
    /// Optional agent runner for scheduled agent-mode jobs.
    /// When set, schedule jobs can run the full agent pipeline
    /// (LLM + tools) and deliver results on fire.
    pub agent_runner: Option<AgentRunner>,
}

impl ToolContext {
    /// Create an empty context (no notification capability).
    pub fn empty() -> Self {
        Self {
            notifier: None,
            agent_runner: None,
        }
    }

    /// Create a context with a notification callback.
    pub fn with_notifier<F: Fn(String) + Send + Sync + 'static>(f: F) -> Self {
        Self {
            notifier: Some(Arc::new(f)),
            agent_runner: None,
        }
    }
}

/// Metadata for a registered tool.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub source: String,
}

/// Registry for tool descriptors.
///
/// Aligned with bub's `ToolRegistry`:
/// - Stores tool metadata by name
/// - Returns sorted descriptors
/// - Supports builtin and skill-sourced tools
pub struct ToolRegistry {
    tools: BTreeMap<String, ToolDescriptor>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }

    /// Register a tool descriptor.
    pub fn register(&mut self, name: &str, description: &str, source: &str) {
        self.tools.insert(
            name.to_string(),
            ToolDescriptor {
                name: name.to_string(),
                description: description.to_string(),
                source: source.to_string(),
            },
        );
    }

    /// Check if a tool exists.
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&ToolDescriptor> {
        self.tools.get(name)
    }

    /// List all tools sorted by name.
    pub fn list(&self) -> Vec<&ToolDescriptor> {
        self.tools.values().collect()
    }

    /// Format tools as compact rows for display.
    pub fn compact_rows(&self) -> Vec<String> {
        self.tools
            .values()
            .map(|t| format!("{}: {} [{}]", t.name, t.description, t.source))
            .collect()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct BuiltinToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: serde_json::Value,
}

fn empty_tool_parameters() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}

pub fn builtin_tool_specs() -> Vec<BuiltinToolSpec> {
    vec![
        BuiltinToolSpec {
            name: "tape.info",
            description: "Show tape session info (entry count, file path)",
            parameters: empty_tool_parameters(),
        },
        BuiltinToolSpec {
            name: "help",
            description: "Show available commands",
            parameters: empty_tool_parameters(),
        },
        BuiltinToolSpec {
            name: "tools",
            description: "List all registered tools",
            parameters: empty_tool_parameters(),
        },
        BuiltinToolSpec {
            name: "skills",
            description: "List discovered skills from workspace",
            parameters: empty_tool_parameters(),
        },
        BuiltinToolSpec {
            name: "shell.exec",
            description: "Execute a shell command in the workspace directory. Returns stdout, stderr, and exit code.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    }
                },
                "required": ["command"]
            }),
        },
        BuiltinToolSpec {
            name: "file.read",
            description: "Read the contents of a file in the workspace. Path is relative to workspace root.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file relative to the workspace root"
                    }
                },
                "required": ["path"]
            }),
        },
        BuiltinToolSpec {
            name: "file.write",
            description: "Write content to a file in the workspace. Creates parent directories if needed.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file relative to the workspace root"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
        },
        BuiltinToolSpec {
            name: "file.list",
            description: "List the contents of a directory in the workspace. Use empty path for workspace root.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the directory relative to the workspace root. Empty string for root."
                    }
                },
                "required": []
            }),
        },
        BuiltinToolSpec {
            name: "file.search",
            description: "Search for text within files in the workspace (recursive grep). Case-insensitive.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Text to search for (case-insensitive)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional directory to search in, relative to workspace root. Empty for entire workspace."
                    }
                },
                "required": ["query"]
            }),
        },
        BuiltinToolSpec {
            name: "file.edit",
            description: "Edit a file by searching for old text and replacing with new text. Supports replace_all.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file relative to the workspace root"
                    },
                    "old": {
                        "type": "string",
                        "description": "The exact text to search for in the file"
                    },
                    "new": {
                        "type": "string",
                        "description": "The replacement text"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "If true, replace all occurrences. Default: false (replace first only)."
                    }
                },
                "required": ["path", "old", "new"]
            }),
        },
        BuiltinToolSpec {
            name: "web.fetch",
            description: "Fetch a URL and return the content as markdown. HTML is converted automatically.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    }
                },
                "required": ["url"]
            }),
        },
        BuiltinToolSpec {
            name: "web.search",
            description: "Search the web for a query. Returns a search URL to fetch.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    }
                },
                "required": ["query"]
            }),
        },
        BuiltinToolSpec {
            name: "schedule.add",
            description: "Schedule a reminder. Specify after_seconds (one-shot) or interval_seconds (repeating).",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "For 'reminder' mode: the text to deliver. For 'agent' mode: the prompt/task that the AI agent will execute (e.g. 'Fetch top 20 HackerNews posts and summarize them in Chinese')."
                    },
                    "after_seconds": {
                        "type": "integer",
                        "description": "Fire once after this many seconds (one-shot timer)"
                    },
                    "interval_seconds": {
                        "type": "integer",
                        "description": "Fire repeatedly at this interval in seconds"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["reminder", "agent"],
                        "description": "IMPORTANT: Use 'agent' when the task requires action (web fetching, analysis, summarization, etc.). Use 'reminder' only for simple text notifications like 'drink water'. Default is 'reminder'."
                    }
                },
                "required": ["message"]
            }),
        },
        BuiltinToolSpec {
            name: "schedule.list",
            description: "List all active scheduled jobs.",
            parameters: empty_tool_parameters(),
        },
        BuiltinToolSpec {
            name: "schedule.remove",
            description: "Remove a scheduled job by its ID.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "The ID of the job to remove"
                    }
                },
                "required": ["job_id"]
            }),
        },
    ]
}

pub fn builtin_tools_contract_block() -> String {
    let mut lines = vec![
        "<tools_contract>".to_string(),
        "You have access to the following built-in tools:".to_string(),
    ];
    for spec in builtin_tool_specs() {
        lines.push(format!("- {}: {}", spec.name, spec.description));
    }
    lines.push("You can also access any discovered skills from the workspace.".to_string());
    lines.push("When helping the user:".to_string());
    lines.push("- Be concise and actionable".to_string());
    lines.push("- Use tools proactively when they would help answer the question".to_string());
    lines.push("- If a shell command fails, analyze the error and suggest fixes".to_string());
    lines.push("- Prefer reading files over asking the user to paste code".to_string());
    lines.push("</tools_contract>".to_string());
    lines.join("\n")
}

/// Create a registry with CrabClaw's built-in tools pre-registered.
pub fn builtin_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for spec in builtin_tool_specs() {
        registry.register(spec.name, spec.description, "builtin");
    }
    registry
}

/// Register discovered skills from the workspace as tools in the registry.
pub fn register_skills(registry: &mut ToolRegistry, workspace: &std::path::Path) {
    use crate::tools::skills::discover_skills;
    for skill in discover_skills(workspace) {
        registry.register(
            &format!("skill.{}", skill.name),
            &skill.description,
            &skill.source,
        );
    }
}

/// Generate OpenAI-compatible tool definitions from the registry.
///
/// Tools with parameters get proper JSON schemas; others get empty params.
pub fn to_tool_definitions(registry: &ToolRegistry) -> Vec<crate::llm::api_types::ToolDefinition> {
    registry
        .list()
        .into_iter()
        .map(|tool| {
            let parameters = tool_parameters(&tool.name);
            crate::llm::api_types::ToolDefinition {
                tool_type: "function".to_string(),
                function: crate::llm::api_types::FunctionDefinition {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters,
                },
            }
        })
        .collect()
}

/// Return the JSON schema for a tool's parameters.
pub fn tool_parameters(name: &str) -> serde_json::Value {
    builtin_tool_specs()
        .into_iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.parameters)
        .unwrap_or_else(empty_tool_parameters)
}

/// Execute a tool by name and return the result as a string.
///
/// Supports builtin tools, `shell.exec`, and skill tools.
/// The `ctx` parameter carries session-specific context (e.g. notification
/// callbacks for schedule jobs).
pub fn execute_tool(
    name: &str,
    args: &str,
    tape: &crate::tape::store::TapeStore,
    workspace: &std::path::Path,
    ctx: &ToolContext,
) -> String {
    match name {
        "tape.info" => {
            let info = tape.info();
            format!(
                "Tape: {}\nEntries: {}\nAnchors: {}\nLast anchor: {}",
                info.name,
                info.entries,
                info.anchors,
                info.last_anchor.as_deref().unwrap_or("none")
            )
        }
        "tape.reset" => {
            // Note: actual reset requires &mut TapeStore, so we just report status
            "Tape reset is only available via the ,tape.reset command.".to_string()
        }
        "help" => {
            let registry = builtin_registry();
            let tools: Vec<String> = registry
                .list()
                .iter()
                .map(|t| format!("  ,{}: {}", t.name, t.description))
                .collect();
            format!("Available commands:\n{}", tools.join("\n"))
        }
        "tools" => {
            let registry = builtin_registry();
            let rows = registry.compact_rows();
            if rows.is_empty() {
                "No tools registered.".to_string()
            } else {
                rows.join("\n")
            }
        }
        "skills" => {
            use crate::tools::skills::discover_skills;
            let skills = discover_skills(workspace);
            if skills.is_empty() {
                "No skills discovered.".to_string()
            } else {
                skills
                    .iter()
                    .map(|s| format!("  {} — {} [{}]", s.name, s.description, s.source))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        "shell.exec" => {
            // Parse the command argument from the JSON args string.
            let command = match serde_json::from_str::<serde_json::Value>(args) {
                Ok(v) => match v["command"].as_str() {
                    Some(cmd) => cmd.to_string(),
                    None => return "Error: 'command' argument is required.".to_string(),
                },
                Err(_) => {
                    // If args is not JSON, treat it as a raw command string.
                    if args.trim().is_empty() {
                        return "Error: no command provided.".to_string();
                    }
                    args.trim().to_string()
                }
            };

            let result = crate::core::shell::execute_shell(&command, workspace);
            let output = crate::core::shell::format_shell_output(&result);

            if result.exit_code == 0 && !result.timed_out {
                output
            } else {
                crate::core::shell::wrap_failure_context(&command, &result)
            }
        }
        "file.read" => {
            use crate::tools::file_ops;
            let path = parse_json_arg(args, "path").unwrap_or_default();
            if path.is_empty() {
                return "Error: 'path' argument is required.".to_string();
            }
            file_ops::read_file(workspace, &path)
        }
        "file.write" => {
            use crate::tools::file_ops;
            let path = parse_json_arg(args, "path").unwrap_or_default();
            let content = parse_json_arg(args, "content").unwrap_or_default();
            if path.is_empty() {
                return "Error: 'path' argument is required.".to_string();
            }
            file_ops::write_file(workspace, &path, &content)
        }
        "file.list" => {
            use crate::tools::file_ops;
            let path = parse_json_arg(args, "path").unwrap_or_default();
            file_ops::list_directory(workspace, &path)
        }
        "file.search" => {
            use crate::tools::file_ops;
            let query = parse_json_arg(args, "query").unwrap_or_default();
            if query.is_empty() {
                return "Error: 'query' argument is required.".to_string();
            }
            let path = parse_json_arg(args, "path").unwrap_or_default();
            file_ops::search_files(workspace, &query, &path)
        }
        "file.edit" => {
            use crate::tools::file_ops;
            let path = parse_json_arg(args, "path").unwrap_or_default();
            let old = parse_json_arg(args, "old").unwrap_or_default();
            let new = parse_json_arg(args, "new").unwrap_or_default();
            if path.is_empty() {
                return "Error: 'path' argument is required.".to_string();
            }
            if old.is_empty() {
                return "Error: 'old' argument is required.".to_string();
            }
            let replace_all = match serde_json::from_str::<serde_json::Value>(args) {
                Ok(v) => v["replace_all"].as_bool().unwrap_or(false),
                Err(_) => false,
            };
            file_ops::edit_file(workspace, &path, &old, &new, replace_all)
        }
        "web.fetch" => {
            use crate::tools::web;
            let url = parse_json_arg(args, "url").unwrap_or_default();
            if url.is_empty() {
                return "Error: 'url' argument is required.".to_string();
            }
            web::fetch_url(&url)
        }
        "web.search" => {
            use crate::tools::web;
            let query = parse_json_arg(args, "query").unwrap_or_default();
            if query.is_empty() {
                return "Error: 'query' argument is required.".to_string();
            }
            web::web_search(&query)
        }
        "schedule.add" => {
            use crate::tools::schedule::{JobMode, global_scheduler};
            let message = parse_json_arg(args, "message").unwrap_or_default();
            if message.is_empty() {
                return "Error: 'message' argument is required.".to_string();
            }
            let after_seconds = match serde_json::from_str::<serde_json::Value>(args) {
                Ok(v) => v["after_seconds"].as_u64(),
                Err(_) => None,
            };
            let interval_seconds = match serde_json::from_str::<serde_json::Value>(args) {
                Ok(v) => v["interval_seconds"].as_u64(),
                Err(_) => None,
            };
            let mode = match parse_json_arg(args, "mode").as_deref() {
                Some("agent") => JobMode::Agent,
                _ => JobMode::Reminder,
            };
            let agent_runner = if mode == JobMode::Agent {
                ctx.agent_runner.clone()
            } else {
                None
            };
            global_scheduler().add_job(
                &message,
                after_seconds,
                interval_seconds,
                mode,
                ctx.notifier.clone(),
                agent_runner,
            )
        }
        "schedule.list" => {
            use crate::tools::schedule::global_scheduler;
            global_scheduler().list_jobs()
        }
        "schedule.remove" => {
            use crate::tools::schedule::global_scheduler;
            let job_id = parse_json_arg(args, "job_id").unwrap_or_default();
            if job_id.is_empty() {
                return "Error: 'job_id' argument is required.".to_string();
            }
            global_scheduler().remove_job(&job_id)
        }
        _ if name.starts_with("skill.") => {
            // Skill tool: load the skill body and return as context.
            let skill_name = &name["skill.".len()..];
            use crate::tools::skills::load_skill_body;
            match load_skill_body(skill_name, workspace) {
                Some(body) => body,
                None => format!("Skill not found: {skill_name}"),
            }
        }
        _ => format!("Unknown tool: {name}"),
    }
}

/// Helper: parse a string value from a JSON args string.
fn parse_json_arg(args: &str, key: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v[key].as_str().map(String::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_list() {
        let mut reg = ToolRegistry::new();
        reg.register("b.tool", "Tool B", "builtin");
        reg.register("a.tool", "Tool A", "builtin");

        let list = reg.list();
        assert_eq!(list.len(), 2);
        // BTreeMap sorts by key
        assert_eq!(list[0].name, "a.tool");
        assert_eq!(list[1].name, "b.tool");
    }

    #[test]
    fn has_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register("fs.read", "Read a file", "builtin");

        assert!(reg.has("fs.read"));
        assert!(!reg.has("fs.write"));

        let desc = reg.get("fs.read").unwrap();
        assert_eq!(desc.description, "Read a file");
        assert_eq!(desc.source, "builtin");

        assert!(reg.get("fs.write").is_none());
    }

    #[test]
    fn later_registration_overwrites() {
        let mut reg = ToolRegistry::new();
        reg.register("tool", "Version 1", "builtin");
        reg.register("tool", "Version 2", "skill");

        let desc = reg.get("tool").unwrap();
        assert_eq!(desc.description, "Version 2");
        assert_eq!(desc.source, "skill");
    }

    #[test]
    fn compact_rows_format() {
        let mut reg = ToolRegistry::new();
        reg.register("help", "Show help", "builtin");

        let rows = reg.compact_rows();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("help"));
        assert!(rows[0].contains("Show help"));
        assert!(rows[0].contains("[builtin]"));
    }

    #[test]
    fn builtin_registry_has_expected_tools() {
        let reg = builtin_registry();
        assert!(reg.has("tape.info"));
        assert!(reg.has("help"));
        assert!(reg.has("tools"));
        assert!(reg.has("skills"));
        assert!(reg.len() >= 4);
    }

    #[test]
    fn len_and_is_empty() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);

        let reg = builtin_registry();
        assert!(!reg.is_empty());
    }

    #[test]
    fn builtin_has_shell_exec() {
        let reg = builtin_registry();
        assert!(reg.has("shell.exec"));
        let desc = reg.get("shell.exec").unwrap();
        assert!(desc.description.contains("shell command"));
    }

    #[test]
    fn execute_shell_exec_tool() {
        let dir = tempfile::tempdir().unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool(
            "shell.exec",
            r#"{"command": "echo tool_works"}"#,
            &tape,
            dir.path(),
            &ToolContext::empty(),
        );
        assert!(result.contains("tool_works"));
    }

    #[test]
    fn execute_shell_exec_empty_args() {
        let dir = tempfile::tempdir().unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool("shell.exec", "", &tape, dir.path(), &ToolContext::empty());
        assert!(result.contains("no command"));
    }

    #[test]
    fn execute_file_read_invalid_json_args() {
        let dir = tempfile::tempdir().unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool(
            "file.read",
            "not-json",
            &tape,
            dir.path(),
            &ToolContext::empty(),
        );
        assert!(result.contains("'path' argument is required"));
    }

    #[test]
    fn execute_file_write_missing_path_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool(
            "file.write",
            r#"{"content":"hello"}"#,
            &tape,
            dir.path(),
            &ToolContext::empty(),
        );
        assert!(result.contains("'path' argument is required"));
    }

    #[test]
    fn execute_file_edit_missing_old_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool(
            "file.edit",
            r#"{"path":"a.txt","new":"x"}"#,
            &tape,
            dir.path(),
            &ToolContext::empty(),
        );
        assert!(result.contains("'old' argument is required"));
    }

    #[test]
    fn execute_skill_tool_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool(
            "skill.nonexistent",
            "{}",
            &tape,
            dir.path(),
            &ToolContext::empty(),
        );
        assert!(result.contains("Skill not found"));
    }

    #[test]
    fn register_skills_from_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join(".agent/skills/my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: A test skill\n---\n# Body",
        )
        .unwrap();

        let mut reg = builtin_registry();
        register_skills(&mut reg, dir.path());
        assert!(reg.has("skill.my-skill"));
    }

    #[test]
    fn tool_definitions_shell_exec_has_params() {
        let reg = builtin_registry();
        let defs = to_tool_definitions(&reg);
        let shell_def = defs.iter().find(|d| d.function.name == "shell.exec");
        assert!(shell_def.is_some());
        let params = &shell_def.unwrap().function.parameters;
        assert!(params["properties"]["command"].is_object());
        assert_eq!(params["required"][0], "command");
    }

    #[test]
    fn execute_file_read_tool() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello from tool").unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool(
            "file.read",
            r#"{"path": "test.txt"}"#,
            &tape,
            dir.path(),
            &ToolContext::empty(),
        );
        assert!(result.contains("hello from tool"));
    }

    #[test]
    fn execute_file_write_tool() {
        let dir = tempfile::tempdir().unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool(
            "file.write",
            r#"{"path": "out.txt", "content": "written by tool"}"#,
            &tape,
            dir.path(),
            &ToolContext::empty(),
        );
        assert!(result.contains("Written"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("out.txt")).unwrap(),
            "written by tool"
        );
    }

    #[test]
    fn execute_file_list_tool() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool(
            "file.list",
            r#"{"path": ""}"#,
            &tape,
            dir.path(),
            &ToolContext::empty(),
        );
        assert!(result.contains("a.txt"));
        assert!(result.contains("sub/"));
    }

    #[test]
    fn tool_definitions_file_read_has_params() {
        let reg = builtin_registry();
        let defs = to_tool_definitions(&reg);
        let def = defs.iter().find(|d| d.function.name == "file.read");
        assert!(def.is_some());
        let params = &def.unwrap().function.parameters;
        assert!(params["properties"]["path"].is_object());
        assert_eq!(params["required"][0], "path");
    }

    #[test]
    fn tool_definitions_file_write_has_params() {
        let reg = builtin_registry();
        let defs = to_tool_definitions(&reg);
        let def = defs.iter().find(|d| d.function.name == "file.write");
        assert!(def.is_some());
        let params = &def.unwrap().function.parameters;
        assert!(params["properties"]["path"].is_object());
        assert!(params["properties"]["content"].is_object());
    }
}
