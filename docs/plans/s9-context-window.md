# S9: Tape Advanced Features + Context Window Management

## Background

Add tape search, anchor-based truncation, handoff command, and sliding window context management to prevent context overflow.

## Architecture

```
src/
├── tape/store.rs      # search(), anchor_entries(), entries_since_last_anchor()
└── core/
    ├── context.rs     # max_context_messages sliding window + truncation notice
    └── config.rs      # MAX_CONTEXT_MESSAGES env var parsing
```

## Implementation

| File | What it does |
|------|-------------|
| `store.rs` | `search(query)` — case-insensitive substring search across all entries. `anchor_entries()` — returns only anchor-type entries. `entries_since_last_anchor()` — context truncation at semantic boundaries. `reset_with_archive()` — archive old tape and start fresh |
| `context.rs` | `build_messages(max_context_messages)` — applies sliding window: keeps only the latest N messages. Injects synthetic system message when truncation occurs: "Older messages have been truncated..." |
| `config.rs` | `MAX_CONTEXT_MESSAGES` env var (default: 50) |

## Key Design Decisions

- **Sliding window over summarization**: Simpler and more predictable than LLM-based summarization. No information hallucination risk
- **Synthetic truncation notice**: Tells the model that context was truncated, so it doesn't assume it has full history
- **Configurable window**: Different use cases need different context sizes

## Verification

- Search: case-insensitive matching, no-match returns empty
- Anchors: only anchor entries returned
- Sliding window: correct message count after truncation
- Truncation notice: synthetic message present when truncation occurs
