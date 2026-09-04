#!/usr/bin/env bash
# 一键 release 构建：前端 wasm + 静态资源 + 后端二进制
#
# cargo-leptos 0.3.7 的两个 index.html 硬编码行为：
#   1. 复制 assets 时跳过 index.html（assets 下放 index.html 会报保留路径错误）
#   2. 清理 site 时保留已有的 index.html（SSR 模式约定由它生成）
# 因此 CSR 模式下需要在此手动复制外壳模板，且之后重复构建不会被清掉。
set -euo pipefail
cd "$(dirname "$0")/.."

# 1. 前端：编译 wasm 并复制静态资源到 target/site（清空后重建）
cargo leptos build --release

# 2. 外壳页面：手动复制项目根模板为 site/index.html
cp index.html target/site/index.html

# 3. 后端二进制（与前端共用同一 Cargo 包，单独编译）
cargo build --release

echo "构建完成：target/release/fuxi-dmn + target/site/"
