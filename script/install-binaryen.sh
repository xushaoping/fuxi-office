#!/usr/bin/env bash
set -euo pipefail

version="${BINARYEN_VERSION:-132}"
archive_name="${BINARYEN_ARCHIVE:-}"
download_url="${BINARYEN_DOWNLOAD_URL:-}"
expected_sha256="${BINARYEN_SHA256:-}"
cache_dir="${BINARYEN_CACHE_DIR:-${XDG_CACHE_HOME:-/tmp}/binaryen}"

if [[ ! "$version" =~ ^[0-9]+$ ]]; then
    printf 'BINARYEN_VERSION 必须为整数版本号。\n' >&2
    exit 1
fi
if [[ -n "$expected_sha256" && ! "$expected_sha256" =~ ^[[:xdigit:]]{64}$ ]]; then
    printf 'BINARYEN_SHA256 必须为 64 位十六进制摘要。\n' >&2
    exit 1
fi

case "$(uname -m)" in
    x86_64) binaryen_arch=x86_64 ;;
    aarch64|arm64) binaryen_arch=aarch64 ;;
    *) printf '不支持的 Binaryen 构建架构：%s\n' "$(uname -m)" >&2; exit 1 ;;
esac

temporary_dir="$(mktemp -d)"
trap 'rm -rf "$temporary_dir"' EXIT
archive_path="$temporary_dir/binaryen.tar.gz"

validate_archive() {
    local candidate="$1"

    if ! tar -tzf "$candidate" >/dev/null 2>&1; then
        return 1
    fi

    if [[ -n "$expected_sha256" ]]; then
        printf '%s  %s\n' "$expected_sha256" "$candidate" | sha256sum --check --strict >/dev/null
    fi
}

if [[ -n "$archive_name" ]]; then
    if [[ "$archive_name" != "$(basename "$archive_name")" ]]; then
        printf '请将 Linux Binaryen 压缩包放在 script/ 下，BINARYEN_ARCHIVE 只填写文件名。\n' >&2
        exit 1
    fi
    if [[ ! -f "/build-tools/$archive_name" ]]; then
        printf '构建容器中找不到 Binaryen 本地包：/build-tools/%s\n' "$archive_name" >&2
        printf '请确认文件位于项目 script/ 下，并在 .dockerignore 的 script/*.tar.gz 规则之后添加 !script/%s\n' "$archive_name" >&2
        exit 1
    fi
    cp "/build-tools/$archive_name" "$archive_path"
else
    download_url="${download_url:-https://github.com/WebAssembly/binaryen/releases/download/version_${version}/binaryen-version_${version}-${binaryen_arch}-linux.tar.gz}"
    mkdir -p "$cache_dir"
    cache_key="$(printf '%s\n%s\n' "$download_url" "$expected_sha256" | sha256sum | awk '{print $1}')"
    cached_archive="$cache_dir/binaryen-version_${version}-${binaryen_arch}-${cache_key}.tar.gz"
    partial_archive="$cached_archive.partial"

    if [[ -s "$cached_archive" ]] && validate_archive "$cached_archive"; then
        printf '复用 Binaryen 缓存：%s\n' "$cached_archive"
        cp "$cached_archive" "$archive_path"
    else
        rm -f "$cached_archive"
        if ! command -v curl >/dev/null 2>&1 || [[ ! -s /etc/ssl/certs/ca-certificates.crt ]]; then
            printf '在线安装 Binaryen 需要 builder 内的 curl 和 CA 证书。请为自定义 RUST_IMAGE 预装 curl/ca-certificates，或通过 BINARYEN_ARCHIVE 使用本地 Linux 包；此脚本不会自动运行 apt。\n' >&2
            exit 1
        fi
        printf '开始下载 Binaryen %s（Linux %s）；可通过 BINARYEN_ARCHIVE 改用本地包。\n' "$version" "$binaryen_arch"
        printf 'Binaryen 缓存文件：%s\n' "$partial_archive"
        printf '下载中断会保留进度；下次构建尝试 HTTP Range 续传，需要服务器支持。\n'
        curl_args=(
            --disable
            --fail
            --location
            --retry 0
            --connect-timeout 15
            --max-time "${BINARYEN_MAX_TIME:-120}"
            --speed-limit "${BINARYEN_SPEED_LIMIT:-32768}"
            --speed-time "${BINARYEN_SPEED_TIME:-30}"
            --proto '=https'
            --proto-redir '=https'
        )
        if [[ -s "$partial_archive" ]]; then
            curl_args+=(--continue-at -)
            printf '从已有进度续传：%s\n' "$partial_archive"
        fi
        if ! curl "${curl_args[@]}" "$download_url" -o "$partial_archive"; then
            printf 'Binaryen 下载失败，部分文件已保留：%s\n' "$partial_archive" >&2
            printf '可重试构建以续传，或使用 BINARYEN_DOWNLOAD_URL 指向更快的可信镜像，或使用 BINARYEN_ARCHIVE 本地包。\n' >&2
            exit 1
        fi
        if ! validate_archive "$partial_archive"; then
            rm -f "$partial_archive"
            printf 'Binaryen 下载结束但压缩包或 SHA-256 校验失败，已删除损坏文件；请检查下载源和 BINARYEN_SHA256。\n' >&2
            exit 1
        fi
        mv "$partial_archive" "$cached_archive"
        cp "$cached_archive" "$archive_path"
    fi
fi

if [[ -n "$expected_sha256" ]]; then
    printf '%s  %s\n' "$expected_sha256" "$archive_path" | sha256sum --check --strict
fi

mkdir -p /opt/binaryen
tar -xzf "$archive_path" --strip-components=1 -C /opt/binaryen
actual_version="$(/opt/binaryen/bin/wasm-opt --version)"
version_pattern="^wasm-opt version ${version}( \\(version_${version}\\))?$"
if [[ ! "$actual_version" =~ $version_pattern ]]; then
    printf 'Binaryen 版本不匹配：期望 %s，实际 %s\n' "$version" "$actual_version" >&2
    exit 1
fi
ln -sf /opt/binaryen/bin/wasm-opt /usr/local/bin/wasm-opt
command -v wasm-opt
wasm-opt --version
