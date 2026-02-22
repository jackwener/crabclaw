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
    registry
}

/// Generate OpenAI-compatible tool definitions from the registry.
///
/// Each tool is exposed as a function with no required parameters.
pub fn to_tool_definitions(registry: &ToolRegistry) -> Vec<crate::llm::api_types::ToolDefinition> {
    registry
        .list()
        .into_iter()
        .map(|tool| crate::llm::api_types::ToolDefinition {
            tool_type: "function".to_string(),
            function: crate::llm::api_types::FunctionDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        })
        .collect()
}

/// Execute a tool by name and return the result as a string.
///
/// Currently supports only informational builtin tools.
pub fn execute_tool(
    name: &str,
    _args: &str,
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
        _ => format!("Unknown tool: {name}"),
    }
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
}
