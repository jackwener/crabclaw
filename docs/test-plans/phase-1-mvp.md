# CrabClaw Test Plan

## Metadata

- Scope: Full test coverage for CrabClaw agentic coding toolchain
- Date: 2026-02-23
- Status: Active
- Related docs: `docs/architecture.md`, `README.md`

## Test Strategy

- **Unit tests** (`cargo test --lib`): Core logic, data mapping, and pure functions.
- **CLI integration tests** (`tests/cli_run.rs`): End-to-end CLI behavior with real binary.
- **Telegram integration tests** (`tests/telegram_integration.rs`): Full pipeline via `process_message` with mock LLM API (`mockito`).
- **CI**: `cargo fmt --check` + `cargo clippy -D warnings` + all test suites on `ubuntu-latest` and `macos-latest`.

## Test Matrix

### Phase 1: Core Foundation (Unit + CLI Integration)

| ID | Area | Type | Scenario | Status |
|---|---|---|---|---|
| TP-001 | Config | Unit | Load from `.env.local`, env vars, CLI flags | ✅ |
| TP-002 | Config | Unit | Missing API key returns structured error | ✅ |
| TP-003 | CLI Input | Integration | Prompt via CLI flag | ✅ |
| TP-004 | CLI Input | Integration | Prompt via stdin | ✅ |
| TP-005 | CLI Input | Integration | Prompt from file | ✅ |
| TP-006 | Request | Unit | Serialize ChatRequest to JSON | ✅ |
| TP-007 | Response | Unit | Deserialize ChatResponse | ✅ |
| TP-008 | Error | Unit | HTTP 401 → auth error | ✅ |
| TP-009 | Error | Unit | HTTP 5xx → API error | ✅ |
| TP-010 | Session | Integration | Tape persistence across runs | ✅ |
| TP-011 | Session | Integration | Reset command clears tape | ✅ |
| TP-012 | Logging | Integration | `RUST_LOG=debug` emits lifecycle logs | ✅ |

### Phase 2: Router + Tape + Tools (Unit)

| ID | Area | Type | Scenario | Status |
|---|---|---|---|---|
| TP-013 | Router | Unit | Comma command routes to internal handler | ✅ |
| TP-014 | Router | Unit | Unknown comma → shell execution | ✅ |
| TP-015 | Router | Unit | Natural language → enter_model=true | ✅ |
| TP-016 | Router | Unit | Failed command fallback to model | ✅ |
| TP-017 | Tape | Unit | Append, read, search, anchors | ✅ |
| TP-018 | Tape | Unit | Anchor-based context truncation | ✅ |
| TP-019 | Tools | Unit | Registry register, list, has, get | ✅ |
| TP-020 | Tools | Unit | Execute shell.exec, file.read/write/list/search | ✅ |
| TP-021 | Skills | Unit | Discover .agent/skills, parse frontmatter | ✅ |
| TP-022 | File Ops | Unit | Path traversal / sandbox enforcement | ✅ |
| TP-023 | Context | Unit | Sliding window truncation | ✅ |
| TP-024 | Context | Unit | Modular system prompt assembly | ✅ |

### Phase 3: Telegram E2E Integration (Mock LLM)

| ID | Area | Type | Scenario | Status |
|---|---|---|---|---|
| TP-025 | Telegram | Integration | OpenAI text reply | ✅ |
| TP-026 | Telegram | Integration | Anthropic text reply | ✅ |
| TP-027 | Telegram | Integration | Comma command bypasses model | ✅ |
| TP-028 | Telegram | Integration | Empty model response (no crash) | ✅ |
| TP-029 | Telegram | Integration | API 500 error → user error | ✅ |
| TP-030 | Telegram | Integration | HTTP 429 rate limit | ✅ |
| TP-031 | Telegram | Integration | Multi-turn session persistence | ✅ |
| TP-032 | Telegram | Integration | OpenAI tool call loop | ✅ |
| TP-033 | Telegram | Integration | Anthropic tool_use → tool_result → final reply | ✅ |
| TP-034 | Telegram | Integration | Anthropic shell.exec real execution | ✅ |
| TP-035 | Telegram | Integration | Anthropic multi-tool (2 tool_use blocks) | ✅ |
| TP-036 | Telegram | Integration | Max iterations breaker (no hang) | ✅ |
| TP-037 | Telegram | Integration | System prompt contains identity + tools sections | ✅ |
| TP-038 | Telegram | Integration | Workspace .agent/system-prompt.md override | ✅ |
| TP-039 | Telegram | Integration | file.write → file.read pipeline | ✅ |
| TP-040 | Telegram | Integration | Unknown tool name → error recovery | ✅ |
| TP-041 | Telegram | Integration | Empty input ignored | ✅ |
| TP-042 | Telegram | Integration | API error during tool loop | ✅ |

## Current Stats

- **Total automated tests**: 205 (177 unit + 10 CLI + 18 Telegram)
- **CI pipeline**: GitHub Actions on push/PR to `main`
- **All tests passing**: ✅

## Tooling

```bash
cargo fmt --check                                          # Format
cargo clippy --all-targets --all-features -- -D warnings   # Lint
cargo test                                                 # All tests
cargo test --test telegram_integration                     # Telegram only
```
