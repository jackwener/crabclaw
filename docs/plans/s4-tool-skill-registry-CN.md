# S4: 工具注册 + Skill 发现

## 背景

实现工具注册系统和自动 skill 文件发现，使 LLM 能调用函数并加载工作区特定的 skills。

## 架构

```
src/tools/
├── registry.rs   # BTreeMap 工具注册表 + 执行多路复用器
└── skills.rs     # .agent/skills/*/SKILL.md 发现 + YAML frontmatter
```

## 实现

| 文件 | 功能 |
|------|------|
| `registry.rs` | `ToolRegistry` — `register()`、`list()`、`has()`、`get()`、`execute()`。预注册内置工具：`tools`、`shell.exec`、`file.read`、`file.write`、`file.list`、`file.search`。生成 `ToolDefinition` JSON Schema 供 LLM function calling |
| `skills.rs` | `discover_skills()` — 递归扫描 `.agent/skills/*/SKILL.md`。解析 YAML frontmatter 获取 name/description。`load_skill_body()` — 返回 skill 内容注入 LLM 上下文。Skills 桥接为 `skill.<name>` 工具 |

## 关键设计决策

- **BTreeMap**：确定性迭代顺序，工具列表一致
- **Schema 驱动**：每个工具有 `ToolDefinition` + JSON Schema 参数，匹配 OpenAI function calling 规范
- **Skill 桥接**：Skills 是被动的（内容注入）而非主动的（代码执行）——LLM 读取 skill 内容后自行决定如何使用

## 验证

- Registry：注册、列表、执行、覆盖语义
- Skills：文件系统发现、frontmatter 解析、大小写不敏感查找
- 工具定义：每个内置工具的 JSON Schema 正确生成
