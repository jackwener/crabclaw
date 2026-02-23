# CrabClaw Architecture

CrabClaw is an OpenClaw-compatible agentic coding toolchain written in Rust. This document outlines its core design philosophy, module organization, and functional architecture.

## 1. Core Philosophy

CrabClaw aims to perfectly decouple **Command Execution** from **Model Reasoning** within a unified environment. Its design prioritizes predictability and auditability:

- **Deterministic Command Routing**: All inputs starting with `,` are treated as commands.
  - Known internal commands (e.g., `,help`, `,tools`, `,handoff`) bypass the model and are handled immediately.
  - Unknown comma-prefixed strings are executed as native shell commands.
  - Non-comma inputs are interpreted as conversational NLP meant for the language model.
- **Single-Turn Data Flow**: User input and Assistant output are processed by the same routing logic. A single unified loop governs both user instructions and model-generated tool calls.
- **Append-Only Memory (Tape)**: Conversation history is recorded in an append-only, JSONL-backed `TapeStore`. This prevents contextual loss, allows deterministic replay, and provides a clear chronological audit trail.

## 2. Directory Structure

The `src/` directory is partitioned into 5 highly-cohesive, domain-driven modules:

```text
src/
├── core/               # Core routing, config, and domain logic
│   ├── config.rs       # Environment parsing, multi-profile resolution
│   ├── error.rs        # Global error enums and domain exceptions
│   ├── router.rs       # Command vs NL routing logic
│   ├── input.rs        # Input normalization (CLI flags vs Stdin)
│   ├── command.rs      # Command detection (Internal vs Shell)
│   ├── context.rs      # Context window builder with sliding window truncation
│   └── shell.rs        # Shell command executor with timeout and failure wrapping
├── llm/                # External AI Provider Boundaries
│   ├── client.rs       # Chat completion client (OpenAI + Anthropic, streaming + non-streaming)
│   └── api_types.rs    # Unified types: Message, ToolCall, AnthropicMessage conversion layer
├── tape/               # Session Memory and Persistence
│   └── store.rs        # JSONL tape: append, search, anchors, context truncation
├── tools/              # LLM Function Calling and Plugin Engine
│   ├── registry.rs     # Tool definition schemas, execute multiplexer, skill bridging
│   ├── skills.rs       # Discovery and parsing of .agent/skills (.md plugins)
│   └── file_ops.rs     # Workspace-sandboxed file.read, file.write, file.list, file.search
├── channels/           # Input/Output Adapters (Multi-channel multiplexing)
│   ├── base.rs         # Common trait for interface channels
│   ├── manager.rs      # Supervisor handling background channel tasks
│   ├── cli.rs          # One-shot command-line interface execution
│   ├── repl.rs         # Interactive terminal with tool calling loop + streaming
│   └── telegram.rs     # Long-polling Telegram bot with tool calling loop
```

## 3. Component Interaction Flow

A complete agentic loop executes roughly as follows:

1. **Input Reception**: A user sends a message via a `Channel` (e.g., CLI, Interactive REPL, Telegram).
2. **Command Routing**: 
   - `core::router::route_user` inspects the message.
   - If it starts with `,`, it executes as an internal command. The result is returned immediately as short-circuit output.
   - If it is natural language, the router flags it for LLM execution (`enter_model = true`).
3. **Context Assembly**: The text is appended to `tape::store::TapeStore` as a `"user"` message. `core::context::build_messages` reconstructs the context history with sliding window truncation (default: 50 messages).
4. **System Prompt Assembly**: `core::context::build_system_prompt` assembles a modular system prompt from multiple sections:
   - **Identity**: Defines CrabClaw's persona and behavioral guidelines.
   - **Config Override / Workspace Prompt**: 3-tier priority (config > `.agent/system-prompt.md` > built-in).
   - **Runtime & Workspace Context**: Dynamic workspace path and runtime contract.
   - **Context / DateTime**: Current timestamp via `chrono::Local::now()`.
   - **Tools Contract**: Lists available tools and usage conventions.
5. **LLM Inference**: `llm::client::send_chat_request` queries the model, providing context and defined tools from `tools::registry`.
   - For Anthropic models, a **message conversion layer** (`convert_messages_for_anthropic`) transforms unified messages into Anthropic's format:
     - `role: tool` messages → `role: user` with `tool_result` content blocks.
     - `assistant` with `tool_calls` → structured `tool_use` content blocks.
     - Tool definitions → `AnthropicToolDefinition` with `input_schema`.
6. **Output Processing**:
   - If the model returns plain text, it acts as a final `"assistant"` response, gets saved to the Tape, and is displayed through the `Channel`.
   - If the model returns `tool_calls`, the execution loop intercepts it.
7. **Tool Loop Execution**: The runtime executes the requested tool via `tools::registry::execute_tool`, generates a `"tool"` role response, appends both the tool call and output to the Tape, and re-invokes the LLM (up to `MAX_TOOL_ITERATIONS = 5`).

## 4. Functional Capabilities

- **Multi-channel**: CLI, Interactive REPL, and Telegram bots with whitelist access controls.
- **Model Agnostic**: Unified adapter supporting OpenRouter (OpenAI format) and native Anthropic schemas, with automatic message format conversion.
- **Streaming Output**: Real-time SSE streaming for both OpenAI and Anthropic providers, with unified `StreamChunk` enum for cross-provider compatibility.
- **Skill Engine**: Automatically scans `.agent/skills/` for Markdown skill specs, bridging them as `skill.<name>` tools callable by the LLM.
- **Shell Execution**: Unknown `,` commands are executed via `/bin/sh -c`. Failures are wrapped in XML context for LLM self-correction. 30-second timeout prevents runaway processes.
- **Tool Calling Loop**: Multi-iteration autonomous reasoning (up to 5 rounds) across REPL and Telegram channels. Supports `shell.exec`, `skill.*`, `file.*`, and custom tools.
- **File Operations**: `file.read`, `file.write`, `file.list`, `file.search` — all workspace-sandboxed with path traversal protection, large file truncation, and 50-match search cap.
- **System Prompt**: Modular 5-section prompt assembly with 3-tier override priority.
- **Context Window Management**: Sliding window truncation with configurable `MAX_CONTEXT_MESSAGES` (default: 50) and synthetic truncation notice.

## 5. Test Architecture

CrabClaw maintains 205 automated tests across three tiers:

| Tier | Count | Scope |
|------|-------|-------|
| Unit tests (`cargo test --lib`) | 177 | Core logic, config, router, tape, tools, file ops, API types |
| CLI integration (`tests/cli_run.rs`) | 10 | End-to-end CLI behavior with real binary |
| Telegram integration (`tests/telegram_integration.rs`) | 18 | Full pipeline via `process_message` with mock LLM API |

The Telegram integration tests use `mockito` to simulate LLM responses, covering:
- OpenAI and Anthropic text replies
- Tool calling loops (single, multi-tool, max iteration breaker)
- System prompt section verification
- File operations via tool pipeline
- Error propagation (API failures, rate limits, unknown tools)
