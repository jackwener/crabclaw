# S8: Tool Calling Loop

## Background

Enable multi-iteration autonomous reasoning: the LLM can call tools, receive results, and reason further for up to 5 rounds.

## Architecture

The tool calling loop runs in each channel's message handler:

```
User message → Model → tool_calls? 
                          ├── Yes → execute tools → append results → re-invoke model (repeat up to 5x)
                          └── No  → return text response
```

## Implementation

| File | What it does |
|------|-------------|
| `telegram.rs` | `process_message()` — main loop with `MAX_TOOL_ITERATIONS = 5`. Detects `tool_calls` in response, executes via `registry::execute_tool()`, appends results as `Message::tool()`, re-invokes model |
| `repl.rs` | Same loop logic adapted for interactive terminal with streaming output |

## Key Design Decisions

- **Fixed iteration cap**: 5 rounds prevents infinite tool-calling loops
- **Channel-specific loops**: Each channel manages its own loop rather than centralizing in the client, allowing channel-specific behavior (e.g., typing indicators in Telegram)
- **Tape recording**: Both tool calls and results are appended to the tape for auditability

## Verification

- Single tool call: model calls tool → result → final text
- Multi-tool: multiple tool_calls in one response → all executed
- Max iterations: model keeps calling tools → loop terminates at 5
