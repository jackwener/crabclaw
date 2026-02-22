use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

use crate::error::{CrabClawError, Result};

pub fn resolve_prompt(prompt: Option<String>, prompt_file: Option<PathBuf>) -> Result<String> {
    if prompt.is_some() && prompt_file.is_some() {
        return Err(CrabClawError::Config(
            "use either --prompt or --prompt-file, not both".to_string(),
        ));
    }

    if let Some(prompt) = prompt {
        if prompt.trim().is_empty() {
            return Err(CrabClawError::Config("prompt cannot be empty".to_string()));
        }
        return Ok(prompt);
    }

    if let Some(file) = prompt_file {
        let content = fs::read_to_string(&file)?;
        if content.trim().is_empty() {
            return Err(CrabClawError::Config(format!(
                "prompt file is empty: {}",
                file.display()
            )));
        }
        return Ok(content);
    }

    if io::stdin().is_terminal() {
        return Err(CrabClawError::Config(
            "no prompt provided; use --prompt, --prompt-file, or stdin".to_string(),
        ));
    }

    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    if buffer.trim().is_empty() {
        return Err(CrabClawError::Config("stdin prompt is empty".to_string()));
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prompt_direct_string() {
        let result = resolve_prompt(Some("hello".to_string()), None);
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn prompt_empty_string_errors() {
        let err = resolve_prompt(Some("  ".to_string()), None).unwrap_err();
        match err {
            CrabClawError::Config(msg) => assert!(msg.contains("empty")),
            other => panic!("expected Config error, got: {other}"),
        }
    }

    #[test]
    fn prompt_both_flags_errors() {
        let err =
            resolve_prompt(Some("hello".to_string()), Some(PathBuf::from("file.txt"))).unwrap_err();
        match err {
            CrabClawError::Config(msg) => assert!(msg.contains("not both")),
            other => panic!("expected Config error, got: {other}"),
        }
    }

    #[test]
    fn prompt_from_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prompt.txt");
        std::fs::write(&path, "file content").unwrap();
        let result = resolve_prompt(None, Some(path));
        assert_eq!(result.unwrap(), "file content");
    }

    #[test]
    fn prompt_empty_file_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, "   ").unwrap();
        let err = resolve_prompt(None, Some(path)).unwrap_err();
        match err {
            CrabClawError::Config(msg) => assert!(msg.contains("empty")),
            other => panic!("expected Config error, got: {other}"),
        }
    }

    #[test]
    fn prompt_missing_file_errors() {
        let err = resolve_prompt(None, Some(PathBuf::from("/nonexistent/file.txt"))).unwrap_err();
        match err {
            CrabClawError::Io(_) => {} // expected
            other => panic!("expected Io error, got: {other}"),
        }
    }
}
