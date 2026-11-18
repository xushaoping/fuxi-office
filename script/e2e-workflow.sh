#!/usr/bin/env bash
set -euo pipefail

# End-to-end acceptance runner for:
# requirement -> analysis -> plan -> confirmation -> DSL -> compile ->
# version -> business cases -> independent review -> verification -> publish ->
# analytics -> optimization proposal -> pattern recommendation.
#
# The script intentionally uses already-issued tokens. Login CAPTCHA solving is
# an operator concern and must not be bypassed by a test helper.

BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"
AUTH_TOKEN="${AUTH_TOKEN:-}"
REVIEW_AUTH_TOKEN="${REVIEW_AUTH_TOKEN:-}"
PUBLISH_AUTH_TOKEN="${PUBLISH_AUTH_TOKEN:-}"
AUTH_USERNAME="${AUTH_USERNAME:-}"
REVIEW_USERNAME="${REVIEW_USERNAME:-}"
PUBLISH_USERNAME="${PUBLISH_USERNAME:-}"
PROJECT_NAME="${PROJECT_NAME:-e2e-$(date +%Y%m%d%H%M%S)}"
PROJECT_DESCRIPTION="${PROJECT_DESCRIPTION:-Intelligence upgrade API E2E validation}"
RISK_LEVEL="${RISK_LEVEL:-high}"
REQUIREMENT="${REQUIREMENT:-订单总额达到 500 元时给予 95 折，否则不打折。输入 orderTotal number，输出 discountRate number。}"
ANSWERS_JSON="${ANSWERS_JSON:-[]}"
ANSWERS_ROUNDS_JSON="${ANSWERS_ROUNDS_JSON:-}"
TEST_CASES_JSON="${TEST_CASES_JSON:-}"

command -v curl >/dev/null 2>&1 || { echo "error: curl is required" >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { echo "error: jq is required" >&2; exit 2; }

for variable in AUTH_TOKEN REVIEW_AUTH_TOKEN PUBLISH_AUTH_TOKEN AUTH_USERNAME REVIEW_USERNAME PUBLISH_USERNAME; do
    if [[ -z "${!variable}" ]]; then
        echo "error: ${variable} is required" >&2
        exit 2
    fi
done

if [[ "${AUTH_TOKEN}" == "${REVIEW_AUTH_TOKEN}" || "${AUTH_TOKEN}" == "${PUBLISH_AUTH_TOKEN}" || "${REVIEW_AUTH_TOKEN}" == "${PUBLISH_AUTH_TOKEN}" ]]; then
    echo "error: AUTH_TOKEN, REVIEW_AUTH_TOKEN, and PUBLISH_AUTH_TOKEN must represent three distinct users" >&2
    exit 2
fi
if [[ "${AUTH_USERNAME}" == "${REVIEW_USERNAME}" || "${AUTH_USERNAME}" == "${PUBLISH_USERNAME}" || "${REVIEW_USERNAME}" == "${PUBLISH_USERNAME}" ]]; then
    echo "error: AUTH_USERNAME, REVIEW_USERNAME, and PUBLISH_USERNAME must be three distinct users" >&2
    exit 2
fi

jq -e 'type == "array"' >/dev/null <<<"${ANSWERS_JSON}" || {
    echo "error: ANSWERS_JSON must be a JSON array" >&2
    exit 2
}
if [[ -n "${ANSWERS_ROUNDS_JSON}" ]]; then
    jq -e 'type == "array" and all(.[]; type == "array")' >/dev/null <<<"${ANSWERS_ROUNDS_JSON}" || {
        echo "error: ANSWERS_ROUNDS_JSON must be an array of answer arrays" >&2
        exit 2
    }
fi
if [[ -z "${TEST_CASES_JSON}" ]]; then
    cat >&2 <<'EOF'
error: TEST_CASES_JSON is required; provide at least one business acceptance case, for example:
  TEST_CASES_JSON='[{"name":"满额折扣","inputJson":{"orderTotal":500},"expectedOutputJson":{"discountRate":0.95}}]'
EOF
    exit 2
fi
jq -e 'type == "array" and length > 0 and all(.[]; .name and .inputJson and .expectedOutputJson)' \
    >/dev/null <<<"${TEST_CASES_JSON}" || {
    echo "error: TEST_CASES_JSON must contain name, inputJson, and expectedOutputJson" >&2
    exit 2
}

work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

request() {
    local method="$1"
    local path="$2"
    local token="$3"
    local payload="$4"
    local expected_status="$5"
    local output_file="$6"
    local status

    if [[ -n "${payload}" ]]; then
        status="$(curl --silent --show-error --request "${method}" \
            "${BASE_URL}${path}" \
            -H "Authorization: Bearer ${token}" \
            -H 'Content-Type: application/json' \
            --data "${payload}" \
            --output "${output_file}" \
            --write-out '%{http_code}')"
    else
        status="$(curl --silent --show-error --request "${method}" \
            "${BASE_URL}${path}" \
            -H "Authorization: Bearer ${token}" \
            --output "${output_file}" \
            --write-out '%{http_code}')"
    fi

    if [[ "${status}" != "${expected_status}" ]]; then
        echo "error: ${method} ${path} returned HTTP ${status}, expected ${expected_status}" >&2
        cat "${output_file}" >&2
        return 1
    fi
}

request_sse() {
    local method="$1"
    local path="$2"
    local token="$3"
    local payload="$4"
    local expected_status="$5"
    local result_event="$6"
    local output_file="$7"
    local status

    status="$(curl --silent --show-error --no-buffer --request "${method}" \
        "${BASE_URL}${path}" \
        -H "Authorization: Bearer ${token}" \
        -H 'Content-Type: application/json' \
        -H 'Accept: text/event-stream' \
        --data "${payload}" \
        --output "${output_file}" \
        --write-out '%{http_code}')"
    if [[ "${status}" != "${expected_status}" ]]; then
        echo "error: ${method} ${path} returned HTTP ${status}, expected ${expected_status}" >&2
        cat "${output_file}" >&2
        return 1
    fi
    python3 script/extract-sse-result.py "${output_file}" "${result_event}" \
        >"${output_file}.json"
    mv "${output_file}.json" "${output_file}"
}

json_field() {
    local file="$1"
    local filter="$2"
    jq -er "${filter}" "${file}"
}

prompt_required_answers() {
    local workflow_file="$1"
    local answers='[]'
    local clarification_id
    local question
    local answer

    if [[ ! -t 0 ]]; then
        echo "error: another clarification round needs answers; rerun interactively or provide ANSWERS_ROUNDS_JSON" >&2
        jq -r '.clarifications[] | select(.required == true) | "- [\(.id)] \(.question)"' \
            "${workflow_file}" >&2
        return 1
    fi

    while IFS= read -r clarification_id; do
        question="$(jq -r --arg id "${clarification_id}" \
            '.clarifications[] | select(.id == $id) | .question' "${workflow_file}")"
        printf '[%s] %s\nAnswer: ' "${clarification_id}" "${question}" >&2
        if ! IFS= read -r answer; then
            echo "error: failed to read a clarification answer" >&2
            return 1
        fi
        answers="$(jq -c --arg id "${clarification_id}" --arg answer "${answer}" \
            '. + [{clarificationId:$id,answer:$answer}]' <<<"${answers}")"
    done < <(jq -r '.clarifications[] | select(.required == true) | .id' "${workflow_file}")

    printf '%s' "${answers}"
}

echo "[1/17] creating project"
request POST /api/projects "${AUTH_TOKEN}" \
    "$(jq -nc --arg name "${PROJECT_NAME}" --arg description "${PROJECT_DESCRIPTION}" --arg risk "${RISK_LEVEL}" \
        '{name:$name,description:$description,riskLevel:$risk}')" 201 "${work_dir}/project.json"
project_id="$(json_field "${work_dir}/project.json" '.id')"

echo "[2/17] assigning independent review and publish roles"
review_username_path="$(jq -rn --arg username "${REVIEW_USERNAME}" '$username | @uri')"
publish_username_path="$(jq -rn --arg username "${PUBLISH_USERNAME}" '$username | @uri')"
request PUT "/api/projects/${project_id}/members/${review_username_path}" "${AUTH_TOKEN}" \
    '{"role":"reviewer"}' 200 "${work_dir}/reviewer-member.json"
request PUT "/api/projects/${project_id}/members/${publish_username_path}" "${AUTH_TOKEN}" \
    '{"role":"approver"}' 200 "${work_dir}/approver-member.json"

echo "[3/17] analyzing requirement (risk=${RISK_LEVEL})"
request_sse POST "/api/projects/${project_id}/workflow/orchestrate-stream" "${AUTH_TOKEN}" \
    "$(jq -nc --arg text "${REQUIREMENT}" '{text:$text}')" 200 workflow_result "${work_dir}/analysis.json"
analysis_status="$(json_field "${work_dir}/analysis.json" '.status')"
analysis_updated_at="$(json_field "${work_dir}/analysis.json" '.updatedAt')"
clarification_round=0
clarification_answers='[]'
while [[ "${analysis_status}" == "clarification" ]]; do
    if [[ -n "${ANSWERS_ROUNDS_JSON}" ]]; then
        clarification_answers="$(jq -c --argjson round "${clarification_round}" \
            '.[$round] // []' <<<"${ANSWERS_ROUNDS_JSON}")"
    elif [[ "${clarification_round}" == "0" ]]; then
        clarification_answers="${ANSWERS_JSON}"
    else
        clarification_answers='[]'
    fi
    required_clarifications="$(jq '[.clarifications[]? | select(.required == true)] | length' "${work_dir}/analysis.json")"
    if [[ "${required_clarifications}" != "0" && "$(jq 'length' <<<"${clarification_answers}")" == "0" ]]; then
        clarification_answers="$(prompt_required_answers "${work_dir}/analysis.json")"
    fi
    echo "[3.$((clarification_round + 1))/17] continuing clarification"
    request_sse POST "/api/projects/${project_id}/workflow/orchestrate-stream" "${AUTH_TOKEN}" \
        "$(jq -nc --argjson answers "${clarification_answers}" --arg updatedAt "${analysis_updated_at}" \
            '{answers:$answers,expectedUpdatedAt:$updatedAt}')" \
        200 workflow_result "${work_dir}/analysis.json"
    analysis_status="$(json_field "${work_dir}/analysis.json" '.status')"
    analysis_updated_at="$(json_field "${work_dir}/analysis.json" '.updatedAt')"
    clarification_answers='[]'
    clarification_round=$((clarification_round + 1))
    if [[ "${analysis_status}" == "clarification" && "${clarification_round}" -ge 8 ]]; then
        echo "error: workflow did not converge after 8 clarification rounds" >&2
        jq -r '.clarifications[] | select(.required == true) | "- [\(.id)] \(.question)"' \
            "${work_dir}/analysis.json" >&2
        exit 1
    fi
done

echo "[4/17] planning decision model (analysis status=${analysis_status})"
request_sse POST "/api/projects/${project_id}/workflow/orchestrate-stream" "${AUTH_TOKEN}" \
    "$(jq -nc --arg updatedAt "${analysis_updated_at}" \
        '{expectedUpdatedAt:$updatedAt}')" 200 workflow_result "${work_dir}/plan.json"
plan_updated_at="$(json_field "${work_dir}/plan.json" '.updatedAt')"

echo "[5/17] confirming plan"
request POST "/api/projects/${project_id}/workflow/confirm" "${AUTH_TOKEN}" \
    "$(jq -nc --arg updatedAt "${plan_updated_at}" '{expectedUpdatedAt:$updatedAt}')" 200 "${work_dir}/confirmed.json"
workflow_updated_at="$(json_field "${work_dir}/confirmed.json" '.updatedAt')"
decision_brief="$(jq -c '.brief' "${work_dir}/confirmed.json")"
decision_plan="$(jq -c '.plan' "${work_dir}/confirmed.json")"

echo "[6/17] generating DSL"
request_sse POST "/api/projects/${project_id}/workflow/orchestrate-stream" "${AUTH_TOKEN}" \
    "$(jq -nc --arg updatedAt "${workflow_updated_at}" \
        '{expectedUpdatedAt:$updatedAt}')" 200 result "${work_dir}/generated.json"
if [[ "$(json_field "${work_dir}/generated.json" '.ok')" != "true" ]]; then
    echo "error: unified workflow did not produce a successful DSL result" >&2
    jq . "${work_dir}/generated.json" >&2
    exit 1
fi
dsl="$(json_field "${work_dir}/generated.json" '.dsl')"
knowledge_citations="$(jq -c '.knowledgeCitations // []' "${work_dir}/generated.json")"

echo "[7/17] validating and compiling DSL"
request POST /api/validate "${AUTH_TOKEN}" \
    "$(jq -nc --arg dsl "${dsl}" '{dsl:$dsl}')" 200 "${work_dir}/validation.json"
json_field "${work_dir}/validation.json" '.ok' | grep -qx 'true'
request POST /api/compile "${AUTH_TOKEN}" \
    "$(jq -nc --arg dsl "${dsl}" '{dsl:$dsl}')" 200 "${work_dir}/compile.json"
xml="$(json_field "${work_dir}/compile.json" '.xml')"

echo "[8/17] creating draft version"
request POST "/api/projects/${project_id}/versions" "${AUTH_TOKEN}" \
    "$(jq -nc --arg dsl "${dsl}" --arg xml "${xml}" --argjson citations "${knowledge_citations}" \
        --argjson brief "${decision_brief}" --argjson plan "${decision_plan}" \
        '{dslYaml:$dsl,dmnXml:$xml,knowledgeCitations:$citations,decisionBrief:$brief,decisionPlan:$plan,changeConfirmed:true}')" \
    201 "${work_dir}/version.json"
version_id="$(json_field "${work_dir}/version.json" '.id')"

echo "[9/17] creating and running business acceptance cases"
case_ids=()
while IFS= read -r test_case; do
    request POST "/api/projects/${project_id}/versions/${version_id}/test-cases" "${AUTH_TOKEN}" \
        "${test_case}" 201 "${work_dir}/case.json"
    case_ids+=("$(json_field "${work_dir}/case.json" '.id')")
done < <(jq -c '.[] | {name,inputJson,expectedOutputJson}' <<<"${TEST_CASES_JSON}")
request POST "/api/projects/${project_id}/versions/${version_id}/test-cases/run" "${AUTH_TOKEN}" '' 200 "${work_dir}/case-results.json"
if [[ "$(jq '[.[] | select(.status != "passed")] | length' "${work_dir}/case-results.json")" != "0" ]]; then
    echo "error: at least one business acceptance case failed" >&2
    jq . "${work_dir}/case-results.json" >&2
    exit 1
fi

echo "[10/17] approving business acceptance cases"
for case_id in "${case_ids[@]}"; do
    request PUT "/api/projects/${project_id}/versions/${version_id}/test-cases/${case_id}/approval" "${REVIEW_AUTH_TOKEN}" \
        '{"approved":true}' 200 "${work_dir}/approval-${case_id}.json"
done

echo "[11/17] recording independent version review"
request POST "/api/projects/${project_id}/versions/${version_id}/review-approval" "${REVIEW_AUTH_TOKEN}" '' 200 "${work_dir}/review.json"

echo "[12/17] running technical and business verification"
request POST "/api/projects/${project_id}/versions/${version_id}/verification" "${AUTH_TOKEN}" '' 200 "${work_dir}/verification.json"
if [[ "$(json_field "${work_dir}/verification.json" '.status')" != "passed" ]]; then
    echo "error: verification did not pass" >&2
    jq . "${work_dir}/verification.json" >&2
    exit 1
fi

echo "[13/17] publishing version"
request POST "/api/projects/${project_id}/versions/${version_id}/publish" "${PUBLISH_AUTH_TOKEN}" \
    '{"publishConfirmed":true}' 200 "${work_dir}/published.json"

echo "[14/17] verifying published-version analytics"
request GET "/api/projects/${project_id}/decision-analytics" "${AUTH_TOKEN}" '' 200 "${work_dir}/analytics.json"
if [[ "$(json_field "${work_dir}/analytics.json" '.versionId')" != "${version_id}" ]]; then
    echo "error: analytics report does not match the published version" >&2
    exit 1
fi

echo "[15/17] generating cold-start optimization proposal"
request POST "/api/projects/${project_id}/optimization-proposals/generate" "${AUTH_TOKEN}" '' 200 "${work_dir}/proposals.json"
if [[ "$(jq '[.[] | select(.findingCode == "NO_RECENT_RUNS")] | length' "${work_dir}/proposals.json")" == "0" ]]; then
    echo "error: expected a no-recent-runs proposal for the newly published project" >&2
    jq . "${work_dir}/proposals.json" >&2
    exit 1
fi

echo "[16/17] querying decision-pattern recommendations"
requirement_query="$(jq -rn --arg text "${REQUIREMENT}" '$text | @uri')"
request GET "/api/model-library/recommendations?q=${requirement_query}&projectId=${project_id}" \
    "${AUTH_TOKEN}" '' 200 "${work_dir}/recommendations.json"
jq -e 'type == "array"' "${work_dir}/recommendations.json" >/dev/null

echo "[17/17] verifying published pattern was indexed"
request GET /api/model-library/analytics "${AUTH_TOKEN}" '' 200 "${work_dir}/pattern-analytics.json"
if [[ "$(json_field "${work_dir}/pattern-analytics.json" '.totalPatterns')" -lt 1 ]]; then
    echo "error: published decision pattern was not added to the model library" >&2
    jq . "${work_dir}/pattern-analytics.json" >&2
    exit 1
fi

echo "workflow E2E passed: project=${project_id} version=${version_id}"
