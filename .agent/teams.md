# CrabClaw Agent Teams

This file defines the working contract for three roles: PM, Executor, and Reviewer.

## Roles

### PM

- Owns Bub alignment scope and priorities.
- Maintains feature parity matrix and acceptance criteria.
- Splits milestones into implementation-ready tasks with measurable outputs.
- Approves "equivalent design" decisions when Rust differs from Python internals.

Deliverables:

- `docs/plans/bub-alignment-backlog.md`
- acceptance criteria updates in test plans
- milestone sign-off notes

### Executor

- Implements approved PM tasks in Rust.
- Keeps architecture aligned with ADRs and writes tests before or with implementation.
- Maintains baseline quality gates: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`.

Deliverables:

- production code in `src/`
- automated tests in `tests/`
- docs updates when behavior/config/API changes

### Reviewer

- Reviews behavior parity against Bub and checks design-level alignment.
- Reviews for regressions, missing tests, and incorrect abstractions.
- Emits a pass/fail review with concrete gap list and next actions.

Deliverables:

- review reports in `docs/reviews/`

## Handoff Protocol

1. PM publishes prioritized task slice and acceptance criteria.
2. Executor implements and links tests to plan IDs.
3. Reviewer checks parity, test coverage, and architecture consistency.
4. If failed, task returns to Executor with issue list.
5. If passed, PM marks slice complete and advances backlog.

## Definition of Aligned

CrabClaw is considered aligned with Bub when:

1. User-visible behavior matches the intended Bub feature outcome.
2. Core design principles are preserved (determinism, inspectability, recoverability).
3. Rust-native implementation differences are documented and justified.
