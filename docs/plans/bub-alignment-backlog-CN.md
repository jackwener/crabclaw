# Bub 对齐进度

- 负责人：PM
- 最后更新：2026-02-23
- 对齐目标：功能级和设计级与 Bub 对齐，而非逐行翻译。

## 对齐规则

1. 优先保证用户可见行为的一致性。
2. 保留 Bub 设计原则：确定性路由、显式命令边界、可检视状态。
3. Rust 实现在行为和架构意图等价的前提下，内部可自由偏离。

## 功能矩阵

| 优先级 | Bub 能力 | CrabClaw 实现 | 状态 |
|---|---|---|---|
| P0 | 配置加载和确定性优先级 | `src/core/config.rs` — `.env.local` + env + CLI | ✅ 完成 |
| P0 | 非交互式消息执行模式 | `src/channels/cli.rs` — `--prompt` / `--prompt-file` / stdin | ✅ 完成 |
| P0 | 结构化错误分类 | `src/core/error.rs` — `thiserror` 分类 | ✅ 完成 |
| P0 | 确定性命令边界（逗号前缀） | `src/core/router.rs` + `src/core/command.rs` | ✅ 完成 |
| P0 | 命令执行失败 fallback 到 model | `src/core/router.rs` — XML 上下文 → model | ✅ 完成 |
| P0 | Tape 优先会话上下文 + 锚点/handoff | `src/tape/store.rs` — JSONL 只追加 + 锚点 + 搜索 | ✅ 完成 |
| P1 | 统一工具 + Skill 注册表视图 | `src/tools/registry.rs` + `src/tools/skills.rs` | ✅ 完成 |
| P1 | Shell 执行 + 失败自纠正 | `src/core/shell.rs` — `/bin/sh -c` + 超时 + stderr | ✅ 完成 |
| P1 | 文件操作 (read/write/list/search) | `src/tools/file_ops.rs` — 工作区沙箱 | ✅ 完成 |
| P1 | 工具调用循环（多轮推理） | REPL + Telegram — 最多 5 轮 | ✅ 完成 |
| P1 | 渠道集成 (Telegram) | `src/channels/telegram.rs` — 长轮询 + ACL | ✅ 完成 |
| P1 | 流式输出 | `src/llm/client.rs` — OpenAI + Anthropic SSE | ✅ 完成 |
| P1 | Anthropic 原生适配器 | `src/llm/client.rs` — 消息转换 + 工具序列化 | ✅ 完成 |
| P1 | 系统提示词模块化组装 | `src/core/context.rs` — 5 段式提示词 | ✅ 完成 |
| P1 | 上下文窗口管理 | `src/core/context.rs` — 滑动窗口截断 | ✅ 完成 |
| P2 | Discord 渠道 | — | 计划中 |
| P2 | 语音 / 多模态输入 | — | 计划中 |
| P2 | 多智能体编排 | — | 计划中 |

## 汇总

- **18 项功能中已完成 15 项**，全部经过自动化测试覆盖。
- **205 个自动化测试**覆盖所有已完成功能。
- CI 管线（GitHub Actions）在 `ubuntu-latest` + `macos-latest` 全绿。
