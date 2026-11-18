# 生产数据库角色与权限

生产环境使用三个不同角色：独立登录管理员负责初始化和升级；`fuxi_owner` 为禁止登录的对象所有者；`fuxi` 为服务运行角色。应用角色不能是超级用户、数据库或对象所有者，也不能具有其他角色成员身份、建库、建角色、复制或绕过 RLS 的权限。

## 新卷初始化

官方 PostgreSQL 镜像先创建数据库，再运行 `script/bootstrap-production-db.sh`。配置 `POSTGRES_USER=postgres`、独立的管理员密码、`POSTGRES_DB=fuxi_dmn`、`APP_DB_USER=fuxi`、`APP_DB_OWNER=fuxi_owner` 和至少 16 字符的应用 `DB_PASSWORD`。Compose 将 SQL 目录挂到 `/opt/fuxi-db-scripts`，并设置 `FUXI_DB_SCRIPT_DIR=/opt/fuxi-db-scripts`。

初始化脚本先创建角色，再以非登录所有者创建业务对象，最后执行分区、统计和 099 权限迁移。已有业务表或同名应用/所有者角色时直接失败，不重置密码、不自动降权。已有 Docker 数据卷应使用旧库升级流程；初始化中断后的残留角色也必须由管理员审查处理。

## 旧库显式迁移

先完成必要的 schema 升级、备份并验证备份可恢复。使用与应用角色不同的超级用户管理员连接目标库，生成只读审查计划：

```sh
psql "$ADMIN_DATABASE_URL" -X -v ON_ERROR_STOP=1 \
  -v app_user=fuxi -v owner_user=fuxi_owner \
  -f script/prepare-production-role-separation.sql
```

计划列出原应用角色拥有的数据库、角色成员关系以及拟执行语句。`REASSIGN OWNED` 除当前库对象外也涉及该角色拥有的共享对象，因此必须审查列出的数据库；若角色还在其他数据库拥有业务对象，脚本会拒绝继续。确认迁移范围后显式应用：

```sh
psql "$ADMIN_DATABASE_URL" -X -v ON_ERROR_STOP=1 \
  -v app_user=fuxi -v owner_user=fuxi_owner \
  -v apply_role_separation=1 -v confirm_role_separation=fuxi \
  -f script/prepare-production-role-separation.sql
```

应用在同一事务中转移所有权、恢复可信所有者权限、撤销成员身份、降低应用角色权限并执行 099。失败时角色和所有权变更一并回滚。恢复可信所有者权限可避免旧库中原所有者曾被撤销审阅表 UPDATE 权限，导致迁移后外键检查失败。

099 可重复执行，但会拒绝应用角色仍有高权限、对象所有权或角色成员身份的数据库；它不会替代上述显式旧库迁移。

## 运行权限与审计保留

应用具有明确业务表的读写权限、序列 USAGE/SELECT，以及数据库 CONNECT 和 public schema USAGE。应用不能创建普通表、临时表或 schema，不能 ALTER 对象、禁用触发器、重置序列、清空表或直接写分区。未来新表和序列必须在对应迁移中明确授权。

`audit_events`、`knowledge_audit_events`、提示词事件和解释审阅仅允许应用读取和追加。核心两张审计表另外有拒绝 UPDATE、DELETE、TRUNCATE 的触发器。项目、版本、用户或知识源删除时，核心审计证据保留原始 ID/用户名；这些引用有意不再通过外键约束，可能指向已删除对象。正常生命周期删除无需修改或删除原审计记录。审计归档或清理应由管理员另行审查执行。

运行时只开放 `maintain_decision_run_partitions(integer)`（参数 1–36）与 `refresh_decision_run_daily_stats()` 两个维护入口。函数使用可信所有者及固定搜索路径；直接建分区和镜像函数不向应用开放。default 分区迟到数据由所有者在受控入口内搬迁，应用自身没有 TEMP 权限。

## 验证与恢复

使用真实应用凭证执行 `script/check-deployment-baseline.sh`。检查会同时验证 schema、角色权限、所有权隔离、审计保护和维护入口。备份恢复时必须恢复正确角色与对象所有权，并处理新建数据库默认授予 PUBLIC 的 CONNECT/TEMP 权限，然后再次以应用凭证检查基线；管理员连接成功不能代表应用角色权限正确。

隔离测试集群可运行：

```sh
FUXI_DATABASE_PRIVILEGE_TEST=1 PGHOST=/tmp PGPORT=55472 \
  script/test_production_database_bootstrap.sh
```

测试会创建固定名称的 fixture 数据库和角色，验证 fresh 初始化、拒绝重跑、旧库默认只审查、缺确认失败、显式迁移、事务回滚和未来表无默认授权。`script/test_production_database_privileges.sql` 直接以应用角色运行，事务回滚测试数据，覆盖 CRUD、分区镜像、受控维护、越权拒绝与删除后审计引用保留。测试脚本只能用于隔离集群，不应连接真实业务库。
