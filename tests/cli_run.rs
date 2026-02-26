use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn base_command() -> Command {
    #[allow(deprecated)]
    let mut cmd = Command::cargo_bin("crabclaw").expect("binary exists");
    cmd.env_remove("API_KEY");
    cmd.env_remove("BASE_URL");
    cmd.env_remove("MODEL");
    cmd.env_remove("PROFILE_DEV_API_KEY");
    cmd.env_remove("PROFILE_DEV_BASE_URL");
    cmd.env_remove("PROFILE_DEV_MODEL");
    cmd
}

#[test]
fn run_accepts_prompt_flag() {
    let mut cmd = base_command();
    cmd.env("API_KEY", "test-key")
        .args(["run", "--prompt", "hello from flag", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"prompt\": \"hello from flag\""))
        .stdout(predicate::str::contains("\"api_key_present\": true"));
}

#[test]
fn run_accepts_prompt_file() {
    let tmp = tempdir().expect("tempdir");
    let prompt_path = tmp.path().join("prompt.txt");
    fs::write(&prompt_path, "hello from file").expect("write prompt");

    let mut cmd = base_command();
    cmd.env("API_KEY", "test-key")
        .args([
            "run",
            "--prompt-file",
            prompt_path.to_str().expect("utf-8 path"),
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"prompt\": \"hello from file\""));
}

#[test]
fn run_accepts_prompt_from_stdin() {
    let mut cmd = base_command();
    cmd.env("API_KEY", "test-key")
        .args(["run", "--dry-run"])
        .write_stdin("hello from stdin")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"prompt\": \"hello from stdin\""));
}

#[test]
fn run_fails_when_api_key_is_missing() {
    // If OAuth tokens are stored locally (~/.crabclaw/auth.json), the config
    // resolves via the OAuth fallback and this test would pass (correct behaviour).
    // Only assert the failure when no OAuth tokens exist (i.e. CI).
    let auth_path = dirs::home_dir()
        .map(|h| h.join(".crabclaw/auth.json"))
        .filter(|p| p.exists());
    if auth_path.is_some() {
        return; // OAuth fallback is valid — skip
    }

    let tmp = tempdir().expect("tempdir");
    let mut cmd = base_command();
    cmd.current_dir(tmp.path())
        .args(["run", "--prompt", "hello", "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("config error: missing API_KEY"));
}

#[test]
fn run_honors_profile_and_precedence() {
    let tmp = tempdir().expect("tempdir");
    fs::write(
        tmp.path().join(".env.local"),
        "API_KEY=dotenv-key\nBASE_URL=https://dotenv-base.example.com\n",
    )
    .expect("write .env.local");

    let mut cmd = base_command();
    cmd.current_dir(tmp.path())
        .env("API_KEY", "env-key")
        .env("PROFILE_DEV_BASE_URL", "https://env-profile.example.com")
        .args([
            "run",
            "--profile",
            "dev",
            "--model",
            "cli-model",
            "--prompt",
            "hello",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"profile\": \"dev\""))
        .stdout(predicate::str::contains(
            "\"api_base\": \"https://env-profile.example.com\"",
        ))
        .stdout(predicate::str::contains("\"model\": \"cli-model\""));
}

#[test]
fn run_comma_command_via_prompt() {
    let mut cmd = base_command();
    cmd.env("API_KEY", "test-key")
        .args(["run", "--prompt", ",help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Available commands"));
}

#[test]
fn run_rejects_both_prompt_sources() {
    let tmp = tempdir().expect("tempdir");
    let prompt_path = tmp.path().join("prompt.txt");
    fs::write(&prompt_path, "hello").expect("write");

    let mut cmd = base_command();
    cmd.env("API_KEY", "test-key")
        .args([
            "run",
            "--prompt",
            "hello",
            "--prompt-file",
            prompt_path.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not both"));
}

#[test]
fn run_rejects_empty_prompt() {
    let mut cmd = base_command();
    cmd.env("API_KEY", "test-key")
        .args(["run", "--prompt", "   "])
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty"));
}
