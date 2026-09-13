#!/usr/bin/env bash
# 本地构建 Docker 镜像并上传到服务器
# 使用方法: ./build-and-upload-image.sh root@your_server_ip
# 可选：RUST_IMAGE=your-registry/rust:1.90-bookworm ./build-and-upload-image.sh root@your_server_ip

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m'

echo_info() { echo -e "${GREEN}✓${NC} $1"; }
echo_warn() { echo -e "${YELLOW}⚠${NC} $1"; }
echo_step() { echo -e "${BLUE}➜${NC} $1"; }
echo_error() { echo -e "${RED}✗${NC} $1"; }

# Banner
cat << 'EOF'
╔═══════════════════════════════════════════╗
║  Fuxi-DMN 本地构建并上传到服务器          ║
╚═══════════════════════════════════════════╝
EOF
echo ""

# 检查参数
if [ -z "${1:-}" ]; then
    echo_error "缺少服务器地址参数"
    echo "用法: ./build-and-upload-image.sh root@your_server_ip"
    exit 1
fi

SERVER=$1
IMAGE_NAME="fuxi-dmn:latest"
TEMP_FILE="/tmp/fuxi-dmn-image.tar.gz"

# 检查 Docker 是否安装
if ! command -v docker &> /dev/null; then
    echo_error "未找到 Docker，请先安装 Docker Desktop"
    exit 1
fi

# 检查 Docker 是否运行
if ! docker info &> /dev/null; then
    echo_error "Docker 未运行，请启动 Docker Desktop"
    exit 1
fi

echo_step "开始本地构建 Docker 镜像..."
echo_warn "这将需要 10-15 分钟（首次构建，后续更快）..."
RUST_IMAGE=${RUST_IMAGE:-rust:1.90-bookworm}
echo_info "Rust 基础镜像: ${RUST_IMAGE}（可通过 RUST_IMAGE 环境变量覆盖）"
echo ""

# 构建镜像
if ! docker build --build-arg "RUST_IMAGE=${RUST_IMAGE}" -t "${IMAGE_NAME}" .; then
    echo_error "Docker 构建失败！"
    echo "如果当前网络无法访问 Docker Hub，请使用可访问的 Rust 镜像重试："
    echo "RUST_IMAGE=your-registry/rust:1.90-bookworm ./build-and-upload-image.sh ${SERVER}"
    exit 1
fi

echo_info "镜像构建完成"

# 保存镜像
echo_step "保存镜像为文件..."
docker save ${IMAGE_NAME} | gzip > ${TEMP_FILE}

IMAGE_SIZE=$(du -h ${TEMP_FILE} | cut -f1)
echo_info "镜像大小: ${IMAGE_SIZE}"

# 上传镜像
echo_step "上传镜像到服务器 ${SERVER}..."
if ! scp ${TEMP_FILE} ${SERVER}:/tmp/; then
    echo_error "上传失败，请检查 SSH 连接"
    rm ${TEMP_FILE}
    exit 1
fi

echo_step "上传数据库初始化脚本..."
if ! scp script/init.sql ${SERVER}:/tmp/fuxi-dmn-init.sql; then
    echo_error "数据库初始化脚本上传失败"
    rm ${TEMP_FILE}
    exit 1
fi

# 在服务器上加载镜像并配置
echo_step "在服务器上加载镜像并配置..."
ssh ${SERVER} << 'ENDSSH'
set -e

echo "➜ 加载 Docker 镜像..."
docker load < /tmp/fuxi-dmn-image.tar.gz
rm /tmp/fuxi-dmn-image.tar.gz

echo "➜ 配置 docker-compose.yml..."
cd /opt/fuxi-dmn
mv /tmp/fuxi-dmn-init.sql ./init.sql

# 备份原配置
if [ -f docker-compose.yml ]; then
    cp docker-compose.yml docker-compose.yml.backup
fi

# 创建使用预构建镜像的配置
cat > docker-compose.yml << 'EOFCOMPOSE'
services:
  postgres:
    image: postgres:16-alpine
    container_name: fuxi-postgres
    restart: unless-stopped
    environment:
      POSTGRES_DB: fuxi_dmn
      POSTGRES_USER: fuxi
      POSTGRES_PASSWORD: ${DB_PASSWORD:?DB_PASSWORD must be set}
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./init.sql:/docker-entrypoint-initdb.d/001-init.sql:ro
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U fuxi -d fuxi_dmn"]
      interval: 10s
      timeout: 5s
      retries: 5

  app:
    image: fuxi-dmn:latest
    container_name: fuxi-dmn
    restart: unless-stopped
    depends_on:
      postgres:
        condition: service_healthy
    environment:
      DATABASE_URL: postgres://fuxi:${DB_PASSWORD:?DB_PASSWORD must be set}@postgres:5432/fuxi_dmn
      BIND_ADDR: 0.0.0.0:3000
      JWT_SECRET: ${JWT_SECRET:?JWT_SECRET must be set}
      ADMIN_PASSWORD: ${ADMIN_PASSWORD:?ADMIN_PASSWORD must be set}
      OPENAI_API_KEY: ${OPENAI_API_KEY}
      OPENAI_BASE_URL: ${OPENAI_BASE_URL:-https://api.openai.com/v1}
      OPENAI_MODEL: ${OPENAI_MODEL:-gpt-4-turbo}
      RUST_LOG: ${RUST_LOG:-info}
    ports:
      - "3000:3000"
    healthcheck:
      test: ["CMD-SHELL", "curl -f http://localhost:3000/api/health || exit 1"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s

  nginx:
    image: nginx:alpine
    container_name: fuxi-nginx
    restart: unless-stopped
    depends_on:
      - app
    ports:
      - "80:80"
    command: >
      sh -c "cat > /etc/nginx/nginx.conf << 'EOFNGINX'
      user nginx;
      worker_processes auto;
      error_log /var/log/nginx/error.log warn;
      pid /var/run/nginx.pid;
      events {
        worker_connections 1024;
      }
      http {
        upstream app {
          server app:3000;
        }
        server {
          listen 80;
          server_name _;
          client_max_body_size 10M;
          location / {
            proxy_pass http://app;
            proxy_http_version 1.1;
            proxy_set_header Upgrade \$$http_upgrade;
            proxy_set_header Connection \"upgrade\";
            proxy_set_header Host \$$host;
            proxy_set_header X-Real-IP \$$remote_addr;
            proxy_set_header X-Forwarded-For \$$proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto \$$scheme;
            proxy_read_timeout 300s;
            proxy_connect_timeout 75s;
          }
        }
      }
      EOFNGINX
      && nginx -g 'daemon off;'"

volumes:
  postgres_data:
    driver: local
EOFCOMPOSE

echo "✓ 配置完成"
docker images | grep fuxi-dmn
ENDSSH

# 清理本地临时文件
rm ${TEMP_FILE}

echo ""
echo_info "部署准备完成！"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  下一步操作（在服务器上执行）"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  ssh ${SERVER}"
echo "  cd /opt/fuxi-dmn"
echo ""
echo "  # 配置环境变量（如果还没有）"
echo "  cp .env.production .env"
echo "  vim .env"
echo ""
echo "  # 启动服务"
echo "  docker compose --env-file .env up -d"
echo ""
echo "  # 查看状态"
echo "  docker compose ps"
echo "  docker compose logs -f app"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
