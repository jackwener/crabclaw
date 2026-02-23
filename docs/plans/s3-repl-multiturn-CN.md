# S3: 交互式 REPL + 多轮对话

## 背景

添加交互式终端会话，支持跨轮次的上下文连续，实现与 LLM 的多轮对话。

## 架构

```
src/
├── core/
│   └── context.rs   # Tape → messages 上下文构建器
└── channels/
    └── repl.rs      # rustyline REPL + 历史持久化
```

## 实现

| 文件 | 功能 |
|------|------|
| `context.rs` | `build_messages()` — 读取最近锚点以来的 tape 记录，转换为 API 所需的 `Message` 结构。`build_system_prompt()` — 组装模块化 5 段式系统提示词 |
| `repl.rs` | `rustyline` REPL，历史持久化到 `~/.crabclaw/history`。优雅处理 Ctrl-C/Ctrl-D。每行输入通过 `core::router` 路由，NL 发送给模型并附带完整上下文 |

## 关键设计决策

- **上下文来自 Tape**：REPL 不维护自己的消息历史——始终从 tape 重建上下文，确保一致性
- **历史持久化**：readline 历史跨 REPL 会话保留
- **优雅退出**：Ctrl-C 清除当前行，Ctrl-D 干净退出

## 验证

- 上下文构建器：从 tape 记录正确重建消息
- 系统提示词：模块化段落组装和三级优先级
