#!/usr/bin/env bash
# Build-only dependency installation. Never continue with a partially updated index.
set -euo pipefail
update_timeout="${APT_UPDATE_TIMEOUT:-180}"
install_timeout="${APT_INSTALL_TIMEOUT:-600}"
network_timeout="${APT_NETWORK_TIMEOUT:-20}"
retries="${APT_DOWNLOAD_RETRIES:-2}"
for value in "$update_timeout" "$install_timeout" "$network_timeout" "$retries"; do
    [[ "$value" =~ ^(0|[1-9][0-9]{0,3})$ ]] || { echo 'invalid APT timeout/retry setting' >&2; exit 2; }
done
(( update_timeout >= 1 && install_timeout >= 1 && network_timeout >= 1 && retries <= 5 )) || {
    echo 'APT deadlines must be positive and retries must be 0-5' >&2; exit 2;
}
(( $# > 0 )) || { echo 'at least one Debian package is required' >&2; exit 2; }
for package in "$@"; do
    [[ "$package" =~ ^[a-z0-9][a-z0-9.+:-]*$ ]] || { echo 'invalid Debian package name' >&2; exit 2; }
done
command -v timeout >/dev/null || { echo 'GNU timeout is required in the Debian build image' >&2; exit 2; }
options=(-o "Acquire::http::Timeout=$network_timeout" -o "Acquire::https::Timeout=$network_timeout"
         -o "Acquire::Retries=$retries" -o 'DPkg::Lock::Timeout=60')
phase() {
    local name="$1" deadline="$2" status
    shift 2
    printf 'Debian dependency phase: %s (deadline %ss)\n' "$name" "$deadline"
    if DEBIAN_FRONTEND=noninteractive timeout --signal=TERM --kill-after=10s "${deadline}s" "$@"; then
        return 0
    else
        status=$?
        printf 'Debian dependency phase failed: %s (exit %s). Check the preceding download/lock diagnostics; no application compilation was attempted in this phase.\n' "$name" "$status" >&2
        exit "$status"
    fi
}
phase update "$update_timeout" apt-get "${options[@]}" -o 'APT::Update::Error-Mode=any' update
phase install "$install_timeout" apt-get "${options[@]}" install --yes --no-install-recommends "$@"
