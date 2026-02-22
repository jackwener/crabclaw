# Bub 对齐待办

- 负责人：PM
- 最后更新：2026-02-21
- 对齐目标：与 Bub 达到功能级和设计级一致性，而非逐行翻译。

## 对齐规则

1. 优先保证用户可见行为的一致性。
2. 保留 Bub 的设计原则：确定性路由、显式命令边界、可检查状态。
3. Rust 实现可以在内部有所不同，只要行为和架构意图保持等价。

## 功能矩阵

| 优先级 | Bub 能力 | Bub 参考 | CrabClaw 计划 | 状态 |
|---|---|---|---|---|
| P0 | 配置加载与确定性优先级 | `src/bub/config/settings.py` | `src/config.rs` + 测试 (`TP-001`,`TP-002`) | ✅ 完成 |
| P0 | 非交互式消息执行模式 | `src/bub/cli/app.py` (`run`) | `run --prompt/--prompt-file/stdin` + 测试 (`TP-003`,`TP-004`,`TP-005`) | ✅ 完成 |
| P0 | 结构化错误分类基线 | `src/bub/core/router.py` + docs | `src/error.rs` 基础分类 | ✅ 完成 |
| P1 | 确定性命令边界（逗号前缀） | `src/bub/core/command_detector.py` + `tests/test_router.py` | Rust router 模块 + 一致性测试 | ✅ 完成 |
| P1 | 命令执行 fallback-to-model 行为 | `src/bub/core/router.py` | router result blocks 和 failure context | ✅ 完成 |
| P1 | Tape-first 会话上下文（anchors/handoff） | `src/bub/tape/service.py` | append-only 本地 tape + anchor API | ✅ 完成 |
| P2 | 统一 tool + skill 注册视图 | `src/bub/tools/registry.py` + skills loader | registry 和 progressive tool view | ✅ 完成 |
| P2 | Channel 集成（Telegram/Discord） | `src/bub/channels/*` | CLI 一致性达成后的可选适配器 | 🔲 计划中 |

## 当前切片（S0–S4 均已完成）

1. ✅ 构建与 ADR 0001 对齐的 Rust `library+CLI` 骨架。
2. ✅ 实现 P0 配置优先级和输入模式。
3. ✅ 建立并验证 CI-ready 命令的测试基线。
4. ✅ 发布第一份 Reviewer 报告（parity gaps）。
5. ✅ 实现命令路由、tape 会话、REPL、多轮对话。
6. ✅ 实现 tool 注册和 skill 发现。

## S0 退出标准

1. ✅ 标记为"进行中"的 P0 项可运行并有自动化测试。
2. ✅ `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings` 和 `cargo test` 全部通过。
3. ✅ Reviewer 在 `docs/reviews/` 中发布了第一份一致性报告。
