#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"
AUTH_TOKEN="${AUTH_TOKEN:-}"
PROJECT_ID="${PROJECT_ID:-}"
REQUESTS="${REQUESTS:-100}"
WARMUP_REQUESTS="${WARMUP_REQUESTS:-5}"
MAX_P95_SECONDS="${MAX_P95_SECONDS:-1}"

if [[ -z "${AUTH_TOKEN}" || -z "${PROJECT_ID}" ]]; then
    echo "error: AUTH_TOKEN and PROJECT_ID are required" >&2
    exit 2
fi
[[ "${PROJECT_ID}" =~ ^[1-9][0-9]*$ ]] || {
    echo "error: PROJECT_ID must be a positive integer" >&2
    exit 2
}
[[ "${REQUESTS}" =~ ^[1-9][0-9]*$ ]] || {
    echo "error: REQUESTS must be a positive integer" >&2
    exit 2
}
[[ "${WARMUP_REQUESTS}" =~ ^[0-9]+$ ]] || {
    echo "error: WARMUP_REQUESTS must be a non-negative integer" >&2
    exit 2
}
[[ "${MAX_P95_SECONDS}" =~ ^[0-9]+([.][0-9]+)?$ ]] || {
    echo "error: MAX_P95_SECONDS must be a non-negative number" >&2
    exit 2
}

BASE_URL="${BASE_URL%/}"

timings_file="$(mktemp)"
trap 'rm -f "${timings_file}"' EXIT
request_analytics() {
    meta="$(curl --silent --show-error \
        "${BASE_URL}/api/projects/${PROJECT_ID}/decision-analytics" \
        -H "Authorization: Bearer ${AUTH_TOKEN}" \
        -o /dev/null \
        -w '%{http_code}\t%{time_total}')"
    status="${meta%%$'\t'*}"
    timing="${meta#*$'\t'}"
    if [[ "${status}" != "200" ]]; then
        echo "error: request ${request} returned HTTP ${status}" >&2
        exit 1
    fi
    printf '%s\n' "${timing}"
}

for ((request=1; request<=WARMUP_REQUESTS; request++)); do
    request_analytics >/dev/null
done
for ((request=1; request<=REQUESTS; request++)); do
    request_analytics >>"${timings_file}"
done

read -r p50 p95 p99 max < <(sort -n "${timings_file}" | awk '
    { values[NR] = $1 }
    END {
        if (NR == 0) exit 1
        p50 = values[int((NR * 0.50) + 0.999999)]
        p95 = values[int((NR * 0.95) + 0.999999)]
        p99 = values[int((NR * 0.99) + 0.999999)]
        print p50, p95, p99, values[NR]
    }')

printf 'decision analytics: project=%s samples=%s warmups=%s p50=%ss p95=%ss p99=%ss max=%ss\n' \
    "${PROJECT_ID}" "${REQUESTS}" "${WARMUP_REQUESTS}" "${p50}" "${p95}" "${p99}" "${max}"
awk -v actual="${p95}" -v limit="${MAX_P95_SECONDS}" \
    'BEGIN { if (actual > limit) { printf "error: p95 %.3fs exceeds %.3fs\n", actual, limit; exit 1 } }'
