# 统一 token Observability：5 个 ACP Backend 的耗费对比与实现要点

摘要
本次实验（exp04）在 [iota-sympantos](https://github.com/feuyeux/iota-sympantos) 上实现了从 ACP usage 到本地 observability 的闭环链路，按 execution 级别归一化并持久化 token 使用数据。本文给出 5 个 backend（claude-code、codex、gemini、hermes、opencode）的耗费统计、对比结论、关键实现技术与复现步骤，便于工程团队与研究者快速理解与复现。

---

## 一、核心结论（简要）
- 最大消耗：claude-code（normalized_total ≈ 28,044）——含大量 cache write/read。  
- 仅 provider total（无法拆分）的 backend：codex（provider total ≈ 23,202）。  
- 最低消耗：gemini（normalized_total ≈ 15,207）。  
- opencode 与 hermes 接近（≈19.1k–19.3k），opencode 含显式 thinking token。  
- 实验数据全部来自归一化后的 `iota observability`，`--show-native` 仅作 parser 对照。

---

## 二、实验概况
- Prompt：请用一句话介绍 Rust 语言的主要特点。  
- 运行：5 个 backend × 3 轮（共 15 条 execution）  
- 数据来源：以 `iota observability` 的归一化事件为准（详见实现与计划文档）  
- 复现命令（示例）：
```bash
cargo build --release
PROMPT="请用一句话介绍 Rust 语言的主要特点。"
for backend in claude-code codex gemini hermes opencode; do
  for run in 1 2 3; do
    ./target/release/iota run --no-daemon --backend "$backend" "$PROMPT"
    sleep 2
  done
done

./target/release/iota observability tokens recent --limit 20 --json
./target/release/iota observability tokens summary --since 1h --json
./target/release/iota observability metrics --prometheus
```

---

## 三、主要统计（mean ± std）

| Backend | normalized_total | provider_total | input | cache_read | cache_write | output | thinking |
|---|---:|---:|---:|---:|---:|---:|---:|
| claude-code | 28044.0 ± 17.3 | 28044.0 | 277.0 ± 0.0 | 16223.3 ± 14049.8 | 11472.0 ± 14047.8 | 71.7 ± 12.4 | N/A |
| codex | N/A | 23202.0 ± 9.5 | N/A | N/A | N/A | N/A | N/A |
| gemini | 15207.0 ± 6.2 | 15207.0 | 15175.0 ± 5.2 | N/A | N/A | 32.0 ± 1.7 | N/A |
| hermes | 19100.7 ± 3.8 | 19100.7 | 19030.3 ± 4.2 | 12306.0 ± 10657.3 | N/A | 70.3 ± 0.6 | 0.0 ± 0.0 |
| opencode | 19302.3 ± 4.9 | 19302.3 | 19236.3 ± 0.6 | N/A | N/A | 36.7 ± 5.0 | 29.3 ± 7.0 |

说明：
- “N/A” 表示该 backend 未上报可拆分字段（例如 codex 只上报 adapter-level total）。
- 表中数字摘要来源于本次 exp04 的 observability 聚合（原始数据见项目日志）。

---

## 四、关键观察与解读
- cache 行为影响大：claude-code 在不同 run 展示了显著的 cache 写入与后续命中（导致同一 prompt 的 total 波动）。  
- Adapter-only 的限制：codex 仅上报 `usage_update.used`（adapter total），无法计算 normalized_total，因此只能在 provider_total 口径下比较。  
- thinking token：opencode 将 reasoning/thinking 作为单独字段，这对 normalized_total 有可观影响。  
- 去重策略必要：部分 backend 会同时产生 adapter-level usage 与 final usage，需按 execution_id 选择“final”事件以避免重复统计。  
- 口径透明：报告同时保留 `provider_reported_total` 与 `normalized_total`，并在表格/图表中明确标注口径差别。

---

## 五、实现要点（工程概览）
1. 采集层：监听 ACP JSON-RPC（prompt result、session update、session complete），提取不同来源的 usage。  
2. 归一化：新增 `TokenUsageNormalizer`，把 provider/adapter 的原始字段映射到统一 schema：
   - `input_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`, `output_tokens`, `thinking_tokens`, `tool_use_prompt_tokens`, `provider_reported_total_tokens`, `normalized_total_tokens`, `raw_payload`。  
3. 持久化：SQLite 表 `token_usage_events`（记录 raw_payload 与所有拆分字段），支持按 `execution_id` 回溯与按 backend 聚合。  
4. 查询与展示：新增 CLI 子命令 `iota observability tokens recent/summary/export`、`metrics --prometheus`；TUI（ratatui）从归一化事件读取并在状态栏、pager 中展示。  
5. 测试：为 OpenAI/Anthropic/Gemini/Adapter-only 编写 parser 与持久化单元测试，确保稳定解析口径。

更多实现细节可参考：
- 设计文档：docs/superpowers/specs/2026-05-17-exp04-token-stats-design.md
- 实现计划：docs/superpowers/plans/2026-05-17-exp04-token-stats.md
- 实验记录与结果：gefsi/exp04-token-stats.md

---

## 六、给工程团队的建议
- 报表中始终同时呈现 `provider_reported_total` 与 `normalized_total`，并注记“口径说明”；对仅提供 adapter total 的 backend（如 codex）在表中高亮 N/A。  
- 将 token 聚合导入 Prometheus/Grafana 做长期趋势监控，并关联 cache 命中率以诊断消耗波动来源。  
- 鼓励 adapter 提供更细粒度的 usage 字段（cache write/read、reasoning tokens），便于跨 provider 的更公平对比。  
- 对于高方差的 backend（如 claude-code 的 cache 行为），应在报告中加入 run-level 原始事件示例以便审计。

---

## 七、复现与命令摘要
构建：
```bash
cargo build --release
```
执行（示例）：
```bash
PROMPT="请用一句话介绍 Rust 语言的主要特点。"
for backend in claude-code codex gemini hermes opencode; do
  for run in 1 2 3; do
    ./target/release/iota run --no-daemon --backend "$backend" "$PROMPT"
    sleep 2
  done
done
```
查询：
```bash
./target/release/iota observability tokens recent --limit 20 --json
./target/release/iota observability tokens summary --since 1h --json
./target/release/iota observability metrics --prometheus
```

---

## 八、AI Coding 纪要

1. 用 `Claude Code` + `Claude Sonnet 4.5` 制定规划

```
/superpowers:brainstorming 在gefsi中创建第4个实验的文档：对5个backend 进行token统计 输入 tokens + 缓存输入 tokens + 输出 tokens 的实验 目标：多次输入指定query 每个backend的token消耗是否稳定 谁消耗的多
```

产物：

- `docs/superpowers/specs/2026-05-17-exp04-token-stats-design.md`
- `docs/superpowers/plans/2026-05-17-exp04-token-stats.md`

2 用 `Claude Code` + `Minimax 2.7` 执行任务

产物：

- `gefsi/exp04-token-stats.md`

3 用 `Codex` + `gpt-5.5` 解决代码待实现功能

产物：

- `src/store/observability.rs`

4 用 `Copilot` + `Auto模式` 善后 包括cli和store的改进

产物：

- `src/cli/`
- `src/store/`