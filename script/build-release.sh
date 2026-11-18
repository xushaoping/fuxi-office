#!/usr/bin/env bash
# 一键 release 构建：前端 wasm + 静态资源 + 后端二进制
#
# cargo-leptos 0.3.7 的两个 index.html 硬编码行为：
#   1. 复制 assets 时跳过 index.html（assets 下放 index.html 会报保留路径错误）
#   2. 清理 site 时保留已有的 index.html（SSR 模式约定由它生成）
# 因此 CSR 模式下需要在此手动复制外壳模板，且之后重复构建不会被清掉。
set -euo pipefail
cd "$(dirname "$0")/.."

rustc --version
cargo leptos --version
for tool in wasm-bindgen wasm-opt; do
    if ! command -v "$tool"; then
        printf '缺少构建工具 %s；请在当前构建环境（Docker 构建时是 builder 容器）中安装。\n' "$tool" >&2
        exit 1
    fi
    "$tool" --version
done
mkdir -p target

# 1. 前端：编译 wasm 并复制静态资源到 target/site（清空后重建）
if cargo leptos build --release 2>&1 | tee target/build-release.log; then
    :
else
    build_status=$?
    printf '\n前端构建失败（退出码 %s）；以下为最后 100 行原始日志：\n' "$build_status" >&2
    tail -n 100 target/build-release.log >&2
    printf '\n请保留完整 Docker --progress=plain 日志，检查 wasm-opt failed with: 后的 stderr；不能仅凭 optimization failed 判断原因。\n' >&2
    exit "$build_status"
fi

# 2. Bind every asset URL to this bundle, so a previous cached client cannot
# accidentally load a different release's WASM or API contract.
asset_version="$(python3 script/verify-release-artifacts.py --stamp-only)"
python3 - "$asset_version" <<'PY'
from pathlib import Path
import sys
source = Path('index.html').read_text(encoding='utf-8')
if '__FUXI_ASSET_VERSION__' not in source:
    raise SystemExit('release index template is missing its asset version placeholder')
Path('target/site/index.html').write_text(source.replace('__FUXI_ASSET_VERSION__', sys.argv[1]), encoding='utf-8')
PY

# 3. 后端二进制（与前端共用同一 Cargo 包，单独编译）
FUXI_ASSET_VERSION="$asset_version" cargo build --release --locked --bin fuxi-dmn
python3 script/verify-release-artifacts.py --output target/site/release-manifest.json

echo "构建完成：target/release/fuxi-dmn + target/site/"
