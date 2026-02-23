# S0: Project Skeleton + Config Baseline

## Background

Bootstrap the CrabClaw project as a Rust `library+CLI` architecture with deterministic configuration precedence, aligned with ADR 0001.

## Architecture

```
src/
├── core/
│   ├── config.rs    # 3-tier config: .env.local → env vars → CLI flags
│   ├── error.rs     # thiserror structured error categories
│   └── input.rs     # --prompt / --prompt-file / stdin normalization
├── channels/
│   └── cli.rs       # clap CLI: run subcommand
└── main.rs          # tracing-subscriber structured logging
```

## Implementation

| File | What it does |
|------|-------------|
| `config.rs` | Parses `OPENROUTER_API_KEY`, `API_BASE`, `MODEL`, `SYSTEM_PROMPT` with `.env.local` → env → CLI precedence |
| `error.rs` | `CrabClawError` enum: `Config`, `Network`, `Auth`, `Api`, `RateLimit` |
| `input.rs` | Unifies `--prompt "..."`, `--prompt-file path`, and stdin pipe into a single `String` |
| `cli.rs` | `clap` derive-based CLI with `run` subcommand |
| `main.rs` | `tracing_subscriber::fmt` with `RUST_LOG` env filter |

## Verification

- Config precedence: unit tests for all priority combinations
- Input modes: integration tests for flag, file, and stdin
- Error categorization: unit tests for each error variant
