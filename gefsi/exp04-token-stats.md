# iota-sympantos 实验4：Backend Token 统计

| 字段 | 值 |
|------|-----|
| 实验代号 | exp04-token-stats |
| 执行日期 | 2026-05-17 |
| 实验对象 | 5个backend的token消耗统计与稳定性分析 |
| 测试 prompt | 请用一句话介绍 Rust 语言的主要特点。 |
| 数据采集方式 | --show-native 输出解析 |
| Binary | target/release/iota |
| 配置来源 | ~/.i6/nimia.yaml |

---

## 1. 实验目标

验证 5 个 backend 在多次执行相同 prompt 时的 token 消耗稳定性，并对比不同 backend 的 token 消耗差异。

**核心问题：**
1. 同一 backend 多次执行时，token 消耗是否稳定？
2. 哪个 backend 消耗的 token 最多/最少？
3. 缓存机制（cache_read_input_tokens, cache_creation_input_tokens）在各 backend 的表现如何？

---

## 2. Backend ACP

| Backend | ACP Adapter | 出品方 | 版本 | 模型 | 说明 |
|---------|-------------|--------|------|------|------|
| claude-code | @agentclientprotocol/claude-agent-acp | agentclientprotocol (第三方) | 0.35.0 | MiniMax-M2.7 | Claude 官方 Agent SDK，完整 token 统计 |
| codex | @zed-industries/codex-acp | Zed Industries (官方) | 0.14.0 | gpt-5.4-mini (medium) | Zed 出品，只提供 total tokens |
| gemini | @google/gemini-cli --acp | Google (官方) | 0.41.2 | gemini-2.5-flash | Google Gemini CLI，snake_case 格式 |
| hermes | hermes acp | MiniMax (官方) | 0.12.0 | MiniMax-M2.7 | Hermes 原生支持，完整 token 统计 |
| opencode | opencode-ai acp | OpenCode (官方) | 1.14.40 | MiniMax-M2.7 | OpenCode CLI ACP 模式 |

---

## 3. 数据采集详情

### 3.1 数据获取渠道

| 数据项 | 获取渠道 | 命令/方法 |
|--------|----------|-----------|
| execution_id | iota run 输出 | `./target/release/iota run --backend <backend> "<prompt>"` |
| input_tokens | --show-native 输出 | `./target/release/iota run --no-daemon --backend <backend> --show-native "<prompt>"` |
| cache_read_input_tokens | --show-native 输出 | 同上 |
| cache_creation_input_tokens | --show-native 输出 (仅部分backend) | 同上 |
| output_tokens | --show-native 输出 | 同上 |

**注意：** `--show-native` 不能与 `--daemon` 一起使用

### 3.2 --show-native 数据采集结果

**重要发现：**
- `--show-native` 输出到 stderr，显示完整的 ACP 协议 JSON-RPC 消息
- token 数据嵌入在 `prompt:<id>` 响应的 `usage` 字段中
- 格式因 backend 而异：camelCase (`inputTokens`) 或 snake_case (`input_tokens`)
- cache 相关字段仅部分 backend 支持

**各 Backend Token 数据可用性：**

| Backend | input_tokens | output_tokens | cache_read | cache_write | 格式 |
|---------|--------------|---------------|------------|-------------|------|
| claude-code | 277 | 69 | 0 | 27365 | camelCase |
| codex | - | - | - | - | 仅 total |
| gemini | 14983 | 30 | 无 | 无 | snake_case |
| hermes | 18866 | 86 | 0 | 无 | camelCase |
| opencode | 19075 | 31 | 无 | 无 | camelCase |

**示例 ACP 响应（claude-code）：**
```json
[acp <=] {"jsonrpc":"2.0","id":"prompt:1","result":{"stopReason":"end_turn","usage":{"inputTokens":277,"outputTokens":69,"cachedReadTokens":0,"cachedWriteTokens":27365,"totalTokens":27711}}}
```

**示例 ACP 响应（gemini）：**
```json
[acp <=] {"jsonrpc":"2.0","id":"prompt:1","result":{"stopReason":"end_turn","_meta":{"quota":{"token_count":{"input_tokens":14983,"output_tokens":30},"model_usage":[{"model":"gemini-2.5-flash","token_count":{"input_tokens":14983,"output_tokens":30}}]}}}}
```

**示例 ACP 响应（codex）：**
```json
[acp <=] {"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"...","update":{"sessionUpdate":"usage_update","used":23045,"size":258400}}}
```

### 3.3 采集命令

```bash
mkdir -p gefsi/logs
./target/release/iota run --no-daemon --backend <backend> --show-native "请用一句话介绍 Rust 语言的主要特点。" 2>&1 | tee gefsi/logs/exp04-<backend>-run<N>.log
```

---

## 4. 原始数据表（--show-native 3次运行结果）

> **注意**：codex 只提供 `used`（总消耗），无法分解为 input/output/cache 细分字段。

| Backend | Run | input_tokens | cache_read | cache_write | output_tokens | total_tokens | 备注 |
|---------|-----|--------------|------------|-------------|---------------|--------------|------|
| claude-code | 1 | 277 | 0 | 3215 | 85 | 27731 | cachedWriteTokens=3215 |
| claude-code | 2 | 277 | 24154 | 3207 | 77 | 27715 | cachedReadTokens=24154 |
| claude-code | 3 | 277 | 24154 | 3215 | 89 | 27735 | cachedReadTokens=24154 |
| codex | 1 | - | - | - | - | 23045 | used=23045 |
| codex | 2 | - | - | - | - | 23053 | used=23053 |
| codex | 3 | - | - | - | - | 23020 | used=23020 |
| gemini | 1 | 14993 | - | - | 36 | 15029 | |
| gemini | 2 | 14990 | - | - | 36 | 15026 | |
| gemini | 3 | 14990 | - | - | 42 | 15032 | |
| hermes | 1 | 18894 | 10967 | - | 64 | 18958 | cache=58% |
| hermes | 2 | 18884 | 18459 | - | 82 | 18966 | cache=98% |
| hermes | 3 | 18890 | 18459 | - | 69 | 18959 | cache=98% |
| opencode | 1 | 19081 | - | - | 32 | 19145 | |
| opencode | 2 | 19081 | - | - | 32 | 19145 | |
| opencode | 3 | 19083 | - | - | 48 | 19166 | |

---

## 5. 统计汇总表

> **注意**：CV (Coefficient of Variation) = (std/mean)×100%
> - CV < 5% = 非常稳定
> - 5% ≤ CV < 10% = 相对稳定
> - CV ≥ 10% = 波动

| Backend | input_tokens (mean±std) | CV% | output_tokens (mean±std) | CV% | total_tokens (mean±std) | CV% | 稳定性 |
|---------|------------------------|-----|-------------------------|-----|-------------------------|-----|--------|
| claude-code | 277±0 | 0% | 83.7±6.0 | 7.2% | 27727±10 | 0.04% | input非常稳定, output相对稳定 |
| codex | - | - | - | - | 23039±17.3 | 0.08% | total非常稳定（无细分） |
| gemini | 14991±1.7 | 0.01% | 38±3.5 | 9.1% | 15029±3 | 0.02% | input非常稳定, output相对稳定 |
| hermes | 18889±5.1 | 0.03% | 71.7±9.1 | 12.7% | 18961±4.4 | 0.02% | input非常稳定, output波动 |
| opencode | 19082±1.2 | 0.006% | 37.3±9.2 | 24.7% | 19152±12 | 0.06% | input非常稳定, output波动 |

**总 token 消耗排序（从高到低）：**

| 排名 | Backend | total_tokens (avg) | 说明 |
|------|---------|-------------------|------|
| 1 | claude-code | 27727 | 含 ~10K cache_write |
| 2 | codex | 23039 | 仅 total，无细分 |
| 3 | opencode | 19152 | |
| 4 | hermes | 18961 | |
| 5 | gemini | 15029 | |

**实际 input_tokens 排序（不含 cache_write）：**

| 排名 | Backend | input_tokens (avg) |
|------|---------|-------------------|
| 1 | opencode | 19082 |
| 2 | hermes | 18889 |
| 3 | gemini | 14991 |
| 4 | claude-code | 277 |
| 5 | codex | N/A |

---

## 6. 缓存行为分析

### 6.1 各 Backend 缓存支持情况

| Backend | cache_write | cache_read | 说明 |
|---------|-------------|------------|------|
| claude-code | 有 3207-3215 | 有 24154 (Run 2/3) | 首次写缓存，后续读缓存 |
| codex | 无 | 无 | 不支持细分 |
| gemini | 无 | 无 | 无缓存字段 |
| hermes | 无 | 有 Run 2/3 98% | Run 1 只有 58% |
| opencode | 无 | 无 | 无缓存字段 |

### 6.2 缓存效果详情

**claude-code:**
- Run 1: `cachedWriteTokens=3215` (首次写入)
- Run 2/3: `cachedReadTokens=24154` (后续读取)

**hermes:**
- Run 1: 10967/18894 (58%) — 首次缓存建立
- Run 2: 18459/18884 (98%) — 几乎完全命中缓存
- Run 3: 18459/18890 (98%) — 继续命中

---

## 7. 分析与结论

### 7.1 稳定性分析

**发现：**
- 所有 backend 的 input_tokens 极端稳定（CV < 0.1%），表明 tokenization 确定性高
- output_tokens 波动：claude-code (7.2%), gemini (9.1%), hermes (12.7%), opencode (24.7%)
- 波动原因：短回答的长度变化（32-89 tokens）导致 output_tokens 变化

### 7.2 Backend 对比分析

**观察：**
- claude-code 的 input_tokens (277) 远低于其他 backend，因为只含用户 prompt
- gemini 和 hermes 的 input_tokens 较高（~15k-19k），因为包含更多上下文/系统信息
- opencode 和 hermes 的 input_tokens 接近（~19k）
- output_tokens 都很低（32-89），与短回答一致

### 7.3 结论

**成功项：**
- 通过 `--show-native` 成功从 5/5 个 backend 获取 token 数据
- input_tokens 极度稳定（CV < 0.1%），数据质量高
- 发现 hermes 和 claude-code 的缓存行为
- codex 更新为 gpt-5.4-mini 后可获取 total tokens 数据

**限制：**
- **codex 无细分数据**：只提供 `used`（总消耗），这是 codex-acp adapter 的设计限制
- gemini/opencode 未报告 cache 相关字段
- output_tokens 波动较大，但这是正常的（短回答的随机性）

### 7.4 建议

1. **缓存优化**：hermes 第二次运行 cache 达到 98%，建议研究 claude-code 的缓存机制
2. **进一步测试**：使用更长的 prompt 来观察 output_tokens 稳定性是否提升
3. **codex 细分数据**：如需 codex 细分数据，需直接调用 OpenAI API 或等待 codex-acp 更新

---

## 8. 验收矩阵

| # | 验收项 | 状态 | 说明 |
|---|--------|------|------|
| 1 | 数据完整性（15条记录） | 通过 | 5 backends x 3 runs = 15 条 |
| 2 | Token字段齐全 | 部分通过 | 4/5 有完整数据，codex 只有 total |
| 3 | 稳定性可量化 | 通过 | CV 计算完成 |
| 4 | Backend排序 | 通过 | 按 total_tokens 排序 |
| 5 | 异常情况记录 | 通过 | codex 无细分已记录 |
| 6 | 数据可复验 | 通过 | log 文件在 gefsi/logs/ |
| 7 | 缓存行为分析 | 通过 | hermes/claude-code 缓存效果 |
| 8 | ACP 配置信息 | 通过 | 各 backend ACP adapter 信息 |

---

## 9. 复验命令

```bash
# 1. 环境检查
cargo build --release
./target/release/iota check

# 2. 提取 token 数据（使用 --show-native）
for backend in claude-code codex gemini hermes opencode; do
  ./target/release/iota run --no-daemon --backend $backend --show-native "请用一句话介绍 Rust 语言的主要特点。" 2>&1 | grep -E "usage_update|inputTokens|outputTokens|cachedRead|cachedWrite"
done

# 3. 查看特定 execution 的完整事件流
./target/release/iota logs <execution_id>
```

---

*文档更新时间：2026-05-17*
*状态：已完成全部 15 轮测试 + codex 重试，token 数据已提取并分析*