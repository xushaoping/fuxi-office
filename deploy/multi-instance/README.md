# 多实例部署

在一台服务器上运行多个相互隔离的 Fuxi App。每个实例拥有独立的 Compose 项目名、本机端口、PostgreSQL 数据卷、JWT 密钥和管理员密码。

推荐拓扑：

```text
公网
  ↓
统一 Nginx：80 / 443
  ├── app-a.example.com → 127.0.0.1:3101 → Fuxi App A → PostgreSQL A
  └── app-b.example.com → 127.0.0.1:3102 → Fuxi App B → PostgreSQL B
```

不要让每个实例自己绑定 `80` 和 `443`。当前仓库默认的 `docker-compose.yml` 适合单实例；多实例请使用 `docker-compose.multi.yml`。

## 1. 准备配置

```bash
cp deploy/multi-instance/app-a.env.example deploy/multi-instance/app-a.env
cp deploy/multi-instance/app-b.env.example deploy/multi-instance/app-b.env
```

每个 `.env` 必须修改：

- `APP_PORT`：本机回环端口，实例之间不能重复
- `INSTANCE_HOST`：对外域名
- `DB_PASSWORD`：该实例数据库密码
- `ADMIN_PASSWORD`：该实例管理员初始密码
- `JWT_SECRET`：该实例独立密钥，至少 32 个字符，实例之间不要复用

生成密钥：

```bash
openssl rand -base64 32
```

## 2. 构建一次镜像，启动多个实例

```bash
script/multi-instance.sh build
script/multi-instance.sh up app-a
script/multi-instance.sh up app-b
script/multi-instance.sh ps
```

等价的 Compose 命令：

```bash
docker build -t fuxi-dmn:local .

docker compose -p fuxi-app-a -f docker-compose.multi.yml \
  --env-file deploy/multi-instance/app-a.env up -d

docker compose -p fuxi-app-b -f docker-compose.multi.yml \
  --env-file deploy/multi-instance/app-b.env up -d
```

`-p fuxi-app-a` 会把容器和数据卷隔离为：

```text
fuxi-app-a-app-1
fuxi-app-a-postgres-1
fuxi-app-a_postgres_data
```

健康检查：

```bash
curl http://127.0.0.1:3101/api/health
curl http://127.0.0.1:3102/api/health
```

## 3. 统一 Nginx

将 `deploy/multi-instance/nginx.conf.example` 作为主机 Nginx 配置，或按实例生成片段：

```bash
script/multi-instance.sh nginx-snippet app-a
script/multi-instance.sh nginx-snippet app-b
```

建议使用独立子域名，不要使用 `/app-a` 这类路径前缀。当前应用包含 SSE、JWT 和固定 API 路径，子域名转发更稳定。

证书放到：

```text
/etc/nginx/ssl/<host>_bundle.crt
/etc/nginx/ssl/<host>.key
```

检查并重载：

```bash
nginx -t && systemctl reload nginx
```

## 4. 运维

```bash
script/multi-instance.sh logs app-a
script/multi-instance.sh backup app-a
script/multi-instance.sh down app-a
```

`down` 只停止容器，不会删除数据卷。只有确认要销毁该实例数据时才执行：

```bash
docker compose -p fuxi-app-a -f docker-compose.multi.yml \
  --env-file deploy/multi-instance/app-a.env down -v
```

备份文件默认写入 `backups/<instance>-<timestamp>.sql`。

恢复示例：

```bash
docker compose -p fuxi-app-a -f docker-compose.multi.yml \
  --env-file deploy/multi-instance/app-a.env exec -T postgres \
  psql -U fuxi -d fuxi_dmn < backups/app-a-20260916-210000.sql
```

`script/init.sql` 只会在该实例 PostgreSQL 数据卷首次创建时自动执行。已有实例升级请按仓库增量 SQL 顺序执行，不要重新初始化数据卷。

## 5. 约束

- 每个实例必须使用不同的 `APP_PORT`、Compose 项目名和数据卷。
- 每个实例必须使用独立 `JWT_SECRET`，避免令牌跨实例生效。
- 不要同时启动默认 `docker-compose.yml` 中的 Nginx 和主机统一 Nginx，否则会争用 `80/443`。
- 应用端口只绑定 `127.0.0.1`，对外流量全部经过 Nginx。
- 同一版本的多个实例应共享同一个镜像标签，例如 `fuxi-dmn:local` 或 `fuxi-dmn:1.0.0`。
