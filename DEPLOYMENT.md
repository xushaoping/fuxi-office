# Fuxi-DMN 腾讯云轻量应用服务器部署指南

本指南详细说明如何将 Fuxi-DMN 项目部署到腾讯云轻量应用服务器。

## 📋 目录

- [服务器要求](#服务器要求)
- [方案选择](#方案选择)
- [方案一：Docker Compose 部署（推荐）](#方案一docker-compose-部署推荐)
- [方案二：二进制部署](#方案二二进制部署)
- [配置说明](#配置说明)
- [常见问题](#常见问题)

---

## 🖥️ 服务器要求

### 最低配置
- CPU: 2核
- 内存: 2GB
- 硬盘: 20GB SSD
- 操作系统: Ubuntu 22.04 LTS / CentOS 8+ / Debian 11+

### 推荐配置
- CPU: 2核或以上
- 内存: 4GB
- 硬盘: 40GB SSD
- 操作系统: Ubuntu 22.04 LTS（最稳定）

### 网络要求
- 开放端口: 80 (HTTP)、443 (HTTPS)、3000 (应用，可选)
- 带宽: 3Mbps 以上

---

## 🎯 方案选择

| 方案 | 优点 | 缺点 | 推荐场景 |
|------|------|------|----------|
| **Docker Compose** | 环境隔离、易于管理、一键部署 | 占用内存稍多 | 生产环境、团队协作 |
| **二进制部署** | 资源占用少、性能更好 | 环境配置复杂 | 资源受限服务器 |

**推荐：Docker Compose 部署**

---

## 🐳 方案一：Docker Compose 部署（推荐）

### 步骤 1: 购买并初始化服务器

1. **登录腾讯云控制台**
   - 访问: https://console.cloud.tencent.com/lighthouse
   - 选择"轻量应用服务器"

2. **创建实例**
   - 地域：选择距离用户最近的（如广州、上海）
   - 镜像：选择 **Ubuntu Server 22.04 LTS**
   - 套餐：2核2GB 或以上
   - 系统盘：40GB

3. **配置防火墙规则**
   ```
   在"防火墙"标签页添加以下规则：
   - TCP:80    (HTTP)
   - TCP:443   (HTTPS)
   - TCP:3000  (可选，用于直接访问应用)
   - TCP:22    (SSH，默认已开启)
   ```

4. **获取服务器信息**
   - 公网 IP: `x.x.x.x`
   - 重置密码或使用密钥对登录

### 步骤 2: 连接服务器

```bash
# 使用密码登录
ssh root@your_server_ip

# 或使用密钥登录
ssh -i /path/to/your-key.pem root@your_server_ip
```

### 步骤 3: 安装 Docker 和 Docker Compose

```bash
# 更新系统
apt update && apt upgrade -y

# 安装依赖
apt install -y curl wget git vim

# 安装 Docker
curl -fsSL https://get.docker.com | sh

# 启动 Docker 服务
systemctl start docker
systemctl enable docker

# 验证 Docker 安装
docker --version

# 安装 Docker Compose（V2）
mkdir -p ~/.docker/cli-plugins/
curl -SL https://github.com/docker/compose/releases/download/v2.24.0/docker-compose-linux-x86_64 -o ~/.docker/cli-plugins/docker-compose
chmod +x ~/.docker/cli-plugins/docker-compose

# 验证 Docker Compose 安装
docker compose version
```

### 步骤 4: 部署项目

#### 4.1 上传代码到服务器

**方法A: 从本地上传（推荐）**

在本地执行：
```bash
# 进入项目目录
cd /Users/xushaoping/rust/fuxi-dmn

# 打包代码（排除 target 等大文件）
tar --exclude='target' \
    --exclude='node_modules' \
    --exclude='.git' \
    --exclude='*.log' \
    -czf fuxi-dmn.tar.gz .

# 上传到服务器
scp fuxi-dmn.tar.gz root@your_server_ip:/opt/

# 清理本地临时文件
rm fuxi-dmn.tar.gz
```

**方法B: 从 Git 仓库克隆**

在服务器执行：
```bash
cd /opt
git clone https://your-git-repo/fuxi-dmn.git
cd fuxi-dmn
```

#### 4.2 解压并配置

在服务器执行：
```bash
# 如果是上传的压缩包
cd /opt
mkdir -p fuxi-dmn
tar -xzf fuxi-dmn.tar.gz -C fuxi-dmn
cd fuxi-dmn

# 复制生产环境配置模板
cp .env.production.example .env.production

# 编辑配置文件
vim .env.production

# Compose 使用 .env 文件
cp .env.production .env
```

**重要：修改以下配置**
```bash
# 数据库密码（务必修改）
DB_PASSWORD=your_strong_db_password_here

# 超级管理员初始密码（务必修改）
ADMIN_PASSWORD=your_admin_password_here

# JWT 密钥（务必修改为随机字符串）
# 生成方法：openssl rand -base64 32
JWT_SECRET=your_random_jwt_secret_here

# OpenAI 配置（如果使用 AI 功能）
OPENAI_API_KEY=your_openai_api_key
OPENAI_BASE_URL=https://api.openai.com/v1
OPENAI_MODEL=gpt-4-turbo

# 日志级别
RUST_LOG=info
```

#### 4.3 初始化数据库（可选）

如果需要手动初始化数据库：
```bash
# 单独启动 PostgreSQL
docker compose up -d postgres

# 等待数据库就绪
sleep 10

# 执行初始化脚本
docker exec -i fuxi-postgres psql -U fuxi -d postgres < script/init.sql
```

#### 4.4 构建并启动服务

```bash
# 构建 Docker 镜像（首次启动需要 5-10 分钟）
docker compose build

# 启动所有服务
docker compose --env-file .env up -d

# 查看服务状态
docker compose ps

# 查看日志
docker compose logs -f app
```

### 步骤 5: 验证部署

```bash
# 检查服务是否正常运行
curl http://localhost:3000

# 检查 Nginx 反向代理
curl http://localhost

# 查看所有容器状态
docker compose ps

# 应该看到以下服务都是 Up 状态：
# - fuxi-postgres
# - fuxi-dmn
# - fuxi-nginx
```

### 步骤 6: 配置域名（可选但推荐）

#### 6.1 配置 DNS 解析

在域名服务商（如腾讯云 DNSPod）添加 A 记录：
```
主机记录: dmn (或 @)
记录类型: A
记录值: your_server_ip
TTL: 600
```

#### 6.2 配置 Nginx（支持 HTTPS）

创建 Nginx 配置文件：
```bash
vim nginx.conf
```

添加以下内容：
```nginx
user nginx;
worker_processes auto;
error_log /var/log/nginx/error.log warn;
pid /var/run/nginx.pid;

events {
    worker_connections 1024;
}

http {
    include /etc/nginx/mime.types;
    default_type application/octet-stream;

    log_format main '$remote_addr - $remote_user [$time_local] "$request" '
                    '$status $body_bytes_sent "$http_referer" '
                    '"$http_user_agent" "$http_x_forwarded_for"';

    access_log /var/log/nginx/access.log main;
    sendfile on;
    keepalive_timeout 65;
    client_max_body_size 10M;

    # Gzip 压缩
    gzip on;
    gzip_vary on;
    gzip_min_length 1024;
    gzip_types text/plain text/css text/xml text/javascript 
               application/x-javascript application/xml+rss 
               application/json application/javascript;

    # HTTP 服务器
    server {
        listen 80;
        server_name your-domain.com;

        # 健康检查端点
        location /health {
            return 200 "OK";
            add_header Content-Type text/plain;
        }

        # 代理到应用
        location / {
            proxy_pass http://app:3000;
            proxy_http_version 1.1;
            proxy_set_header Upgrade $http_upgrade;
            proxy_set_header Connection "upgrade";
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto $scheme;
            proxy_read_timeout 300s;
            proxy_connect_timeout 75s;
        }
    }

    # HTTPS 服务器（配置 SSL 证书后启用）
    # server {
    #     listen 443 ssl http2;
    #     server_name your-domain.com;
    #
    #     ssl_certificate /etc/nginx/ssl/fullchain.pem;
    #     ssl_certificate_key /etc/nginx/ssl/privkey.pem;
    #     ssl_protocols TLSv1.2 TLSv1.3;
    #     ssl_ciphers HIGH:!aNULL:!MD5;
    #
    #     location / {
    #         proxy_pass http://app:3000;
    #         proxy_http_version 1.1;
    #         proxy_set_header Upgrade $http_upgrade;
    #         proxy_set_header Connection "upgrade";
    #         proxy_set_header Host $host;
    #         proxy_set_header X-Real-IP $remote_addr;
    #         proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    #         proxy_set_header X-Forwarded-Proto $scheme;
    #     }
    # }
}
```

#### 6.3 申请 SSL 证书（可选）

使用 Let's Encrypt 免费证书：
```bash
# 安装 certbot
apt install -y certbot

# 停止 Nginx（首次申请需要）
docker compose stop nginx

# 申请证书
certbot certonly --standalone -d your-domain.com

# 证书会保存在：/etc/letsencrypt/live/your-domain.com/

# 复制证书到项目目录
mkdir -p ssl
cp /etc/letsencrypt/live/your-domain.com/fullchain.pem ssl/
cp /etc/letsencrypt/live/your-domain.com/privkey.pem ssl/

# 修改 docker-compose.yml 中的 nginx 配置，取消 HTTPS 部分的注释

# 重启服务
docker compose restart nginx
```

### 步骤 7: 设置开机自启动

```bash
# 创建 systemd 服务文件
cat > /etc/systemd/system/fuxi-dmn.service << 'EOF'
[Unit]
Description=Fuxi DMN Application
Requires=docker.service
After=docker.service

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=/opt/fuxi-dmn
ExecStart=/usr/bin/docker compose --env-file .env up -d
ExecStop=/usr/bin/docker compose --env-file .env down
TimeoutStartSec=0

[Install]
WantedBy=multi-user.target
EOF

# 重载 systemd
systemctl daemon-reload

# 启用开机自启动
systemctl enable fuxi-dmn

# 测试服务
systemctl start fuxi-dmn
systemctl status fuxi-dmn
```

### 步骤 8: 访问应用

在浏览器访问：
- HTTP: `http://your_server_ip` 或 `http://your-domain.com`
- HTTPS: `https://your-domain.com`（如果配置了 SSL）

默认管理员账号：
- 用户名: `admin`
- 密码: 在 `.env` 中设置的 `ADMIN_PASSWORD`

---

## 💾 方案二：二进制部署

### 步骤 1-3: 同方案一

### 步骤 4: 安装运行时依赖

```bash
# 安装 PostgreSQL
apt install -y postgresql postgresql-contrib

# 启动 PostgreSQL
systemctl start postgresql
systemctl enable postgresql

# 创建数据库用户和数据库
sudo -u postgres psql << EOF
CREATE USER fuxi WITH PASSWORD 'your_password';
CREATE DATABASE fuxi_office OWNER fuxi;
GRANT ALL PRIVILEGES ON DATABASE fuxi_office TO fuxi;
\q
EOF

# 初始化数据库
sudo -u postgres psql -U fuxi -d fuxi_office < /opt/fuxi-dmn/script/init.sql
```

### 步骤 5: 构建二进制

**在本地机器构建**（因为服务器可能不支持编译）：

```bash
# 在本地项目目录
cd /Users/xushaoping/rust/fuxi-dmn

# 构建 release 版本
./script/build-release.sh

# 打包
tar -czf fuxi-dmn-binary.tar.gz \
    target/release/fuxi-dmn \
    target/site

# 上传到服务器
scp fuxi-dmn-binary.tar.gz root@your_server_ip:/opt/
```

### 步骤 6: 部署二进制

在服务器执行：
```bash
cd /opt/fuxi-dmn
tar -xzf ../fuxi-dmn-binary.tar.gz

# 创建配置文件
cat > .env << 'EOF'
DATABASE_URL=postgres://fuxi:your_password@localhost:5432/fuxi_office
BIND_ADDR=0.0.0.0:3000
JWT_SECRET=your_jwt_secret
ADMIN_PASSWORD=your_admin_password
OPENAI_API_KEY=your_openai_key
RUST_LOG=info
EOF

# 创建 systemd 服务
cat > /etc/systemd/system/fuxi-dmn.service << 'EOF'
[Unit]
Description=Fuxi DMN Application
After=network.target postgresql.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/fuxi-dmn
EnvironmentFile=/opt/fuxi-dmn/.env
ExecStart=/opt/fuxi-dmn/target/release/fuxi-dmn
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
EOF

# 启动服务
systemctl daemon-reload
systemctl start fuxi-dmn
systemctl enable fuxi-dmn
systemctl status fuxi-dmn
```

### 步骤 7: 配置 Nginx

```bash
# 安装 Nginx
apt install -y nginx

# 配置站点
cat > /etc/nginx/sites-available/fuxi-dmn << 'EOF'
server {
    listen 80;
    server_name your-domain.com;

    location / {
        proxy_pass http://localhost:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}
EOF

# 启用站点
ln -s /etc/nginx/sites-available/fuxi-dmn /etc/nginx/sites-enabled/
nginx -t
systemctl restart nginx
```

---

## ⚙️ 配置说明

### 环境变量详解

| 变量名 | 说明 | 示例 |
|--------|------|------|
| `DB_PASSWORD` | PostgreSQL 密码 | `strong_password_123` |
| `DATABASE_URL` | 数据库连接字符串 | `postgres://user:pass@host:5432/db` |
| `BIND_ADDR` | 应用监听地址 | `0.0.0.0:3000` |
| `JWT_SECRET` | JWT 签名密钥 | 随机字符串（至少 32 字符） |
| `ADMIN_PASSWORD` | admin 初始密码 | 强密码（至少 8 字符） |
| `OPENAI_API_KEY` | OpenAI API 密钥 | `sk-...` |
| `OPENAI_BASE_URL` | OpenAI API 地址 | `https://api.openai.com/v1` |
| `OPENAI_MODEL` | 使用的模型 | `gpt-4-turbo` |
| `RUST_LOG` | 日志级别 | `info`, `debug`, `error` |

### 生成安全的密钥

```bash
# 生成 JWT 密钥
openssl rand -base64 32

# 生成数据库密码
openssl rand -base64 24

# 生成管理员密码（更安全）
openssl rand -base64 16
```

---

## 🔧 常用运维命令

### Docker Compose 方式

```bash
# 查看服务状态
docker compose ps

# 查看实时日志
docker compose logs -f app

# 重启服务
docker compose restart app

# 停止所有服务
docker compose down

# 更新并重启
git pull  # 或重新上传代码
docker compose build
docker compose up -d

# 备份数据库
docker exec fuxi-postgres pg_dump -U fuxi fuxi_office > backup_$(date +%Y%m%d).sql

# 恢复数据库
docker exec -i fuxi-postgres psql -U fuxi fuxi_office < backup.sql

# 清理未使用的镜像和容器
docker system prune -a
```

### 二进制方式

```bash
# 查看服务状态
systemctl status fuxi-dmn

# 查看日志
journalctl -u fuxi-dmn -f

# 重启服务
systemctl restart fuxi-dmn

# 备份数据库
pg_dump -U fuxi fuxi_office > backup_$(date +%Y%m%d).sql
```

---

## 🛡️ 安全建议

1. **修改默认密码**
   - 首次登录后立即修改 admin 密码

2. **配置防火墙**
   ```bash
   # 使用 ufw
   ufw allow 22/tcp
   ufw allow 80/tcp
   ufw allow 443/tcp
   ufw enable
   ```

3. **启用 HTTPS**
   - 生产环境务必使用 HTTPS
   - 使用 Let's Encrypt 免费证书

4. **定期备份**
   ```bash
   # 创建备份脚本
   cat > /opt/backup.sh << 'EOF'
   #!/bin/bash
   DATE=$(date +%Y%m%d_%H%M%S)
   docker exec fuxi-postgres pg_dump -U fuxi fuxi_office > /opt/backups/fuxi_${DATE}.sql
   find /opt/backups -name "fuxi_*.sql" -mtime +7 -delete
   EOF
   
   chmod +x /opt/backup.sh
   
   # 添加到 crontab（每天凌晨 2 点备份）
   echo "0 2 * * * /opt/backup.sh" | crontab -
   ```

5. **监控资源**
   ```bash
   # 安装监控工具
   apt install -y htop iotop nethogs
   
   # 查看资源使用
   docker stats
   ```

---

## ❓ 常见问题

### 1. 构建镜像时内存不足

**解决方案：**
```bash
# 增加交换空间
fallocate -l 2G /swapfile
chmod 600 /swapfile
mkswap /swapfile
swapon /swapfile
echo '/swapfile none swap sw 0 0' >> /etc/fstab
```

### 2. 端口冲突

**解决方案：**
```bash
# 查看端口占用
netstat -tlnp | grep :3000

# 修改 docker-compose.yml 中的端口映射
ports:
  - "8000:3000"  # 改为其他端口
```

### 3. 数据库连接失败

**排查步骤：**
```bash
# 1. 检查 PostgreSQL 是否运行
docker compose ps postgres

# 2. 查看数据库日志
docker compose logs postgres

# 3. 测试数据库连接
docker exec -it fuxi-postgres psql -U fuxi -d fuxi_office

# 4. 检查连接字符串
cat .env | grep DATABASE_URL
```

### 4. 应用无法启动

**排查步骤：**
```bash
# 1. 查看应用日志
docker compose logs app

# 2. 检查配置文件
cat .env

# 3. 手动运行应用测试
docker compose run --rm app /app/fuxi-dmn
```

### 5. Nginx 502 错误

**解决方案：**
```bash
# 1. 检查应用是否运行
docker compose ps app

# 2. 检查 Nginx 配置
nginx -t

# 3. 查看 Nginx 日志
docker compose logs nginx
```

---

## 📞 技术支持

如遇到问题，请提供以下信息：
1. 服务器配置和操作系统版本
2. 部署方式（Docker 或二进制）
3. 错误日志（`docker compose logs` 或 `journalctl`）
4. 配置文件（脱敏后的 `.env`）

---

## 📝 更新日志

- **2024-09-03**: 初始版本，支持 Docker Compose 和二进制两种部署方式
- **2024-09-03**: 添加删除用户功能

---

**部署完成后，请记得：**
1. ✅ 修改所有默认密码
2. ✅ 配置 HTTPS 证书
3. ✅ 设置定期备份
4. ✅ 配置防火墙规则
5. ✅ 首次登录并测试功能
