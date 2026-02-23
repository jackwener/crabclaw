# S10: 文件操作 + 流式输出 + Anthropic 工具调用

## 背景

以工作区沙箱化文件操作、实时流式输出和完整的 Anthropic 工具调用集成来完善工具链。

## 架构

```
src/
├── tools/file_ops.rs     # file.read/write/list/search + 沙箱
├── llm/client.rs         # OpenAI + Anthropic SSE 流式输出
├── llm/api_types.rs      # AnthropicToolDefinition, AnthropicMessage, convert_messages_for_anthropic
└── core/context.rs       # 模块化 5 段式系统提示词
```

## 实现

| 文件 | 功能 |
|------|------|
| `file_ops.rs` | `file.read` — 读取文件内容（100KB 截断）。`file.write` — 创建/覆盖 + 自动创建父目录。`file.list` — 目录列表（类型/大小）。`file.search` — 跨文件正则搜索（最多 50 条匹配）。所有路径工作区沙箱化：拒绝 `..` 穿越和工作区外绝对路径 |
| `client.rs` | `send_openai_request_stream()` / `send_anthropic_request_stream()` — `reqwest` + `tokio::mpsc` SSE 流式。统一 `StreamChunk` 枚举：`Content(String)` / `ToolCall(...)` / `Done` |
| `api_types.rs` | `AnthropicToolDefinition` + `input_schema`。`AnthropicMessage` / `AnthropicContent` / `AnthropicContentItem` 结构化 content blocks。`convert_messages_for_anthropic()` — `role: tool` → `role: user` + `tool_result`，`assistant` + `tool_calls` → `tool_use` |
| `context.rs` | 5 段式模块化提示词：Identity → 配置/工作区覆盖 → 运行时上下文 → 日期时间 → 工具契约 |

## 关键设计决策

- **工作区沙箱**：安全优先——所有文件操作拒绝路径穿越。绝对路径必须在工作区内
- **消息转换层**：Anthropic API 对工具结果要求不同的消息格式。不污染统一的 `Message` 类型，而是在边界处转换
- **模块化系统提示词**：每个段落可独立测试和配置

## 验证

- 文件操作：read/write/list/search + 沙箱强制（路径穿越、绝对路径）
- 流式输出：两个提供商的 SSE 解析
- Anthropic 工具调用：通过 `process_message` + mock API 的完整集成测试
- 系统提示词：通过请求体匹配验证段落存在
