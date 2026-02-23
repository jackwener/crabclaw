# S7: Shell 命令执行

## 背景

启用 shell 命令执行，既作为逗号前缀快捷方式（`,git status`），也作为 LLM 可调用工具（`shell.exec`），带失败自纠正。

## 架构

```
src/core/
└── shell.rs     # /bin/sh -c 执行器 + 超时 + 输出捕获
```

## 实现

| 文件 | 功能 |
|------|------|
| `shell.rs` | `execute()` — 通过 `/bin/sh -c` 运行命令，可配置工作目录。捕获 stdout、stderr、exit code。30 秒超时（`tokio::time::timeout`）。`format_output()` — 结构化输出显示 |
| `router.rs` | 未知 `,` 命令 → `shell::execute()`。失败时：stderr + exit code 包装为 `<command>` XML 上下文发给模型自纠正 |
| `registry.rs` | `shell.exec` 工具 — LLM 可通过 `{command: "..."}` 参数自主调用 |

## 关键设计决策

- **超时保护**：30 秒硬超时防止失控进程（如 `yes` 或死循环）
- **失败即上下文**：失败的命令不是错误——是 LLM 的学习机会
- **工作目录**：命令在工作区目录执行，而非 CrabClaw 二进制所在目录

## 验证

- 成功执行：echo、pwd、多命令
- Stderr 捕获：写入 stderr 的命令
- 超时：长时间运行的命令 30 秒后被终止
- 失败包装：非零 exit code → XML 上下文格式
