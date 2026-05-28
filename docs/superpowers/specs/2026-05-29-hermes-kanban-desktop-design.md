# Hermes Kanban Desktop 官方感 MVP 规划

## 背景

当前 iota desktop 已经具备基础 Kanban 能力：任务列表、状态筛选、dispatcher tick、任务详情、runs/events/comments 读取，以及 worker stdout/stderr tail 展示。相比 Hermes 官方 Kanban dashboard，主要缺口在于桌面端仍像“任务列表 + 详情卡片”，而不是“多列任务板 + worker lanes + run history drawer”的操作台。

本规划选择 **B. Official-Feeling MVP**：优先复刻 Hermes 官方 Kanban 的核心使用体验，但不在本阶段强行实现官方全量能力中需要更深数据模型的部分，例如 tenant、per-task max retries、max runtime、run metadata schema、orchestrator auto/manual、lane registry、stranded diagnostics。

## 目标

- 让 desktop Kanban 第一屏呈现为 Hermes 风格的六列 board：`triage`、`todo`、`ready`、`running`、`blocked`、`done`。
- 将右侧任务详情从“插入列表顶部的 detail panel”升级为 drawer 风格信息面板。
- 在 `running` 列提供 `Lanes by profile` 视图，按 `assignee/profile` 分组展示 worker lane。
- 将 `Dispatch` 文案调整为更贴近官方的 `Nudge dispatcher`，并显示最近 tick 结果。
- 在 drawer 中集中展示官方关键审计信息：run history、comments、links/dependencies、events、worker logs。
- 提供基础人工操作：创建任务、编辑任务核心字段、添加评论、unblock blocked task、常用状态推进。
- 保持当前 Rust store 和 dispatcher 语义稳定，不引入需要迁移的大规模 schema 变更。

## 非目标

- 不实现 Hermes 官方完整 orchestration：`Auto/Manual`、`Decompose`、`Specify` 暂不进入本阶段。
- 不实现 tenant、多 board 物理隔离、per-board SQLite 切换；当前仍使用 iota 的 `~/.i6/kanban/iota.db`。
- 不实现外部 CLI worker lane registry；本阶段仍以 Hermes profile lane 为默认执行模型。
- 不实现完整 `hermes kanban tail/watch/runs` CLI 等价命令，只在 desktop drawer 内提供等效可视化读取。
- 不在本阶段改变 worker spawn 协议，除非修复明确 bug。

## 官方能力映射

| Hermes 官方能力 | Desktop MVP 处理 |
| :-- | :-- |
| Six columns board | 实现为横向可滚动六列 board |
| Nudge dispatcher | `Dispatch` 改名并增强反馈 |
| Lanes by profile | `running` 列按 `task.assignee ?? "default"` 分组 |
| Flat view | 提供 `Lanes by profile` toggle，关闭后 running 扁平展示 |
| Card drawer | 点击任务打开右侧 drawer，不再复制显示任务卡片 |
| Run History | 使用现有 `runs[]` 展示每次 attempt |
| Events / tail / watch | 使用 `events[]` 和 log tail 展示近似能力 |
| Comments | drawer 支持查看和添加评论 |
| Dependencies | 使用现有 `links[]` 展示 parent/blocks/related |
| Unblock | blocked task 支持转回 `ready` |
| Review-required convention | 识别 `blocked` + comment/reason 文案，但不新增专用状态 |
| Structured handoff metadata | 现 store 只有 `output_summary`，metadata 作为后续 schema 项 |

## 信息架构

### 顶部控制条

- 标题：`Hermes Kanban`
- 状态副文案：最近 dispatcher tick 摘要，例如 `1 spawned · 0 failed · 1 active worker`
- 搜索框：按 title/body/tag/assignee 本地过滤
- Board 选择：当前阶段显示 board slug 筛选，不实现物理切库
- Assignee 筛选：从任务中动态聚合 assignee/profile
- `Lanes by profile` toggle
- `Refresh`
- `Nudge dispatcher`
- `New task`

### Board 主体

六列固定状态：

- `Triage`：原始想法或未细化任务
- `Todo`：等待依赖或尚未 ready
- `Ready`：可被 dispatcher 领取
- `Running`：worker 正在执行
- `Blocked`：需要人工输入或失败熔断
- `Done`：已完成

列头显示数量。每张卡片显示：

- `#id`
- board slug
- title
- body 前两到三行
- assignee/profile badge
- priority badge
- tags
- claimed/run 指示：running task 显示 active run/profile

### Running Lanes

当 `Lanes by profile` 开启：

- `running` 列内按 assignee 分组。
- 空 assignee 使用 `default`。
- 每个 lane 显示 active task 数量。
- 该能力只影响视觉分组，不改变 dispatcher 调度逻辑。

当关闭：

- `running` 列按 `claimed_at` 或 `updated_at` 扁平排序。

### Task Drawer

Drawer 分为以下区域：

- Header：title、status badge、board、assignee、priority、close
- Quick actions：`Move to ready`、`Mark done`、`Block`、`Archive`，按当前状态显示合法操作
- Body：任务描述、tags、workspace info
- Dependencies：parent/blocks/related links
- Run History：runs 按时间倒序，展示 status、profile、started/finished、exit code、summary
- Comments：评论列表和 add comment 输入
- Events：最近相关 events，可展开 payload
- Worker Logs：stdout/stderr tail，显示路径和内容

## API 变更

优先复用现有 Tauri commands：

- `list_boards`
- `list_tasks`
- `dispatch_kanban`
- `get_kanban_task_detail`
- `create_task`
- `transition_task`
- `add_comment`

需要补充的 commands：

- `update_kanban_task(task_id, patch)`：编辑 title/body/assignee/priority/tags/status。
- `create_kanban_link(from_id, to_id, kind)`：创建 parent/blocks/related link。
- `remove_kanban_link(from_id, to_id, kind)`：删除 link。
- 可选：`delete_kanban_task(task_id)`，仅作为后续安全操作，不放在 MVP 首屏。

## 状态操作规则

Desktop 必须遵守 `state_machine.rs` 的合法状态迁移：

- `triage -> todo`
- `todo -> ready`
- `ready -> running`
- `running -> done`
- `running -> blocked`
- `running -> ready`
- `blocked -> ready`
- `blocked -> done`
- `done -> archived`

UI 不展示非法按钮。若后端仍返回非法迁移错误，drawer 用内联错误提示，不吞掉错误。

## 数据与兼容性

- 本阶段不改 `iota-kanban` 数据表结构。
- run metadata 只展示现有 `output_summary`、`exit_code`、`status`、时间字段。
- worker log 继续从 `~/.i6/kanban/<task_id>.stdout.log` 和 `~/.i6/kanban/<task_id>.stderr.log` tail。
- `assignee` 视作 Hermes profile lane 名称；没有 assignee 时使用 `default`。
- `links` 语义延续现有 `LinkKind::{Parent, Blocks, Related}`。

## 分期实施

### Phase 1：Board 视图与 Drawer 骨架

- 将 `KanbanWorkspace` 从单列表改为六列 board。
- 保留现有状态筛选，但改为列头和全局筛选共同工作。
- 点击任务打开右侧 drawer。
- 展开任务不再在列表中重复显示。
- `Dispatch` 改名为 `Nudge dispatcher`，显示最近 tick 时间和结果。

验收：

- 六列在 desktop 宽屏下同时可见，窄宽度横向滚动。
- 当前截图中的重复任务问题不再出现。
- `Ready 0 / Running 1` 时点击 nudge 能看到 `No ready tasks` 反馈。

### Phase 2：Lanes by Profile 与过滤

- 增加 `Lanes by profile` toggle。
- `running` 列按 assignee/profile 分组。
- 增加 search、assignee、board 过滤。
- 过滤不改变任务状态，仅影响可见任务。

验收：

- 多个 running task 能按 `backend-dev`、`reviewer`、`default` 等 lane 分组。
- 关闭 toggle 后 running task 回到扁平列表。
- 搜索 title/body/tag/assignee 均有效。

### Phase 3：Drawer 审计信息补全

- Run History 展示全部 runs，而不是只展示前三条。
- Events 支持按 run/task 展开 payload。
- Logs 区分 stdout/stderr，并显示 tail 截断提示。
- Comments 支持添加。
- Links/dependencies 展示更接近 parent/child/blocker 的文案。

验收：

- 运行中任务 drawer 能看到 active run 和日志增长。
- 完成/失败/blocked 的任务能看到历史 run 列表。
- 添加评论后无需重启即可刷新显示。

### Phase 4：人工操作与任务创建

- `New task` 表单支持 title/body/status/assignee/priority/tags。
- Drawer 支持合法状态按钮：ready、done、blocked、archive、unblock。
- 支持编辑核心字段。
- 支持创建/删除 links。

验收：

- blocked task 可以从 drawer unblock 到 ready。
- 新建 ready task 后点击 nudge 可触发 dispatcher。
- 非法状态迁移按钮不会出现。

### Phase 5：后续官方 parity 路线

这些不进入 MVP，但作为后续迭代项记录：

- Orchestration：Auto/Manual、Decompose、Specify。
- Worker lane diagnostics：unknown assignee、stranded ready task、spawn failures 聚合。
- Retry policy UI：per-task max retries、failure count、gave_up 语义。
- Runtime policy UI：per-task max runtime、claim TTL、heartbeat timeout。
- Structured handoff：run metadata JSON、review-required convention 的专用呈现。
- Multi-board isolation：按官方 board root / per-board DB 模式扩展。
- `tail/watch/runs` 等 CLI 对应的 live stream 面板。

## 测试计划

- TypeScript：新增纯函数测试，覆盖任务分列、lane 分组、过滤、dispatch 文案。
- Rust：补 Tauri command 单元测试或集成测试，覆盖 `update_kanban_task`、link commands、log tail 缺文件场景。
- Manual：启动 `npm run dev:clean`，用真实 desktop 验证：
  - ready task nudge 后进入 running。
  - running task drawer 显示 run/log。
  - blocked task unblock 后回 ready。
  - lanes toggle 和 filters 行为正确。

## 风险

- 当前 `Run` 缺少官方的 metadata/log_path/outcome 细分字段，drawer 只能近似展示官方 run history。
- worker logs 当前按 task id 命名，多次 run 会覆盖同一 task 的日志文件；如要保留 per-run logs，需要后续 schema 和 worker 改造。
- `transition_task` 的合法迁移较严格，UI 需要避免提供官方 dashboard 中更自由的拖拽状态修改。
- Orchestration 需要 Hermes profile 和辅助模型配置，目前不适合塞进 desktop MVP。

## 成功标准

桌面端打开 Kanban 后，用户无需看 CLI 就能完成以下流程：

1. 查看六列 board 和各状态数量。
2. 按 assignee/profile 观察 running worker lanes。
3. 点击 `Nudge dispatcher` 并理解是否领取了任务。
4. 打开任务 drawer，查看 run history、events、comments、logs。
5. 对 blocked task 添加评论并 unblock。
6. 新建 ready task，观察 dispatcher 将其推进到 running。

