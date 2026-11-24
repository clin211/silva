# Silva 可观测性

当前实现覆盖 API、数据库适配器、会话维护 Worker，以及数据库中发布、采集和 Outbox 的队列快照。发布构建、外部来源采集和 Outbox 消费尚未实现；空队列不能作为这些业务链路可用的证据。

## 启动与入口

在 `server/` 运行：

```sh
make observability-up
make observability-check
make observability-smoke
```

`observability-up` 启动 PostgreSQL、迁移、API、Worker 和观测组件。普通 `make up` 保持原有开发启动方式；观测栈通过 `docker-compose.observability.yaml` 单独启用。初次运行需要下载镜像和编译 Rust。

告警接收与冒烟校验由 Workspace 内的 `silva-xtask` Rust 工具实现，随项目执行格式、Clippy 和测试检查。卷权限初始化使用 BusyBox Shell，观测栈不依赖 Python。告警接收器使用 Dockerfile 的 `observability-tools` 阶段构建，业务运行镜像只包含 API、Worker 和 CLI。

Makefile 保留原有入口，也可直接执行：

```sh
cargo run --locked -p silva-xtask -- smoke
cargo run --locked -p silva-xtask -- fault-smoke
cargo run --locked -p silva-xtask -- alert-receiver --bind 127.0.0.1:9094
```

冒烟校验可通过 `SILVA_SMOKE_API`、`SILVA_SMOKE_PROMETHEUS`、`SILVA_SMOKE_GRAFANA` 覆盖默认地址；故障演练读取 `SILVA_SMOKE_API` 和 `SILVA_SMOKE_RECEIVER`（默认 `http://127.0.0.1:9094`），操作的仍是本仓库 Compose 定义的本地服务。

| 入口 | 地址 | 用途 |
| --- | --- | --- |
| Grafana（日常统一入口） | http://localhost:3001 | 总览、专项看板、告警、日志和链路查询 |
| Prometheus | http://localhost:9090 | 指标、抓取状态和规则状态 |
| Alertmanager | http://localhost:9093 | 当前告警、分组与静默 |
| 本地通知记录 | http://localhost:9094/events | 最近 100 次告警及恢复通知，重启接收器后清空 |
| OTLP/HTTP | http://localhost:4318 | 宿主机进程发送日志和链路 |
| API 指标 | http://localhost:8088/metrics | Prometheus 文本指标 |

Grafana 本地默认用户名 `admin`，密码 `silva-observability-local`；首次启动可用 `GRAFANA_ADMIN_PASSWORD` 覆盖。已有 Grafana 数据卷的密码通过 Grafana 管理功能修改，单独改变环境变量不会重置已有账户。脚本通过环境变量读取密码，不把密码作为命令参数。

五张看板位于 Silva 文件夹：总览、API、数据库、Worker、发布与采集。它们和数据源由仓库配置自动创建。默认首页为 Silva 总览，也可直接访问 http://localhost:3001/d/silva-overview 。已有用户或组织若自行设置了首页偏好，需在 Grafana 中清除该偏好或选择总览。

日常排障从总览开始：确认 API 就绪、数据库连接、Worker 最近成功时间、业务采集新鲜度及告警，再进入专项看板。顶部导航保留所选时间范围；点击 API 请求量、P95、P99 曲线数据点可选择「查看该路由日志」，携带路由模板与时间范围进入 Loki。其他指标面板可查看同时间范围的服务日志；数据库面板关联调用数据库的 API 日志，Worker 数据库问题从 Worker 日志排查。日志 `trace_id` 可跳转 Tempo 链路，链路可跳转关联日志。

总览包含监控目标抓取状态、Collector 导出失败速率和队列使用率、通知投递失败速率。顶部卡片与表格查询所选时间范围终点的即时值；无业务流量时错误率和延迟可能无值，无数据不等于正常。Collector 某些失败计数器仅在发生失败后出现，无值时需结合抓取状态与其他导出指标排查。队列总览仅展示采集成功且一分钟内更新的快照，过期值不展示；发布、采集和 Outbox 消费仍未实现，空队列不代表链路就绪。

顶部「告警规则」在 Grafana 内查看 Prometheus 规则与状态；「告警静默」选择外部 `Alertmanager` 管理静默。规则仍由 `alerts.yaml` 管理，通知策略、接收器和模板仍由 `alertmanager.yaml` 管理，不另外创建 Grafana 告警规则。Prometheus、Alertmanager 原生页面用于底层诊断，本地通知记录用于检查 Webhook 投递。

首次应用统一入口配置，在 `server/` 执行以下命令重新创建 Grafana，再运行 smoke；无需重建业务服务：

```sh
docker compose -f docker-compose.yaml -f docker-compose.observability.yaml --profile app --profile observability up -d --no-deps grafana
make observability-smoke
```

所有发布到宿主机的管理端口只绑定回环地址。Worker 指标只在容器内网的 `worker:9091/metrics` 提供，避免占用宿主机端口。Loki 和 Tempo 也只在容器内网访问。

停止完整栈并保留数据：

```sh
make observability-down
```

此命令同时停止本地 API、Worker 和数据库，保留所有命名卷。不要给 `down` 添加 `--volumes`，除非明确需要删除本地数据。

## 数据流

```text
API / Worker / CLI
  ├─ tracing JSON ──────────────── 标准输出
  ├─ OTLP 日志 ── Collector ────── Loki
  └─ OTLP 链路 ── Collector ────── Tempo

API / Worker / PostgreSQL exporter / Blackbox / 观测组件
  └─ /metrics ── Prometheus ── 告警规则 ── Alertmanager ── 本地接收器
                         └──────────────── Grafana
```

Collector 使用内存限制、批量发送、有限重试和持久队列。SDK 使用后台批量发送，退出时 flush；Collector 不可用时业务请求继续执行，遥测可能丢失，JSON 标准输出仍保留。SDK 导出器自身日志不再次发送到同一个 OTLP 日志管道，避免递归失败。后端及观测组件的 Docker JSON 日志按每个文件 10MB、最多 3 个文件轮转。

Prometheus 保留 15 天、最多 2GB 的时间序列块；Loki 和 Tempo 配置 7 天保留期。WAL、索引、压缩和删除具有额外空间与延迟，时间保留期不是磁盘容量硬上限。应用到生产时根据实际流量设置存储预算和磁盘监控。

## 埋点与口径

- HTTP 响应包含服务端生成的 UUIDv7 `X-Request-Id`，以及 32 位十六进制 W3C `X-Trace-Id`。两者含义不同。只接受标准 `traceparent` / `tracestate`，不传播客户端 baggage，不采信客户端请求 ID。
- 请求根 span 自动关联用例和数据库适配器 span。日志等级改变不会关闭业务 span；`RUST_LOG=warn` 时错误仍有请求上下文。
- 指标标签只使用路由模板、固定方法枚举、状态码、有限任务类型和错误分类。未知路由统一 `unmatched`，自定义 HTTP 方法统一 `OTHER`。用户 ID、request ID、trace ID、原始 URL、查询参数不作为指标标签。
- 密码、access / refresh token、JWT 签名密钥、请求体、数据库连接串不进入埋点。`#[instrument]` 显式使用 `skip_all`；SQLx 原始语句日志关闭。
- HTTP 延迟使用 Histogram，口径从请求中间件进入到响应头生成，不包含响应体传输。`traffic=application` 为业务流量；健康检查、OpenAPI、metrics 使用 `traffic=operations`，从业务错误率和延迟告警中排除。
- 数据库操作延迟包含适配器调用中的连接等待、SQL 和事务，不冒充单条 SQL 耗时。获取连接指标的 `transaction` 包含 BEGIN 往返。PostgreSQL exporter 提供数据库自身状态。Linux 容器额外提供进程 CPU、RSS、文件描述符等指标；macOS 宿主机不提供 Linux 进程采集器。
- Worker 指标包括执行结果、耗时、删除会话数量、执行周期和最近成功时间；监听可用不能替代任务成功心跳。
- 业务队列每 15 秒读取一次数据库，单次最多 3 秒。`/metrics` 本身不进行数据库 I/O。失败时 `silva_business_metrics_up=0`，保留旧队列值与最后成功时间。读取队列前检查采集成功及时间新鲜度。
- 发布 pending 为 desired generation 与 active generation 不同；等待年龄追溯 active 之后、desired 以内最早提交的任务，避免连续提交掩盖长时间未上线。超过五分钟时 `silva_publish_delayed=1`。激活目标后归零。
- 采集积压只包含 queued/running；等待时间以 started_at 或 UTC slot_date 起点估算，尚非精确排队时间。Outbox 积压为未设置 processed_at 的事件。

主要指标包括：

| 范围 | 指标 |
| --- | --- |
| HTTP | `silva_http_requests_total`、`silva_http_request_duration_seconds`、`silva_http_requests_in_flight` |
| 数据库 | `silva_db_acquire_duration_seconds`、`silva_db_operation_duration_seconds`、`silva_db_errors_total`、`silva_db_pool_*` |
| Worker | `silva_worker_runs_total`、`silva_worker_run_duration_seconds`、`silva_worker_last_success_timestamp_seconds` |
| 业务状态 | `silva_business_metrics_up`、`silva_business_metrics_last_success_timestamp_seconds`、`silva_queue_pending`、`silva_queue_oldest_age_seconds`、`silva_publish_delayed` |

新增发布、采集和 Outbox 消费者时，同步实现任务执行结果、重试耗尽、构建耗时、提交到激活耗时和来源新鲜度。API 生产任务时通过 `telemetry::inject_context()` 保存 W3C 上下文，Worker 使用 `set_parent()` 关联消费 span；长时间等待或多次消费可采用 span link。业务事件、任务 ID 和 generation 放在日志/span 属性中，不能放指标标签。当前未增加无消费者的任务追踪字段或虚构成功率。

## 配置

| 变量 | 含义 |
| --- | --- |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP/HTTP 基础地址，自动追加 `/v1/traces`、`/v1/logs`；未设置则不导出对应信号 |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` | 单独覆盖链路完整地址，包含 `/v1/traces` |
| `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` | 单独覆盖日志完整地址，包含 `/v1/logs` |
| `OTEL_EXPORTER_OTLP_HEADERS` | 导出鉴权，按 SDK 标准环境格式提供；不写入仓库 |
| `OTEL_TRACES_SAMPLER_ARG` | 根链路采样比例 `0..1`，默认 1；父链路采样决定会被继承 |
| `SILVA_WORKER_METRICS_BIND` | 宿主机默认 `127.0.0.1:9091`；容器覆盖为 `0.0.0.0:9091` |
| `RUST_LOG` | JSON / OTLP 日志过滤；不影响业务 span 的创建 |

代码固定使用 OTLP/HTTP protobuf；不支持通过环境变量切换到 gRPC。宿主机启动 API/Worker 时设置 `OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318`。容器覆盖文件自动注入内网地址；CLI 如需导出，需单独设置端点。

## 告警与排障

规则在 `alerts.yaml`，行为测试在 `alerts.test.yaml`。当前 14 条规则覆盖抓取失败/目标消失、API readiness、业务 5xx 和 P95、数据库不可连接/获取超时、Worker 停滞/连续失败、业务指标陈旧、发布延迟、遥测导出失败/队列饱和、通知投递失败。

错误率和延迟告警要求五分钟内至少 100 个业务请求，持续五分钟超标才触发。Worker 停滞从进程启动或上次成功执行起计算，超过三个配置周期并持续一分钟触发。各阈值是初始配置，需按真实流量调整。

| 现象 / 告警 | 排查顺序 | 恢复条件 |
| --- | --- | --- |
| `SilvaTargetDown` / `SilvaTargetsMissing` | Prometheus Targets → Compose 状态 → 监听和网络 | 目标出现且 up=1 |
| `SilvaReadinessFailed` / `SilvaDatabaseDown` | 数据库容器 → 连接配置 → 迁移版本 → API 日志 | pg_up=1 且 readiness 返回 200 |
| `SilvaHighErrorRate` / `SilvaHighLatency` | API 看板定位路由 → Loki 错误 → Tempo 子链路 → 数据库看板 | 五分钟窗口指标回到阈值内 |
| `SilvaDatabaseAcquireTimeout` | 连接池 → 长事务/慢操作 → 数据库连接上限 | 五分钟没有新增超时 |
| `SilvaWorkerStalled` / `SilvaWorkerFailures` | Worker 看板 → Worker 日志 → 数据库可用性 | 心跳恢复，错误窗口清空 |
| `SilvaBusinessMetricsStale` | 先停止使用队列旧值判断业务 → API 采集日志 → 数据库 | 采集成功且更新时间在一分钟内 |
| `SilvaPublishDelayed` | 检查 desired/active generation → 发布任务与租约 | 目标激活；当前里程碑尚无发布消费者 |
| `SilvaTelemetryExportFailed` / `SilvaTelemetryQueueFull` | Collector 日志 → Loki/Tempo 状态 → 存储和队列 | 导出失败速率归零，队列低于 80% |
| `SilvaNotificationFailed` | Alertmanager 日志 → 本地接收器 `/health` | 通知成功投递 |

本地接收器只记录通知，不向外部发送消息。接入飞书、邮件等生产渠道时，替换 `alertmanager.yaml` 中 receiver，并通过 Secret 注入渠道凭据。上线前必须实际验证外部触达和恢复通知。监控栈与业务在同一台机器时，整机断电不会依靠本机告警自行通知，需要另设独立外部可用性探测。

实际故障演练：

```sh
make observability-fault-smoke
```

此命令会短暂停止本地 `silva` 项目的 Worker 和 PostgreSQL，验证数据库断连时 metrics 仍可读，以及三类真实告警的触发、Webhook 投递和恢复。Rust 工具在校验成功、失败或收到 Ctrl+C / SIGTERM 后均尝试恢复两个服务；停止操作部分失败时也会恢复，恢复失败会明确报错。强制终止进程或机器断电时无法自动恢复，需手动启动服务。演练通常耗时两到三分钟。不要在生产或其他人正在使用的开发栈执行。

配置修改后，Prometheus / Collector / Loki / Tempo 需要重启对应组件；Grafana 看板文件自动刷新，数据源配置在 Grafana 启动时加载。镜像使用固定版本，升级时重新运行配置检查、smoke 和故障演练。生产数据库 exporter 使用独立最小权限监控角色；生产管理入口须经过鉴权，API `/metrics`、健康详情与 OpenAPI 限制在运维网络。
