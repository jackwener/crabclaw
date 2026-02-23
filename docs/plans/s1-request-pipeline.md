# S1: Request Execution Pipeline

## Background

Enable CrabClaw to actually send HTTP requests to OpenAI-compatible APIs and parse responses into typed models.

## Architecture

```
src/llm/
├── api_types.rs    # ChatRequest, ChatResponse, Message, ToolCall, ToolDefinition
└── client.rs       # reqwest HTTP client with status code error classification
```

## Implementation

| File | What it does |
|------|-------------|
| `api_types.rs` | Typed request/response models matching OpenAI chat completions API. `Message` with role/content/tool_calls, `ChatRequest` with model/messages/max_tokens/tools |
| `client.rs` | `send_chat_request()` — POST to `/chat/completions`, classifies HTTP 401/403 → Auth, 429 → RateLimit, 5xx → Api errors. Parses non-standard error bodies (e.g., GLM's `{code, msg}`) |

## Key Design Decisions

- **Serde for everything**: All API types derive `Serialize`/`Deserialize` with `skip_serializing_if` for optional fields
- **Error body sniffing**: Handles APIs that return 200 with error payloads (e.g., `{success: false}`)
- **Generic base URL**: `api_base` is configurable to support OpenRouter, GLM, or any OpenAI-compatible provider

## Verification

- Serialization shape: `ChatRequest` → expected JSON structure
- Response parsing: various JSON shapes → typed `ChatResponse`
- Error classification: HTTP status codes → correct error categories
- Mock HTTP: `mockito` for all client tests
