# 生产发布与恢复操作手册

本文对应当前生产部署模板、099 数据库权限迁移及发布制品检查器。命令供操作人员在选定环境审核后执行；编写本文没有连接目标库、执行迁移、调用模型或发布服务。功能代码、隔离测试和完整生产验收应分别留存证据，不能凭本手册判定整体验收通过。

## 1. 冻结候选与配置

先确定本次发布版本，并将 `Cargo.toml` package version 与 `v<version>` Git tag 统一。不得复用或移动已有 tag；版本号应高于上一发布 tag。将本次全部源码、迁移、测试、CI 和文档纳入同一个可复现的 Git 提交。在干净检出中构建；记录提交、构建工具版本、镜像 ID/仓库摘要及测试报告。工作区有未提交或未跟踪交付文件时，只能生成诊断制品。

发布门禁必须同时核对 Cargo 元数据版本、选定的发布版本、tag 指向的提交及工作区清洁状态。CI 和完整镜像 smoke 通过后，再为已经验收的提交创建不可变版本 tag；若需要修订，使用新的版本/tag。当前仓库尚未选择下一发布版本，不能仅凭 Cargo 文件或最近 tag 推断候选版本。

CI 对 `v*` tag push 会运行 `check-release-candidate.py --require-tag`，检查 Cargo 版本与 tag 一致、版本高于既有稳定 tag、tag 指向当前提交且 checkout 中所有交付文件均已提交。此 job 只有在真实 tag push 时触发；分支 CI 通过不代表版本候选已验收。

选定版本并更新 `Cargo.toml` 后，在干净候选提交上运行版本递增和来源检查；CI 与镜像验收后创建 tag，再运行 tag 绑定检查：

```sh
python3 script/check-release-candidate.py --version 0.2.0
# 完成候选验收并为该提交创建不可变 v0.2.0 tag 后：
python3 script/check-release-candidate.py --version 0.2.0 --require-tag
```

示例版本仅说明参数格式，不是对本项目下一版本的指定。检查器不会改写 Cargo 版本、创建/移动 tag 或提交工作区；任一源码、迁移、CI 或文档变动都须先提交，再重新构建和验收。

从 [生产配置示例](../.env.production.example) 复制私有配置文件，替换全部凭证占位值并限制文件权限。管理员数据库密码 `DB_ADMIN_PASSWORD`、运行账户密码 `DB_PASSWORD`、应用 JWT 密钥和应用管理员密码分别配置。不要把真实密码写进命令行、共享日志或版本库；数据库工具优先使用受保护的 `PGPASSFILE`/`.pgpass`，需要 `DB_PASSWORD` 的初始化脚本通过安全环境注入读取它。关闭 shell 跟踪，避免打印完整 `docker compose config` 或容器环境。

三份入口为 [默认 Compose](../docker-compose.yml)、[预构建 Compose](../docker-compose.prebuilt.yml) 和 [多实例 Compose](../docker-compose.multi.yml)。例如，只解析预构建配置而不启动：

```sh
docker compose --env-file .env.production -f docker-compose.prebuilt.yml config --quiet
```

该检查仅证明配置可解析，还需在隔离部署核对声明启用的功能实际收到所需参数。预算等可选数值不使用时保持未设置；空字符串会触发校验失败。`DATABASE_URL` 的密码必须正确进行 URI 编码；Compose 内部连接串直接拼接 `DB_PASSWORD`，因此该值应使用 URL 安全字符。

当前默认范围如下：

| 能力 | 默认配置及放行前补充证据 |
| --- | --- |
| 生产错误脱敏 | `APP_ENV=production`；客户端不返回底层数据库诊断 |
| 语义 Embedding | `EMBEDDING_ENABLED=false`；设置 `FUXI_EMBEDDING_ENABLED` 会覆盖它。关闭时使用关键词检索 |
| 自动草稿 | `OPTIMIZATION_AUTODRAFT_MODE=off`；`shadow`/`draft` 需明确项目白名单。当前由候选模拟触发，不包含后台自动发现/调度 |
| 外部 LLM | 示例密钥为空，未配置供应商时不能验收真实生成能力；按需配置任务模型、预算、并发和回退链 |
| 漂移 Webhook | 未提供白名单和签名密钥时不可启用；启用前完成目标域名、验签、重试及关闭演练 |
| 运维 Copilot | 默认开启，可用 `DECISION_COPILOT_ENABLED=false` 关闭；这不代表真实用户意图准确率已验收 |

## 2. 三类数据库角色

| 角色 | 默认名称 | 职责 |
| --- | --- | --- |
| 独立管理员 | `postgres` | 初始化、经审查的角色/所有权迁移与恢复；不用于应用连接 |
| 对象所有者 | `fuxi_owner` | `NOLOGIN`，持有应用数据库和对象；管理员按需 `SET ROLE` 执行迁移 |
| 应用运行账户 | `fuxi` | `LOGIN`，无超级用户、建库、建角色、复制、绕过 RLS 或角色成员资格，不拥有业务对象 |

[099 权限迁移](../script/099_production_database_privileges.sql) 提供显式业务权限；分区维护与日统计刷新通过固定 `search_path`、受控所有者的 `SECURITY DEFINER` 入口执行。应用不能直接调用分区内部入口，也不能通过授予超级用户或对象所有权解决维护失败。

099 将 `audit_events` 和 `knowledge_audit_events` 设为追加式证据：应用只可读取、插入，更新、删除、截断受限，并有触发器保护。迁移去除两表原有外键，以免业务对象删除时级联删除或 `SET NULL` 改写证据。因此审计行可以保留已删除项目、版本、知识源或用户的原始引用；查询和恢复核对不能把这些引用误认成待清理脏数据。提示词发布事件、解释审阅也保持相应的追加权限边界。

## 3. 全新库初始化

[bootstrap-production-db.sh](../script/bootstrap-production-db.sh) **仅适用于全新库及全新应用/所有者角色**。发现已有核心表或同名应用/所有者角色会拒绝执行；它不承担旧库升级、角色复用或静默降权。

Docker 官方入口首次初始化空数据卷时先创建 `POSTGRES_DB`，再调用该脚本。三份模板使用 `pgvector/pgvector:pg18`，数据卷挂载到 `/var/lib/postgresql`。已有 PostgreSQL 卷必须先核对数据库主版本及真实数据目录，按备份恢复或经验证的大版本升级路径处理，不能仅改镜像标签或挂载目录。

原生 PostgreSQL 安装需先使用独立管理员创建空数据库。以下假定 `PGHOST`、`PGPORT`、管理员认证已通过安全环境/密码文件配置，并且当前集群确实不存在这些目标对象；`DB_PASSWORD` 由密钥管理或私有环境注入，不在命令中填写：

```sh
psql -X -U postgres -d postgres -v ON_ERROR_STOP=1 \
  -c 'CREATE DATABASE fuxi_dmn TEMPLATE template0;'
POSTGRES_DB=fuxi_dmn POSTGRES_USER=postgres \
  ./script/bootstrap-production-db.sh
```

脚本还支持环境变量 `APP_DB_USER`、`APP_DB_OWNER`，默认分别为 `fuxi`、`fuxi_owner`。初始化会创建核心结构、分区、日统计与 099 权限。原生主机需要预装与 PostgreSQL 匹配的 pgvector 才能创建扩展；扩展不可用时脚本容忍关键词降级，不能据此判定语义检索已完成验收。

## 4. 已有库升级及旧超级用户拆分

部署核心编排预算降级前，现有库需由迁移所有者应用 [100 步骤跳过状态迁移](../script/100_agent_step_skipped_status.sql)，以便 `agent_steps` 持久记录被预算门控跳过的可选 Tester。全新库初始化结构已包含该状态。目标库迁移结果须单独核验，不能用本地 Rust 测试代替。

部署编排 SSE 事件回放前，现有库需由迁移所有者应用 [101 SSE 事件迁移](../script/101_agent_orchestration_sse_events.sql)。事件表仅允许项目成员按游标读取，记录在 24 小时后过期；需将迁移结果和应用角色的表/序列权限纳入发布检查。

先暂停入口和写入任务，取得并验证备份，在还原的独立环境演练。确认旧应用角色是否拥有其他库、共享对象或角色成员资格；所有权转移可能影响当前库之外的共享对象，不能只审查业务表。

旧库先依据和实际结构确定缺失迁移，使用独立迁移管理员补齐 099 所需的业务表及前置结构。不要把 `init.sql` 当作未知旧库的通用升级器，也不要按文件名无条件重放全部历史约束。提示词和解释已有专用辅助脚本：

- [upgrade-prompt-registry.sh](../script/upgrade-prompt-registry.sh) 依赖 `DATABASE_URL`、`APP_DB_USER`，执行提示词注册表累计升级；它会实际连接并写库，没有 dry-run 参数。
- [upgrade-decision-explanations.sh](../script/upgrade-decision-explanations.sh) 支持 `--dry-run`（不连接）、`--check`（只读前置/解释结构检查）及实际升级。其迁移集合为 092、094、095、096、097；不会重放更窄的 091/093 提示词约束，且不负责 098/099 或其他底座迁移。

先查看解释升级计划而不连接数据库：

```sh
./script/upgrade-decision-explanations.sh --dry-run
```

旧应用使用超级用户或持有对象时，099 会拒绝直接执行。使用 [prepare-production-role-separation.sql](../script/prepare-production-role-separation.sql)，连接必须是与旧应用角色不同的独立超级用户。以下示例通过既有安全 libpq 配置连接所选数据库，不包含密码：

```sh
psql -X -U postgres -d fuxi_dmn -v ON_ERROR_STOP=1 \
  -v app_user=fuxi -v owner_user=fuxi_owner \
  -f script/prepare-production-role-separation.sql
```

默认 `apply_role_separation=0`，仅输出数据库所有权、成员资格和计划 SQL，不修改角色/所有权/权限。脚本会拒绝应用角色拥有其他数据库内对象的情况；应先改为独立集群或另行审查涉及的全部数据库。检查计划中的共享对象和数据库归属，确认停止旧进程、备份恢复有效及应用连接身份正确，再在批准的维护窗口执行：

```sh
psql -X -U postgres -d fuxi_dmn -v ON_ERROR_STOP=1 \
  -v app_user=fuxi -v owner_user=fuxi_owner \
  -v apply_role_separation=1 -v confirm_role_separation=fuxi \
  -f script/prepare-production-role-separation.sql
```

`confirm_role_separation` 必须精确匹配旧应用角色名。执行分支在同一事务中转移所有权、撤销其全部角色成员资格、降权并运行 099；失败不应通过跳过校验或提高运行账户权限处理。

已经完成角色分离且仅缺少 099 时，独立管理员可在完成前置核查后执行：

```sh
psql -X -U postgres -d fuxi_dmn -v ON_ERROR_STOP=1 \
  -v app_user=fuxi -f script/099_production_database_privileges.sql
```

随后以**实际应用身份**运行只读基线。`DATABASE_URL` 从安全配置注入，不能继续使用管理员连接串：

```sh
./script/check-deployment-baseline.sh
```

保存退出码和脱敏报告。此检查覆盖结构、列、关键索引/触发器、审计权限、角色和维护入口；缺少必需项即失败。它是当前结构契约检查，不是完整历史迁移账本，也不能证明真实模型或业务质量已验收。

## 5. 就绪、存活与停机

| 接口 | 用途 | 数据库或结构故障时 |
| --- | --- | --- |
| `GET /api/health` | 兼容的进程存活检查 | 进程仍可响应时继续返回 200 |
| `GET /api/ready` | 接流量门禁；每次受限时查询数据库和同源完整基线 | 返回 503；不对外暴露表名、角色或底层诊断 |

服务在知识回填、外部 Embedding 和 LLM 初始化前校验必需配置及完整数据库基线。配置/初始化失败以非零码退出；不会因缺少 Prompt Registry 而降级成“完整功能已就绪”。就绪探测总时限 5 秒，数据库语句与锁等待也有限时。Compose 使用 `/api/ready` 判断应用健康。

SIGTERM/Ctrl-C 会先使 ready 失败，再排空 HTTP、取消后台循环并有界回收连接。`SHUTDOWN_GRACE_SECONDS` 默认 30 秒，是包括 drain 在内的总宽限期；`SHUTDOWN_DRAIN_SECONDS` 默认 2 秒且必须小于总宽限期。Compose `stop_grace_period` 为 45 秒，应保持大于应用总宽限期。初始化期间也可取消；请求或连接排空超时会报告失败。外部代理应按就绪结果摘流；Docker 的 unhealthy 状态本身不等于自动从所有代理中摘除。

## 6. TLS 与代理入口

[nginx.conf](../nginx.conf) 的 HTTP 入口返回 308 到 HTTPS，HTTPS 使用配置中指定的域名与证书文件。发布前替换为实际域名，在 `certs/` 提供匹配的证书链和私钥，并验证证书有效期、域名、HTTP 重定向及 SSE/长请求。

应用容器内部监听 `0.0.0.0:3000`，宿主机映射仅为 `127.0.0.1:${APP_PORT}:3000`。默认和预构建模板带 Nginx；多实例模板由宿主机可信 TLS 代理转发至各实例 loopback 端口。不要额外把业务端口公开到外网。`TRUST_PROXY_HEADERS=true` 仅适用于请求确实经过可信代理的边界；模板代理覆盖客户端传入的转发头。容器网络内的其他入口也须受网络边界约束。

## 7. 完整制品与隔离镜像冒烟

原生 release 构建需要匹配的 Rust、cargo-leptos、wasm-bindgen、wasm-opt 和 Python 工具。实际构建与清洁候选校验入口：

```sh
./script/build-release.sh
python3 script/verify-release-artifacts.py --require-clean \
  --output target/site/release-manifest.json
```

构建生成服务端二进制、CSR 的 JS/WASM/CSS、页面外壳及 `release-manifest.json`。清单记录源码提交/清洁状态、资产版本、文件 SHA256 和大小；服务端嵌入页面与静态页面使用相同资产摘要。`--require-clean` 检查提交和工作区，清单的作用是制品完整性与来源记录，不是运行或业务验收。CI Docker 构建在排除 `.git` 的上下文中注入 `FUXI_BUILD_COMMIT`、`FUXI_SOURCE_CLEAN`；这些必须来自可信的干净检出，不能手工伪造为已通过。

Docker 默认从有固定 SHA256 的官方源获取 Binaryen 132，自定义归档、URL 或版本必须同时指定校验和。干净检出不依赖本机被忽略的压缩包。`.github/workflows/ci.yml` 的 `production-image` 任务构建完整镜像并执行：

```sh
python3 script/smoke-production-image.py --image fuxi-dmn:ci \
  --require-production-source --workflow --browser --output target/production-smoke.json
```

手动使用该命令时，`--image` 必须是已在本机构建的镜像；脚本解析并固定镜像 ID，使用随机项目名、临时凭证、全新卷和 loopback 临时端口，结束后清理自建资源。它不继承开发者的数据库/模型配置，不允许隐式拉取依赖，因此应预先准备所需数据库镜像。覆盖内容包括就绪/存活、服务资产哈希、部分项目授权、缺列拒绝就绪、数据库停止与恢复、SIGTERM 和缺配置退出 1。`--workflow` 额外运行普通项目所有者及 modeler/reviewer/approver/viewer 的确定性建模、业务验收、独立复核、审批发布、凭证生成/执行/撤销、已有 JWT 成员移除链路；兼任角色也不能绕过创建者/复核者身份限制。`--browser` 需和 `--workflow` 一起使用，并需预装 Node.js、`playwright-core` 与 Chromium；CI 把结果写入 smoke JSON，并上传报告和截图。

这是一次隔离部署冒烟：JWT 由临时密钥直接签发，DSL 使用固定夹具；浏览器模式覆盖登录 PoW 拒绝和五类角色页面，但不验证正确密码登录或完整浏览器建模。所有模式都不覆盖真实需求生成、供应商调用、性能或质量验收。CI 配置存在也不代表指定候选的 CI 已实际通过；发布记录需附对应任务结果及镜像摘要。

[build-and-upload-image.sh](../build-and-upload-image.sh) 仅构建并上传私有 release bundle，包含镜像、Compose/SQL/脚本、镜像身份及校验和；不会执行远端命令或部署。上传成功与生产放行分别记录。

### 7.1 原生 release 的真实隔离验收

安装 PostgreSQL 服务端工具（`initdb`、`pg_ctl`、`psql` 和 `pg_config`），完成本机完整 release 构建后，可运行：

```sh
python3 script/smoke-production-binary.py --output target/production-binary-smoke.json
```

安装 Node.js、`playwright-core` 和 Chromium 后，可增加浏览器层检查：

```sh
python3 script/smoke-production-binary.py --browser \
  --output target/production-browser-smoke.json
```

浏览器脚本检查登录页、验证码图片与本地 PoW、错误凭据拒绝、release WASM 加载，以及 owner/modeler/reviewer/approver/viewer 五类临时会话的项目管理页面。可通过 `PLAYWRIGHT_CORE_PATH` 指定 `playwright-core` 模块路径；报告截图保存在 `target/test-results/production-browser-projects.png`。该 smoke 不验证正确密码登录、验证码答案识别，也不执行完整浏览器建模/复核操作；后者仍需实际候选的浏览器 E2E。

非默认 PostgreSQL 安装可指定 `--postgres-bin /绝对路径/bin`。脚本校验现有 manifest 与二进制/静态文件摘要，随后新建专用临时集群和随机 loopback 应用端口。数据库只使用自建私有 Unix socket；不接受目标数据库连接串，也不继承当前环境的数据库、供应商、代理或模型配置。会实际启动该 release 二进制，执行上述多角色确定性业务链路，并检查实际数据库错误脱敏、缺列就绪失败、数据库停止/恢复、SIGTERM 及缺配置非零退出。

报告写到指定文件，应用和 PostgreSQL 的夹具日志保存在 `target/test-results/production-binary-*.log`；结束时只清理自建服务和集群。报告在开始时原子写为 `running`，失败为 `failed`，全部检查及清理成功后才写为 `passed`，避免沿用上次成功报告。镜像工具也使用同样的报告状态约定。

不带 `--browser` 时，这证明本机 release/API 的确定性行为；带 `--browser` 只增加本机 Chromium 页面、PoW 拒绝路径、WASM 资源和五类角色项目页检查。两种模式都不证明 Linux 镜像、正确密码登录、完整浏览器建模、真实 LLM/Embedding 或业务指标验收。当前工作区未清洁时，报告继续标记 `productionSourceEligible=false`，不能因此放行生产。

### 7.2 构建依赖限时与诊断

Docker 的 builder 和 runtime 均通过 `install-debian-packages.sh` 安装依赖。默认索引更新总时限 180 秒、安装总时限 600 秒，网络空闲超时 20 秒、下载重试 2 次，终止后最多再等待 10 秒。索引更新要求所有源成功；不会在部分更新失败后继续安装。超时保留退出码 124，日志明确标注 `update` 或 `install`；不会据此把依赖问题归为源码编译失败。

可在核实下载带宽和可信源后，使用 Docker 构建参数 `APT_UPDATE_TIMEOUT`、`APT_INSTALL_TIMEOUT`、`APT_NETWORK_TIMEOUT`、`APT_DOWNLOAD_RETRIES` 调整。BuildKit 缓存挂载等待发生在脚本开始前，不计入脚本超时；CI 另有作业总时限。APT 索引缓存保留供后续构建复用，失败不会自动清理 Docker 全局缓存或切换镜像源。实际镜像仍须构建并运行冒烟通过，原生验收不能替代这一步。

Binaryen 132 默认从官方 GitHub Release 下载，按目标架构校验固定 SHA-256；`BINARYEN_MAX_TIME` 默认 1800 秒，单实例和多实例 Compose 均可覆盖，下载低速门槛仍为 32 KiB/s 持续 30 秒。延长总时限不关闭低速检测或归档校验。下载中断时保留的部分文件仅在下一次构建通过 HTTP Range 续传并完成摘要校验后才会安装。

## 8. 备份及独立还原验证

为所选候选记录备份开始/结束、源库版本、数据截止点、备份文件摘要、角色/所有权、扩展和对象存储关联。数据库备份不包含知识文件所在的外部对象存储、TLS 私钥或应用密钥；这些需按各自恢复计划保留。暂停写入后进行业务对账，或为在线备份制定一致的业务截止点，避免把不同时刻的计数差异当成还原损坏。

以下为人工演练模板。先在私有 libpq service 配置中分别定义 `fuxi-source-admin` 和指向**独立还原环境**的 `fuxi-restore-admin`，用受保护密码文件认证；配置安全备份目录 `FUXI_BACKUP_DIR`。不要将还原服务配置指向生产源库：

```sh
umask 077
: "${FUXI_BACKUP_DIR:?set an existing private backup directory}"
PGSERVICE=fuxi-source-admin pg_dump --format=custom \
  --file="$FUXI_BACKUP_DIR/fuxi_dmn.dump" fuxi_dmn
PGSERVICE=fuxi-source-admin pg_dumpall --globals-only \
  --file="$FUXI_BACKUP_DIR/globals.sql"
```

全局对象文件可能含密码哈希，按凭证级别保护。新集群应先人工审查并配置必要角色/扩展，不能把整个 globals 文件盲目应用到已有共享集群。保留原所有者和 ACL；不要用 `--no-owner`/`--no-acl` 隐藏权限恢复问题。确认恢复环境有相应角色且独立管理员可恢复后：

```sh
PGSERVICE=fuxi-restore-admin createdb --template=template0 --owner=fuxi_owner fuxi_restore_verification
PGSERVICE=fuxi-restore-admin psql -X -v ON_ERROR_STOP=1 --dbname=postgres \
  -c 'REVOKE ALL ON DATABASE fuxi_restore_verification FROM PUBLIC; GRANT CONNECT ON DATABASE fuxi_restore_verification TO fuxi;'
PGSERVICE=fuxi-restore-admin pg_restore --exit-on-error --single-transaction \
  --dbname=fuxi_restore_verification "$FUXI_BACKUP_DIR/fuxi_dmn.dump"
```

还原到不同库名时，普通 `pg_restore` 不会替新数据库恢复原 DATABASE 对象的 ACL；上例显式指定非登录所有者、撤销 PUBLIC 权限并只授予应用 CONNECT。角色名须按源库审查结果替换，不能保留新库默认的 PUBLIC TEMP 权限。详见 [数据库权限说明](production-database-permissions.md)。

恢复后切换到独立库的实际应用角色，再运行只读基线、关键业务/审计核对和候选应用冒烟。需核对用户/成员角色、版本/产物/正式用例、运行输入输出/外部结果、提示词与解释审阅、编排状态、分区与日统计，以及业务删除后仍保留的审计引用。生产大表采用有界分页、业务截止点和分段摘要，不能仅比较全表计数或“pg_restore 返回 0”。

仓库 [隔离迁移回归](../script/test_decision_run_partition_migrations.sh) 会对自建临时集群执行真实 `pg_dump`/`pg_restore`，恢复到新的 `fuxi_restore_verification`，比较测试表计数/摘要并复查应用基线和权限。其 [指纹 SQL](../script/test_backup_fingerprint.sql) 使用面向小型夹具的全量 `string_agg`，**不用于生产大表**。本地夹具恢复耗时只能作为该测试的记录，不是生产 RPO/RTO；生产恢复时长、数据规模、跨机传输和供应商/对象存储恢复仍需实测。完整脚本可在具备 PostgreSQL 服务端工具的隔离开发/CI 环境执行：

```sh
./script/test_decision_run_partition_migrations.sh
```

## 9. 应用回退与流量放行

预先保留上一候选的准确镜像摘要、配置及资产清单，演练“旧应用读取新结构/按受限角色写入”的兼容性。尤其检查 097 的解释版本隔离和 099 的追加审计约束是否与旧应用的写法兼容。旧版依赖超级用户、修改审计历史或旧结构时，不能靠撤销这些保护来恢复服务。

失败时先摘流、关闭新增自动化入口并停止写入，保留失败日志与数据库状态。只有已证明新结构兼容旧应用时，才切回已固定摘要的旧镜像和配套静态资产。结构不兼容时，先在独立库还原维护前备份、完成业务/审计及权限核对，再按已批准的数据损失窗口切换连接和镜像；不要直接覆盖正在运行的源库。恢复旧库可能丢失备份后写入，补录、审计保存与外部回传幂等处理需事先明确。

接入流量前，依次完成实际应用角色基线、目标版本 `/api/ready`、TLS/代理、业务关键接口和四类角色浏览器链路。按发布承诺另外完成真实 LLM/Embedding、预算、Webhook、Shadow/Draft 关闭演练、稳定负载与故障恢复。最后将候选摘要、配置版本、迁移/恢复报告、业务质量报告与批准记录关联；隔离测试成功不能替代这些目标环境证据。
