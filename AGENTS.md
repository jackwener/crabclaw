# AGENTS Policy

This file is the canonical policy source for this repository. If guidance conflicts with other local notes, this file wins.

## Project

- Name: CrabClaw
- Scope: Rust implementation of bub/OpenClaw capabilities.

## Design Philosophy Source

- Canonical philosophy notes: `IDEA.md`
- Upstream thinking source: https://frostming.com/posts/2026/create-a-claw/

CrabClaw implementation must align with this principle:

1. Keep a small deterministic kernel.
2. Prefer prompt/skill-driven evolution for non-kernel capabilities.
3. Add framework complexity only with explicit safety or determinism benefit.

## Mandatory Engineering Rules

1. Define acceptance criteria before implementation.
2. Produce a feature-level Test Plan before writing code.
3. Run applicable self-checks before merge or push.
4. Update docs when behavior, API, or config changes.
5. For bug fixes: commit immediately after the fix is complete.
6. For features: auto-commit when the feature is complete.
7. Push only after tests pass, review checklist passes, and docs impact is handled.
8. Any framework expansion that conflicts with `IDEA.md` must be justified in `docs/adr/`.

## Self-Check Policy

Run checks based on available stack and project files:

- Rust: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`
- TypeScript/JavaScript: prefer `vitest` (fallback `jest`)
- Python: `pytest`
- Go: `go test`

If tests are skipped, include explicit rationale in commit message or change notes.

## Experience Capture

- Record architecture decisions in `docs/adr/`.
- Record troubleshooting and recovery procedures in `docs/runbooks/`.
- Promote reusable workflow patterns back into this file.
- Keep philosophy-to-implementation mappings updated in `IDEA.md`.
