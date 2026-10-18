# ==================== 构建阶段 ====================
# The Rust image can be overridden for environments with a local or regional
# registry mirror, but the default is the public Docker Hub image.
ARG DEBIAN_MIRROR=http://deb.debian.org/debian
ARG DEBIAN_SECURITY_MIRROR=http://deb.debian.org/debian-security
ARG RUST_IMAGE=rust:1.90-slim-bookworm
FROM ${RUST_IMAGE} AS builder

ARG DEBIAN_MIRROR
ARG DEBIAN_SECURITY_MIRROR

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
    rm -rf /var/lib/apt/lists/* && \
    for source in /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list; do \
        if [ -f "$source" ]; then \
            sed -i \
                -e "s#https\\?://deb.debian.org/debian-security#${DEBIAN_SECURITY_MIRROR}#g" \
                -e "s#https\\?://deb.debian.org/debian#${DEBIAN_MIRROR}#g" \
                "$source"; \
        fi; \
    done && \
    apt-get update && apt-get install -y \
    --no-install-recommends \
    ca-certificates \
    curl \
    git \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 安装项目固定的 Rust toolchain 和 wasm32 target。
# 只缓存 rustup 的下载目录，不能把整个 /usr/local/rustup 挂载为空缓存，
# 否则会遮蔽基础镜像内已有的 toolchain，触发 "no default toolchain"。
RUN --mount=type=cache,target=/usr/local/rustup/downloads,sharing=locked \
    rustup toolchain install "${RUST_TOOLCHAIN}" \
        --profile minimal \
        --target wasm32-unknown-unknown \
        --no-self-update && \
    rustup default "${RUST_TOOLCHAIN}"

# cargo-leptos 的构建依赖需要 Rust 1.90；显式选择工具链并输出版本。
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    rustup run "${RUST_TOOLCHAIN}" rustc --version && \
    cargo +${RUST_TOOLCHAIN} install cargo-leptos --version 0.3.7 --locked

ARG WASM_BINDGEN_VERSION=0.2.129
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo +${RUST_TOOLCHAIN} install wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked && \
    wasm-bindgen --version

ARG BINARYEN_VERSION=132
ARG BINARYEN_ARCHIVE=
ARG BINARYEN_DOWNLOAD_URL=
ARG BINARYEN_SHA256=
ARG BINARYEN_MAX_TIME=120
ARG BINARYEN_SPEED_LIMIT=32768
ARG BINARYEN_SPEED_TIME=30
RUN --mount=type=cache,target=/var/cache/binaryen,sharing=locked \
    --mount=type=bind,source=script,target=/build-tools,readonly \
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
    mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    mkdir -p src/dmn src/feel src/nl_dsl && \
    echo "pub fn dummy() {}" > src/lib.rs && \
    cargo build --release && \
    rm -rf src

# 复制实际源码和资源
COPY . .

# 构建前端和后端
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    ./script/build-release.sh

# ==================== 运行阶段 ====================
FROM debian:bookworm-slim

ARG DEBIAN_MIRROR
ARG DEBIAN_SECURITY_MIRROR

# 安装运行时依赖
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    rm -rf /var/lib/apt/lists/* && \
    for source in /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list; do \
        if [ -f "$source" ]; then \
            sed -i \
                -e "s#https\\?://deb.debian.org/debian-security#${DEBIAN_SECURITY_MIRROR}#g" \
                -e "s#https\\?://deb.debian.org/debian#${DEBIAN_MIRROR}#g" \
                "$source"; \
        fi; \
    done && \
    apt-get update && apt-get install -y \
    --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 从构建阶段复制二进制和静态资源
COPY --from=builder /app/target/release/fuxi-dmn /app/fuxi-dmn
COPY --from=builder /app/target/site /app/target/site

# 创建非 root 用户
RUN useradd -m -u 1001 appuser && \
    chown -R appuser:appuser /app
USER appuser

# 环境变量（生产环境应通过 docker-compose 或 k8s 注入）
ENV RUST_LOG=info
ENV BIND_ADDR=0.0.0.0:3000

EXPOSE 3000

CMD ["./fuxi-dmn"]
