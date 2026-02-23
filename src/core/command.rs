use std::fmt;

/// A detected command parsed from user input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedCommand {
    pub kind: CommandKind,
    pub name: String,
    pub args: ParsedArgs,
    pub raw: String,
}

/// Whether a command is internal or shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandKind {
    Internal,
    Shell,
}

impl fmt::Display for CommandKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandKind::Internal => write!(f, "internal"),
            CommandKind::Shell => write!(f, "shell"),
        }
    }
}

/// Parsed command arguments with positional and key-value pairs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedArgs {
    pub positional: Vec<String>,
    pub kwargs: Vec<(String, String)>,
    pub flags: Vec<String>,
}

impl ParsedArgs {
    /// Get a keyword argument value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.kwargs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Check if a flag is set.
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

const INTERNAL_PREFIX: char = ',';

/// Known internal command names.
const KNOWN_INTERNAL_COMMANDS: &[&str] = &[
    "help",
    "quit",
    "tape",
    "tape.info",
    "tape.reset",
    "tape.search",
    "tools",
    "tool.describe",
    "skills",
    "skills.describe",
    "anchors",
    "handoff",
];

/// Detect whether a line of input is a command.
///
/// Rules (aligned with bub):
/// - Lines starting with `,` are commands.
/// - If the first token matches a known internal name → `CommandKind::Internal`.
/// - Otherwise → `CommandKind::Shell` (arbitrary shell execution).
/// - All other input is routed to the model.
pub fn detect_command(input: &str) -> Option<DetectedCommand> {
    let stripped = input.trim();
    if stripped.is_empty() {
        return None;
    }

    if !stripped.starts_with(INTERNAL_PREFIX) {
        return None;
    }

    let body = stripped[1..].trim_start();
    if body.is_empty() {
        return None;
    }

    let tokens = shell_split(body);
    if tokens.is_empty() {
        return None;
    }

    let name = tokens[0].clone();
    let is_internal = KNOWN_INTERNAL_COMMANDS.iter().any(|&cmd| cmd == name);

    if is_internal {
        let args = parse_kv_arguments(&tokens[1..]);
        Some(DetectedCommand {
            kind: CommandKind::Internal,
            name,
            args,
            raw: stripped.to_string(),
        })
    } else {
        // Shell command: store the full command line (everything after the comma)
        Some(DetectedCommand {
            kind: CommandKind::Shell,
            name,
            args: ParsedArgs::default(),
            raw: body.to_string(),
        })
    }
}

/// Parse tool-style arguments from tokens.
///
/// Supports:
/// - `--flag` (boolean flag)
/// - `--key value` or `--key=value`
/// - `key=value`
/// - positional arguments
pub fn parse_kv_arguments(tokens: &[String]) -> ParsedArgs {
    let mut args = ParsedArgs::default();
    let mut idx = 0;

    while idx < tokens.len() {
        let token = &tokens[idx];

        if let Some(rest) = token.strip_prefix("--") {
            if let Some((key, value)) = rest.split_once('=') {
                args.kwargs.push((key.to_string(), value.to_string()));
                idx += 1;
                continue;
            }

            if idx + 1 < tokens.len() && !tokens[idx + 1].starts_with("--") {
                args.kwargs
                    .push((rest.to_string(), tokens[idx + 1].clone()));
                idx += 2;
                continue;
            }

            args.flags.push(rest.to_string());
            idx += 1;
            continue;
        }

        #[allow(clippy::collapsible_if)]
        if let Some((key, value)) = token.split_once('=') {
            if !key.is_empty() {
                args.kwargs.push((key.to_string(), value.to_string()));
                idx += 1;
                continue;
            }
        }

        args.positional.push(token.clone());
        idx += 1;
    }

    args
}

/// Simple shell-like tokenizer that handles basic quoting.
fn shell_split(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;

    for ch in input.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }

        if ch == '\\' && !in_single_quote {
            escape_next = true;
            continue;
        }

        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            continue;
        }

        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            continue;
        }

        if ch.is_whitespace() && !in_single_quote && !in_double_quote {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }

        current.push(ch);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_none() {
        assert!(detect_command("").is_none());
        assert!(detect_command("   ").is_none());
    }

    #[test]
    fn non_comma_input_returns_none() {
        assert!(detect_command("hello world").is_none());
        assert!(detect_command("what is rust?").is_none());
    }

    #[test]
    fn bare_comma_returns_none() {
        assert!(detect_command(",").is_none());
        assert!(detect_command(",  ").is_none());
    }

    #[test]
    fn help_command() {
        let cmd = detect_command(",help").unwrap();
        assert_eq!(cmd.kind, CommandKind::Internal);
        assert_eq!(cmd.name, "help");
        assert!(cmd.args.positional.is_empty());
    }

    #[test]
    fn help_with_leading_space() {
        let cmd = detect_command(", help").unwrap();
        assert_eq!(cmd.name, "help");
    }

    #[test]
    fn tape_info_command() {
        let cmd = detect_command(",tape.info").unwrap();
        assert_eq!(cmd.kind, CommandKind::Internal);
        assert_eq!(cmd.name, "tape.info");
    }

    #[test]
    fn quit_command() {
        let cmd = detect_command(",quit").unwrap();
        assert_eq!(cmd.name, "quit");
    }

    #[test]
    fn command_with_kv_args() {
        let cmd = detect_command(",handoff name=phase-1 summary=\"bootstrap done\"").unwrap();
        assert_eq!(cmd.name, "handoff");
        assert_eq!(cmd.args.get("name"), Some("phase-1"));
        assert_eq!(cmd.args.get("summary"), Some("bootstrap done"));
    }

    #[test]
    fn command_with_flag_args() {
        let cmd = detect_command(",tape.reset --archive").unwrap();
        assert_eq!(cmd.name, "tape.reset");
        assert!(cmd.args.has_flag("archive"));
    }

    #[test]
    fn command_with_double_dash_kv() {
        let cmd = detect_command(",tool.describe --name fs.read").unwrap();
        assert_eq!(cmd.name, "tool.describe");
        assert_eq!(cmd.args.get("name"), Some("fs.read"));
    }

    #[test]
    fn command_with_positional_args() {
        let cmd = detect_command(",skills.describe friendly-python").unwrap();
        assert_eq!(cmd.name, "skills.describe");
        assert_eq!(cmd.args.positional, vec!["friendly-python"]);
    }

    #[test]
    fn shell_split_handles_quotes() {
        let tokens = shell_split(r#"hello "world foo" bar"#);
        assert_eq!(tokens, vec!["hello", "world foo", "bar"]);
    }

    #[test]
    fn shell_split_handles_single_quotes() {
        let tokens = shell_split("hello 'world foo' bar");
        assert_eq!(tokens, vec!["hello", "world foo", "bar"]);
    }

    #[test]
    fn shell_split_handles_escapes() {
        let tokens = shell_split(r"hello\ world bar");
        assert_eq!(tokens, vec!["hello world", "bar"]);
    }

    #[test]
    fn git_status_detected_as_shell() {
        let cmd = detect_command(",git status").unwrap();
        assert_eq!(cmd.kind, CommandKind::Shell);
        assert_eq!(cmd.name, "git");
        assert_eq!(cmd.raw, "git status");
    }

    #[test]
    fn ls_detected_as_shell() {
        let cmd = detect_command(",ls -la").unwrap();
        assert_eq!(cmd.kind, CommandKind::Shell);
        assert_eq!(cmd.name, "ls");
        assert_eq!(cmd.raw, "ls -la");
    }

    #[test]
    fn help_detected_as_internal() {
        let cmd = detect_command(",help").unwrap();
        assert_eq!(cmd.kind, CommandKind::Internal);
    }

    #[test]
    fn tape_info_detected_as_internal() {
        let cmd = detect_command(",tape.info").unwrap();
        assert_eq!(cmd.kind, CommandKind::Internal);
    }
}
