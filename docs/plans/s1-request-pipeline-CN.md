# S1: 请求执行管线

## 背景

使 CrabClaw 能够实际向 OpenAI 兼容 API 发送 HTTP 请求，并将响应解析为类型化模型。

## 架构

```
src/llm/
├── api_types.rs    # ChatRequest, ChatResponse, Message, ToolCall, ToolDefinition
└── client.rs       # reqwest HTTP 客户端 + 状态码错误分类
```

## 实现

| 文件 | 功能 |
|------|------|
| `api_types.rs` | 类型化请求/响应模型，匹配 OpenAI chat completions API。`Message` 含 role/content/tool_calls，`ChatRequest` 含 model/messages/max_tokens/tools |
| `client.rs` | `send_chat_request()` — POST 到 `/chat/completions`，HTTP 401/403 → Auth，429 → RateLimit，5xx → Api。解析非标准错误体（如 GLM 的 `{code, msg}`） |

## 关键设计决策

- **Serde 一切**：所有 API 类型 derive `Serialize`/`Deserialize`，optional 字段用 `skip_serializing_if`
- **错误体嗅探**：处理以 200 返回错误的 API（如 `{success: false}`）
- **通用 base URL**：`api_base` 可配置，支持 OpenRouter、GLM 或任何 OpenAI 兼容提供商

## 验证

- 序列化格式：`ChatRequest` → 预期 JSON 结构
- 响应解析：各种 JSON 格式 → 类型化 `ChatResponse`
- 错误分类：HTTP 状态码 → 正确的错误类别
- Mock HTTP：所有客户端测试使用 `mockito`
