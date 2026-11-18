#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"
AUTH_TOKEN="${AUTH_TOKEN:-}"
PROJECT_ID="${PROJECT_ID:-}"
VERSION_ID="${VERSION_ID:-}"
REQUESTS="${REQUESTS:-1}"
MIN_CASES="${MIN_CASES:-1000}"
MAX_ELAPSED_MS="${MAX_ELAPSED_MS:-15000}"

for variable in AUTH_TOKEN PROJECT_ID VERSION_ID; do
    if [[ -z "${!variable}" ]]; then
        echo "error: ${variable} is required" >&2
        exit 2
    fi
done
for variable in REQUESTS MIN_CASES MAX_ELAPSED_MS; do
    if [[ ! "${!variable}" =~ ^[1-9][0-9]*$ ]]; then
        echo "error: ${variable} must be a positive integer" >&2
        exit 2
    fi
done
for variable in PROJECT_ID VERSION_ID; do
    if [[ ! "${!variable}" =~ ^[1-9][0-9]*$ ]]; then
        echo "error: ${variable} must be a positive integer" >&2
        exit 2
    fi
done
command -v curl >/dev/null 2>&1 || { echo "error: curl is required" >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { echo "error: jq is required" >&2; exit 2; }

work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT
elapsed_file="${work_dir}/elapsed-ms"
echo "warning: each request reruns regression tests and upserts result rows" >&2

for ((request = 1; request <= REQUESTS; request++)); do
    meta="$(curl --silent --show-error --request POST \
        "${BASE_URL}/api/projects/${PROJECT_ID}/versions/${VERSION_ID}/regression-tests/run" \
        -H "Authorization: Bearer ${AUTH_TOKEN}" \
        -o "${work_dir}/response.json" \
        -w '%{http_code}\t%{time_total}')"
    status="${meta%%$'\t'*}"
    http_seconds="${meta#*$'\t'}"
    if [[ "${status}" != "200" ]]; then
        echo "error: regression endpoint returned HTTP ${status}" >&2
        jq . "${work_dir}/response.json" >&2 2>/dev/null || cat "${work_dir}/response.json" >&2
        exit 1
    fi
    jq -e '(.elapsedMs | type == "number") and (.generatedCount | type == "number")' \
        "${work_dir}/response.json" >/dev/null || {
        echo "error: regression response lacks elapsedMs or generatedCount" >&2
        exit 1
    }
    generated="$(jq -r '.generatedCount' "${work_dir}/response.json")"
    elapsed="$(jq -r '.elapsedMs' "${work_dir}/response.json")"
    if ((generated < MIN_CASES)); then
        echo "error: generated ${generated} cases; benchmark requires at least ${MIN_CASES}" >&2
        exit 1
    fi
    printf '%s\n' "${elapsed}" >>"${elapsed_file}"
    printf 'request=%s cases=%s server_elapsed_ms=%s http_elapsed_s=%s\n' \
        "${request}" "${generated}" "${elapsed}" "${http_seconds}"
done

max_elapsed="$(sort -n "${elapsed_file}" | tail -n 1)"
printf 'regression benchmark: requests=%s min_cases=%s max_server_elapsed_ms=%s target_ms=%s\n' \
    "${REQUESTS}" "${MIN_CASES}" "${max_elapsed}" "${MAX_ELAPSED_MS}"
if ((max_elapsed > MAX_ELAPSED_MS)); then
    echo "error: max server elapsed ${max_elapsed}ms exceeds ${MAX_ELAPSED_MS}ms" >&2
    exit 1
fi
