# CrabClaw 架构文档

CrabClaw 是一个基于 Rust 实现的基线项目，其代理 (Agentic) 设计灵感来源于 [bub.build](https://bub.build)。本文档概述了它的核心设计理念、模块组织结构以及功能架构。

## 1. 核心设计理念

CrabClaw 致力于在一个统一的环境中，完美地将 **“命令执行 (Command Execution)”** 与 **“模型推理 (Model Reasoning)”** 解耦。它的设计极度关注行为的可预测性与可审计性：

- **确定性命令路由 (Deterministic Command Routing)**：所有以 `,` 开头的输入都被严格视为命令。
  - 已知的内部命令（例如 `,help`, `,tools`）会绕过大模型，由应用程序直接拦截处理。
  - 未来将支持把未知的 `,` 前缀字符串路由给原生的操作系统 Shell 执行。
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
│   ├── router.rs       # 核心路由于分发逻辑（区分命令与自然语言）
│   ├── input.rs        # 输入标准化（处理 CLI 参数传入还是 Stdin）
│   ├── command.rs      # 内部命令的注册与执行（如 `help`, `tape.info`）
│   ├── context.rs      # 从 Tape 历史记录中重建模型的 Context Window
│   └── shell.rs        # Shell 命令执行器（带超时和结构化失败上下文包装）
├── llm/                # 外部 AI 平台交互层
│   ├── client.rs       # 通用的对话补全客户端（适配 Anthropic 和通用的 OpenAI 格式）
│   └── api_types.rs    # 兼容 OpenAI 格式的数据结构 (`Message`, `ToolCall`, `ToolDefinition`)
├── tape/               # 会话记忆与持久化
│   └── store.rs        # JSONL 文件读写器，管理时间戳与自增对数 ID
├── tools/              # LLM 函数调用与插件引擎
│   ├── registry.rs     # 工具定义 (Schema) 生成器与执行多路复用器
│   └── skills.rs       # 自动发现并解析工作区下 `.agent/skills` 目录内的 Markdown 插件
├── channels/           # 多渠道输入/输出适配器 (Channels)
│   ├── base.rs         # 适用于各种不同接口渠道的通用 Trait
│   ├── manager.rs      # 用于管理所有后台 Channel 任务的调度器
│   ├── cli.rs          # 一次性命令行接口 (One-shot CLI) 执行逻辑
│   ├── repl.rs         # 交互式终端 (Interactive REPL) 包装器
│   └── telegram.rs     # 长期轮询的 Telegram 机器人集成（含正在输入状态指示与媒体消息解析）
```

## 3. 组件交互流程 (Component Interaction Flow)

一个完整的智能体 (Agentic) 循环大致如下：

1. **接收输入**：用户通过某个 `Channel`（例如 CLI、交互式 REPL、Telegram）发送一条消息。
2. **路由寻址**：
   - `core::router::route_user` 会检查这条消息。
   - 如果它以 `,` 开头，它会作为内部命令去执行。执行结果会被当作短路输出立即返回。
   - 如果它是自然语言，路由器会为其打上需要模型执行的标记 (`enter_model = true`)。
3. **组装上下文**：这条文本会作为一条 `"user"` 消息追加到 `tape::store::TapeStore` 中。然后由 `core::context::build_messages` 重构出整个上下文历史。
4. **模型推理**：`llm::client::send_chat_request` 向模型发起请求，同时会带上整理好的上下文，以及从 `tools::registry` 中获取的可用工具列表。
5. **处理输出**：
   - 如果模型返回纯文本，这会被视作最终的 `"assistant"` 响应，保存回 Tape，并通过 `Channel` 显示给用户。
   - 如果模型返回 `tool_calls`（例如调用 `fs.read`），主要的执行循环（通常在 Channel 特定的运行器中，如 `telegram::process_message`）会将其拦截。
6. **工具调用循环 (Tool Loop)**：运行时通过 `tools::registry::execute_tool` 执行模型请求的工具，生成带有 `"tool"` 角色的执行结果，将工具调用请求和工具输出一并追加到 Tape 中，最后再次调用 LLM 依据工具返回的内容进行推理（最高重试次数由 `MAX_ITERATIONS` 限制）。

## 4. 功能特性

- **多端渠道接入 (Multi-channel)**：目前支持本地单次 CLI、本地交互式 REPL、以及远程 Telegram 机器人（自带白名单访问控制）。
- **模型无关 (Model Agnostic)**：自带统一适配器，支持 `openrouter` (OpenAI 格式) 和原生的 `Anthropic` 数据结构。
- **技能引擎 (Skill Engine)**：自动扫描用户工作区下的 `.agent/skills/` 目录，将基于 Markdown 编写的技能说明自动转换为智能体的活动上下文。
- **Shell 命令执行 (Shell Execution)**：未知的 `,` 前缀命令（如 `,git status`, `,ls -la`）会被作为原生 Shell 命令通过 `/bin/sh -c` 执行。执行结果会捕获 stdout/stderr/exit code。成功的结果直接返回给用户；失败的结果会被包装成结构化的 `<command>` XML 上下文并回传给 LLM 进行自我纠正。同时内置 30 秒超时保护以防止失控进程。
