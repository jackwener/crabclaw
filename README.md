# CrabClaw

CrabClaw is a Rust implementation of [bub](https://github.com/PsiACE/bub), aiming to provide an OpenClaw-compatible toolchain in Rust.

## Project Intent

- Build a Rust-native version of the bub/OpenClaw workflow.
- Keep behavior predictable and test-driven.
- Preserve architecture and operations knowledge in versioned docs.

## MVP Scope (Phase 1)

The first milestone is an OpenClaw-compatible baseline aligned with core bub workflows:

1. Configuration and profile resolution
   - Load config from `.env.local`, environment variables, and CLI flags.
   - Support explicit profile selection and deterministic precedence.
2. Request execution pipeline
   - Send chat/completion-style requests to an OpenClaw-compatible endpoint.
   - Provide consistent request/response mapping with typed Rust models.
3. CLI operating modes
   - Non-interactive execution from direct prompt input.
   - Input from stdin or file for script-friendly usage.
4. Session and context baseline
   - Optional local session persistence for short conversational context.
   - Clear reset semantics for stateless runs.
5. Reliability and observability
   - Structured error categories (config, network, auth, API).
   - Verbose debug logging via `RUST_LOG`.

Out of scope for Phase 1:

- Plugin system or extension marketplace.
- Multi-provider routing beyond OpenClaw-compatible API.
- GUI/TUI experience beyond minimal CLI output.

## Quick Start

1. Install stable Rust toolchain.
2. Copy `.env.example` to `.env.local` and set local values.
3. Run `cargo test`.

## Documentation

- Workflow rules: `.agent/workflow.md`
- Agent team contract: `.agent/teams.md`
- Team policy source: `AGENTS.md`
- Philosophy analysis notes: `IDEA.md`
- Feature test plan: `docs/test-plans/phase-1-mvp.md`
- Bub alignment backlog: `docs/plans/bub-alignment-backlog.md`
- Architecture decisions: `docs/adr/`
- Review reports: `docs/reviews/`
- Operational runbooks: `docs/runbooks/`
