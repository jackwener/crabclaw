# ADR 0001: CLI-only vs library+CLI

- Status: Accepted
- Date: 2026-02-21
- Deciders: CrabClaw maintainers

## Context

CrabClaw targets a Rust implementation of bub/OpenClaw workflows.  
We must choose whether to implement all logic directly inside a binary (`CLI-only`) or split reusable logic into a library crate with a thin CLI layer (`library+CLI`).

## Decision Drivers

1. Testability of core behavior without process-level CLI orchestration.
2. Long-term maintainability as features expand beyond initial commands.
3. Reusability for future integration scenarios (automation, embedding, alternate frontends).
4. Clear separation of concerns between domain logic and command parsing.

## Considered Options

## Option A: CLI-only

Implement config, request pipeline, session handling, and output formatting directly in binary command handlers.

Pros:

- Faster initial implementation for very small scope.
- Fewer crate/module boundaries at the beginning.

Cons:

- Harder unit testing; business logic tends to depend on CLI process context.
- Reuse becomes expensive when adding other interfaces.
- Increased risk of tightly coupled command handlers and domain behavior.

## Option B: library+CLI

Implement domain logic in a library crate and keep CLI as an adapter layer for input/output and command routing.

Pros:

- Better unit-test coverage for core logic with isolated modules.
- Cleaner boundaries between parsing, orchestration, and domain operations.
- Enables future reuse by alternate interfaces without major refactor.

Cons:

- Slightly higher upfront design and module planning cost.
- Requires discipline to keep CLI layer thin.

## Decision

Adopt `library+CLI`.

CrabClaw will use:

- `src/lib.rs` for core modules (config, client, session, error, models).
- `src/main.rs` for CLI parsing and calling library APIs.
- Integration tests for CLI behavior and unit tests for library modules.

## Consequences

Positive:

- Higher confidence from focused unit tests.
- Lower refactor risk when extending commands or adding new frontends.
- Cleaner API boundaries for request/response handling.

Negative:

- Slightly more boilerplate in early commits.
- Requires explicit module contracts from the start.

## Implementation Notes

1. Keep command parsing and terminal formatting in binary layer only.
2. Keep API client, config resolution, and session persistence in library modules.
3. Prefer dependency injection for HTTP client and storage interfaces to simplify tests.

## Revisit Triggers

Revisit this ADR if:

1. Project scope is permanently reduced to one static command with no reuse needs.
2. Runtime or compile complexity from module boundaries becomes disproportionate to delivered value.
