# S6: Anthropic Adapter

## Background

Add native support for Anthropic API format, enabling CrabClaw to work with Anthropic-compatible models (e.g., GLM-5 via `anthropic:` prefix).

## Architecture

The Anthropic adapter lives alongside the OpenAI client, selected by model prefix:

```
model: "gpt-4"              → OpenAI path (/chat/completions)
model: "anthropic:claude-3"  → Anthropic path (/v1/messages)
```

## Implementation

| File | What it does |
|------|-------------|
| `api_types.rs` | `AnthropicRequest` (system as top-level field), `AnthropicResponse`, `AnthropicContentBlock`. Response conversion via `into_chat_response()` |
| `client.rs` | `send_anthropic_request()` — POST to `/v1/messages`. Extracts system messages from the messages array into the `system` field. Maps `stop_reason: "end_turn"` to `finish_reason: "stop"` |

## Key Design Decisions

- **Prefix-based routing**: `anthropic:` prefix stripped before sending to API, simple and explicit
- **Unified response**: Anthropic responses are converted to the same `ChatResponse` type used by OpenAI, so downstream code is provider-agnostic
- **System field extraction**: Anthropic requires system prompt as a separate field, not in the messages array

## Verification

- Response conversion: Anthropic JSON → unified `ChatResponse`
- Content extraction: text blocks → assistant content
