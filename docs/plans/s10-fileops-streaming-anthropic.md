# S10: File Operations + Streaming + Anthropic Tool Calling

## Background

Complete the toolchain with workspace-sandboxed file operations, real-time streaming output, and full Anthropic tool calling integration.

## Architecture

```
src/
├── tools/file_ops.rs     # file.read/write/list/search with sandbox
├── llm/client.rs         # SSE streaming for OpenAI + Anthropic
├── llm/api_types.rs      # AnthropicToolDefinition, AnthropicMessage, convert_messages_for_anthropic
└── core/context.rs       # Modular 5-section system prompt
```

## Implementation

| File | What it does |
|------|-------------|
| `file_ops.rs` | `file.read` — read file content (truncated at 100KB). `file.write` — create/overwrite with auto parent dir creation. `file.list` — directory listing with type/size. `file.search` — regex search across files (max 50 matches). All paths workspace-sandboxed: rejects `..` traversal and absolute paths outside workspace |
| `client.rs` | `send_openai_request_stream()` / `send_anthropic_request_stream()` — SSE streaming via `reqwest` + `tokio::mpsc`. Unified `StreamChunk` enum: `Content(String)` / `ToolCall(...)` / `Done` |
| `api_types.rs` | `AnthropicToolDefinition` with `input_schema`. `AnthropicMessage` / `AnthropicContent` / `AnthropicContentItem` for structured content blocks. `convert_messages_for_anthropic()` — converts `role: tool` → `role: user` + `tool_result` blocks, `assistant` + `tool_calls` → `tool_use` blocks |
| `context.rs` | 5-section modular prompt: Identity → Config/Workspace override → Runtime context → DateTime → Tools contract |

## Key Design Decisions

- **Workspace sandbox**: Security-first — all file operations reject path traversal. Absolute paths must be within workspace
- **Message conversion layer**: Anthropic API requires different message formats for tool results. Rather than contaminating the unified `Message` type, a conversion layer translates at the boundary
- **Modular system prompt**: Each section is independently testable and configurable

## Verification

- File ops: read/write/list/search + sandbox enforcement (path traversal, absolute paths)
- Streaming: SSE parsing for both providers
- Anthropic tool calling: full integration tests via `process_message` with mock API
- System prompt: section presence verified via request body matching
