# CrabClaw Phase 1 MVP Test Plan

## Metadata

- Scope: Phase 1 MVP baseline for OpenClaw-compatible CLI
- Date: 2026-02-21
- Status: Draft
- Related docs: `README.md`, `docs/adr/0001-cli-only-vs-library-plus-cli.md`

## Acceptance Criteria

1. Configuration precedence is deterministic and documented.
2. The CLI can execute a prompt from direct argument, stdin, and file input.
3. Request payloads and response mapping are validated with typed models.
4. Error handling differentiates config, network, auth, and API failures.
5. Optional session persistence can be enabled and reset explicitly.
6. Debug logging can be enabled by `RUST_LOG` without code changes.
7. Phase 1 behavior is covered by automated tests for core paths.

## Test Strategy

- Unit tests: validate pure logic and data mapping.
- Integration tests: validate end-to-end CLI behavior with mocked HTTP.
- Regression checks: retain golden behavior for config precedence and error output.
- Non-functional checks: lint, formatting, and compile-time checks.

## Test Matrix

| ID | Area | Type | Scenario | Expected Result |
|---|---|---|---|---|
| TP-001 | Config | Unit | Load values from `.env.local`, env vars, and CLI flags | Final config follows defined precedence |
| TP-002 | Config | Unit | Missing required API key | Returns structured config error |
| TP-003 | CLI Input | Integration | Prompt provided via CLI flag | Request is sent with expected prompt content |
| TP-004 | CLI Input | Integration | Prompt provided through stdin | Request is sent with stdin content |
| TP-005 | CLI Input | Integration | Prompt loaded from file | Request is sent with file content |
| TP-006 | Request Mapping | Unit | Serialize request model | JSON shape matches API contract |
| TP-007 | Response Mapping | Unit | Deserialize success response | Typed model values are populated |
| TP-008 | Error Mapping | Unit | HTTP 401 response | Returned as auth-category error |
| TP-009 | Error Mapping | Unit | HTTP 5xx response | Returned as API-category error |
| TP-010 | Session | Integration | Session persistence enabled across two runs | Second run loads prior context |
| TP-011 | Session | Integration | Explicit reset command/flag | Stored session is cleared |
| TP-012 | Logging | Integration | `RUST_LOG=debug` | Debug logs are emitted for request lifecycle |

## Tooling and Commands

- Format check: `cargo fmt --check`
- Lint check: `cargo clippy --all-targets --all-features -- -D warnings`
- Test execution: `cargo test`

## Exit Criteria

1. All tests in this plan are implemented or explicitly deferred with rationale.
2. `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all pass.
3. User-facing behavior changes are reflected in `README.md`.
4. Architecture-impacting decisions are captured in ADRs.

## Deferred Items Policy

If any planned test is deferred:

- Record the reason in change notes or commit message.
- Open a follow-up task linked to the deferred test ID.
