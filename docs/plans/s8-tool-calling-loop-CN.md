# S8: 工具调用循环

## 背景

启用多轮自主推理：LLM 可调用工具、接收结果、继续推理，最多 5 轮。

## 架构

工具调用循环在每个 channel 的消息处理器中运行：

```
用户消息 → 模型 → 有 tool_calls？
                    ├── 是 → 执行工具 → 追加结果 → 重新调用模型（最多重复 5 次）
                    └── 否 → 返回文本响应
```

## 实现

| 文件 | 功能 |
|------|------|
| `telegram.rs` | `process_message()` — 主循环，`MAX_TOOL_ITERATIONS = 5`。检测响应中的 `tool_calls`，通过 `registry::execute_tool()` 执行，追加 `Message::tool()` 结果，重新调用模型 |
| `repl.rs` | 同样的循环逻辑，适配交互式终端 + 流式输出 |

## 关键设计决策

- **固定迭代上限**：5 轮防止无限工具调用循环
- **Channel 专属循环**：每个 channel 管理自己的循环，而非集中在 client 中，允许 channel 特有行为（如 Telegram 的 typing indicator）
- **Tape 录制**：工具调用和结果都追加到 tape，确保可审计

## 验证

- 单工具调用：模型调用工具 → 结果 → 最终文本
- 多工具：一次响应中多个 tool_calls → 全部执行
- 最大迭代：模型持续调用工具 → 循环在 5 次后终止
