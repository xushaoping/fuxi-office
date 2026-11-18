#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"
AUTH_TOKEN="${AUTH_TOKEN:-}"
QUERY="${QUERY:-信贷审批流程}"
PROJECT_ID="${PROJECT_ID:-}"
REQUESTS="${REQUESTS:-20}"
MAX_P99_SECONDS="${MAX_P99_SECONDS:-30}"

if [[ -z "${AUTH_TOKEN}" ]]; then
    echo "error: AUTH_TOKEN is required" >&2
    exit 2
fi
[[ "${REQUESTS}" =~ ^[1-9][0-9]*$ ]] || {
    echo "error: REQUESTS must be a positive integer" >&2
    exit 2
}

query_args=(--get --data-urlencode "q=${QUERY}")
if [[ -n "${PROJECT_ID}" ]]; then
    query_args+=(--data-urlencode "projectId=${PROJECT_ID}")
fi

timings_file="$(mktemp)"
trap 'rm -f "${timings_file}"' EXIT
for ((request=1; request<=REQUESTS; request++)); do
    meta="$(curl --silent --show-error "${query_args[@]}" \
        "${BASE_URL}/api/model-library/recommendations" \
        -H "Authorization: Bearer ${AUTH_TOKEN}" \
        -o /dev/null \
        -w '%{http_code}\t%{time_total}')"
    status="${meta%%$'\t'*}"
    timing="${meta#*$'\t'}"
    if [[ "${status}" != "200" ]]; then
        echo "error: request ${request} returned HTTP ${status}" >&2
        exit 1
    fi
    printf '%s\n' "${timing}" >>"${timings_file}"
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

printf 'pattern recommendations: requests=%s p50=%ss p95=%ss p99=%ss max=%ss\n' \
    "${REQUESTS}" "${p50}" "${p95}" "${p99}" "${max}"
awk -v actual="${p99}" -v limit="${MAX_P99_SECONDS}" \
    'BEGIN { if (actual > limit) { printf "error: p99 %.3fs exceeds %.3fs\n", actual, limit; exit 1 } }'
