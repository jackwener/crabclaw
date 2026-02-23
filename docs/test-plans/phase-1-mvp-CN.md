# CrabClaw 测试计划

## 元数据

- 范围：CrabClaw 智能代码工具链全覆盖测试
- 日期：2026-02-23
- 状态：活跃
- 相关文档：`docs/architecture.zh-CN.md`, `README.md`

## 测试策略

- **单元测试** (`cargo test --lib`)：核心逻辑、数据映射、纯函数。
- **CLI 集成测试** (`tests/cli_run.rs`)：真实二进制的端到端 CLI 行为。
- **Telegram 集成测试** (`tests/telegram_integration.rs`)：通过 `process_message` 的全链路管线（用 `mockito` mock LLM API）。
- **CI**：`cargo fmt --check` + `cargo clippy -D warnings` + 全部测试套件在 `ubuntu-latest` 和 `macos-latest` 上运行。

## 测试矩阵

### 阶段 1：核心基础（单元 + CLI 集成）

| ID | 领域 | 类型 | 场景 | 状态 |
|---|---|---|---|---|
| TP-001 | 配置 | 单元 | 从 `.env.local`、环境变量、CLI 参数加载 | ✅ |
| TP-002 | 配置 | 单元 | 缺少 API key 返回结构化错误 | ✅ |
| TP-003 | CLI 输入 | 集成 | 通过 CLI flag 传入 prompt | ✅ |
| TP-004 | CLI 输入 | 集成 | 通过 stdin 传入 prompt | ✅ |
| TP-005 | CLI 输入 | 集成 | 从文件加载 prompt | ✅ |
| TP-006 | 请求 | 单元 | 序列化 ChatRequest 为 JSON | ✅ |
| TP-007 | 响应 | 单元 | 反序列化 ChatResponse | ✅ |
| TP-008 | 错误 | 单元 | HTTP 401 → 认证错误 | ✅ |
| TP-009 | 错误 | 单元 | HTTP 5xx → API 错误 | ✅ |
| TP-010 | 会话 | 集成 | Tape 跨运行持久化 | ✅ |
| TP-011 | 会话 | 集成 | Reset 命令清空 tape | ✅ |
| TP-012 | 日志 | 集成 | `RUST_LOG=debug` 输出生命周期日志 | ✅ |

### 阶段 2：路由 + Tape + 工具（单元）

| ID | 领域 | 类型 | 场景 | 状态 |
|---|---|---|---|---|
| TP-013 | 路由 | 单元 | 逗号命令路由到内部处理器 | ✅ |
| TP-014 | 路由 | 单元 | 未知逗号命令 → shell 执行 | ✅ |
| TP-015 | 路由 | 单元 | 自然语言 → enter_model=true | ✅ |
| TP-016 | 路由 | 单元 | 失败命令 fallback 到 model | ✅ |
| TP-017 | Tape | 单元 | 追加、读取、搜索、锚点 | ✅ |
| TP-018 | Tape | 单元 | 基于锚点的上下文截断 | ✅ |
| TP-019 | 工具 | 单元 | Registry 注册、列表、查询、获取 | ✅ |
| TP-020 | 工具 | 单元 | 执行 shell.exec, file.read/write/list/search | ✅ |
| TP-021 | Skills | 单元 | 发现 .agent/skills, 解析 frontmatter | ✅ |
| TP-022 | 文件 | 单元 | 路径穿越 / 沙箱强制执行 | ✅ |
| TP-023 | 上下文 | 单元 | 滑动窗口截断 | ✅ |
| TP-024 | 上下文 | 单元 | 模块化系统提示词组装 | ✅ |

### 阶段 3：Telegram 端到端集成（Mock LLM）

| ID | 领域 | 类型 | 场景 | 状态 |
|---|---|---|---|---|
| TP-025 | Telegram | 集成 | OpenAI 文本回复 | ✅ |
| TP-026 | Telegram | 集成 | Anthropic 文本回复 | ✅ |
| TP-027 | Telegram | 集成 | 逗号命令绕过模型 | ✅ |
| TP-028 | Telegram | 集成 | 空模型响应（不崩溃） | ✅ |
| TP-029 | Telegram | 集成 | API 500 错误 → 用户错误提示 | ✅ |
| TP-030 | Telegram | 集成 | HTTP 429 限流 | ✅ |
| TP-031 | Telegram | 集成 | 多轮会话持久化 | ✅ |
| TP-032 | Telegram | 集成 | OpenAI 工具调用循环 | ✅ |
| TP-033 | Telegram | 集成 | Anthropic tool_use → tool_result → 最终回复 | ✅ |
| TP-034 | Telegram | 集成 | Anthropic shell.exec 真实执行 | ✅ |
| TP-035 | Telegram | 集成 | Anthropic 多工具（2 个 tool_use 块） | ✅ |
| TP-036 | Telegram | 集成 | 最大迭代中断（不挂起） | ✅ |
| TP-037 | Telegram | 集成 | 系统提示词包含 identity + tools 段 | ✅ |
| TP-038 | Telegram | 集成 | 工作区 .agent/system-prompt.md 覆盖 | ✅ |
| TP-039 | Telegram | 集成 | file.write → file.read 管线 | ✅ |
| TP-040 | Telegram | 集成 | 未知工具名 → 错误恢复 | ✅ |
| TP-041 | Telegram | 集成 | 空输入忽略 | ✅ |
| TP-042 | Telegram | 集成 | 工具循环中 API 错误 | ✅ |

## 当前统计

- **自动化测试总数**：205（177 单元 + 10 CLI + 18 Telegram）
- **CI 管线**：GitHub Actions，push/PR 到 `main` 时触发
- **全部测试通过**：✅
