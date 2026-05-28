# iota sympantos

Cross-platform Rust CLI/TUI，将 prompt 路由到五个 ACP 后端（claude-code / codex / gemini / hermes / opencode），共享统一的记忆、技能与上下文层。内置 Kanban 任务看板，支持 Agent 长期任务的调度、追踪与多节点同步。

## 核心功能

| 功能 | 说明 |
| :------| :------|
| **跨后端记忆** | SQLite 存储（SHA-256 去重、FTS5、6 召回桶），任一后端写入的记忆可在其他后端召回注入 |
| **确定性技能** | YAML 声明，由 Rust 引擎分发；触发匹配与输出模板与后端无关 |
| **iota-fun** | 7 语言片段运行器（C++ / TypeScript / Rust / Zig / Java / Python / Go），含编译缓存与 `parallel: true` |
| **Kanban** | 内置任务看板：状态机、Dispatcher、Shadow 工作区、Event Sourcing、HTTP 同步 |
| **Daemon 热路径** | 可选 TCP daemon 保持 ACP 客户端预热，`--daemon/-d` 路由 |
| **TUI** | ratatui 内联视图，多行编辑器、Markdown 渲染、流式输出、Ctrl+C 双击退出 |

## 快速开始

```bash
rustup install 1.95.0
rustup default 1.95.0 && rustup toolchain uninstall stable
cargo build -p iota-cli -p iota-core -p iota-kanban

iota                                    # 交互式 TUI
iota run codex "ping"                   # 单次 prompt
iota run --backend claude "解释递归"    # 指定后端
iota check                              # 检查后端配置
```

## 开发

```bash
cargo test               # 运行全部测试
cargo check --offline
RUST_LOG=debug cargo run -p iota-cli --quiet
cargo run -p iota-cli --quiet -- run codex "ping" --timing

# 启动桌面端开发模式 (Tauri) 
# npm install -D @tauri-apps/cli@latest
## ubuntu
# sudo apt-get update && sudo apt-get install -y libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev
cd crates/iota-desktop && npm run dev:clean
```

### 配置文件

`~/.i6/nimia.yaml`，每个后端的关键字段：

```yaml
codex:
  enabled: true
  acp:
    command: npx
    args: ["-y", "@zed-industries/codex-acp@0.12.0"]
  model:
    provider: ninerouter
    name: gh/gpt-5.4
    base_url: http://localhost:20128/v1
    api_key: "<router-api-key>"
```

`iota check` 查看所有后端生效配置。

### Hermes 后端

```bash
pip install 'hermes-agent[acp]'
```
  
## 文档

| 文档 | 说明 |
| :------| :------|
| [`docs/iota book.md`](docs/iota%20book.md) | **《iota 技术指南》（iota book）** —— 面向程序员与 AI 从业者的系统化核心设计与实现指南 |
| [`docs/architecture.md`](docs/architecture.md) | 系统架构设计 |
| [`docs/code-call-chains.md`](docs/code-call-chains.md) | 代码调用链路 |
| [`docs/observability.md`](docs/observability.md) | logs / trace / metrics |
| [`docs/debugging.md`](docs/debugging.md) | 调试指南 |

---

- `nimia`  词源：*μνημεία*
- `iota` 词源：*ιώτα*
- `sýmpantos` 词源：*σύμπαντος*
- `gefsi` 词源： `γεύση`

https://v2.tauri.app/release/
