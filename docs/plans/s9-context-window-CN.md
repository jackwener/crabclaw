# S9: Tape 高级功能 + 上下文窗口管理

## 背景

添加 tape 搜索、基于锚点的截断、handoff 命令和滑动窗口上下文管理，防止上下文溢出。

## 架构

```
src/
├── tape/store.rs      # search(), anchor_entries(), entries_since_last_anchor()
└── core/
    ├── context.rs     # max_context_messages 滑动窗口 + 截断通知
    └── config.rs      # MAX_CONTEXT_MESSAGES 环境变量解析
```

## 实现

| 文件 | 功能 |
|------|------|
| `store.rs` | `search(query)` — 大小写不敏感的子串搜索。`anchor_entries()` — 返回锚点记录。`entries_since_last_anchor()` — 基于语义边界的上下文截断。`reset_with_archive()` — 归档旧 tape 并重新开始 |
| `context.rs` | `build_messages(max_context_messages)` — 滑动窗口：只保留最新 N 条消息。截断时注入合成系统消息："Older messages have been truncated..." |
| `config.rs` | `MAX_CONTEXT_MESSAGES` 环境变量（默认：50） |

## 关键设计决策

- **滑动窗口优于摘要**：比 LLM 摘要更简单、更可预测。无信息幻觉风险
- **合成截断通知**：告诉模型上下文被截断了，避免模型以为它看到了完整历史
- **可配置窗口**：不同用例需要不同的上下文大小

## 验证

- 搜索：大小写不敏感匹配，无匹配返回空
- 锚点：只返回锚点记录
- 滑动窗口：截断后消息数量正确
- 截断通知：截断发生时合成消息存在
