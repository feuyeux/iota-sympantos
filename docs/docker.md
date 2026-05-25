# Docker 容器化方案与最佳实践

`iota-sympantos` 提供了一套成熟且生产就绪的 Docker 容器化编排方案，旨在简化环境依赖并自动集成全套分布式可观测性遥测组件（OpenTelemetry Collector, Jaeger, Prometheus, Loki, Grafana）。

---

## 1. 架构优势与设计

*   **多阶段构建（Dockerfile）**：采用官方 `rust:1.81-slim-bookworm` 进行安全高效的构建，使用 Cargo 挂载缓存，可大幅缩短二次编译时长。
*   **富运行时（Debian-Slim）**：运行时基于轻量级的 `debian:bookworm-slim`，内置完整的 `nodejs` 与 `npm/npx` 运行环境（解决 AI 编程助手后端如 `claude-code`, `codex` 的环境依赖），且提供 `git`、`ca-certificates` 与 `sqlite3` 调试命令行。
*   **极速冷启动（Daemon 编排）**：`iota-daemon` 作为容器中的常驻服务，通过桥接网络暴露 `47661` 端口，能提供极速的 ACP 后端多进程复用池。
*   **全栈可观测性**：`iota-daemon` 默认将 trace 和 metric 事件汇聚并流式推送到容器内的 `otel-collector`，实现透明的可观测性跟踪。

---

## 2. 快速启动编排

在项目根目录下，使用以下命令即可一键构建并拉起整个 `iota-daemon` 以及可观测性监控栈：

```bash
# 启动所有服务（在后台运行）
docker compose -f docker/docker-compose.yml up -d --build
```

### 拓扑与服务端口说明

启动后，下列服务将在宿主机或容器网络中可用：

| 服务名称 | 暴露端口 | 职责说明 | 默认登录凭证 / URL |
| :--- | :--- | :--- | :--- |
| **iota-daemon** | `47661` | Iota 常驻核心进程池，处理 ACP 后端编排 | `127.0.0.1:47661` |
| **jaeger** | `16686` | 查询 Trace 分布式调用链与瀑布图 | `http://localhost:16686` |
| **prometheus** | `9090` | 抓取本地 Token 消耗与状态直方图指标 | `http://localhost:9090` |
| **loki** | `3100` | 高并发集中日志检索组件 | `http://localhost:3100` |
| **grafana** | `3000` | 可观测性大屏看板，已默认导入数据源 | `http://localhost:3000` (admin/admin) |

---

## 3. 关键数据卷挂载与持久化

方案中配置了两个极其重要的数据卷挂载，它们确保了状态的连贯性：

1.  **`iota-config` (命名卷) $\rightarrow$ `/home/iota/.i6`**
    *   **职责**：存储 iota 的唯一配置文件 `nimia.yaml`，以及 SQLite 本地数据库（包含 `memory.sqlite` 长期记忆桶、`events.sqlite` token 可观测性明细、`approvals.sqlite` 工具授权历史）。
    *   **自动初始化**：如果挂载的卷中不存在 `nimia.yaml`，容器在启动时会通过 `entrypoint.sh` **自动使用 `nimia.yaml.template` 初始化一份默认配置**，避免报错退出的同时，提示用户填写 API 密钥。

2.  **`../` (宿主机项目根目录) $\rightarrow$ `/workspace`**
    *   **职责**：将开发工作区挂载到容器中。AI 助手进行的所有代码修改、git 提交都会实时且忠实地投影在您的宿主机文件系统上。

---

## 4. 协同工作流（最佳实践）

### 场景 A：宿主机客户端 $\rightarrow$ 容器内 Daemon（极速零冷启动）

您无需进入容器，在宿主机上即可享受秒级响应。宿主机的命令行客户端可以直接复用容器内长期存活的 `iota-daemon`：

1.  **宿主机环境配置**：
    在您的终端中设置环境变量：
    ```bash
    # Windows PowerShell
    $env:IOTA_DAEMON_ADDR="127.0.0.1:47661"
    
    # macOS/Linux Bash
    export IOTA_DAEMON_ADDR="127.0.0.1:47661"
    ```
2.  **触发执行**：
    在宿主机运行命令：
    ```bash
    iota run codex "用三句话写一首关于宇宙的诗"
    ```
    此时，宿主机 CLI 会在毫秒级内通过 `47661` 端口连接容器内的 daemon 引擎。容器内的 `EnginePool` 会快速调度已有 ACP 实例返回响应，并且其 Trace 完美发送给容器的 Jaeger 和 Loki。

---

### 场景 B：在容器内交互式运行 TUI 终端

如果您期望使用 iota 的全功能多行输入、Ctrl+R 历史搜索、Markdown 渲染及 Approval 授权的 TUI 终端：

```bash
docker exec -it iota-daemon iota
```
> [!NOTE]
> 必须加上 `-it`（分配交互式伪终端），否则 ratatui 后端将无法捕获输入与输出。

---

## 5. 运维与常见问题排查

### 权限与 UID/GID 冲突

容器内的默认用户为 `iota`（UID 1000, GID 1000）。
如果您在 Linux 宿主机上运行，且宿主机的当前用户 UID 不是 1000，可能导致容器内的 git 后端无法读取或写入挂载的 `/workspace`（提示 Permission Denied）。

*   **解决方案**：
    在 `docker/docker-compose.yml` 中为 `iota-daemon` 指定 `user` 字段覆盖，或者在启动前确保宿主机项目目录对 UID 1000 可读写：
    ```bash
    chown -R 1000:1000 .
    ```

### 查看本地配置文件路径

要查看命名卷在宿主机上的实际路径或直接修改容器内的 `nimia.yaml`：

```bash
# 拷贝容器内的配置文件进行快速修改
docker cp iota-daemon:/home/iota/.i6/nimia.yaml ./nimia.yaml
# 修改后覆写回去
docker cp ./nimia.yaml iota-daemon:/home/iota/.i6/nimia.yaml
```
