# CrabClaw Architecture

CrabClaw is a Rust implementation baseline inspired by the [bub.build](https://bub.build) agentic design. This document outlines its core design philosophy, module organization, and functional architecture.

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
│   ├── context.rs      # Context window from Tape (anchor-truncated)
│   └── shell.rs        # Shell command executor with timeout and failure wrapping
├── llm/                # External AI Provider Boundaries
│   ├── client.rs       # Generic chat completion client (Anthropic adapter, OpenAI format)
│   └── api_types.rs    # OpenAI-compatible Message, ToolCall, ToolDefinition
├── tape/               # Session Memory and Persistence
│   └── store.rs        # JSONL tape: append, search, anchors, context truncation
├── tools/              # LLM Function Calling and Plugin Engine
│   ├── registry.rs     # Tool definition schemas, execute multiplexer, skill bridging
│   ├── skills.rs       # Discovery and parsing of .agent/skills (.md plugins)
│   └── file_ops.rs     # Workspace-sandboxed file.read, file.write, file.list
├── channels/           # Input/Output Adapters (Multi-channel multiplexing)
│   ├── base.rs         # Common trait for interface channels
│   ├── manager.rs      # Supervisor handling background channel tasks
│   ├── cli.rs          # One-shot command-line interface execution
│   ├── repl.rs         # Interactive terminal with tool calling loop
│   └── telegram.rs     # Long-polling Telegram bot with tool calling loop
```

## 3. Component Interaction Flow

A complete agentic loop executes roughly as follows:

1. **Input Reception**: A user sends a message via a `Channel` (e.g., CLI, Interactive REPL, Telegram).
2. **Command Routing**: 
   - `core::router::route_user` inspects the message.
   - If it starts with `,`, it executes as an internal command. The result is returned immediately as short-circuit output.
   - If it is natural language, the router flags it for LLM execution (`enter_model = true`).
3. **Context Assembly**: The text is appended to `tape::store::TapeStore` as a `"user"` message. `core::context::build_messages` reconstructs the context history.
4. **LLM Inference**: `llm::client::send_chat_request` queries the model, providing context and defined tools from `tools::registry`.
5. **Output Processing**:
   - If the model returns plain text, it acts as a final `"assistant"` response, gets saved to the Tape, and is displayed through the `Channel`.
   - If the model returns `tool_calls` (e.g., using `fs.read`), the main execution loop (often managed in the Channel specific runner, like `telegram::process_message`) intercepts it.
6. **Tool Loop Execution**: The runtime executes the requested tool via `tools::registry::execute_tool`, generates a `"tool"` role response, appends both the tool call and output to the Tape, and re-invokes the LLM for reasoning (up to a mapped `MAX_ITERATIONS` limit).

## 4. Functional Capabilities

- **Multi-channel**: Currently supports local CLI, local Interactive REPL, and remote Telegram bots, with built-in access controls.
- **Model Agnostic**: Employs an adapter wrapper for `openrouter` (OpenAI format) and native `Anthropic` schemas.
- **Skill Engine**: Automatically scans the user's workspace for `.agent/skills/` repositories, converting Markdown-driven skill specifications into active agent context. Discovered skills are bridged into the tool registry as `skill.<name>` tools, making them callable by the LLM.
- **Shell Execution**: Unknown `,` commands (e.g., `,git status`, `,ls -la`) are executed as native shell commands via `/bin/sh -c`. Stdout/stderr/exit code are captured. Successful results are returned directly; failures are wrapped in structured `<command>` XML context and fed back to the LLM for self-correction. A 30-second timeout prevents runaway processes.
- **Tool Calling Loop**: Both REPL and Telegram channels support multi-iteration tool calling. The LLM can invoke `shell.exec` to run commands, `skill.*` to load skill context, or any builtin tool. Results are fed back for up to 5 iterations, enabling autonomous multi-step reasoning.
- **File Operations**: The LLM can read, write, list, and search files in the workspace via `file.read`, `file.write`, `file.list`, and `file.search` tools. All file operations are workspace-sandboxed — path traversal attempts (`..`) and absolute paths outside the workspace are rejected. Large files are truncated; search results are capped at 50 matches.
- **System Prompt**: Supports a 3-tier system prompt: explicit config override > workspace-local `.agent/system-prompt.md` > built-in default that describes available tools and behavioral guidelines.

