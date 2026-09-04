# ==================== 构建阶段 ====================
# The Rust image can be overridden for environments with a local or regional
# registry mirror, but the default is the public Docker Hub image.
ARG RUST_IMAGE=rust:1.88-bookworm
FROM ${RUST_IMAGE} AS builder

# 使用国内镜像加速（可选，国内服务器推荐）
ENV RUSTUP_DIST_SERVER=https://rsproxy.cn
ENV RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup

# 配置 cargo 使用国内镜像
RUN mkdir -p /usr/local/cargo && \
    echo '[source.crates-io]' > /usr/local/cargo/config.toml && \
    echo 'replace-with = "rsproxy-sparse"' >> /usr/local/cargo/config.toml && \
    echo '[source.rsproxy-sparse]' >> /usr/local/cargo/config.toml && \
    echo 'registry = "sparse+https://rsproxy.cn/index/"' >> /usr/local/cargo/config.toml

# 安装系统依赖
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 安装 wasm32 target
RUN rustup target add wasm32-unknown-unknown

# 安装 cargo-leptos（指定版本，使用国内镜像）
RUN cargo install cargo-leptos --version 0.3.7 --locked

WORKDIR /app

# 复制依赖文件（利用 Docker 缓存层）
COPY Cargo.toml Cargo.lock ./

# 创建虚拟源码以预编译依赖
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    mkdir -p src/dmn src/feel src/nl_dsl && \
    echo "pub fn dummy() {}" > src/lib.rs && \
    cargo build --release || true && \
    rm -rf src

# 复制实际源码和资源
COPY . .

# 构建前端和后端
RUN ./script/build-release.sh

# ==================== 运行阶段 ====================
FROM debian:bookworm-slim

# 安装运行时依赖
RUN apt-get update && apt-get install -y \
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
