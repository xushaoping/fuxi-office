# 配置参考

应用通过环境变量读取配置。仓库提供了两份模板：本地开发使用 [`.env.example`](../.env.example)，生产部署使用 [`.env.production.example`](../.env.production.example)。复制模板后替换其中的密码和密钥；不要把真实凭据提交到版本库。

本地运行时，应用会从工作目录中的 `.env` 加载变量。Docker Compose 使用 `--env-file` 读取部署变量，例如：

```bash
cp .env.example .env
docker compose --env-file .env up -d --build
```

生产部署请复制为独立的 `.env.production`，替换所有占位值，并按生产部署文档检查数据库迁移和角色权限：

```bash
cp .env.production.example .env.production
docker compose --env-file .env.production -f docker-compose.prebuilt.yml config --quiet
```

## 必需配置

| 变量 | 说明 |
| --- | --- |
| `DATABASE_URL` | PostgreSQL 连接串。直接运行应用时必需；Docker Compose 会根据 `APP_DB_USER` 和 `DB_PASSWORD` 为应用构造容器内连接串。密码中的特殊字符需要 URL 编码。 |
| `JWT_SECRET` | JWT 签名密钥。生产环境应设置为独立生成的随机值，至少 32 字节。 |
| `ADMIN_PASSWORD` | 首次初始化管理员账户使用的密码，长度须为 6 到 1024 字节；生产环境不得使用示例占位值。 |

使用 Docker Compose 初始化数据库时还需要设置：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `DB_ADMIN_PASSWORD` | 无 | PostgreSQL 管理角色密码，仅用于数据库服务初始化。 |
| `DB_PASSWORD` | 无 | 应用运行时数据库角色密码。应与管理员密码分开设置。 |
| `APP_DB_USER` | `fuxi` | 应用运行时数据库角色。 |
| `APP_DB_OWNER` | `fuxi_owner` | 数据库对象所有者角色，默认不可登录。 |

已有数据库的数据目录不会因修改初始化环境变量而自动更改角色或密码。升级现有实例前，请先备份并按[生产数据库权限说明](production-database-permissions.md)操作。

## 服务与安全

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `APP_ENV` | 未设置 | 环境标识。设为 `production` 或 `prod` 时启用生产凭据校验。 |
| `BIND_ADDR` | `127.0.0.1:3000` | 应用监听地址。容器部署时 Compose 将容器内地址设为 `0.0.0.0:3000`。 |
| `APP_PORT` | `3000` | Compose 发布到宿主机的应用端口；默认只绑定 loopback。 |
| `TRUST_PROXY_HEADERS` | `false`（应用） | 仅当应用只能通过可信反向代理访问时设为 `true`。生产 Compose 模板默认启用。 |
| `CAPTCHA_POW_DIFFICULTY` | `14` | 登录工作量证明基础难度。应用接受 8 到 20 范围内的值；超出范围时会限制到该范围。 |
| `SHUTDOWN_GRACE_SECONDS` | `30` | 优雅停止总等待秒数，最大为 300。 |
| `SHUTDOWN_DRAIN_SECONDS` | `2` | 收到停止信号后，在关闭 HTTP 监听前等待代理排空请求的秒数，最大为 30，且必须小于 `SHUTDOWN_GRACE_SECONDS`。 |
| `RUST_LOG` | Compose 中为 `info` | Rust 日志过滤配置，主要用于容器运行。 |

生产环境中，Compose 将应用端口限制为宿主机 loopback。请通过配置好的 TLS 反向代理对外提供访问，并只在代理可信且无法被绕过时启用 `TRUST_PROXY_HEADERS`。

## 决策与后台任务

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `DECISION_COPILOT_ENABLED` | `true` | 控制只读决策运营 Copilot 查询功能。设为 `false` 可关闭。 |
| `REQUIRE_HISTORICAL_REGRESSION_BEFORE_PUBLISH` | `true` | 有历史基线的版本发布前是否必须通过历史回归。仅建议在紧急兼容场景中关闭。 |
| `OPTIMIZATION_AUTODRAFT_MODE` | `off` | 自动优化草稿模式：`off`、`shadow` 或 `draft`。启用 `shadow` 或 `draft` 时必须设置项目白名单。 |
| `OPTIMIZATION_AUTODRAFT_PROJECT_IDS` | 未设置 | 允许自动优化的项目 ID，使用逗号分隔。Compose 模板需要显式调整才能传入该值。草稿模式只创建低风险草稿，不会自动审批或发布。 |
| `AGENT_ORCHESTRATION_STALE_AFTER_SECONDS` | `120` | 判定 Agent 编排失联的心跳超时，允许范围为 30 到 86400 秒。 |
| `DRIFT_WEBHOOK_ALLOWED_HOSTS` | 未设置 | 漂移事件 Webhook 的精确主机名白名单，多个主机名用逗号分隔。 |
| `DRIFT_WEBHOOK_SIGNING_SECRET` | 未设置 | Webhook 签名密钥，至少 32 字节。应使用私密随机值。 |

## LLM 与知识检索

LLM 配置是可选的。未配置 API 密钥时，涉及模型调用的功能不可用；不启用语义检索时，知识检索使用关键词方式。

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `OPENAI_API_KEY` | 空 | OpenAI 兼容服务的 API 密钥。 |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | 聊天/响应模型接口基址。 |
| `OPENAI_MODEL` | `gpt-4o-mini` | 默认模型。可按任务使用 `OPENAI_ANALYSIS_MODEL`、`OPENAI_PLAN_MODEL`、`OPENAI_DSL_MODEL` 和 `OPENAI_CHANGE_INTENT_MODEL` 覆盖。 |
| `EMBEDDING_ENABLED` | `false` | 是否启用语义向量检索。 |
| `FUXI_EMBEDDING_ENABLED` | 回退到 `EMBEDDING_ENABLED` | 应用专用开关；设置后优先于通用开关。 |
| `FUXI_EMBEDDING_PROVIDER` | `openai-compatible` | 向量提供方：`openai-compatible` 或 `local`。`local` 使用确定性的本地特征哈希，不请求外部嵌入服务。 |
| `OPENAI_EMBEDDING_MODEL` | `text-embedding-3-large` | 向量模型名称。 |
| `OPENAI_API_BASE` | 回退到 `OPENAI_BASE_URL` | 嵌入接口基址。 |

可选模型控制项：

| 变量 | 用途 |
| --- | --- |
| `OPENAI_FALLBACK_MODELS` | 默认模型失败时依次尝试的备用模型，最多三个；可使用 `OPENAI_ANALYSIS_FALLBACK_MODELS` 等任务级变量覆盖。 |
| `OPENAI_MODEL_SELECTION_POLICY` | 基于已记录模型表现选择模型的全局策略；任务级 `OPENAI_<TASK>_MODEL_SELECTION_POLICY` 优先。 |
| `OPENAI_MODEL_CANDIDATES_JSON` | 模型候选数组，可提供模型名称、估算成本和成功率等选择信息。 |
| `OPENAI_MODEL_MAX_CONCURRENCY` | 每个模型允许的并发 HTTP 请求数。 |
| `OPENAI_MAX_COMPLETION_TOKENS` | 每次模型响应请求的最大输出 token 数。 |
| `OPENAI_REQUEST_OPTIONS_JSON` | 发送给兼容服务的额外请求选项；可按任务设置 `OPENAI_<TASK>_REQUEST_OPTIONS_JSON`。 |
| `OPENAI_OPTIMIZATION_MODEL` | 优化建议专用模型；未设置时使用计划任务模型。 |

任务名 `<TASK>` 使用 `ANALYSIS`、`PLAN`、`DSL` 或 `CHANGE_INTENT`。模型选择在应用启动时完成；修改相关模型或策略后需重启服务。

项目级实验变量使用 `OPENAI_<TASK>_EXPERIMENT_NAME`、`OPENAI_<TASK>_EXPERIMENT_MODEL` 和可选的 `OPENAI_<TASK>_EXPERIMENT_TREATMENT_PERCENT`。实验只用于符合条件的非高风险任务，处理比例默认 50%；名称应在各任务间保持唯一。分析任务还支持 `OPENAI_ANALYSIS_SIMPLE_MODEL`、`OPENAI_ANALYSIS_COMPLEX_MODEL`，DSL 任务支持 `OPENAI_DSL_SIMPLE_MODEL`。

## 模型用量限制

以下变量均为可选。未设置时不启用对应限制。成本限制依据配置的模型估价计算，不等同于服务商账单；需要为所有可能选中的主模型和备用模型提供价格估算。

| 变量 | 作用范围 |
| --- | --- |
| `OPENAI_MAX_ESTIMATED_CALL_COST_USD` | 单次模型请求估算成本上限。 |
| `OPENAI_MAX_PROJECT_DAILY_TOKENS` | 每个项目每日 token 上限。 |
| `OPENAI_MAX_PROJECT_DAILY_ESTIMATED_COST_USD` | 每个项目每日估算响应成本上限。 |
| `OPENAI_MAX_ORCHESTRATION_ESTIMATED_COST_USD` | 一棵 Agent 编排及其子任务的估算成本上限，包含运行中的预留成本。 |
| `OPENAI_MAX_ORCHESTRATION_OBSERVED_TOKENS` | Agent 编排累计观察到的 prompt 与 completion token 门槛；在用量返回前，单个并发任务仍可能超过该值。 |

对于 A/B 实验、成本与 token 限制、模型候选 JSON 和各任务的请求选项，请参考 `.env.example` 中相应变量旁的注释。Docker Compose 模板显式声明了可传入应用容器的配置；若新增变量没有被 Compose 声明，需要同步调整 Compose 文件，否则宿主机 `.env` 中的值不会自动进入应用容器。

## 构建配置

下列变量用于 Docker 镜像构建，不是应用运行时配置：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `APP_PLATFORM` | Compose 默认使用 `linux/amd64` | 目标容器平台。ARM 主机可按目标环境设为 `linux/arm64`。 |
| `FUXI_IMAGE` | `fuxi-dmn:local` | Compose 使用的应用镜像名。 |
| `RUST_IMAGE` | `rust:1.90-slim-bookworm` | 构建阶段 Rust 基础镜像。 |
| `BINARYEN_VERSION` | `132` | 构建 WASM 前端所用 Binaryen 版本。 |
| `BINARYEN_ARCHIVE`、`BINARYEN_DOWNLOAD_URL`、`BINARYEN_SHA256` | 官方校验下载 | 自定义 Binaryen 归档或 URL 时必须提供相应 SHA256。 |
| `APT_UPDATE_TIMEOUT`、`APT_INSTALL_TIMEOUT`、`APT_NETWORK_TIMEOUT`、`APT_DOWNLOAD_RETRIES` | `180`、`600`、`20`、`2` | Docker 构建时 APT 更新和安装的网络超时、重试设置。 |

完整的发行部署步骤见[生产发布运行手册](production-release-runbook.md)。
