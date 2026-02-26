---
description: 
---

# Engineering Workflow

This workflow is mandatory for all contributions.

## 1. Define Acceptance Criteria First

Before implementation, write explicit acceptance criteria covering:

- Behavior and user-visible outcomes
- Error handling and edge cases
- Constraints and non-goals

## 2. Produce a Test Plan Before Coding

Create a feature-level Test Plan that includes:

- Unit tests to add or update
- Integration checks if behavior crosses module boundaries
- Regression checks for touched paths

Default framework guidance by stack:

- Rust: `cargo test`
- TypeScript/JavaScript: `vitest` (fallback `jest`)
- Python: `pytest`
- Go: `go test`

If tests are skipped, record the reason in commit message or change notes.

## 3. Implement and Run Self-Checks

Run checks based on project availability:

- Rust: `cargo fmt --check`
- Rust: `cargo clippy --all-targets --all-features -- -D warnings`
- Rust: `cargo test`
- TypeScript/JavaScript: project lint/typecheck/tests
- Python: project lint/typecheck/tests
- Go: project lint/test commands

## 4. Update Documentation with Behavior Changes

When behavior, API, or config changes:

- Update `README.md` when user-facing behavior changes.
- Add or update ADRs in `docs/adr/` for architectural decisions.
- Add or update runbooks in `docs/runbooks/` for operations or incident handling.

## 5. Commit Policy

- Bug fix: commit immediately after fix completion.
- Feature: auto-commit when feature completion criteria are met.

## 6. Push Policy

Push is allowed only when all are true:

- cargo fmt --all && cargo clippy
- Tests pass
- Review checklist passes
- Documentation impact handled (updated or explicitly N/A)

## 7. Review Checklist

- Acceptance criteria defined before coding
- Test Plan created before coding
- Self-checks executed and passing
- Docs updated or marked N/A
- Commit policy followed
