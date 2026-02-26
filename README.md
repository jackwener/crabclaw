# CrabClaw

[![CI](https://github.com/jackwener/crabclaw/actions/workflows/ci.yml/badge.svg)](https://github.com/jackwener/crabclaw/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

CrabClaw is an OpenClaw-compatible agentic coding toolchain written in Rust.

## Features

- **Multi-channel**: CLI, interactive REPL, and Telegram bot with whitelist access control
- **Model agnostic**: OpenRouter (OpenAI format) and native Anthropic adapters
- **AgentLoop**: Unified abstraction: route → model → tool → tape in a single `handle_input` call
- **Skill engine**: Auto-discovers `.agent/skills/` and bridges them as LLM-callable tools
- **Shell execution**: Run shell commands via `,git status` or `shell.exec` tool, with failure self-correction
- **File operations**: `file.read`, `file.write`, `file.edit`, `file.list`, `file.search` with workspace-sandboxed security
- **Assistant routing**: Model output is scanned for comma-commands and auto-executed (`route_assistant`)
- **Tool calling loop**: Up to 5-iteration autonomous reasoning in REPL and Telegram
- **Progressive tool view**: Token-efficient tool hinting — full schemas expand on demand
- **Tape system**: Append-only JSONL session recording with anchors, search, handoff, and context truncation
- **System prompt**: 3-tier priority — config override > `.agent/system-prompt.md` > built-in default
- **Profile resolution**: `.env.local`, environment variables, CLI flags with deterministic precedence

## Quick Start

1. Install stable Rust toolchain.
2. Copy `.env.example` to `.env.local` and configure:
   ```bash
   cp .env.example .env.local
   # Edit .env.local — set OPENROUTER_API_KEY (or ANTHROPIC_API_KEY)
   ```
3. Build and verify:
   ```bash
   cargo build && cargo test
   ```
4. Choose your mode:
   ```bash
   cargo run -- repl                # Interactive REPL
   cargo run -- run --prompt "..."  # One-shot CLI
   cargo run -- serve               # Telegram bot (requires TELEGRAM_BOT_TOKEN)
   ```

## Usage

In REPL or Telegram, prefix commands with `,`:

```
,help                    Show all commands
,tools                   List registered tools
,tool.describe file.read Show tool parameters
,git status              Execute shell command
,tape.search <query>     Search conversation history
,handoff                 Reset context window
```

Natural language input goes to the LLM, which can autonomously call tools:

```
> Read the Cargo.toml and tell me the project version
  [tool] file.read → 1432 chars

The project version is 0.1.0...
```

## Development

```bash
cargo test               # Run all tests (unit + integration + live if configured)
cargo clippy             # Lint check
cargo fmt                # Format
./scripts/smoke-test.sh  # Full verification (build + clippy + tests + live API)
```

## Documentation

- [Architecture (EN)](docs/architecture.md) | [中文](docs/architecture.zh-CN.md)
- Feature test plan: `docs/test-plans/phase-1-mvp.md`
- Architecture decisions: `docs/adr/`

## Acknowledgements

Inspired by [bub](https://github.com/PsiACE/bub).
