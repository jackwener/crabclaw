# Bub Alignment Backlog

- Owner: PM
- Last updated: 2026-02-23
- Alignment target: feature-level and design-level parity with Bub, not line-by-line translation.

## Alignment Rules

1. Prioritize user-visible behavior parity first.
2. Preserve Bub design principles: deterministic routing, explicit command boundaries, inspectable state.
3. Rust implementation may diverge internally if behavior and architecture intent remain equivalent.

## Feature Matrix

| Priority | Bub Capability | CrabClaw Implementation | Status |
|---|---|---|---|
| P0 | Config loading and deterministic precedence | `src/core/config.rs` — `.env.local` + env vars + CLI flags | ✅ Done |
| P0 | Non-interactive message execution modes | `src/channels/cli.rs` — `--prompt` / `--prompt-file` / stdin | ✅ Done |
| P0 | Structured error categorization baseline | `src/core/error.rs` — `thiserror` categories | ✅ Done |
| P0 | Deterministic command boundary (comma-prefixed) | `src/core/router.rs` + `src/core/command.rs` | ✅ Done |
| P0 | Command execution fallback-to-model behavior | `src/core/router.rs` — failure XML context → model | ✅ Done |
| P0 | Tape-first session context with anchors/handoff | `src/tape/store.rs` — JSONL append-only + anchors + search | ✅ Done |
| P1 | Unified tool + skill registry view | `src/tools/registry.rs` + `src/tools/skills.rs` | ✅ Done |
| P1 | Shell execution with failure self-correction | `src/core/shell.rs` — `/bin/sh -c` + timeout + stderr capture | ✅ Done |
| P1 | File operations (read/write/list/search) | `src/tools/file_ops.rs` — workspace-sandboxed | ✅ Done |
| P1 | Tool calling loop (multi-iteration reasoning) | REPL + Telegram — up to 5 rounds | ✅ Done |
| P1 | Channel integrations (Telegram) | `src/channels/telegram.rs` — long polling + ACL | ✅ Done |
| P1 | Streaming output | `src/llm/client.rs` — SSE for OpenAI + Anthropic | ✅ Done |
| P1 | Anthropic native adapter | `src/llm/client.rs` — message conversion + tool serialization | ✅ Done |
| P1 | System prompt modular assembly | `src/core/context.rs` — 5-section prompt | ✅ Done |
| P1 | Context window management | `src/core/context.rs` — sliding window truncation | ✅ Done |
| P2 | Discord channel | — | Planned |
| P2 | Voice / multimodal input | — | Planned |
| P2 | Multi-agent orchestration | — | Planned |

## Summary

- **15 of 18** features implemented and tested.
- **205 automated tests** covering all completed features.
- CI pipeline (GitHub Actions) is green on `ubuntu-latest` + `macos-latest`.
