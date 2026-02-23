# S6: Anthropic 适配器

## 背景

添加 Anthropic API 格式的原生支持，使 CrabClaw 能与 Anthropic 兼容模型（如 GLM-5 通过 `anthropic:` 前缀）协作。

## 架构

Anthropic 适配器与 OpenAI 客户端并行，通过模型前缀选择：

```
model: "gpt-4"              → OpenAI 路径 (/chat/completions)
model: "anthropic:claude-3"  → Anthropic 路径 (/v1/messages)
```

## 实现

| 文件 | 功能 |
|------|------|
| `api_types.rs` | `AnthropicRequest`（system 为顶层字段）、`AnthropicResponse`、`AnthropicContentBlock`。通过 `into_chat_response()` 转换响应 |
| `client.rs` | `send_anthropic_request()` — POST 到 `/v1/messages`。从 messages 数组中提取 system 消息到 `system` 字段。映射 `stop_reason: "end_turn"` 到 `finish_reason: "stop"` |

## 关键设计决策

- **前缀路由**：`anthropic:` 前缀发送前去除，简单明确
- **统一响应**：Anthropic 响应转换为与 OpenAI 相同的 `ChatResponse` 类型，下游代码无需感知提供商
- **System 字段提取**：Anthropic 要求 system prompt 作为独立字段，不在 messages 数组中

## 验证

- 响应转换：Anthropic JSON → 统一 `ChatResponse`
- 内容提取：text blocks → assistant content
