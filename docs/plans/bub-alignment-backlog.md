# Bub Alignment Backlog

- Owner: PM
- Last updated: 2026-02-21
- Alignment target: feature-level and design-level parity with Bub, not line-by-line translation.

## Alignment Rules

1. Prioritize user-visible behavior parity first.
2. Preserve Bub design principles: deterministic routing, explicit command boundaries, inspectable state.
3. Rust implementation may diverge internally if behavior and architecture intent remain equivalent.

## Feature Matrix

| Priority | Bub Capability | Bub Reference | CrabClaw Plan | Status |
|---|---|---|---|---|
| P0 | Config loading and deterministic precedence | `src/bub/config/settings.py` | `src/config.rs` + tests (`TP-001`,`TP-002`) | In Progress |
| P0 | Non-interactive message execution modes | `src/bub/cli/app.py` (`run`) | `run --prompt/--prompt-file/stdin` + tests (`TP-003`,`TP-004`,`TP-005`) | In Progress |
| P0 | Structured error categorization baseline | `src/bub/core/router.py` + docs | `src/error.rs` base categories | In Progress |
| P1 | Deterministic command boundary (comma-prefixed) | `src/bub/core/command_detector.py` + `tests/test_router.py` | Rust router module + parity tests | Planned |
| P1 | Command execution fallback-to-model behavior | `src/bub/core/router.py` | router result blocks and failure context | Planned |
| P1 | Tape-first session context with anchors/handoff | `src/bub/tape/service.py` | append-only local tape + anchor APIs | Planned |
| P2 | Unified tool + skill registry view | `src/bub/tools/registry.py` + skills loader | registry and progressive tool view | Planned |
| P2 | Channel integrations (Telegram/Discord) | `src/bub/channels/*` | optional adapters after CLI parity | Planned |

## Current Slice (S0)

1. Build Rust `library+CLI` skeleton aligned with ADR 0001.
2. Implement P0 config precedence and input modes.
3. Set and validate testing baseline in CI-ready commands.
4. Produce first Reviewer report with parity gaps.

## Exit Criteria for S0

1. P0 items marked "In Progress" are runnable with automated tests.
2. `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` pass.
3. Reviewer publishes first parity report in `docs/reviews/`.
