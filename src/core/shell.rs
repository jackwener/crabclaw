use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Result of executing a shell command.
#[derive(Debug, Clone)]
pub struct ShellResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

/// Default timeout for shell commands (30 seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Execute an arbitrary shell command in the given workspace directory.
///
/// Uses `/bin/sh -c` to run the command. Captures stdout, stderr, and exit code.
/// Enforces a default timeout of 30 seconds.
pub fn execute_shell(cmd_line: &str, workspace: &Path) -> ShellResult {
    execute_shell_with_timeout(
        cmd_line,
        workspace,
        Duration::from_secs(DEFAULT_TIMEOUT_SECS),
    )
}

/// Execute a shell command with a custom timeout.
pub fn execute_shell_with_timeout(
    cmd_line: &str,
    workspace: &Path,
    timeout: Duration,
) -> ShellResult {
    let mut child = match Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd_line)
        .current_dir(workspace)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return ShellResult {
                stdout: String::new(),
                stderr: format!("failed to spawn shell: {e}"),
                exit_code: -1,
                timed_out: false,
            };
        }
    };

    // Wait with timeout using a simple polling approach.
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                // Process finished; collect output.
                let output = child.wait_with_output().unwrap_or_else(|e| {
                    // Shouldn't happen since child already exited but handle gracefully.
                    panic!("unexpected error collecting output: {e}");
                });
                return ShellResult {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: output.status.code().unwrap_or(-1),
                    timed_out: false,
                };
            }
            Ok(None) => {
                // Still running, check timeout.
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait(); // Reap the process.
                    return ShellResult {
                        stdout: String::new(),
                        stderr: format!("command timed out after {}s", timeout.as_secs()),
                        exit_code: -1,
                        timed_out: true,
                    };
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return ShellResult {
                    stdout: String::new(),
                    stderr: format!("error waiting for process: {e}"),
                    exit_code: -1,
                    timed_out: false,
                };
            }
        }
    }
}

/// Format a shell result into a combined output string for display.
pub fn format_shell_output(result: &ShellResult) -> String {
    let mut parts = Vec::new();
    if !result.stdout.is_empty() {
        parts.push(result.stdout.trim_end().to_string());
    }
    if !result.stderr.is_empty() {
        parts.push(format!("[stderr] {}", result.stderr.trim_end()));
    }
    if parts.is_empty() {
        "(no output)".to_string()
    } else {
        parts.join("\n")
    }
}

/// Wrap a failed command result into a structured XML context block for the LLM.
pub fn wrap_failure_context(cmd_line: &str, result: &ShellResult) -> String {
    let output = format_shell_output(result);
    format!(
        "<command cmd=\"{}\" exit_code=\"{}\">\n{}\n</command>",
        cmd_line, result.exit_code, output
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn successful_echo_command() {
        let dir = tempdir().unwrap();
        let result = execute_shell("echo hello", dir.path());
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello");
        assert!(result.stderr.is_empty());
        assert!(!result.timed_out);
    }

    #[test]
    fn captures_stderr() {
        let dir = tempdir().unwrap();
        let result = execute_shell("echo oops >&2", dir.path());
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
        assert_eq!(result.stderr.trim(), "oops");
    }

    #[test]
    fn non_zero_exit_code() {
        let dir = tempdir().unwrap();
        let result = execute_shell("exit 42", dir.path());
        assert_eq!(result.exit_code, 42);
        assert!(!result.timed_out);
    }

    #[test]
    fn mixed_stdout_and_stderr() {
        let dir = tempdir().unwrap();
        let result = execute_shell("echo out && echo err >&2", dir.path());
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "out");
        assert_eq!(result.stderr.trim(), "err");
    }

    #[test]
    fn timeout_kills_process() {
        let dir = tempdir().unwrap();
        let result = execute_shell_with_timeout("sleep 60", dir.path(), Duration::from_millis(200));
        assert!(result.timed_out);
        assert_eq!(result.exit_code, -1);
        assert!(result.stderr.contains("timed out"));
    }

    #[test]
    fn runs_in_workspace_directory() {
        let dir = tempdir().unwrap();
        let result = execute_shell("pwd", dir.path());
        assert_eq!(result.exit_code, 0);
        // The output should contain the temp directory path.
        let canonical = dir.path().canonicalize().unwrap();
        let output_path = std::path::PathBuf::from(result.stdout.trim())
            .canonicalize()
            .unwrap();
        assert_eq!(output_path, canonical);
    }

    #[test]
    fn format_output_stdout_only() {
        let result = ShellResult {
            stdout: "hello\n".to_string(),
            stderr: String::new(),
            exit_code: 0,
            timed_out: false,
        };
        assert_eq!(format_shell_output(&result), "hello");
    }

    #[test]
    fn format_output_stderr_only() {
        let result = ShellResult {
            stdout: String::new(),
            stderr: "error msg\n".to_string(),
            exit_code: 1,
            timed_out: false,
        };
        assert_eq!(format_shell_output(&result), "[stderr] error msg");
    }

    #[test]
    fn format_output_both() {
        let result = ShellResult {
            stdout: "out\n".to_string(),
            stderr: "err\n".to_string(),
            exit_code: 1,
            timed_out: false,
        };
        assert_eq!(format_shell_output(&result), "out\n[stderr] err");
    }

    #[test]
    fn format_output_empty() {
        let result = ShellResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            timed_out: false,
        };
        assert_eq!(format_shell_output(&result), "(no output)");
    }

    #[test]
    fn wrap_failure_context_format() {
        let result = ShellResult {
            stdout: String::new(),
            stderr: "file not found\n".to_string(),
            exit_code: 1,
            timed_out: false,
        };
        let ctx = wrap_failure_context("cat missing.txt", &result);
        assert!(ctx.contains("<command cmd=\"cat missing.txt\" exit_code=\"1\">"));
        assert!(ctx.contains("file not found"));
        assert!(ctx.contains("</command>"));
    }
}
