# S7: Shell Command Execution

## Background

Enable shell command execution both as comma-prefixed shortcuts (`,git status`) and as an LLM-callable tool (`shell.exec`), with failure self-correction.

## Architecture

```
src/core/
└── shell.rs     # /bin/sh -c executor with timeout + output capture
```

## Implementation

| File | What it does |
|------|-------------|
| `shell.rs` | `execute()` — runs command via `/bin/sh -c` with configurable working directory. Captures stdout, stderr, exit code. 30-second timeout via `tokio::time::timeout`. `format_output()` — structured output display |
| `router.rs` | Unknown `,` commands → `shell::execute()`. On failure: wraps stderr + exit code in `<command>` XML context and sends to model for self-correction |
| `registry.rs` | `shell.exec` tool — LLM can autonomously invoke shell commands with `{command: "..."}` parameter |

## Key Design Decisions

- **Timeout protection**: 30-second hard timeout prevents runaway processes (e.g., `yes` or infinite loops)
- **Failure-as-context**: Failed commands aren't errors — they're learning opportunities for the LLM
- **Working directory**: Commands run in the workspace directory, not the CrabClaw binary directory

## Verification

- Successful execution: echo, pwd, multi-command
- Stderr capture: commands that write to stderr
- Timeout: long-running commands killed after 30s
- Failure wrapping: non-zero exit → XML context format
