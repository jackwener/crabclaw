# S3: Interactive REPL + Multi-turn Conversation

## Background

Add an interactive terminal session with context continuity across turns, enabling multi-turn conversations with the LLM.

## Architecture

```
src/
├── core/
│   └── context.rs   # Tape → messages context builder
└── channels/
    └── repl.rs      # rustyline REPL with history
```

## Implementation

| File | What it does |
|------|-------------|
| `context.rs` | `build_messages()` — reads tape entries since last anchor, converts them to `Message` structs for the API. `build_system_prompt()` — assembles modular 5-section system prompt |
| `repl.rs` | `rustyline` REPL with `~/.crabclaw/history` persistence. Graceful Ctrl-C/Ctrl-D handling. Routes each line through `core::router`, sends NL to model with full context |

## Key Design Decisions

- **Context from tape**: The REPL doesn't maintain its own message history — it always rebuilds context from the tape, ensuring consistency
- **History persistence**: readline history survives across REPL sessions
- **Graceful exit**: Ctrl-C clears current line, Ctrl-D exits cleanly

## Verification

- Context builder: correct message reconstruction from tape entries
- System prompt: modular section assembly and 3-tier priority
