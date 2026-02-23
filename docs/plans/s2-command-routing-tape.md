# S2: Command Routing + Tape Session

## Background

Implement the core interaction model: comma-prefixed command routing and append-only session recording, aligned with bub's `command_detector` + `tape/service`.

## Architecture

```
src/
├── core/
│   ├── command.rs   # Comma-prefix parser, shell-like tokenizer
│   └── router.rs    # Input dispatch: command → execute, NL → model
└── tape/
    └── store.rs     # JSONL append-only tape with anchors
```

## Implementation

| File | What it does |
|------|-------------|
| `command.rs` | `,help` → internal command, `,git status` → shell command. Tokenizer splits args respecting quotes. KV parameter parsing for structured commands |
| `router.rs` | `route_user()` — inspects first char: `,` → command path (internal or shell), else → model path. Failed shell commands wrapped in XML context for model self-correction |
| `store.rs` | JSONL-backed `TapeStore`: `append()`, `read_all()`, `search()`, `anchor_entries()`, `entries_since_last_anchor()`, `reset()`. Each entry has auto-increment ID, RFC3339 timestamp, event type, and content |

## Key Design Decisions

- **Deterministic routing**: Single character (`,`) unambiguously separates commands from natural language
- **Failure fallback**: Failed shell commands don't error — they're wrapped and sent to the model for self-correction
- **Append-only tape**: No mutation, no deletion — full auditability

## Verification

- Command parsing: internal vs shell vs NL classification
- Router dispatch: correct path selection for each input type
- Tape CRUD: append, read, search, anchor, reset, persistence across reopens
