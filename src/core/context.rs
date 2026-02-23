use crate::llm::api_types::Message;
use crate::tape::store::TapeStore;
use std::path::Path;

/// Default system prompt that tells the LLM about CrabClaw's capabilities.
const DEFAULT_SYSTEM_PROMPT: &str = "\
You are CrabClaw, a helpful coding assistant running in a terminal environment.

You have access to the following tools:
- shell.exec: Execute shell commands in the user's workspace
- file.read: Read file contents (workspace-sandboxed)
- file.write: Write or create files (workspace-sandboxed)
- file.list: List directory contents
- file.search: Search for text within files (recursive grep)

You can also access any discovered skills from the workspace.

When helping the user:
- Be concise and actionable
- Use tools proactively when they would help answer the question
- If a shell command fails, analyze the error and suggest fixes
- Prefer reading files over asking the user to paste code";

/// Build the system prompt from available sources.
///
/// Priority (highest first):
/// 1. Explicit config override (e.g., CLI flag or env var)
/// 2. `.agent/system-prompt.md` from workspace
/// 3. Default built-in prompt
pub fn build_system_prompt(config_prompt: Option<&str>, workspace: &Path) -> String {
    // 1. Explicit override takes precedence
    if let Some(prompt) = config_prompt {
        let trimmed = prompt.trim();
        if !trimmed.is_empty() {
            return prompt.to_string();
        }
    }

    // 2. Try loading from workspace
    let custom_path = workspace.join(".agent/system-prompt.md");
    #[allow(clippy::collapsible_if)]
    if custom_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&custom_path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    // 3. Fall back to default
    DEFAULT_SYSTEM_PROMPT.to_string()
}

/// Build a list of messages from tape entries for multi-turn conversation.
///
/// Aligned with bub's `tape/context.py::_select_messages`:
/// - Only includes entries since the last anchor (context truncation)
/// - Extracts entries with kind "message"
/// - Preserves role and content from payload
/// - Optionally prepends a system prompt
pub fn build_messages(
    tape: &TapeStore,
    system_prompt: Option<&str>,
    max_context_messages: usize,
) -> Vec<Message> {
    let mut messages = Vec::new();

    if let Some(prompt) = system_prompt {
        let trimmed = prompt.trim();
        if !trimmed.is_empty() {
            messages.push(Message::system(prompt));
        }
    }

    // Use entries since last anchor for context truncation
    let mut tape_messages = Vec::new();
    for entry in tape.entries_since_last_anchor() {
        if entry.kind != "message" {
            continue;
        }

        let role = entry
            .payload
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("user");

        let content = entry
            .payload
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if content.is_empty() {
            continue;
        }

        tape_messages.push(Message {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    if tape_messages.len() > max_context_messages {
        messages.push(Message::system(
            "Older messages in this session have been truncated to fit the context window.",
        ));
        let keep_start = tape_messages.len() - max_context_messages;
        messages.extend(tape_messages.into_iter().skip(keep_start));
    } else {
        messages.extend(tape_messages);
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn empty_tape_no_system_prompt() {
        let dir = tempdir().unwrap();
        let tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        let msgs = build_messages(&tape, None, 50);
        assert!(msgs.is_empty());
    }

    #[test]
    fn empty_tape_with_system_prompt() {
        let dir = tempdir().unwrap();
        let tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        let msgs = build_messages(&tape, Some("You are a helpful assistant."), 50);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content, "You are a helpful assistant.");
    }

    #[test]
    fn tape_messages_in_order() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        tape.append_message("user", "Hello").unwrap();
        tape.append_message("assistant", "Hi there!").unwrap();
        tape.append_message("user", "How are you?").unwrap();

        let msgs = build_messages(&tape, None, 50);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "Hello");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "Hi there!");
        assert_eq!(msgs[2].role, "user");
        assert_eq!(msgs[2].content, "How are you?");
    }

    #[test]
    fn skips_non_message_entries() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        tape.anchor("session/start", serde_json::json!({})).unwrap();
        tape.append_event("route", serde_json::json!({"kind": "model"}))
            .unwrap();
        tape.append_message("user", "Hello").unwrap();
        tape.append_event("command", serde_json::json!({"name": "help"}))
            .unwrap();
        tape.append_message("assistant", "Hi").unwrap();

        let msgs = build_messages(&tape, None, 50);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "Hello");
        assert_eq!(msgs[1].content, "Hi");
    }

    #[test]
    fn system_prompt_prepended_before_messages() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        tape.append_message("user", "Hello").unwrap();

        let msgs = build_messages(&tape, Some("Be concise."), 50);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content, "Be concise.");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[1].content, "Hello");
    }

    #[test]
    fn blank_system_prompt_is_ignored() {
        let dir = tempdir().unwrap();
        let tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        let msgs = build_messages(&tape, Some("   "), 50);
        assert!(msgs.is_empty());
    }

    #[test]
    fn skips_empty_content_messages() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        tape.append_message("user", "").unwrap();
        tape.append_message("user", "real").unwrap();

        let msgs = build_messages(&tape, None, 50);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "real");
    }

    #[test]
    fn anchor_truncates_context_window() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "ctx-trunc").unwrap();

        // Old context (before anchor)
        tape.append_message("user", "old question").unwrap();
        tape.append_message("assistant", "old answer").unwrap();

        // Create anchor
        tape.anchor("handoff", serde_json::json!({"owner": "human"}))
            .unwrap();

        // New context (after anchor)
        tape.append_message("user", "new question").unwrap();
        tape.append_message("assistant", "new answer").unwrap();

        let msgs = build_messages(&tape, None, 50);
        // Only new messages should be in context
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "new question");
        assert_eq!(msgs[1].content, "new answer");
    }

    #[test]
    fn system_prompt_config_override() {
        let dir = tempdir().unwrap();
        let result = build_system_prompt(Some("Custom prompt"), dir.path());
        assert_eq!(result, "Custom prompt");
    }

    #[test]
    fn system_prompt_workspace_file() {
        let dir = tempdir().unwrap();
        let agent_dir = dir.path().join(".agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("system-prompt.md"), "Workspace prompt").unwrap();

        let result = build_system_prompt(None, dir.path());
        assert_eq!(result, "Workspace prompt");
    }

    #[test]
    fn system_prompt_default_fallback() {
        let dir = tempdir().unwrap();
        let result = build_system_prompt(None, dir.path());
        assert!(result.contains("CrabClaw"));
        assert!(result.contains("shell.exec"));
        assert!(result.contains("file.read"));
    }

    #[test]
    fn system_prompt_config_overrides_workspace_file() {
        let dir = tempdir().unwrap();
        let agent_dir = dir.path().join(".agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("system-prompt.md"), "Workspace prompt").unwrap();

        // Config override should win even when workspace file exists
        let result = build_system_prompt(Some("From config"), dir.path());
        assert_eq!(result, "From config");
    }

    #[test]
    fn test_max_context_messages_truncation() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "ctx-test").unwrap();

        // Add 5 messages
        for i in 1..=5 {
            tape.append_message("user", &format!("Msg {}", i)).unwrap();
        }

        // Test with max_context_messages = 3
        let msgs = build_messages(&tape, Some("System Prompt"), 3);

        // Expected:
        // 1. "System Prompt"
        // 2. "Older messages in this session have been truncated..."
        // 3. "Msg 3"
        // 4. "Msg 4"
        // 5. "Msg 5"
        assert_eq!(msgs.len(), 5);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content, "System Prompt");

        assert_eq!(msgs[1].role, "system");
        assert!(
            msgs[1]
                .content
                .contains("truncated to fit the context window")
        );

        assert_eq!(msgs[2].content, "Msg 3");
        assert_eq!(msgs[3].content, "Msg 4");
        assert_eq!(msgs[4].content, "Msg 5");
    }
}
