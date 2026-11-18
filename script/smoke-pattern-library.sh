#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"
AUTH_TOKEN="${AUTH_TOKEN:-}"
QUERY="${QUERY:-信贷审批流程}"
PROJECT_ID="${PROJECT_ID:-}"
RECOMMENDATION_ID="${RECOMMENDATION_ID:-}"
FEEDBACK_ACCEPTED="${FEEDBACK_ACCEPTED:-true}"
FEEDBACK_FIRST="${FEEDBACK_FIRST:-false}"
REUSE="${REUSE:-false}"
REUSE_PROJECT_NAME="${REUSE_PROJECT_NAME:-pattern-smoke-$(date +%Y%m%d%H%M%S)}"
REUSE_DESCRIPTION="${REUSE_DESCRIPTION:-由模式库 smoke 验证创建的独立草稿}"

if [[ -z "${AUTH_TOKEN}" ]]; then
    echo "error: AUTH_TOKEN is required" >&2
    exit 2
fi
if [[ "${FEEDBACK_ACCEPTED}" != "true" && "${FEEDBACK_ACCEPTED}" != "false" ]]; then
    echo "error: FEEDBACK_ACCEPTED must be true or false" >&2
    exit 2
fi

health_time="$(curl --fail --silent --show-error "${BASE_URL}/api/health" -w '%{time_total}' -o /dev/null)"
echo "health: ok (${health_time}s)"

query_args=(--get --data-urlencode "q=${QUERY}")
if [[ -n "${PROJECT_ID}" ]]; then
    query_args+=(--data-urlencode "projectId=${PROJECT_ID}")
fi

response_file="$(mktemp)"
trap 'rm -f "${response_file}"' EXIT
response_meta="$(curl --silent --show-error "${query_args[@]}" \
    "${BASE_URL}/api/model-library/recommendations" \
    -H "Authorization: Bearer ${AUTH_TOKEN}" \
    -o "${response_file}" \
    -w '%{http_code}\t%{time_total}')"
status_code="${response_meta%%$'\t'*}"
recommendation_time="${response_meta#*$'\t'}"
if [[ "${status_code}" != "200" ]]; then
    echo "error: recommendation endpoint returned HTTP ${status_code}" >&2
    cat "${response_file}" >&2
    exit 1
fi

echo "recommendations latency: ${recommendation_time}s"

analytics_meta="$(curl --silent --show-error \
    "${BASE_URL}/api/model-library/analytics" \
    -H "Authorization: Bearer ${AUTH_TOKEN}" \
    -o /dev/null \
    -w '%{http_code}\t%{time_total}')"
analytics_status="${analytics_meta%%$'\t'*}"
analytics_time="${analytics_meta#*$'\t'}"
if [[ "${analytics_status}" != "200" ]]; then
    echo "error: analytics endpoint returned HTTP ${analytics_status}" >&2
    exit 1
fi
echo "analytics latency: ${analytics_time}s"

if command -v jq >/dev/null 2>&1; then
    recommendation_count="$(jq 'length' "${response_file}")"
    echo "recommendations: ${recommendation_count}"
    jq -r '.[] | "- \(.name) [\(.matchScore)%] recommendationId=\(.recommendationId)"' "${response_file}"
else
    echo "recommendation endpoint: HTTP 200 (install jq for result details)"
fi

if [[ "${FEEDBACK_FIRST}" == "true" && -z "${RECOMMENDATION_ID}" ]]; then
    [[ -x "$(command -v jq || true)" ]] || {
        echo "error: FEEDBACK_FIRST=true requires jq" >&2
        exit 2
    }
    RECOMMENDATION_ID="$(jq -r '.[0].recommendationId // empty' "${response_file}")"
fi

if [[ -n "${RECOMMENDATION_ID}" ]]; then
    curl --fail --silent --show-error -X POST \
        "${BASE_URL}/api/model-library/recommendations/${RECOMMENDATION_ID}/feedback" \
        -H "Authorization: Bearer ${AUTH_TOKEN}" \
        -H 'Content-Type: application/json' \
        -d "{\"accepted\":${FEEDBACK_ACCEPTED}}" >/dev/null
    echo "feedback recorded: recommendation=${RECOMMENDATION_ID} accepted=${FEEDBACK_ACCEPTED}"
fi

if [[ "${REUSE}" == "true" ]]; then
    command -v jq >/dev/null 2>&1 || {
        echo "error: REUSE=true requires jq" >&2
        exit 2
    }
    source_version_id="$(jq -r '.[0].sourceVersionId // empty' "${response_file}")"
    [[ -n "${source_version_id}" ]] || {
        echo "error: no recommendation available for reuse" >&2
        exit 1
    }
    reuse_payload="$(jq -n \
        --arg name "${REUSE_PROJECT_NAME}" \
        --arg description "${REUSE_DESCRIPTION}" \
        '{projectName: $name, description: $description, instantiateConfirmed: true}')"
    reuse_file="$(mktemp)"
    trap 'rm -f "${response_file}" "${reuse_file}"' EXIT
    reuse_status="$(curl --silent --show-error -X POST \
        "${BASE_URL}/api/model-library/history/${source_version_id}/instantiate" \
        -H "Authorization: Bearer ${AUTH_TOKEN}" \
        -H 'Content-Type: application/json' \
        -d "${reuse_payload}" \
        -o "${reuse_file}" \
        -w '%{http_code}')"
    if [[ "${reuse_status}" != "201" ]]; then
        echo "error: reuse endpoint returned HTTP ${reuse_status}" >&2
        cat "${reuse_file}" >&2
        exit 1
    fi
    echo "reuse: created project $(jq -r '.project.id // "unknown"' "${reuse_file}")"
fi
