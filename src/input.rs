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
