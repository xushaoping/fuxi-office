#!/usr/bin/env bash
# Build and upload a self-contained release bundle; never start or modify a remote deployment.
set -euo pipefail
umask 077

usage() {
    echo 'Usage: ./build-and-upload-image.sh user@host [remote-directory]'
    echo 'Builds FUXI_IMAGE (default fuxi-dmn:local), then uploads a private release bundle.'
    echo 'The destination must already exist. No remote commands or deployments are executed.'
}
if [[ "${1:-}" == -h || "${1:-}" == --help ]]; then usage; exit 0; fi
[[ $# -ge 1 && $# -le 2 ]] || { usage >&2; exit 1; }
server="$1"
remote_dir="${2:-/tmp}"
# Keep scp targets unambiguous; support plain SSH host aliases and user@host.
[[ "$server" =~ ^[A-Za-z0-9_][A-Za-z0-9_.@-]*$ ]] || { echo 'Invalid SSH host' >&2; exit 1; }
[[ "$remote_dir" =~ ^/[A-Za-z0-9_./-]*$ && "$remote_dir" != *'..'* ]] || { echo 'Remote directory must be an absolute path without spaces or traversal' >&2; exit 1; }
root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
image="${FUXI_IMAGE:-fuxi-dmn:local}"
command -v docker >/dev/null || { echo 'Docker is required' >&2; exit 1; }
command -v scp >/dev/null || { echo 'scp is required' >&2; exit 1; }
for required in docker-compose.prebuilt.yml nginx.conf .env.production.example script/bootstrap-production-db.sh; do
    [[ -f "$root/$required" ]] || { echo "Missing release input: $required" >&2; exit 1; }
done
staging="$(mktemp -d "${TMPDIR:-/tmp}/fuxi-release.XXXXXXXX")"
trap 'rm -rf "$staging"' EXIT
bundle="$staging/fuxi-release"
mkdir "$bundle"
commit="$(git -C "$root" rev-parse HEAD)"
source_clean=0
if [[ -z "$(git -C "$root" status --porcelain --untracked-files=normal)" ]]; then source_clean=1; fi
if [[ "$source_clean" != 1 ]]; then
    echo 'Working tree has changes; this diagnostic bundle is not a frozen release candidate.' >&2
fi
args=(--build-arg "RUST_IMAGE=${RUST_IMAGE:-rust:1.90-slim-bookworm}"
      --build-arg "FUXI_BUILD_COMMIT=$commit" --build-arg "FUXI_SOURCE_CLEAN=$source_clean")
# Custom Binaryen sources always need an explicit checksum (enforced by Dockerfile).
for name in BINARYEN_VERSION BINARYEN_ARCHIVE BINARYEN_DOWNLOAD_URL BINARYEN_SHA256 DEBIAN_MIRROR DEBIAN_SECURITY_MIRROR; do
    if [[ -n "${!name:-}" ]]; then args+=(--build-arg "$name=${!name}"); fi
done
docker build --platform "${APP_PLATFORM:-linux/amd64}" "${args[@]}" -t "$image" "$root"
docker save "$image" | gzip > "$bundle/image.tar.gz"
cp "$root/docker-compose.prebuilt.yml" "$root/nginx.conf" "$root/.env.production.example" "$bundle/"
# SQL/init/migration helpers are versioned release inputs; exclude ignored local archives.
mkdir "$bundle/script"
while IFS= read -r -d '' resource; do
    cp "$resource" "$bundle/script/"
done < <(find "$root/script" -maxdepth 1 -type f \( -name '*.sql' -o -name '*.sh' -o -name '*.py' \) -print0)
printf '%s\n' "$image" > "$bundle/image-reference.txt"
# Stable image ID ties the uploaded image to this build even if its local tag changes.
docker image inspect --format '{{.Id}}' "$image" > "$bundle/image-id.txt"
(
    cd "$bundle"
    if command -v sha256sum >/dev/null; then
        find . -type f ! -name SHA256SUMS -exec sha256sum {} \; > SHA256SUMS
    else
        find . -type f ! -name SHA256SUMS -exec shasum -a 256 {} \; > SHA256SUMS
    fi
)
release_name="fuxi-release-$(date -u +%Y%m%dT%H%M%SZ)-${staging##*.}.tar.gz"
tar -czf "$staging/$release_name" -C "$staging" fuxi-release
scp "$staging/$release_name" "$server:$remote_dir/$release_name"
printf 'Uploaded %s to %s:%s/\n' "$release_name" "$server" "$remote_dir"
printf '%s\n' 'Review checksums, image identity, credentials, certificates and database upgrade/restore evidence before separately approving deployment.'
