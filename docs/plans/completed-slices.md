# CrabClaw 已完成切片汇总 (S0–S4)

## S0: 项目骨架 + 配置基线

**目标**：建立 library+CLI 架构，实现确定性配置优先级。

| 文件 | 功能 |
|------|------|
| `src/config.rs` | `.env.local` + 环境变量 + CLI flag 三级优先级配置加载 |
| `src/input.rs` | `--prompt` / `--prompt-file` / stdin 三种输入方式 |
| `src/error.rs` | `thiserror` 结构化错误分类 |
| `src/cli.rs` | `clap` CLI 解析 + `run` 子命令 + `--dry-run` |
| `src/main.rs` | `tracing-subscriber` 结构化日志 |

---

## S1: 请求执行管线

**目标**：能够实际发送 HTTP 请求到 OpenAI 兼容 API。

| 文件 | 功能 |
|------|------|
| `src/api_types.rs` | `ChatRequest` / `ChatResponse` / `Message` 类型化模型 |
| `src/client.rs` | `reqwest` HTTP 请求发送，状态码分类错误处理 |

---

## S2: 命令路由 + Tape 会话

**目标**：实现 bub 的核心交互模型——逗号命令路由 + 会话录制。

| 文件 | 功能 |
|------|------|
| `src/command.rs` | 逗号前缀命令解析器，shell-like tokenizer，KV 参数解析 |
| `src/router.rs` | 输入路由：命令直接执行，NL → model，失败命令 fallback |
| `src/tape.rs` | JSONL append-only tape store + anchor 语义边界 |

---

## S3: 交互式 REPL + 多轮对话

**目标**：支持交互式对话和上下文连续。

| 文件 | 功能 |
|------|------|
| `src/repl.rs` | `rustyline` REPL + 历史持久化 + 优雅中断 |
| `src/context.rs` | Tape → messages 上下文构建器 |

---

## S4: Tool 注册 + Skill 发现

**目标**：实现工具注册表和 skill 文件发现。

| 文件 | 功能 |
|------|------|
| `src/tools.rs` | BTreeMap 工具注册表 + builtin 预注册 |
| `src/skills.rs` | `.agent/skills/*/SKILL.md` 发现 + YAML frontmatter |

---

## 测试汇总

- **S0–S4 总计**：105 tests（95 unit + 10 integration）
- 所有 `cargo fmt` / `clippy -D warnings` / `cargo test` 通过
