#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${ROOT}/docker-compose.multi.yml"
ENV_DIR="${ROOT}/deploy/multi-instance"
IMAGE_NAME="${FUXI_IMAGE:-fuxi-dmn:local}"

usage() {
    cat <<'USAGE'
Usage:
  script/multi-instance.sh <command> [instance]

Commands:
  build                 Build the shared Fuxi image once
  up <instance>         Start one instance, e.g. app-a
  down <instance>       Stop one instance without deleting its data volume
  ps [instance]         Show running containers
  logs <instance>       Follow instance logs
  config <instance>     Render the Compose file after substituting env
  backup <instance>     Dump PostgreSQL to ./backups/<instance>-<timestamp>.sql
  nginx-snippet <instance>
                        Print an Nginx server block for the instance

Examples:
  cp deploy/multi-instance/app-a.env.example deploy/multi-instance/app-a.env
  script/multi-instance.sh build
  script/multi-instance.sh up app-a
  script/multi-instance.sh up app-b
USAGE
}

die() {
    echo "error: $*" >&2
    exit 1
}

env_file_for() {
    local instance="$1"
    local path="${ENV_DIR}/${instance}.env"
    [[ -f "$path" ]] || die "missing ${path}; copy ${instance}.env.example first"
    echo "$path"
}

load_env() {
    local env_file="$1"
    set -a
    # shellcheck disable=SC1090
    source "$env_file"
    set +a
    [[ -n "${APP_PORT:-}" ]] || die "APP_PORT is required in ${env_file}"
    [[ -n "${DB_PASSWORD:-}" ]] || die "DB_PASSWORD is required in ${env_file}"
    [[ -n "${JWT_SECRET:-}" ]] || die "JWT_SECRET is required in ${env_file}"
    [[ -n "${ADMIN_PASSWORD:-}" ]] || die "ADMIN_PASSWORD is required in ${env_file}"
}

project_name() {
    local instance="$1"
    echo "fuxi-${instance}"
}

compose() {
    local instance="$1"
    shift
    local env_file
    env_file="$(env_file_for "$instance")"
    docker compose -p "$(project_name "$instance")" -f "$COMPOSE_FILE" --env-file "$env_file" "$@"
}

require_instance() {
    [[ $# -ge 1 ]] || die "instance name is required, e.g. app-a"
}

cmd_build() {
    docker build -t "$IMAGE_NAME" "$ROOT"
    echo "built ${IMAGE_NAME}"
}

cmd_up() {
    require_instance "$@"
    local instance="$1"
    local env_file
    env_file="$(env_file_for "$instance")"
    load_env "$env_file"
    if ! docker image inspect "$IMAGE_NAME" >/dev/null 2>&1; then
        cmd_build
    fi
    FUXI_IMAGE="$IMAGE_NAME" compose "$instance" up -d
    echo "started $(project_name "$instance") on 127.0.0.1:${APP_PORT}"
}

cmd_down() {
    require_instance "$@"
    compose "$1" down
}

cmd_ps() {
    if [[ $# -ge 1 ]]; then
        compose "$1" ps
        return
    fi
    docker ps --filter "label=com.docker.compose.project" --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
}

cmd_logs() {
    require_instance "$@"
    compose "$1" logs -f --tail=200
}

cmd_config() {
    require_instance "$@"
    compose "$1" config
}

cmd_backup() {
    require_instance "$@"
    local instance="$1"
    local env_file stamp backup_dir backup_file
    env_file="$(env_file_for "$instance")"
    load_env "$env_file"
    stamp="$(date +%Y%m%d-%H%M%S)"
    backup_dir="${ROOT}/backups"
    backup_file="${backup_dir}/${instance}-${stamp}.sql"
    mkdir -p "$backup_dir"
    docker compose -p "$(project_name "$instance")" -f "$COMPOSE_FILE" --env-file "$env_file" \
        exec -T postgres pg_dump -U fuxi -d fuxi_dmn > "$backup_file"
    echo "wrote ${backup_file}"
}

cmd_nginx_snippet() {
    require_instance "$@"
    local instance="$1"
    local env_file host port
    env_file="$(env_file_for "$instance")"
    load_env "$env_file"
    host="${INSTANCE_HOST:-${instance}.example.com}"
    port="${APP_PORT}"
    cat <<SNIPPET
upstream fuxi_${instance//-/_} {
    server 127.0.0.1:${port};
}

server {
    listen 80;
    server_name ${host};
    client_max_body_size 10M;

    location / {
        proxy_pass http://fuxi_${instance//-/_};
        proxy_http_version 1.1;
        proxy_set_header Upgrade \$http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
        proxy_read_timeout 300s;
        proxy_connect_timeout 75s;
    }
}

# HTTPS server
server {
    listen 443 ssl http2;
    server_name ${host};
    client_max_body_size 10M;

    ssl_certificate /etc/nginx/ssl/${host}_bundle.crt;
    ssl_certificate_key /etc/nginx/ssl/${host}.key;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    location / {
        proxy_pass http://fuxi_${instance//-/_};
        proxy_http_version 1.1;
        proxy_set_header Upgrade \$http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
        proxy_read_timeout 300s;
        proxy_connect_timeout 75s;
    }
}
SNIPPET
}

command="${1:-}"
shift || true
case "$command" in
    build) cmd_build ;;
    up) cmd_up "$@" ;;
    down) cmd_down "$@" ;;
    ps) cmd_ps "$@" ;;
    logs) cmd_logs "$@" ;;
    config) cmd_config "$@" ;;
    backup) cmd_backup "$@" ;;
    nginx-snippet) cmd_nginx_snippet "$@" ;;
    -h|--help|help|"") usage ;;
    *) die "unknown command: ${command}" ;;
esac
