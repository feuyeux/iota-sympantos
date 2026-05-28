# UT 标准化设计：内联测试模块分离

> Archive note: this is a historical design spec. For current behavior and commands, see [../../iota book.md](../../iota%20book.md), [../../architecture.md](../../architecture.md), and [../../command.md](../../command.md).

## 目标

将 iota-core 和 iota-cli 中所有内联测试模块 (`mod tests {}`) 提取到独立的 `*_tests.rs` 文件中。

## 标准化模式

### Rust

**源文件 (e.g., `foo.rs`)**:
```rust
// 文件底部
#[cfg(test)]
#[path = "foo_tests.rs"]
mod tests;
```

**测试文件 (e.g., `foo_tests.rs`)**:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    // 测试内容保持不变
}
```

**命名规范**: `原文件名_tests.rs` (下划线分隔)

### TypeScript

已符合标准，仅 `*.test.ts` 文件，无需修改。

## 批次分组

| Batch | 模块 | 内联测试文件数 |
|-------|------|---------------|
| 1 | `crates/iota-core/src/kanban/*` | 9 个 |
| 2 | `crates/iota-core/src/acp/*` | 8 个 |
| 3 | `crates/iota-core/src/mcp/*` | 3 个 |
| 4 | `crates/iota-core/src/daemon/*` | 3 个 |
| 5 | `crates/iota-core/src/store/*` | 3 个 |
| 6 | `crates/iota-core/src/skill/*` | 2 个 |
| 7 | `crates/iota-core/src/memory/*` | 2 个 |
| 8 | `crates/iota-core/src/context, utils, config, runtime_event, engine` | 7 个 |
| 9 | `crates/iota-cli/src/tui/*` | 8 个 |
| 10 | `crates/iota-cli/src/cli/*` | 4 个 |

## 执行规则

1. 每批次作为独立 PR
2. 仅移动测试代码，不修改实现逻辑
3. 保持 `use super::*;` 引用
4. 保留原有测试函数签名和行为
5. 提交信息格式: `refactor: extract inline tests from {module} to {file}_tests.rs`
