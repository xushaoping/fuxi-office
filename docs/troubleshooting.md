# 故障排查

先确定故障发生在浏览器、应用、数据库、反向代理还是外部模型服务。先检查状态和经过脱敏的日志，不要把 JWT、服务密钥、数据库口令或含业务数据的请求体贴入工单。

## 快速检查

```sh
docker compose --env-file .env.production ps
docker compose --env-file .env.production logs --tail=100 app postgres nginx
curl -i http://127.0.0.1:3000/api/health
curl -i http://127.0.0.1:3000/api/ready
```

本地非 Docker 部署可直接访问应用监听地址。`/api/health` 是进程存活检查，`/api/ready` 检查数据库连接和部署基线；代理应依据 ready 状态决定是否转发流量。

## 服务未启动或容器不健康

- 查看 `app` 首条错误及退出码。启动时会先校验 `DATABASE_URL`、`JWT_SECRET`、`ADMIN_PASSWORD` 和数据库完整基线。
- `DATABASE_URL` 未设置或密码 URI 编码错误时，服务会在连接数据库时失败。Compose 内部连接地址由 `APP_DB_USER`、`DB_PASSWORD` 和服务名构造；宿主机的 localhost 地址不能用作容器间地址。
- `JWT_SECRET` 在生产模式必须是至少 32 字节的独立随机值。`ADMIN_PASSWORD` 必须满足长度要求且不能保留示例占位值。
- `BIND_ADDR` 必须是有效 `host:port`。端口已被占用或容器监听地址配置错误时，查看绑定错误和端口映射。
- 应用监听 readiness 但返回 503 时，检查数据库迁移、角色权限和基线报告。用应用运行角色执行 `./script/check-deployment-baseline.sh`；不要用提升应用权限来掩盖缺失权限。

## 数据库连接、迁移或权限

先确认 PostgreSQL 容器为 healthy、目标数据库名正确、网络可达，再用受保护的 `PGPASSFILE` 或服务配置进行连接测试。不要在 shell 历史和诊断日志中输出口令。

迁移缺失、关键索引或触发器不完整会使 `/api/ready` 失败。核对部署版本要求和迁移脚本状态，再在备份和维护窗口内由独立迁移管理员补齐。`script/bootstrap-production-db.sh` 只用于全新库，不是已有库升级工具。角色分离和审计表权限问题按[数据库权限说明](production-database-permissions.md)处理。

PostgreSQL 大版本升级前确认数据目录、备份及官方升级路径；不要只修改镜像标签。读写失败时识别实际连接角色，检查对象所有者和明确授权，不要将运行账户改成超级用户。

## 登录和权限错误

- 确认先取得最新验证码 challenge，再在浏览器完成 PoW 并提交登录。验证码是一次性的；失败后重新获取。
- 生产管理员密码由 `ADMIN_PASSWORD` 在首次初始化账户时使用。之后的密码变更应通过受支持的账户管理流程完成，修改环境变量不会自动重置现有用户密码。
- 401 通常表示缺失、过期或无效 JWT；重新登录并检查 `Authorization: Bearer ...`。
- 403 表示当前用户没有所需角色或项目权限；404 也可能用于隐藏用户无权访问的项目资源。检查成员角色及操作者是否满足独立复核规则。
- 若应用在可信代理后错误识别客户端地址，确认代理覆盖转发头、应用端口未被绕过访问，再检查 `TRUST_PROXY_HEADERS`。

## 页面空白或资源加载失败

```sh
curl -I http://127.0.0.1:3000/
curl -I http://127.0.0.1:3000/pkg/
```

检查浏览器控制台中 JS、WASM、CSS 的 404/CSP 错误，并确认部署二进制和 `target/site` 静态资产来自同一次构建。release 资产使用版本摘要绑定，缓存中的旧页面和新二进制混用可能导致资源加载失败。重新构建或部署一套完整制品，并清理反向代理/CDN 中过期缓存。

## LLM 或 Embedding 失败

- 检查服务商 API 基址、模型名称、API key 注入和服务端到提供商的网络；不要打印密钥。使用与实际服务商一致的兼容 API 测试。
- 模型配置和选择策略在服务启动时加载；更改后重启应用。核对任务级 `OPENAI_<TASK>_*` 是否覆盖了全局值，备用模型是否适用于同一服务商。
- 预算配置为空字符串、缺少模型价格估算或预算超限时，请求可能按设计失败关闭。先查管理员用量和编排详情，再核对所有主模型与 fallback 的成本配置。
- Embedding 默认关闭。启用 OpenAI-compatible 向量检索需要可用 key；也可选 `FUXI_EMBEDDING_PROVIDER=local`。检查 pgvector 是否可用及知识源索引状态；语义检索关闭时关键词检索仍可工作。

## 工作流卡住或 SSE 中断

检查网络代理是否缓冲 SSE。Nginx 应关闭响应缓冲并保留长连接；客户端设置 `Accept: text/event-stream`。从 `/api/projects/{id}/orchestrations` 查询持久化状态和步骤，必要时读取事件回放接口。服务会按心跳超时回收失联编排，默认 `AGENT_ORCHESTRATION_STALE_AFTER_SECONDS=120`。

SSE 重连前先读取工作流和编排状态，避免重复提交分析或 DSL 生成。提交更新使用最近返回的 `expectedUpdatedAt`；并发更新冲突时刷新状态。人工澄清、计划确认、Tester 复核、业务验收和版本独立审批是明确门禁，任务停在门禁阶段不等同于后台故障。

## 反向代理、HTTPS 和优雅关闭

- HTTP 入口应按部署配置重定向至 HTTPS。检查域名、证书链、私钥挂载和证书有效期。
- 默认应用端口绑定宿主机 loopback；外部流量应经过 TLS 代理。检查代理上游地址和应用健康状态，不要直接公开应用端口。
- 停机期间应用先将 readiness 置为失败，再等待代理排空和连接关闭。`SHUTDOWN_DRAIN_SECONDS` 必须小于 `SHUTDOWN_GRACE_SECONDS`，Compose 停止宽限期应大于应用总宽限期。

## 评测或 E2E 失败

先检查脚本 `--help`、必需环境变量、token 对应的项目角色、fixture 格式和应用版本。在线评测需要授权 API token；E2E 流程要求三个互不相同的创建者、复核者、发布者用户，并且必须提供业务验收用例。不要通过关闭鉴权或复用同一用户来让验收通过。

生产镜像或数据库升级的详细诊断、报告范围及隔离限制见[生产发布运行手册](production-release-runbook.md)。
