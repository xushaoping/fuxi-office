#!/usr/bin/env bash
# Fuxi-DMN 快速部署脚本 - 适用于腾讯云轻量应用服务器
# 使用方法：./quick-deploy.sh

set -euo pipefail

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo_info() { echo -e "${GREEN}✓${NC} $1"; }
echo_warn() { echo -e "${YELLOW}⚠${NC} $1"; }
echo_error() { echo -e "${RED}✗${NC} $1"; }
echo_step() { echo -e "${BLUE}➜${NC} $1"; }

# Banner
cat << 'EOF'
╔═══════════════════════════════════════╗
║   Fuxi-DMN 快速部署脚本               ║
║   适用于腾讯云轻量应用服务器           ║
╚═══════════════════════════════════════╝
EOF

echo ""

# 检测操作系统
detect_os() {
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        OS=$ID
        VERSION=$VERSION_ID
    else
        echo_error "无法检测操作系统"
        exit 1
    fi
    echo_info "检测到操作系统: $OS $VERSION"
}

# 检查是否为 root 用户
check_root() {
    if [ "$EUID" -ne 0 ]; then
        echo_error "请使用 root 权限运行此脚本"
        echo "使用: sudo ./quick-deploy.sh"
        exit 1
    fi
}

# 安装 Docker
install_docker() {
    echo_step "检查 Docker 安装状态..."
    if command -v docker &> /dev/null; then
        echo_info "Docker 已安装: $(docker --version)"
        return 0
    fi

    echo_step "安装 Docker..."
    curl -fsSL https://get.docker.com | sh
    systemctl start docker
    systemctl enable docker
    echo_info "Docker 安装完成"
}

# 安装 Docker Compose
install_docker_compose() {
    echo_step "检查 Docker Compose 安装状态..."
    if docker compose version &> /dev/null; then
        echo_info "Docker Compose 已安装: $(docker compose version)"
        return 0
    fi

    echo_step "安装 Docker Compose..."
    COMPOSE_VERSION="v2.24.0"
    mkdir -p /usr/local/lib/docker/cli-plugins
    curl -SL "https://github.com/docker/compose/releases/download/${COMPOSE_VERSION}/docker-compose-linux-x86_64" \
        -o /usr/local/lib/docker/cli-plugins/docker-compose
    chmod +x /usr/local/lib/docker/cli-plugins/docker-compose
    echo_info "Docker Compose 安装完成"
}

# 生成随机密码
generate_password() {
    openssl rand -base64 24 | tr -d "=+/" | cut -c1-20
}

# 配置环境变量
setup_env() {
    echo_step "配置环境变量..."

    if [ -f .env ]; then
        echo_warn ".env 文件已存在，是否覆盖？(y/N)"
        read -r response
        if [[ ! "$response" =~ ^[Yy]$ ]]; then
            echo_info "跳过环境变量配置"
            return 0
        fi
    fi

    # 生成随机密码
    DB_PASSWORD=$(generate_password)
    JWT_SECRET=$(openssl rand -base64 32)
    ADMIN_PASSWORD=$(generate_password)

    # 获取用户输入
    echo ""
    echo_step "请输入配置信息（直接回车使用默认值）："

    read -p "OpenAI API Key（可选）: " OPENAI_API_KEY
    if [ -z "$OPENAI_API_KEY" ]; then
        OPENAI_API_KEY=""
    fi

    read -p "OpenAI Base URL [https://api.openai.com/v1]: " OPENAI_BASE_URL
    OPENAI_BASE_URL=${OPENAI_BASE_URL:-https://api.openai.com/v1}

    read -p "OpenAI Model [gpt-4-turbo]: " OPENAI_MODEL
    OPENAI_MODEL=${OPENAI_MODEL:-gpt-4-turbo}

    # 创建 .env 文件
    cat > .env << EOF
# ==================== 生产环境配置 ====================
# 此文件由 quick-deploy.sh 自动生成于 $(date)

# 数据库配置
DB_PASSWORD=${DB_PASSWORD}

# 超级管理员 admin 的初始密码（首次登录后请立即修改）
ADMIN_PASSWORD=${ADMIN_PASSWORD}

# JWT 密钥（请勿泄露）
JWT_SECRET=${JWT_SECRET}

# OpenAI 配置
OPENAI_API_KEY=${OPENAI_API_KEY}
OPENAI_BASE_URL=${OPENAI_BASE_URL}
OPENAI_MODEL=${OPENAI_MODEL}

# 日志级别
RUST_LOG=info
EOF

    echo_info "环境变量配置完成"
    echo ""
    echo_warn "重要信息（请妥善保存）："
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "数据库密码: ${DB_PASSWORD}"
    echo "Admin 密码: ${ADMIN_PASSWORD}"
    echo "JWT 密钥: ${JWT_SECRET}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""

    # 保存到文件
    cat > /root/fuxi-credentials.txt << EOF
Fuxi-DMN 部署凭证
生成时间: $(date)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
数据库密码: ${DB_PASSWORD}
Admin 用户名: admin
Admin 密码: ${ADMIN_PASSWORD}
JWT 密钥: ${JWT_SECRET}
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
EOF

    echo_info "凭证已保存到: /root/fuxi-credentials.txt"
    sleep 3
}

# 初始化数据库
init_database() {
    echo_step "初始化数据库..."

    # 启动 PostgreSQL
    docker compose up -d postgres

    # 等待数据库就绪
    echo_step "等待数据库启动..."
    sleep 10

    # 检查数据库是否已初始化
    if docker exec fuxi-postgres psql -U fuxi -d fuxi_office -c "SELECT 1 FROM users LIMIT 1;" &> /dev/null; then
        echo_info "数据库已初始化，跳过"
        return 0
    fi

    # 执行初始化脚本
    if [ -f script/init.sql ]; then
        docker exec -i fuxi-postgres psql -U fuxi -d postgres < script/init.sql
        echo_info "数据库初始化完成"
    else
        echo_warn "未找到初始化脚本 script/init.sql"
    fi
}

# 构建并启动服务
start_services() {
    echo_step "构建 Docker 镜像..."
    docker compose build

    echo_step "启动所有服务..."
    docker compose --env-file .env up -d

    # 等待服务启动
    echo_step "等待服务启动..."
    sleep 5

    # 检查服务状态
    docker compose ps
}

# 配置防火墙
setup_firewall() {
    echo_step "配置防火墙..."

    if command -v ufw &> /dev/null; then
        ufw --force enable
        ufw allow 22/tcp comment 'SSH'
        ufw allow 80/tcp comment 'HTTP'
        ufw allow 443/tcp comment 'HTTPS'
        echo_info "防火墙配置完成（使用 ufw）"
    elif command -v firewall-cmd &> /dev/null; then
        firewall-cmd --permanent --add-service=ssh
        firewall-cmd --permanent --add-service=http
        firewall-cmd --permanent --add-service=https
        firewall-cmd --reload
        echo_info "防火墙配置完成（使用 firewalld）"
    else
        echo_warn "未检测到防火墙工具，请手动配置"
    fi
}

# 配置开机自启动
setup_autostart() {
    echo_step "配置开机自启动..."

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

    systemctl daemon-reload
    systemctl enable fuxi-dmn
    echo_info "开机自启动配置完成"
}

# 显示访问信息
show_access_info() {
    # 获取服务器 IP
    SERVER_IP=$(curl -s ifconfig.me || curl -s icanhazip.com || echo "未知")

    echo ""
    echo_info "部署完成！"
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  访问信息"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  应用地址: http://${SERVER_IP}"
    echo "  用户名: admin"
    echo "  密码: （见上方输出或 /root/fuxi-credentials.txt）"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "常用命令："
    echo "  查看服务状态: docker compose ps"
    echo "  查看日志: docker compose logs -f app"
    echo "  重启服务: docker compose restart"
    echo "  停止服务: docker compose down"
    echo ""
    echo "凭证文件: /root/fuxi-credentials.txt"
    echo ""
    echo_warn "首次登录后请立即修改 admin 密码！"
}

# 主函数
main() {
    check_root
    detect_os

    echo ""
    echo_step "开始部署..."
    echo ""

    # 安装依赖
    install_docker
    install_docker_compose

    # 进入部署目录
    if [ ! -d "/opt/fuxi-dmn" ]; then
        echo_error "未找到项目目录 /opt/fuxi-dmn"
        echo "请先将项目代码上传到 /opt/fuxi-dmn"
        exit 1
    fi

    cd /opt/fuxi-dmn

    # 配置环境
    setup_env

    # 初始化数据库
    init_database

    # 启动服务
    start_services

    # 配置防火墙和自启动
    setup_firewall
    setup_autostart

    # 显示访问信息
    show_access_info
}

# 执行主函数
main "$@"
