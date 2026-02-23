# S2: 命令路由 + Tape 会话

## 背景

实现核心交互模型：逗号前缀命令路由 + 只追加会话录制，对齐 bub 的 `command_detector` + `tape/service`。

## 架构

```
src/
├── core/
│   ├── command.rs   # 逗号前缀解析器，shell-like tokenizer
│   └── router.rs    # 输入分发：命令 → 执行，NL → 模型
└── tape/
    └── store.rs     # JSONL 只追加 tape + 锚点
```

## 实现

| 文件 | 功能 |
|------|------|
| `command.rs` | `,help` → 内部命令，`,git status` → shell 命令。Tokenizer 支持引号分割。KV 参数解析 |
| `router.rs` | `route_user()` — 检查首字符：`,` → 命令路径（内部或 shell），否则 → 模型路径。失败的 shell 命令包装为 XML 上下文供模型自纠正 |
| `store.rs` | JSONL `TapeStore`：`append()`、`read_all()`、`search()`、`anchor_entries()`、`entries_since_last_anchor()`、`reset()`。每条记录有自增 ID、RFC3339 时间戳、事件类型和内容 |

## 关键设计决策

- **确定性路由**：单字符 `,` 明确分隔命令与自然语言
- **失败 Fallback**：失败的 shell 命令不报错——包装后发给模型自纠正
- **只追加 Tape**：无修改、无删除——完整可审计

## 验证

- 命令解析：内部 vs shell vs NL 分类
- 路由分发：每种输入类型的正确路径选择
- Tape CRUD：追加、读取、搜索、锚点、重置、跨重启持久化
