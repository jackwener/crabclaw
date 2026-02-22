# CrabClaw

[![CI](https://github.com/jackwener/crabclaw/actions/workflows/ci.yml/badge.svg)](https://github.com/jackwener/crabclaw/actions/workflows/ci.yml)

CrabClaw is a Rust implementation of [bub](https://github.com/PsiACE/bub), providing an OpenClaw-compatible agentic coding toolchain.

## Features

- **Multi-channel**: CLI, interactive REPL, and Telegram bot with whitelist access control
- **Model agnostic**: OpenRouter (OpenAI format) and native Anthropic adapters
- **Skill engine**: Auto-discovers `.agent/skills/` and bridges them as LLM-callable tools
- **Shell execution**: Run shell commands via `,git status` or `shell.exec` tool, with failure self-correction
- **File operations**: `file.read`, `file.write`, `file.list` with workspace-sandboxed security
- **Tool calling loop**: Up to 5-iteration autonomous reasoning in REPL and Telegram
- **Tape system**: Append-only JSONL session recording with anchors, search, handoff, and context truncation
- **Profile resolution**: `.env.local`, environment variables, CLI flags with deterministic precedence

## Quick Start

1. Install stable Rust toolchain.
2. Copy `.env.example` to `.env.local` and set `OPENROUTER_API_KEY`.
3. `cargo build && cargo test`
4. `cargo run -- repl` for interactive mode.

## Development

```bash
cargo test          # Run all 178 tests
cargo clippy        # Lint check
cargo fmt           # Format
./scripts/smoke-test.sh  # Full verification (build + clippy + tests + live API)
```

## Documentation

- [Architecture (EN)](docs/architecture.md) | [中文](docs/architecture.zh-CN.md)
- Feature test plan: `docs/test-plans/phase-1-mvp.md`
- Bub alignment backlog: `docs/plans/bub-alignment-backlog.md`
- Architecture decisions: `docs/adr/`

