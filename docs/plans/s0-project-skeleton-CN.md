# S0: 项目骨架 + 配置基线

## 背景

以 Rust `library+CLI` 架构引导 CrabClaw 项目，实现确定性配置优先级，对齐 ADR 0001。

## 架构

```
src/
├── core/
│   ├── config.rs    # 三级配置：.env.local → 环境变量 → CLI 参数
│   ├── error.rs     # thiserror 结构化错误分类
│   └── input.rs     # --prompt / --prompt-file / stdin 输入标准化
├── channels/
│   └── cli.rs       # clap CLI：run 子命令
└── main.rs          # tracing-subscriber 结构化日志
```

## 实现

| 文件 | 功能 |
|------|------|
| `config.rs` | 解析 `OPENROUTER_API_KEY`、`API_BASE`、`MODEL`、`SYSTEM_PROMPT`，`.env.local` → env → CLI 优先级 |
| `error.rs` | `CrabClawError` 枚举：`Config`、`Network`、`Auth`、`Api`、`RateLimit` |
| `input.rs` | 统一 `--prompt "..."`、`--prompt-file path`、stdin pipe 为单一 `String` |
| `cli.rs` | `clap` derive 风格 CLI + `run` 子命令 |
| `main.rs` | `tracing_subscriber::fmt` + `RUST_LOG` 环境变量过滤 |

## 验证

- 配置优先级：所有优先级组合的单元测试
- 输入模式：flag、file、stdin 的集成测试
- 错误分类：每种错误变体的单元测试
