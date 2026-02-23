# S5: Telegram Channel 集成

## 背景

CrabClaw 目前仅支持 CLI 交互（REPL + `run` 命令）。本切片将添加 Telegram Bot 作为第一个 channel 适配器，对齐 bub 的 `channels/` 架构。

## 架构

对齐 bub 的三层设计：

```
┌──────────────────┐
│  ChannelManager   │  协调多个 channel，async 并发运行
├──────────────────┤
│  Channel trait    │  start / get_session_prompt / process_output
├──────────────────┤
│  TelegramChannel  │  teloxide long polling，ACL，typing indicator
└──────────────────┘
```

## 实现

| 文件 | 功能 |
|------|------|
| `channels/base.rs` | `Channel` trait — `name()`、`start()`、`stop()` |
| `channels/manager.rs` | `ChannelManager` — `register()`、`run()`（`tokio::select!` 并发）、`shutdown()` |
| `channels/telegram.rs` | `teloxide` 长轮询 + ACL 白名单（`allow_from` + `allow_chats`）+ typing indicator + 消息路由到 `process_message` |
| `core/config.rs` | `telegram_token`、`telegram_allow_from`、`telegram_allow_chats`、`telegram_proxy` 配置 |

## 关键设计决策

- **Long Polling**：相比 Webhook 更简单，无需公网 IP
- **ACL 白名单**：user ID/username + chat ID 双重过滤
- **process_message 公开**：暴露为 `pub` 函数，支持直接集成测试（无需真实 Telegram 连接）

## 验证

- ACL：白名单通过、非白名单拒绝
- 消息路由：文本、逗号命令、NL 正确分发
- 集成测试：18 个 mock LLM 测试覆盖完整管线
