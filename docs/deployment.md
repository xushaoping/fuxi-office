# 部署指南

本文说明本地 Compose 部署和生产部署入口。生产发布、已有数据库迁移、备份恢复、镜像验收等完整流程见[生产发布运行手册](production-release-runbook.md)；环境变量说明见[配置参考](configuration.md)。

## 部署模式

| Compose 文件 | 用途 | 数据库 | Web 入口 |
| --- | --- | --- | --- |
| `docker-compose.yml` | 本地构建镜像并启动完整服务 | PostgreSQL 18 + pgvector | Nginx HTTP/HTTPS |
| `docker-compose.prebuilt.yml` | 使用已构建的应用镜像 | PostgreSQL 18 + pgvector | Nginx HTTP/HTTPS |
| `docker-compose.multi.yml` | 多个应用实例 | PostgreSQL 16 + pgvector | 由宿主机可信 TLS 代理负载均衡 |

不要在已有 PostgreSQL 数据卷上直接更换大版本镜像标签。多实例模式的启动、端口和 `INSTANCE_HOST` 配置参见 [multi-instance.sh](../script/multi-instance.sh) 和 Compose 文件。

## 首次部署

1. 安装 Docker Engine 和 Docker Compose 插件，准备域名、TLS 证书和私有配置文件。
2. 从 [生产环境变量模板](../.env.production.example) 创建私有 `.env.production`。分别设置数据库管理员密码、应用运行角色密码、JWT 密钥和管理员密码。使用随机独立凭据，数据库 URI 密码应采用 URL 安全字符。
3. 检查 Compose 配置可解析：

   ```sh
   docker compose --env-file .env.production -f docker-compose.prebuilt.yml config --quiet
   ```

4. 若使用仓库本地构建，将 `docker-compose.prebuilt.yml` 替换为 `docker-compose.yml`，构建并启动：

   ```sh
   docker compose --env-file .env.production up -d --build
   ```

   预构建部署应先从受信任发布制品导入镜像，再启动：

   ```sh
   docker compose --env-file .env.production -f docker-compose.prebuilt.yml up -d
   ```

5. 查看服务状态和日志，并确认 `/api/ready` 返回成功：

   ```sh
   docker compose --env-file .env.production ps
   docker compose --env-file .env.production logs --tail=100 app postgres nginx
   curl --fail --silent --show-error http://127.0.0.1:3000/api/ready
   ```

6. 配置 `nginx.conf` 使用的域名和 `certs/` 中匹配的证书、私钥，验证外部 HTTPS 访问及 HTTP 到 HTTPS 的重定向。默认应用端口只映射到宿主机 loopback。

数据库空卷的初始化由 `script/bootstrap-production-db.sh` 执行，会建立独立管理员、`NOLOGIN` 对象所有者及应用运行角色并初始化 schema。脚本拒绝把已有数据库或已有同名角色当作全新库处理。生产部署使用三角色权限模型；不要让应用用 PostgreSQL 超级用户运行。

## 发布与更新

发布应使用已验收的不可变镜像和配置版本。上线前核验镜像摘要、源码来源、数据库迁移、权限基线、就绪探针、备份及还原演练。以应用角色运行：

```sh
./script/check-deployment-baseline.sh
```

该命令通过 `DATABASE_URL` 连接目标库并执行只读结构和权限基线检查。它不能代替候选版本的集成测试、真实模型验收或备份恢复演练。

更新预构建镜像后，可在批准的维护窗口执行：

```sh
docker compose --env-file .env.production -f docker-compose.prebuilt.yml pull app
docker compose --env-file .env.production -f docker-compose.prebuilt.yml up -d app
```

确认 Nginx、应用和数据库健康，`/api/ready` 成功且关键业务链路可用后再关闭维护窗口。数据库 schema 回退不等于应用镜像回退；需要回滚时按生产运行手册核对迁移的向后兼容性和数据恢复方案。

## 停止和备份

使用 Compose 停止服务以触发应用的有界优雅关闭：

```sh
docker compose --env-file .env.production -f docker-compose.prebuilt.yml down
```

不要添加 `-v`，除非明确要删除 Compose 数据卷。PostgreSQL 数据卷、知识文件存储、TLS 私钥和应用密钥需要分别纳入备份与恢复流程。至少定期在隔离环境还原数据库，并用运行账户重新执行权限基线检查。

## 生产验收

生产发布还应执行适用于候选版本的构建制品验证、隔离镜像或二进制冒烟、浏览器检查和真实环境 smoke。`/api/health` 只反映进程是否响应，负载入口应检查 `/api/ready`。完整约束和检查局限见[生产发布运行手册](production-release-runbook.md)。
