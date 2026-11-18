# ==================== 构建阶段 ====================
# The Rust image can be overridden for environments with a local or regional
# registry mirror, but the default is the public Docker Hub image.
ARG DEBIAN_MIRROR=http://deb.debian.org/debian
ARG DEBIAN_SECURITY_MIRROR=http://deb.debian.org/debian-security
ARG APT_UPDATE_TIMEOUT=180
ARG APT_INSTALL_TIMEOUT=600
ARG APT_NETWORK_TIMEOUT=20
ARG APT_DOWNLOAD_RETRIES=2
ARG RUST_IMAGE=rust:1.90-slim-bookworm
FROM ${RUST_IMAGE} AS builder

ARG DEBIAN_MIRROR
ARG DEBIAN_SECURITY_MIRROR
ARG APT_UPDATE_TIMEOUT
ARG APT_INSTALL_TIMEOUT
ARG APT_NETWORK_TIMEOUT
ARG APT_DOWNLOAD_RETRIES
ARG TARGETARCH

# 可通过 DEBIAN_MIRROR 和 DEBIAN_SECURITY_MIRROR 切换到可访问的区域镜像。
ARG RUSTUP_DIST_SERVER=https://rsproxy.cn
ARG RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
ARG RUST_TOOLCHAIN=1.90
ENV RUSTUP_DIST_SERVER=${RUSTUP_DIST_SERVER}
ENV RUSTUP_UPDATE_ROOT=${RUSTUP_UPDATE_ROOT}
ENV RUST_TOOLCHAIN=${RUST_TOOLCHAIN}
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
# Avoid long stalls when rsproxy has a transient connection issue.
ENV RUSTUP_MAX_RETRIES=5

# 配置 cargo 使用国内镜像
RUN mkdir -p /usr/local/cargo && \
    echo '[source.crates-io]' > /usr/local/cargo/config.toml && \
    echo 'replace-with = "rsproxy-sparse"' >> /usr/local/cargo/config.toml && \
    echo '[source.rsproxy-sparse]' >> /usr/local/cargo/config.toml && \
    echo 'registry = "sparse+https://rsproxy.cn/index/"' >> /usr/local/cargo/config.toml

# 安装系统依赖
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    --mount=type=bind,source=script,target=/build-tools,readonly \
    for source in /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list; do \
        if [ -f "$source" ]; then \
            sed -i \
                -e "s#https\\?://deb.debian.org/debian-security#${DEBIAN_SECURITY_MIRROR}#g" \
                -e "s#https\\?://deb.debian.org/debian#${DEBIAN_MIRROR}#g" \
                "$source"; \
        fi; \
    done && \
    bash /build-tools/install-debian-packages.sh \
    ca-certificates \
    curl \
    git \
    build-essential \
    python3 \
    pkg-config \
    libssl-dev

# 安装项目固定的 Rust toolchain 和 wasm32 target。
# 只缓存 rustup 的下载目录，不能把整个 /usr/local/rustup 挂载为空缓存，
# 否则会遮蔽基础镜像内已有的 toolchain，触发 "no default toolchain"。
RUN --mount=type=cache,target=/usr/local/rustup/downloads,sharing=locked \
    rustup toolchain install "${RUST_TOOLCHAIN}" \
        --profile minimal \
        --target wasm32-unknown-unknown \
        --component rustfmt \
        --component clippy \
        --no-self-update && \
    rustup default "${RUST_TOOLCHAIN}"

# cargo-leptos 的构建依赖需要 Rust 1.90；显式选择工具链并输出版本。
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/var/cache/cargo-install-target,sharing=locked,id=fuxi-tools-${TARGETARCH}-rust-${RUST_TOOLCHAIN} \
    rustup run "${RUST_TOOLCHAIN}" rustc --version && \
    CARGO_TARGET_DIR=/var/cache/cargo-install-target \
        cargo +${RUST_TOOLCHAIN} install cargo-leptos --version 0.3.7 --locked

ARG WASM_BINDGEN_VERSION=0.2.129
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/var/cache/cargo-install-target,sharing=locked,id=fuxi-tools-${TARGETARCH}-rust-${RUST_TOOLCHAIN} \
    CARGO_TARGET_DIR=/var/cache/cargo-install-target \
        cargo +${RUST_TOOLCHAIN} install wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked && \
    wasm-bindgen --version

ARG BINARYEN_VERSION=132
ARG BINARYEN_ARCHIVE=
ARG BINARYEN_DOWNLOAD_URL=
ARG BINARYEN_SHA256=
ARG BINARYEN_MAX_TIME=1800
ARG BINARYEN_SPEED_LIMIT=32768
ARG BINARYEN_SPEED_TIME=30
RUN --mount=type=cache,target=/var/cache/binaryen,sharing=locked \
    --mount=type=bind,source=script,target=/build-tools,readonly \
    set -eu; \
    if [ -z "$BINARYEN_SHA256" ]; then \
        if [ "$BINARYEN_VERSION" != 132 ] || [ -n "$BINARYEN_ARCHIVE" ] || [ -n "$BINARYEN_DOWNLOAD_URL" ]; then \
            echo "Custom Binaryen inputs require an explicit BINARYEN_SHA256" >&2; exit 1; \
        fi; \
        case "$(uname -m)" in \
            x86_64) BINARYEN_SHA256=195ddc94f9bc89f45abdabb0b9eea86023d727ba90eac8b35b80f2544fc30572 ;; \
            aarch64|arm64) BINARYEN_SHA256=c58562417836c5d0493d89bdefc434933bdc097db641b483df86bcfa557a107f ;; \
            *) echo "Unsupported Binaryen architecture" >&2; exit 1 ;; \
        esac; \
    fi; \
    BINARYEN_VERSION="${BINARYEN_VERSION}" \
    BINARYEN_ARCHIVE="${BINARYEN_ARCHIVE}" \
    BINARYEN_DOWNLOAD_URL="${BINARYEN_DOWNLOAD_URL}" \
    BINARYEN_SHA256="${BINARYEN_SHA256}" \
    BINARYEN_MAX_TIME="${BINARYEN_MAX_TIME}" \
    BINARYEN_SPEED_LIMIT="${BINARYEN_SPEED_LIMIT}" \
    BINARYEN_SPEED_TIME="${BINARYEN_SPEED_TIME}" \
    BINARYEN_CACHE_DIR=/var/cache/binaryen \
    bash /build-tools/install-binaryen.sh

WORKDIR /app

# 复制依赖文件（利用 Docker 缓存层）
COPY Cargo.toml Cargo.lock ./

# 创建虚拟源码以预编译依赖
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked,id=fuxi-target-${TARGETARCH}-rust-${RUST_TOOLCHAIN} \
    mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    mkdir -p src/dmn src/feel src/nl_dsl && \
    echo "pub fn dummy() {}" > src/lib.rs && \
    cargo build --release --lib --bin fuxi-dmn && \
    cargo build --release --target wasm32-unknown-unknown --features csr --lib && \
    rm -rf src

# 复制实际源码和资源
COPY . .

# 构建前端和后端。Git metadata is injected by the release caller (.git is excluded).
ARG FUXI_BUILD_COMMIT=
ARG FUXI_SOURCE_CLEAN=
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked,id=fuxi-target-${TARGETARCH}-rust-${RUST_TOOLCHAIN} \
    FUXI_BUILD_COMMIT="${FUXI_BUILD_COMMIT}" FUXI_SOURCE_CLEAN="${FUXI_SOURCE_CLEAN}" ./script/build-release.sh && \
    install -D target/release/fuxi-dmn /build-output/fuxi-dmn && \
    cp -a target/site /build-output/site

# ==================== 运行阶段 ====================
FROM debian:bookworm-slim

ARG DEBIAN_MIRROR
ARG DEBIAN_SECURITY_MIRROR
ARG APT_UPDATE_TIMEOUT
ARG APT_INSTALL_TIMEOUT
ARG APT_NETWORK_TIMEOUT
ARG APT_DOWNLOAD_RETRIES

# 安装运行时依赖
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    --mount=type=bind,source=script,target=/build-tools,readonly \
    for source in /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list; do \
        if [ -f "$source" ]; then \
            sed -i \
                -e "s#https\\?://deb.debian.org/debian-security#${DEBIAN_SECURITY_MIRROR}#g" \
                -e "s#https\\?://deb.debian.org/debian#${DEBIAN_MIRROR}#g" \
                "$source"; \
        fi; \
    done && \
    bash /build-tools/install-debian-packages.sh \
    ca-certificates \
    curl \
    libssl3

WORKDIR /app

# 从构建阶段复制二进制和静态资源
COPY --from=builder /build-output/fuxi-dmn /app/fuxi-dmn
COPY --from=builder /build-output/site /app/target/site

# 创建非 root 用户
RUN useradd -m -u 1001 appuser && \
    chown -R appuser:appuser /app
USER appuser

# 环境变量（生产环境应通过 docker-compose 或 k8s 注入）
ENV APP_ENV=production
ENV RUST_LOG=info
ENV BIND_ADDR=0.0.0.0:3000

EXPOSE 3000

CMD ["./fuxi-dmn"]
