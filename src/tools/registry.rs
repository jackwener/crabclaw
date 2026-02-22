use std::collections::BTreeMap;

use serde::Serialize;

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

/// Create a registry with CrabClaw's built-in tools pre-registered.
pub fn builtin_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(
        "tape.info",
        "Show tape session info (entry count, file path)",
        "builtin",
    );
    registry.register(
        "tape.reset",
        "Reset the tape session and clear conversation history",
        "builtin",
    );
    registry.register("help", "Show available commands", "builtin");
    registry.register("tools", "List all registered tools", "builtin");
    registry.register("skills", "List discovered skills from workspace", "builtin");
    registry.register(
        "shell.exec",
        "Execute a shell command in the workspace directory. Returns stdout, stderr, and exit code.",
        "builtin",
    );
    registry.register(
        "file.read",
        "Read the contents of a file in the workspace. Path is relative to workspace root.",
        "builtin",
    );
    registry.register(
        "file.write",
        "Write content to a file in the workspace. Creates parent directories if needed.",
        "builtin",
    );
    registry.register(
        "file.list",
        "List the contents of a directory in the workspace. Use empty path for workspace root.",
        "builtin",
    );
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
    match name {
        "shell.exec" => serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                }
            },
            "required": ["command"]
        }),
        "file.read" => serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file relative to the workspace root"
                }
            },
            "required": ["path"]
        }),
        "file.write" => serde_json::json!({
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
        "file.list" => serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory relative to the workspace root. Empty string for root."
                }
            },
            "required": []
        }),
        _ => serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    }
}

/// Execute a tool by name and return the result as a string.
///
/// Supports builtin tools, `shell.exec`, and skill tools.
pub fn execute_tool(
    name: &str,
    args: &str,
    tape: &crate::tape::store::TapeStore,
    workspace: &std::path::Path,
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
                Ok(v) => v["command"]
                    .as_str()
                    .unwrap_or("echo 'missing command argument'")
                    .to_string(),
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
        assert!(reg.has("tape.reset"));
        assert!(reg.has("help"));
        assert!(reg.has("tools"));
        assert!(reg.has("skills"));
        assert!(reg.len() >= 5);
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
        );
        assert!(result.contains("tool_works"));
    }

    #[test]
    fn execute_shell_exec_empty_args() {
        let dir = tempfile::tempdir().unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool("shell.exec", "", &tape, dir.path());
        assert!(result.contains("no command"));
    }

    #[test]
    fn execute_skill_tool_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let tape = crate::tape::store::TapeStore::open(dir.path(), "test").unwrap();
        let result = execute_tool("skill.nonexistent", "{}", &tape, dir.path());
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
        let result = execute_tool("file.read", r#"{"path": "test.txt"}"#, &tape, dir.path());
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
        let result = execute_tool("file.list", r#"{"path": ""}"#, &tape, dir.path());
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
