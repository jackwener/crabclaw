# CrabClaw Phase 1 MVP 测试方案

## 元数据

- 范围：Phase 1 MVP 基线（兼容 OpenClaw 的 CLI）
- 日期：2026-02-21
- 状态：草案
- 相关文档：`README.md`、`docs/adr/0001-cli-only-vs-library-plus-cli.md`

## 验收标准

1. 配置优先级是确定性的且有文档记录。
2. CLI 能够从直接参数、stdin 和文件输入执行 prompt。
3. 请求 payload 和响应映射通过类型化模型验证。
4. 错误处理能区分 config、network、auth 和 API 故障。
5. 可选的会话持久化可以被启用和显式重置。
6. 调试日志可通过 `RUST_LOG` 启用，无需修改代码。
7. Phase 1 行为通过核心路径的自动化测试覆盖。

## 测试策略

- 单元测试：验证纯逻辑和数据映射。
- 集成测试：使用 mock HTTP 验证端到端 CLI 行为。
- 回归检查：保留配置优先级和错误输出的 golden 行为。
- 非功能检查：lint、格式化和编译时检查。

## 测试矩阵

| ID | 领域 | 类型 | 场景 | 预期结果 |
|---|---|---|---|---|
| TP-001 | 配置 | 单元 | 从 `.env.local`、环境变量和 CLI flags 加载值 | 最终配置遵循定义的优先级 |
| TP-002 | 配置 | 单元 | 缺少必需的 API key | 返回结构化配置错误 |
| TP-003 | CLI 输入 | 集成 | 通过 CLI flag 提供 prompt | 请求以预期的 prompt 内容发送 |
| TP-004 | CLI 输入 | 集成 | 通过 stdin 提供 prompt | 请求以 stdin 内容发送 |
| TP-005 | CLI 输入 | 集成 | 从文件加载 prompt | 请求以文件内容发送 |
| TP-006 | 请求映射 | 单元 | 序列化请求模型 | JSON 结构符合 API 契约 |
| TP-007 | 响应映射 | 单元 | 反序列化成功响应 | 类型化模型值被正确填充 |
| TP-008 | 错误映射 | 单元 | HTTP 401 响应 | 返回 auth 类别错误 |
| TP-009 | 错误映射 | 单元 | HTTP 5xx 响应 | 返回 API 类别错误 |
| TP-010 | 会话 | 集成 | 两次运行之间启用会话持久化 | 第二次运行加载先前的上下文 |
| TP-011 | 会话 | 集成 | 显式 reset 命令/flag | 存储的会话被清除 |
| TP-012 | 日志 | 集成 | `RUST_LOG=debug` | 请求生命周期的 debug 日志被输出 |

## 工具和命令

- 格式检查：`cargo fmt --check`
- Lint 检查：`cargo clippy --all-targets --all-features -- -D warnings`
- 测试执行：`cargo test`

## 退出标准

1. 本方案中的所有测试已实现或明确推迟并附带理由。
2. `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings` 和 `cargo test` 全部通过。
3. 面向用户的行为变更已反映在 `README.md` 中。
4. 影响架构的决策已在 ADR 中记录。

## 推迟项策略

如果任何计划中的测试被推迟：

- 在变更说明或 commit message 中记录原因。
- 开启一个后续任务，关联到被推迟的测试 ID。
