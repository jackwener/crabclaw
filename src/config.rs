use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::{CrabClawError, Result};

const DEFAULT_API_BASE: &str = "https://api.example.com";
const DEFAULT_MODEL: &str = "openclaw/default";
const API_KEY_KEY: &str = "OPENCLAW_API_KEY";
const API_BASE_KEY: &str = "OPENCLAW_BASE_URL";
const MODEL_KEY: &str = "CRABCLAW_MODEL";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppConfig {
    pub profile: String,
    pub api_key: String,
    pub api_base: String,
    pub model: String,
}

#[derive(Debug, Clone, Default)]
pub struct CliConfigOverrides {
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub model: Option<String>,
}

pub fn load_runtime_config(
    workspace: &Path,
    profile: Option<&str>,
    cli_overrides: &CliConfigOverrides,
) -> Result<AppConfig> {
    let env_vars: HashMap<String, String> = std::env::vars().collect();
    let dotenv_vars = load_dotenv_map(&workspace.join(".env.local"))?;
    resolve_config(profile, cli_overrides, &env_vars, &dotenv_vars)
}

pub fn resolve_config(
    profile: Option<&str>,
    cli_overrides: &CliConfigOverrides,
    env_vars: &HashMap<String, String>,
    dotenv_vars: &HashMap<String, String>,
) -> Result<AppConfig> {
    let profile_name = profile.unwrap_or("default").trim().to_string();
    let profile_token = normalize_profile_token(&profile_name);
    let profiled_api_key = format!("CRABCLAW_PROFILE_{profile_token}_{API_KEY_KEY}");
    let profiled_api_base = format!("CRABCLAW_PROFILE_{profile_token}_{API_BASE_KEY}");
    let profiled_model = format!("CRABCLAW_PROFILE_{profile_token}_{MODEL_KEY}");

    let api_key = first_present([
        cli_overrides.api_key.as_ref(),
        env_vars.get(&profiled_api_key),
        env_vars.get(API_KEY_KEY),
        dotenv_vars.get(&profiled_api_key),
        dotenv_vars.get(API_KEY_KEY),
    ])
    .ok_or_else(|| CrabClawError::Config("missing OPENCLAW_API_KEY".to_string()))?;

    let api_base = first_present([
        cli_overrides.api_base.as_ref(),
        env_vars.get(&profiled_api_base),
        env_vars.get(API_BASE_KEY),
        dotenv_vars.get(&profiled_api_base),
        dotenv_vars.get(API_BASE_KEY),
        Some(&DEFAULT_API_BASE.to_string()),
    ])
    .unwrap_or_else(|| DEFAULT_API_BASE.to_string());

    let model = first_present([
        cli_overrides.model.as_ref(),
        env_vars.get(&profiled_model),
        env_vars.get(MODEL_KEY),
        dotenv_vars.get(&profiled_model),
        dotenv_vars.get(MODEL_KEY),
        Some(&DEFAULT_MODEL.to_string()),
    ])
    .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    Ok(AppConfig {
        profile: profile_name,
        api_key,
        api_base,
        model,
    })
}

fn first_present<const N: usize>(values: [Option<&String>; N]) -> Option<String> {
    values.into_iter().flatten().find_map(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.to_string())
    })
}

fn normalize_profile_token(profile: &str) -> String {
    let mut out = String::with_capacity(profile.len());
    for ch in profile.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "DEFAULT".to_string()
    } else {
        out
    }
}

fn load_dotenv_map(path: &Path) -> Result<HashMap<String, String>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let content = fs::read_to_string(path)?;
    Ok(parse_dotenv(&content))
}

fn parse_dotenv(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let body = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let Some((key, value)) = body.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = strip_quotes(value.trim()).to_string();
        map.insert(key.to_string(), value);
    }
    map
}

fn strip_quotes(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return &value[1..value.len() - 1];
    }
    if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        return &value[1..value.len() - 1];
    }
    value
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::config::{CliConfigOverrides, resolve_config};
    use crate::error::CrabClawError;

    #[test]
    fn resolves_config_with_deterministic_precedence() {
        let mut env_vars = HashMap::new();
        env_vars.insert("OPENCLAW_API_KEY".to_string(), "env-base-key".to_string());
        env_vars.insert(
            "CRABCLAW_PROFILE_DEV_OPENCLAW_BASE_URL".to_string(),
            "https://env-profile.example.com".to_string(),
        );
        env_vars.insert("CRABCLAW_MODEL".to_string(), "env-base-model".to_string());

        let mut dotenv_vars = HashMap::new();
        dotenv_vars.insert(
            "OPENCLAW_API_KEY".to_string(),
            "dotenv-base-key".to_string(),
        );
        dotenv_vars.insert(
            "CRABCLAW_PROFILE_DEV_OPENCLAW_API_KEY".to_string(),
            "dotenv-profile-key".to_string(),
        );
        dotenv_vars.insert(
            "OPENCLAW_BASE_URL".to_string(),
            "https://dotenv-base.example.com".to_string(),
        );
        dotenv_vars.insert(
            "CRABCLAW_PROFILE_DEV_CRABCLAW_MODEL".to_string(),
            "dotenv-profile-model".to_string(),
        );

        let overrides = CliConfigOverrides {
            api_key: Some("cli-key".to_string()),
            api_base: None,
            model: Some("cli-model".to_string()),
        };

        let config =
            resolve_config(Some("dev"), &overrides, &env_vars, &dotenv_vars).expect("must resolve");

        assert_eq!(config.profile, "dev");
        assert_eq!(config.api_key, "cli-key");
        assert_eq!(config.api_base, "https://env-profile.example.com");
        assert_eq!(config.model, "cli-model");
    }

    #[test]
    fn errors_when_api_key_missing() {
        let env_vars = HashMap::new();
        let dotenv_vars = HashMap::new();
        let overrides = CliConfigOverrides::default();

        let err = resolve_config(None, &overrides, &env_vars, &dotenv_vars).expect_err("must fail");
        match err {
            CrabClawError::Config(msg) => assert!(msg.contains("OPENCLAW_API_KEY")),
            other => panic!("unexpected error: {other}"),
        }
    }
}
