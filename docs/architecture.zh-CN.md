# CrabClaw 架构文档

CrabClaw 是一个使用 Rust 编写的 OpenClaw 兼容智能代码工具链。本文档概述了它的核心设计理念、模块组织结构以及功能架构。

## 1. 核心设计理念

CrabClaw 致力于在一个统一的环境中，完美地将 **"命令执行 (Command Execution)"** 与 **"模型推理 (Model Reasoning)"** 解耦。它的设计极度关注行为的可预测性与可审计性：

- **确定性命令路由 (Deterministic Command Routing)**：所有以 `,` 开头的输入都被视为命令。
  - 已知的内部命令（例如 `,help`, `,tools`, `,handoff`）会绕过大模型，由应用程序直接拦截处理。
  - 未知的 `,` 前缀字符串会被路由给原生的操作系统 Shell 执行。
  - 非 `,` 开头的输入会被作为纯自然语言，直接交给大语言模型 (LLM) 解析。
- **单向数据流 (Single-Turn Data Flow)**：用户的输入和 AI 助手的输出都由同一套路由逻辑处理。一个统一的循环同时管控着用户指令和模型生成的函数调用 (Tool Calls)。
- **只追加记忆层 (Append-Only Tape)**：对话历史记录被保存在一个只允许追加写入的、基于 JSONL 的 `TapeStore` 中。这避免了上下文丢失，使得对话能够确定性地重放，并提供了清晰的按时间线排列的审计追踪。

## 2. 目录结构

`src/` 目录被划分为 5 个高内聚的、基于领域驱动 (Domain-Driven) 的子模块：

```text
src/
├── core/               # 核心路由、配置与领域逻辑
│   ├── config.rs       # 环境变量解析，多 Profile 配置覆盖
│   ├── error.rs        # 全局错误枚举与领域异常处理
│   ├── router.rs       # 核心路由与分发逻辑（区分命令与自然语言）
│   ├── input.rs        # 输入标准化（处理 CLI 参数传入还是 Stdin）
│   ├── command.rs      # 命令检测（区分内部命令与 Shell 命令）
│   ├── context.rs      # Context Window 构建器（滑动窗口截断 + 模块化系统提示词）
│   └── shell.rs        # Shell 命令执行器（带超时和失败上下文包装）
├── llm/                # 外部 AI 平台交互层
│   ├── client.rs       # 对话补全客户端（OpenAI + Anthropic，支持流式和非流式）
│   └── api_types.rs    # 统一数据结构 + Anthropic 消息格式转换层
├── tape/               # 会话记忆与持久化
│   └── store.rs        # JSONL Tape：追加、搜索、锚点、上下文截断
├── tools/              # LLM 函数调用与插件引擎
│   ├── registry.rs     # 工具定义 Schema、执行多路复用器、技能桥接
│   ├── skills.rs       # 自动发现并解析 .agent/skills 目录内的 Markdown 插件
│   └── file_ops.rs     # 工作区沙箱化的 file.read, file.write, file.list, file.search
├── channels/           # 多渠道输入/输出适配器 (Channels)
│   ├── base.rs         # 适用于各种接口渠道的通用 Trait
│   ├── manager.rs      # 用于管理所有后台 Channel 任务的调度器
│   ├── cli.rs          # 一次性命令行接口 (One-shot CLI) 执行逻辑
│   ├── repl.rs         # 交互式终端（带工具调用循环 + 流式输出）
│   └── telegram.rs     # 长期轮询 Telegram 机器人（带工具调用循环）
```

## 3. 组件交互流程

一个完整的智能体 (Agentic) 循环大致如下：

1. **接收输入**：用户通过某个 `Channel`（例如 CLI、交互式 REPL、Telegram）发送一条消息。
2. **路由寻址**：
   - `core::router::route_user` 会检查这条消息。
   - 如果它以 `,` 开头，它会作为内部命令去执行。执行结果会被当作短路输出立即返回。
   - 如果它是自然语言，路由器会为其打上需要模型执行的标记 (`enter_model = true`)。
3. **组装上下文**：这条文本会作为一条 `"user"` 消息追加到 `tape::store::TapeStore` 中。然后由 `core::context::build_messages` 重构出整个上下文历史（滑动窗口截断，默认 50 条）。
4. **系统提示词组装**：`core::context::build_system_prompt` 从 5 个模块化部分组装系统提示词：
   - **Identity**：定义 CrabClaw 的角色与行为准则。
   - **配置覆盖 / 工作区提示词**：3 层优先级（配置 > `.agent/system-prompt.md` > 内置默认）。
   - **运行时与工作区上下文**：动态注入工作区路径和运行时约定。
   - **上下文 / 日期时间**：通过 `chrono::Local::now()` 注入当前时间戳。
   - **工具契约**：列出可用工具及使用约定。
5. **模型推理**：`llm::client::send_chat_request` 向模型发起请求，同时带上上下文和工具列表。
   - 对于 Anthropic 模型，**消息转换层** (`convert_messages_for_anthropic`) 会自动将统一格式转换为 Anthropic 专用格式：
     - `role: tool` 消息 → `role: user` + `tool_result` content blocks。
     - 带 `tool_calls` 的 `assistant` 消息 → 结构化的 `tool_use` content blocks。
     - 工具定义 → `AnthropicToolDefinition` + `input_schema`。
6. **处理输出**：
   - 如果模型返回纯文本，这会被视作最终的 `"assistant"` 响应，保存回 Tape，并通过 `Channel` 显示给用户。
   - 如果模型返回 `tool_calls`，执行循环会将其拦截。
7. **工具调用循环**：运行时通过 `tools::registry::execute_tool` 执行模型请求的工具，生成带有 `"tool"` 角色的执行结果，将工具调用请求和工具输出一并追加到 Tape 中，最后再次调用 LLM 依据工具返回的内容进行推理（最高 `MAX_TOOL_ITERATIONS = 5` 轮）。

## 4. 功能特性

- **多端渠道接入**：CLI、交互式 REPL、Telegram 机器人（自带白名单访问控制）。
- **模型无关**：统一适配器支持 OpenRouter (OpenAI 格式) 和原生 Anthropic，自动完成消息格式转换。
- **流式输出**：支持 OpenAI 和 Anthropic 的 SSE 实时流式输出，统一 `StreamChunk` 枚举实现跨平台兼容。
- **技能引擎**：自动扫描 `.agent/skills/`，将 Markdown 技能说明桥接为 `skill.<name>` 工具。
- **Shell 命令执行**：未知 `,` 命令通过 `/bin/sh -c` 执行。失败结果包装为 XML 上下文供 LLM 自我纠正。30 秒超时保护。
- **工具调用循环**：REPL 和 Telegram 均支持最多 5 轮自主多步推理。支持 `shell.exec`、`skill.*`、`file.*` 及自定义工具。
- **文件操作**：`file.read`、`file.write`、`file.list`、`file.search` — 全部工作区沙箱化。
- **系统提示词**：模块化 5 段式组装，3 层优先级覆盖。
- **上下文窗口管理**：滑动窗口截断（可配置 `MAX_CONTEXT_MESSAGES`，默认 50 条），自动注入截断通知。

## 5. 测试架构

CrabClaw 维护 205 个自动化测试，分为三层：

| 层级 | 数量 | 范围 |
|------|------|------|
| 单元测试 (`cargo test --lib`) | 177 | 核心逻辑、配置、路由、tape、工具、文件操作、API 类型 |
| CLI 集成测试 (`tests/cli_run.rs`) | 10 | 端到端 CLI 行为 |
| Telegram 集成测试 (`tests/telegram_integration.rs`) | 18 | 通过 `process_message` 的全链路管线（mock LLM API） |

Telegram 集成测试使用 `mockito` 模拟 LLM 响应，覆盖：OpenAI/Anthropic 文本回复、工具调用循环（单工具/多工具/最大迭代中断）、系统提示词验证、文件操作管线、错误传播（API 故障、限流、未知工具）。
