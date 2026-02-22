use crate::llm::api_types::Message;
use crate::tape::store::TapeStore;

/// Build a list of messages from tape entries for multi-turn conversation.
///
/// Aligned with bub's `tape/context.py::_select_messages`:
/// - Extracts entries with kind "message"
/// - Preserves role and content from payload
/// - Optionally prepends a system prompt
pub fn build_messages(tape: &TapeStore, system_prompt: Option<&str>) -> Vec<Message> {
    let mut messages = Vec::new();

    if let Some(prompt) = system_prompt {
        if !prompt.trim().is_empty() {
            messages.push(Message::system(prompt));
        }
    }

    for entry in tape.entries() {
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

        messages.push(Message {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
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
        let msgs = build_messages(&tape, None);
        assert!(msgs.is_empty());
    }

    #[test]
    fn empty_tape_with_system_prompt() {
        let dir = tempdir().unwrap();
        let tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        let msgs = build_messages(&tape, Some("You are a helpful assistant."));
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

        let msgs = build_messages(&tape, None);
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

        let msgs = build_messages(&tape, None);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "Hello");
        assert_eq!(msgs[1].content, "Hi");
    }

    #[test]
    fn system_prompt_prepended_before_messages() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        tape.append_message("user", "Hello").unwrap();

        let msgs = build_messages(&tape, Some("Be concise."));
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
        let msgs = build_messages(&tape, Some("   "));
        assert!(msgs.is_empty());
    }

    #[test]
    fn skips_empty_content_messages() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "ctx-test").unwrap();
        tape.append_message("user", "").unwrap();
        tape.append_message("user", "real").unwrap();

        let msgs = build_messages(&tape, None);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "real");
    }
}
