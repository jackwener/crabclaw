# CrabClaw 已完成切片汇总 (S0–S10)

## S0: 项目骨架 + 配置基线

**目标**：建立 library+CLI 架构，实现确定性配置优先级。

| 文件 | 功能 |
|------|------|
| `src/core/config.rs` | `.env.local` + 环境变量 + CLI flag 三级优先级配置加载 |
| `src/core/input.rs` | `--prompt` / `--prompt-file` / stdin 三种输入方式 |
| `src/core/error.rs` | `thiserror` 结构化错误分类 |
| `src/channels/cli.rs` | `clap` CLI 解析 + `run` 子命令 |
| `src/main.rs` | `tracing-subscriber` 结构化日志 |

---

## S1: 请求执行管线

**目标**：能够实际发送 HTTP 请求到 OpenAI 兼容 API。

| 文件 | 功能 |
|------|------|
| `src/llm/api_types.rs` | `ChatRequest` / `ChatResponse` / `Message` 类型化模型 |
| `src/llm/client.rs` | `reqwest` HTTP 请求发送，状态码分类错误处理 |

---

## S2: 命令路由 + Tape 会话

**目标**：实现核心交互模型——逗号命令路由 + 会话录制。

| 文件 | 功能 |
|------|------|
| `src/core/command.rs` | 逗号前缀命令解析器，shell-like tokenizer，KV 参数解析 |
| `src/core/router.rs` | 输入路由：命令直接执行，NL → model，失败命令 fallback |
| `src/tape/store.rs` | JSONL append-only tape store + anchor 语义边界 |

---

## S3: 交互式 REPL + 多轮对话

**目标**：支持交互式对话和上下文连续。

| 文件 | 功能 |
|------|------|
| `src/channels/repl.rs` | `rustyline` REPL + 历史持久化 + 优雅中断 |
| `src/core/context.rs` | Tape → messages 上下文构建器 |

---

## S4: Tool 注册 + Skill 发现

**目标**：实现工具注册表和 skill 文件发现。

| 文件 | 功能 |
|------|------|
| `src/tools/registry.rs` | BTreeMap 工具注册表 + builtin 预注册 |
| `src/tools/skills.rs` | `.agent/skills/*/SKILL.md` 发现 + YAML frontmatter |

---

## S5: Telegram Channel

**目标**：实现 Telegram 长轮询机器人接入。

| 文件 | 功能 |
|------|------|
| `src/channels/telegram.rs` | `teloxide` 长轮询 + ACL 白名单 + typing indicator |
| `src/channels/manager.rs` | Channel 注册与生命周期管理 |
| `src/channels/base.rs` | Channel trait 抽象 |

---

## S6: Anthropic 适配器

**目标**：原生支持 Anthropic API 格式。

| 文件 | 功能 |
|------|------|
| `src/llm/client.rs` | `anthropic:` 前缀模型路由 + system 字段提取 |
| `src/llm/api_types.rs` | `AnthropicRequest` / `AnthropicResponse` 类型 |

---

## S7: Shell 命令执行

**目标**：实现 `,git status` 等 Shell 命令 + `shell.exec` 工具。

| 文件 | 功能 |
|------|------|
| `src/core/shell.rs` | `/bin/sh -c` 执行 + 超时 + stdout/stderr 捕获 |
| `src/core/router.rs` | Shell 命令路由 + 失败 fallback 到 model |

---

## S8: Skill 桥接 + 工具调用循环

**目标**：LLM 可自主调用工具并多轮推理。

| 文件 | 功能 |
|------|------|
| `src/channels/repl.rs` | 工具调用循环（最多 5 轮） |
| `src/channels/telegram.rs` | 工具调用循环（最多 5 轮） |

---

## S9: Tape 高级功能 + 上下文截断

**目标**：搜索、锚点、handoff、滑动窗口截断。

| 文件 | 功能 |
|------|------|
| `src/tape/store.rs` | `search()` / `anchor_entries()` / `entries_since_last_anchor()` |
| `src/core/context.rs` | `max_context_messages` 滑动窗口 + 截断通知注入 |
| `src/core/config.rs` | `MAX_CONTEXT_MESSAGES` 环境变量解析 |

---

## S10: 文件操作 + 流式输出 + Anthropic 工具调用

**目标**：完善文件操作、流式输出、Anthropic 工具调用集成。

| 文件 | 功能 |
|------|------|
| `src/tools/file_ops.rs` | `file.read` / `file.write` / `file.list` / `file.search` + 沙箱 |
| `src/llm/client.rs` | SSE 流式输出（OpenAI + Anthropic） |
| `src/llm/api_types.rs` | `AnthropicToolDefinition` / `AnthropicMessage` / `convert_messages_for_anthropic` |
| `src/core/context.rs` | 模块化系统提示词（5 段式组装） |

---

## S11: file.edit 工具

**目标**：实现精确文件编辑（search-replace），比 file.write 更节省 token。

| 文件 | 功能 |
|------|------|
| `src/tools/file_ops.rs` | `edit_file()` — search old text → replace new text，支持 replace_all |
| `src/tools/registry.rs` | 注册 `file.edit` 到 builtin，添加 JSON Schema + execute_tool 分发 |

---

## S12: route_assistant (助手输出自动路由)

**目标**：Model 输出中的逗号命令自动执行，启用 model 自主操作能力。

| 文件 | 功能 |
|------|------|
| `src/core/router.rs` | `route_assistant()` 逐行扫描输出，检测并执行逗号命令 |
| `src/core/router.rs` | `AssistantRouteResult` — visible_text / command_blocks / exit_requested |
| `src/core/agent_loop.rs` | 在 `process_turn_result` 中集成 route_assistant |

---

## 测试汇总

- **S0–S12 总计**：自动化测试覆盖 unit + AgentLoop integration + CLI integration + Telegram integration（数量随演进动态增长）
- 另有 10 个 live E2E 测试（需 API Key）
- 所有 `cargo fmt` / `clippy -D warnings` / `cargo test` 通过
- GitHub Actions CI 在 `ubuntu-latest` + `macos-latest` 全绿
