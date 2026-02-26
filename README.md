# CrabClaw

[![CI](https://github.com/jackwener/crabclaw/actions/workflows/ci.yml/badge.svg)](https://github.com/jackwener/crabclaw/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

CrabClaw is an OpenClaw-compatible agentic coding toolchain written in Rust.

## Features

- **Multi-channel**: CLI, interactive REPL, and Telegram bot with whitelist access control
- **Model agnostic**: OpenAI-compatible (Chat Completions), native Anthropic (Messages API), and Codex (Responses API via OAuth)
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
2. Authenticate (choose one):
   ```bash
   # Option A: API Key
   cp .env.example .env.local
   # Edit .env.local — set API_KEY, BASE_URL, MODEL (e.g. MODEL=openai:gpt-4o)

   # Option B: OAuth (use your ChatGPT Plus/Pro subscription)
   cargo run -- auth login
   ```
3. Build and verify:
   ```bash
   cargo build && cargo test
   ```
4. Choose your mode:
   ```bash
   cargo run -- interactive          # Interactive REPL
   cargo run -- run --prompt "..."   # One-shot CLI
   cargo run -- serve                # Telegram bot (requires TELEGRAM_BOT_TOKEN)
   cargo run -- auth status          # Check auth status
   ```

## LLM Configuration

CrabClaw supports three provider modes. All models **must** have a provider prefix:

### Provider Modes

| Prefix | Provider | API Format | Auth | Example |
|--------|----------|-----------|------|---------------|
| `openai:` | OpenAI-compatible | Chat Completions | `API_KEY` | `openai:gpt-4o` |
| `anthropic:` | Anthropic | Messages API | `API_KEY` | `anthropic:claude-sonnet-4-20250514` |
| `codex:` | OpenAI Codex | Responses API | OAuth | `codex:gpt-5.3-codex` |

### Option A: API Key (OpenAI-compatible / Anthropic)

Works with OpenAI, OpenRouter, GLM, DeepSeek, or any OpenAI-compatible endpoint.

```bash
# .env.local
API_KEY=sk-xxx
BASE_URL=https://api.openai.com/v1      # or https://openrouter.ai/api/v1
MODEL=openai:gpt-4o                     # or anthropic:claude-sonnet-4-20250514
```

### Option B: OAuth + Codex (ChatGPT Plus/Pro subscription)

Uses your ChatGPT subscription quota — **no API credits needed**.

```bash
# Step 1: Login via browser
cargo run -- auth login

# Step 2: Configure model
# .env.local
MODEL=codex:gpt-5.3-codex
# No API_KEY or BASE_URL needed — Codex uses chatgpt.com backend
```

Available Codex models: `gpt-5.3-codex`, `gpt-5-codex`, `gpt-5.1-codex-mini`

### Auth Management

```bash
cargo run -- auth login    # Open browser for ChatGPT OAuth login
cargo run -- auth status   # Check token expiry and refresh status
cargo run -- auth logout   # Remove stored tokens
```

Tokens are stored in `~/.crabclaw/auth.json` with automatic refresh.

### Configuration Precedence

Settings resolve in this order (first wins):

1. CLI flags (`--api-key`, `--api-base`, `--model`)
2. Profile-specific env vars (`PROFILE_<NAME>_API_KEY`)
3. Environment variables (`API_KEY`, `BASE_URL`, `MODEL`)
4. `.env.local` file
5. OAuth tokens (fallback when no `API_KEY` is set)
6. Built-in defaults (`MODEL=openai:gpt-4o`)

### Reasoning Effort (Codex models)

```bash
CODEX_REASONING_EFFORT=high   # low | medium | high (default: high)
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

### Setup

```bash
# Enable pre-commit hook (runs cargo fmt + clippy before each commit)
git config core.hooksPath .githooks
```

### Commands

```bash
cargo test               # Run all tests (unit + integration + live if configured)
cargo clippy             # Lint check
cargo fmt                # Format
./scripts/smoke-test.sh  # Full verification (build + clippy + tests + live API)
```

### Test Suites

| Suite | Command | Description |
|-------|---------|-------------|
| Unit tests | `cargo test --lib` | All unit tests |
| CLI | `cargo test --test cli_run` | CLI flag parsing, dry-run |
| AgentLoop | `cargo test --test agent_loop_*` | Routing, tool calling |
| Telegram | `cargo test --test telegram_*` | Channel routing, providers |
| OpenAI-compatible | `cargo test --test openai_provider_integration` | Reply, tool call, error, rate limit |
| Live E2E | `cargo test --test live_integration` | Requires `API_KEY` in `.env.local` |

## Documentation

- [Architecture (EN)](docs/architecture.md) | [中文](docs/architecture.zh-CN.md)
- Feature test plan: `docs/test-plans/phase-1-mvp.md`
- Architecture decisions: `docs/adr/`

## Acknowledgements

Inspired by [bub](https://github.com/PsiACE/bub).
