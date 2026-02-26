use std::collections::HashSet;

use regex::Regex;

use crate::tools::registry::ToolRegistry;

/// Progressive tool view that starts compact and expands on demand.
///
/// Initially, only compact tool descriptions are included in the system prompt.
/// When a `$hint` (e.g. `$file.write`) is detected in model output, the full
/// JSON schema for that tool is expanded into the context.
///
/// This saves significant tokens: instead of sending ~720 tokens of tool schemas
/// on every request, we start with ~50 tokens of compact descriptions and only
/// expand tools the model actually wants to use.
pub struct ProgressiveToolView {
    registry: ToolRegistry,
    expanded: HashSet<String>,
}

impl ProgressiveToolView {
    /// Create a new progressive view over the given registry.
    pub fn new(registry: ToolRegistry) -> Self {
        Self {
            registry,
            expanded: HashSet::new(),
        }
    }

    /// Detect `$hint` patterns in text and expand matching tools.
    ///
    /// Scans for `$tool_name` patterns (e.g. `$file.write`, `$shell.exec`).
    /// When a match corresponds to a registered tool, it gets expanded.
    ///
    /// Returns names of newly expanded tools.
    pub fn activate_hints(&mut self, text: &str) -> Vec<String> {
        lazy_static_regex();
        let re = hint_regex();
        let mut newly_expanded = Vec::new();

        for cap in re.captures_iter(text) {
            if let Some(hint) = cap.get(1) {
                let hint_text = hint.as_str();
                if self.note_hint(hint_text) {
                    newly_expanded.push(hint_text.to_string());
                }
            }
        }

        newly_expanded
    }

    /// Try to expand a tool by hint name (case-insensitive).
    ///
    /// Returns true if a matching tool was found and newly expanded.
    pub fn note_hint(&mut self, hint: &str) -> bool {
        let normalized = hint.to_lowercase();
        for descriptor in self.registry.list() {
            if descriptor.name.to_lowercase() == normalized {
                return self.expanded.insert(descriptor.name.clone());
            }
        }
        false
    }

    /// Mark a tool as selected/expanded (e.g. when model calls it).
    pub fn note_selected(&mut self, name: &str) {
        if self.registry.has(name) {
            self.expanded.insert(name.to_string());
        }
    }

    /// Get all registered tool names.
    pub fn all_tools(&self) -> Vec<String> {
        self.registry
            .list()
            .iter()
            .map(|d| d.name.clone())
            .collect()
    }

    /// Get compact tool descriptions block for system prompt.
    ///
    /// Returns XML-tagged block with one-line descriptions:
    /// ```text
    /// <tool_view>
    ///   - shell.exec: Execute shell commands in the user's workspace
    ///   - file.read: Read file contents (workspace-sandboxed)
    ///   ...
    /// </tool_view>
    /// ```
    pub fn compact_block(&self) -> String {
        let mut lines = vec!["<tool_view>".to_string()];
        for row in self.registry.compact_rows() {
            lines.push(format!("  - {row}"));
        }
        lines.push("</tool_view>".to_string());
        lines.join("\n")
    }

    /// Get expanded tool detail block (only for activated tools).
    ///
    /// Returns XML with full JSON schema for expanded tools.
    /// Returns empty string if no tools have been expanded.
    pub fn expanded_block(&self) -> String {
        if self.expanded.is_empty() {
            return String::new();
        }

        let mut lines = vec!["<tool_details>".to_string()];
        for name in sorted_expanded(&self.expanded) {
            if let Some(descriptor) = self.registry.get(&name) {
                let params = crate::tools::registry::tool_parameters(&name);
                lines.push(format!("  <tool name=\"{}\">", descriptor.name));
                lines.push(format!("    description: {}", descriptor.description));
                lines.push(format!(
                    "    parameters: {}",
                    serde_json::to_string(&params).unwrap_or_default()
                ));
                lines.push("  </tool>".to_string());
            }
        }
        lines.push("</tool_details>".to_string());
        lines.join("\n")
    }

    /// Get tool definitions for API call.
    ///
    /// Only expanded tools get full JSON schema definitions sent to the API.
    /// Non-expanded tools are described in the system prompt but NOT sent
    /// as API tool definitions — this is the key token-saving mechanism.
    pub fn tool_definitions(&self) -> Vec<crate::llm::api_types::ToolDefinition> {
        if self.expanded.is_empty() {
            // No tools expanded yet — send all tools so the model can
            // start calling them. This is the fallback for the first turn.
            return crate::tools::registry::to_tool_definitions(&self.registry);
        }

        // Only send expanded tools as API definitions
        let mut defs = Vec::new();
        for name in sorted_expanded(&self.expanded) {
            if self.registry.has(&name) {
                let params = crate::tools::registry::tool_parameters(&name);
                defs.push(crate::llm::api_types::ToolDefinition {
                    tool_type: "function".to_string(),
                    function: crate::llm::api_types::FunctionDefinition {
                        name: name.clone(),
                        description: self
                            .registry
                            .get(&name)
                            .map(|d| d.description.clone())
                            .unwrap_or_default(),
                        parameters: params,
                    },
                });
            }
        }
        defs
    }

    /// Number of currently expanded tools.
    pub fn expanded_count(&self) -> usize {
        self.expanded.len()
    }

    /// Clear expanded state.
    pub fn reset(&mut self) {
        self.expanded.clear();
    }
}

fn sorted_expanded(expanded: &HashSet<String>) -> Vec<String> {
    let mut names: Vec<String> = expanded.iter().cloned().collect();
    names.sort();
    names
}

fn hint_regex() -> &'static Regex {
    HINT_RE.get_or_init(|| Regex::new(r"\$([A-Za-z0-9_.-]+)").expect("invalid hint regex"))
}

fn lazy_static_regex() {
    let _ = hint_regex();
}

use std::sync::OnceLock;
static HINT_RE: OnceLock<Regex> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register("shell.exec", "Execute shell commands", "builtin");
        registry.register("file.read", "Read file contents", "builtin");
        registry.register("file.write", "Write files", "builtin");
        registry.register("file.list", "List directory contents", "builtin");
        registry
    }

    #[test]
    fn compact_block_contains_all_tools() {
        let view = ProgressiveToolView::new(test_registry());
        let block = view.compact_block();
        assert!(block.contains("<tool_view>"));
        assert!(block.contains("shell.exec"));
        assert!(block.contains("file.read"));
        assert!(block.contains("file.write"));
        assert!(block.contains("file.list"));
        assert!(block.contains("</tool_view>"));
    }

    #[test]
    fn expanded_block_empty_when_no_hints() {
        let view = ProgressiveToolView::new(test_registry());
        assert!(view.expanded_block().is_empty());
    }

    #[test]
    fn note_hint_expands_tool() {
        let mut view = ProgressiveToolView::new(test_registry());
        assert!(view.note_hint("file.write"));
        assert_eq!(view.expanded_count(), 1);
        let block = view.expanded_block();
        assert!(block.contains("file.write"));
        assert!(!block.contains("shell.exec"));
    }

    #[test]
    fn note_hint_case_insensitive() {
        let mut view = ProgressiveToolView::new(test_registry());
        assert!(view.note_hint("FILE.WRITE"));
        assert_eq!(view.expanded_count(), 1);
    }

    #[test]
    fn note_hint_unknown_tool_returns_false() {
        let mut view = ProgressiveToolView::new(test_registry());
        assert!(!view.note_hint("unknown.tool"));
        assert_eq!(view.expanded_count(), 0);
    }

    #[test]
    fn note_hint_duplicate_returns_false() {
        let mut view = ProgressiveToolView::new(test_registry());
        assert!(view.note_hint("file.write"));
        assert!(!view.note_hint("file.write"));
        assert_eq!(view.expanded_count(), 1);
    }

    #[test]
    fn activate_hints_detects_dollar_patterns() {
        let mut view = ProgressiveToolView::new(test_registry());
        let expanded = view.activate_hints("I need to use $file.write and $shell.exec");
        assert_eq!(expanded.len(), 2);
        assert!(expanded.contains(&"file.write".to_string()));
        assert!(expanded.contains(&"shell.exec".to_string()));
        assert_eq!(view.expanded_count(), 2);
    }

    #[test]
    fn activate_hints_ignores_unknown_tools() {
        let mut view = ProgressiveToolView::new(test_registry());
        let expanded = view.activate_hints("$unknown.tool $file.read");
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains(&"file.read".to_string()));
    }

    #[test]
    fn note_selected_expands_tool() {
        let mut view = ProgressiveToolView::new(test_registry());
        view.note_selected("shell.exec");
        assert_eq!(view.expanded_count(), 1);
        let block = view.expanded_block();
        assert!(block.contains("shell.exec"));
    }

    #[test]
    fn reset_clears_expanded() {
        let mut view = ProgressiveToolView::new(test_registry());
        view.note_hint("file.write");
        view.note_hint("shell.exec");
        assert_eq!(view.expanded_count(), 2);
        view.reset();
        assert_eq!(view.expanded_count(), 0);
        assert!(view.expanded_block().is_empty());
    }

    #[test]
    fn tool_definitions_returns_all_when_none_expanded() {
        let view = ProgressiveToolView::new(test_registry());
        let defs = view.tool_definitions();
        assert_eq!(defs.len(), 4); // all tools
    }

    #[test]
    fn tool_definitions_returns_only_expanded() {
        let mut view = ProgressiveToolView::new(test_registry());
        view.note_selected("file.write");
        let defs = view.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].function.name, "file.write");
    }

    #[test]
    fn all_tools_returns_names() {
        let view = ProgressiveToolView::new(test_registry());
        let names = view.all_tools();
        assert_eq!(names.len(), 4);
        assert!(names.contains(&"shell.exec".to_string()));
    }
}
