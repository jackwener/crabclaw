# S4: Tool Registry + Skill Discovery

## Background

Implement the tool registration system and automatic skill file discovery, enabling the LLM to call functions and load workspace-specific skills.

## Architecture

```
src/tools/
├── registry.rs   # BTreeMap tool registry + execute multiplexer
└── skills.rs     # .agent/skills/*/SKILL.md discovery + YAML frontmatter
```

## Implementation

| File | What it does |
|------|-------------|
| `registry.rs` | `ToolRegistry` — `register()`, `list()`, `has()`, `get()`, `execute()`. Pre-registers builtins: `tools`, `shell.exec`, `file.read`, `file.write`, `file.list`, `file.search`. Generates `ToolDefinition` JSON schemas for LLM function calling |
| `skills.rs` | `discover_skills()` — recursively scans `.agent/skills/*/SKILL.md`. Parses YAML frontmatter for name/description. `load_skill_body()` — returns skill content for injection into LLM context. Skills are bridged as `skill.<name>` tools |

## Key Design Decisions

- **BTreeMap**: Deterministic iteration order for consistent tool listing
- **Schema-driven**: Each tool has a `ToolDefinition` with JSON Schema parameters, matching OpenAI's function calling spec
- **Skill bridging**: Skills are passive (content injection) not active (code execution) — the LLM reads the skill content and decides how to use it

## Verification

- Registry: register, list, execute, overwrite semantics
- Skills: discovery from filesystem, frontmatter parsing, case-insensitive lookup
- Tool definitions: correct JSON schema generation for each builtin
