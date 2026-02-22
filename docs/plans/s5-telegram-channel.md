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

### Bub 参考

| 文件 | 功能 |
|------|------|
| `channels/base.py` | `BaseChannel` 抽象：`start` / `get_session_prompt` / `process_output` |
| `channels/manager.py` | `ChannelManager`：注册 channels，`asyncio.gather` 并发运行 |
| `channels/telegram.py` | 431 行，long polling，ACL（allow_from/allow_chats），typing indicator，媒体解析 |
| `channels/utils.py` | Proxy 解析（explicit → env → macOS system） |

## Proposed Changes

### Channel 核心层

#### [NEW] `src/channel.rs`

Channel trait 定义，对齐 bub 的 `BaseChannel`：

```rust
#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&mut self) -> Result<()>;
    async fn stop(&mut self) -> Result<()>;
}
```

- Message 通过 callback / channel (tokio mpsc) 发送给 router
- 每条消息携带 `ChannelMessage` 元数据（session_id、chat_id、message_id 等）

#### [NEW] `src/channel_manager.rs`

管理多个 channel 的生命周期：

- `register(channel)` — 注册 channel
- `run()` — `tokio::select!` 并发运行所有 channel
- `shutdown()` — 优雅停止

---

### Telegram 适配器

#### [NEW] `src/telegram.rs`

基于 `teloxide` crate 实现：

- **Long Polling** — `teloxide::dispatching` 接收更新
- **ACL** — `allow_from` (user ID/username) + `allow_chats` (chat ID) 白名单
- **Typing Indicator** — 处理期间持续发送 `ChatAction::Typing`
- **消息解析** — 文本、图片（caption）、语音、文档等
- **回复上下文** — 解析 `reply_to_message` 元数据
- **输出** — 将 router 结果发送回 Telegram

---

### 配置扩展

#### [MODIFY] `src/config.rs`

新增 Telegram 相关配置字段：

```rust
pub struct AppConfig {
    // ... existing fields
    pub telegram_enabled: bool,
    pub telegram_token: Option<String>,
    pub telegram_allow_from: Vec<String>,
    pub telegram_allow_chats: Vec<String>,
    pub telegram_proxy: Option<String>,
}
```

环境变量映射：
- `BUB_TELEGRAM_TOKEN` → telegram_token
- `BUB_TELEGRAM_ALLOW_FROM` → 逗号分隔列表
- `BUB_TELEGRAM_ALLOW_CHATS` → 逗号分隔列表
- `HTTPS_PROXY` / `ALL_PROXY` → proxy

---

### CLI 扩展

#### [MODIFY] `src/cli.rs`

新增 `serve` 子命令：

```bash
crabclaw serve              # 启动所有已配置的 channel
crabclaw serve --telegram   # 仅启动 Telegram channel
```

---

### 依赖

#### [MODIFY] `Cargo.toml`

```toml
teloxide = { version = "0.13", features = ["macros"] }
async-trait = "0.1"
```

## Verification Plan

### 单元测试

| 测试 | 验证 |
|------|------|
| `channel_manager_register` | 注册和列出 channels |
| `telegram_config_from_env` | 环境变量解析 |
| `telegram_acl_allow` | 白名单用户通过 |
| `telegram_acl_deny` | 非白名单用户拒绝 |
| `telegram_parse_text` | 文本消息解析 |
| `telegram_parse_media` | 媒体消息携带元数据 |
| `telegram_session_id_format` | session ID = `telegram:{chat_id}` |

### 集成测试

```bash
# 需要 BUB_TELEGRAM_TOKEN 环境变量
BUB_TELEGRAM_TOKEN=xxx crabclaw serve --telegram

# 在 Telegram 中发送消息给 bot，验证：
# 1. Bot 回复消息
# 2. typing indicator 在处理期间显示
# 3. 逗号命令 (,help) 在 Telegram 中工作
# 4. 非白名单用户被拒绝
```

## 开发顺序

1. `channel.rs` — Channel trait + ChannelMessage
2. `config.rs` — 添加 Telegram 配置
3. `telegram.rs` — Telegram 适配器（MVP：纯文本消息）
4. `channel_manager.rs` — Manager + serve 子命令
5. `cli.rs` — `serve` 子命令
6. 测试 + 媒体解析扩展
