use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn base_command() -> Command {
    let mut cmd = Command::cargo_bin("crabclaw").expect("binary exists");
    cmd.env_remove("OPENCLAW_API_KEY");
    cmd.env_remove("OPENCLAW_BASE_URL");
    cmd.env_remove("CRABCLAW_MODEL");
    cmd.env_remove("CRABCLAW_PROFILE_DEV_OPENCLAW_API_KEY");
    cmd.env_remove("CRABCLAW_PROFILE_DEV_OPENCLAW_BASE_URL");
    cmd.env_remove("CRABCLAW_PROFILE_DEV_CRABCLAW_MODEL");
    cmd
}

#[test]
fn run_accepts_prompt_flag() {
    let mut cmd = base_command();
    cmd.env("OPENCLAW_API_KEY", "test-key")
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
    cmd.env("OPENCLAW_API_KEY", "test-key")
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
    cmd.env("OPENCLAW_API_KEY", "test-key")
        .args(["run", "--dry-run"])
        .write_stdin("hello from stdin")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"prompt\": \"hello from stdin\""));
}

#[test]
fn run_fails_when_api_key_is_missing() {
    let mut cmd = base_command();
    cmd.args(["run", "--prompt", "hello", "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("config error: missing OPENCLAW_API_KEY"));
}

#[test]
fn run_honors_profile_and_precedence() {
    let tmp = tempdir().expect("tempdir");
    fs::write(
        tmp.path().join(".env.local"),
        "OPENCLAW_API_KEY=dotenv-key\nOPENCLAW_BASE_URL=https://dotenv-base.example.com\n",
    )
    .expect("write .env.local");

    let mut cmd = base_command();
    cmd.current_dir(tmp.path())
        .env("OPENCLAW_API_KEY", "env-key")
        .env(
            "CRABCLAW_PROFILE_DEV_OPENCLAW_BASE_URL",
            "https://env-profile.example.com",
        )
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
        .stdout(predicate::str::contains("\"api_base\": \"https://env-profile.example.com\""))
        .stdout(predicate::str::contains("\"model\": \"cli-model\""));
}
