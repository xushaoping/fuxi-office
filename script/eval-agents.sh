#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

results_root="${EVAL_RESULTS_DIR:-target/eval-results}"
run_started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$"
results_dir="${results_root}/${run_id}"
mkdir -p "${results_dir}"

commit="$(git rev-parse HEAD 2>/dev/null || printf 'unknown')"
worktree="clean"
if ! git diff --quiet || ! git diff --cached --quiet || [[ -n "$(git ls-files --others --exclude-standard)" ]]; then
    worktree="dirty"
fi
rustc_version="$(rustc --version)"

declare -a suite_names=("需求到 DSL" "需求分析" "规划结构" "修复分类" "复杂度路由")
declare -a suite_logs=("dsl.log" "analysis.log" "planning.log" "repair.log" "complexity.log")
declare -a suite_results=()
declare -a suite_test_counts=()
declare -a suite_case_counts=()
failure_count=0

count_golden_cases() {
    awk 'NF && substr($0, 1, 1) != "#" { count++ } END { print count + 0 }' "$1"
}

suite_case_counts=(
    "$(count_golden_cases tests/golden/age-thresholds.tsv)"
    "$(count_golden_cases tests/golden/business-patterns.tsv)"
    "$(count_golden_cases tests/golden/planning-contracts.tsv)"
    "$(count_golden_cases tests/golden/repair-diagnostics.tsv)"
    "$(count_golden_cases tests/golden/requirement-complexity.tsv)"
)
suite_golden_files=(
    "tests/golden/age-thresholds.tsv"
    "tests/golden/business-patterns.tsv"
    "tests/golden/planning-contracts.tsv"
    "tests/golden/repair-diagnostics.tsv"
    "tests/golden/requirement-complexity.tsv"
)
suite_golden_hashes=()
for file in "${suite_golden_files[@]}"; do
    suite_golden_hashes+=("$(shasum -a 256 "$file" | awk '{print $1}')")
done
suite_minimum_cases=(50 20 20 18 28)

run_suite() {
    local index="$1"
    local minimum_cases="$2"
    shift
    shift
    if "$@" -- --nocapture >"${results_dir}/${suite_logs[index]}" 2>&1; then
        suite_test_counts[index]="$(awk '$1 == "running" && $2 ~ /^[0-9]+$/ { print $2; exit }' \
            "${results_dir}/${suite_logs[index]}")"
        if [[ "${suite_test_counts[index]:-0}" =~ ^[1-9][0-9]*$ \
            && "${suite_case_counts[index]}" =~ ^[0-9]+$ \
            && "${suite_case_counts[index]}" -ge "${minimum_cases}" ]]; then
            suite_results[index]="通过"
        else
            suite_results[index]="失败"
            failure_count=$((failure_count + 1))
            printf 'error: %s has %s golden cases (minimum %s) and ran %s Rust tests\n' \
                "${suite_names[index]}" "${suite_case_counts[index]}" \
                "${minimum_cases}" "${suite_test_counts[index]:-0}" >&2
        fi
    else
        suite_results[index]="失败"
        suite_test_counts[index]="0"
        failure_count=$((failure_count + 1))
    fi
    printf '%s：%s，黄金案例 %s 条，Rust 测试 %s 项（日志：%s）\n' \
        "${suite_names[index]}" "${suite_results[index]}" \
        "${suite_case_counts[index]}" "${suite_test_counts[index]}" \
        "${suite_logs[index]}"
}

run_suite 0 "${suite_minimum_cases[0]}" cargo test --bin fuxi-dmn nl_dsl::evaluation::tests
run_suite 1 "${suite_minimum_cases[1]}" cargo test --bin fuxi-dmn nl_dsl::analysis_engine::tests::golden_requirement_patterns_match_expected_analysis_categories
run_suite 2 "${suite_minimum_cases[2]}" cargo test --bin fuxi-dmn workflow_api::tests::golden_planning_requirements_satisfy_the_plan_contract
run_suite 3 "${suite_minimum_cases[3]}" cargo test --bin fuxi-dmn nl_dsl::repair::tests::golden_repair_diagnostics_match_expected_categories
run_suite 4 "${suite_minimum_cases[4]}" cargo test --bin fuxi-dmn workflow_api::tests::golden_requirement_complexities_match_expected_routes

complexity_metrics_file="${results_dir}/complexity-metrics.json"
if cargo run --quiet --bin eval-complexity -- tests/golden/requirement-complexity.tsv \
    >"${complexity_metrics_file}" 2>"${results_dir}/complexity-metrics.log"; then
    printf '复杂度路由指标已保存：%s\n' "${complexity_metrics_file}"
else
    suite_results[4]="失败"
    failure_count=$((failure_count + 1))
    printf 'error: failed to calculate complexity metrics; see complexity-metrics.log\n' >&2
fi

prompt_assets_file="${results_dir}/prompt-assets.json"
if cargo run --quiet --bin eval-prompt-assets >"${prompt_assets_file}" \
    2>"${results_dir}/prompt-assets.log"; then
    printf '内置提示词版本与摘要已保存：%s\n' "${prompt_assets_file}"
else
    failure_count=$((failure_count + 1))
    printf 'error: failed to calculate prompt asset digests; see prompt-assets.log\n' >&2
fi

{
    printf '<!doctype html>\n<html lang="zh-CN"><meta charset="utf-8">\n'
    printf '<title>Agent Golden Evaluation</title>\n'
    printf '<style>body{font:16px system-ui,sans-serif;max-width:760px;margin:40px auto;padding:0 20px;color:#202124}table{border-collapse:collapse;width:100%%}th,td{padding:10px 12px;border:1px solid #d7dce0;text-align:left}th{background:#f2f5f7}.pass{color:#18794e}.fail{color:#b42318}</style>\n'
    printf '<h1>Agent Golden Evaluation</h1>\n'
    printf '<p>Suites: %d; failed: %d; commit: %s; worktree: %s; Rust: %s</p>\n' \
        "${#suite_names[@]}" "${failure_count}" "${commit}" "${worktree}" "${rustc_version}"
    printf '<table><thead><tr><th>任务</th><th>结果</th><th>黄金案例</th><th>Rust 测试项</th><th>日志</th></tr></thead><tbody>\n'
    for index in "${!suite_names[@]}"; do
        class="pass"
        [[ "${suite_results[index]}" == "失败" ]] && class="fail"
        printf '<tr><td>%s</td><td class="%s">%s</td><td>%s</td><td>%s</td><td><a href="%s">查看</a></td></tr>\n' \
            "${suite_names[index]}" "${class}" "${suite_results[index]}" \
            "${suite_case_counts[index]}" "${suite_test_counts[index]}" \
            "${suite_logs[index]}"
    done
    printf '</tbody></table>\n'
    printf '<p>复杂度分类指标：<a href="complexity-metrics.json">JSON 报告</a></p>\n'
    printf '<p>提示词版本与摘要：<a href="prompt-assets.json">JSON 清单</a>（仅离线代码契约回归，不代表 LLM 生成质量评测）</p>\n'
    printf '</html>\n'
} >"${results_dir}/report.html"

{
    printf '{\n'
    printf '  "runStartedAt": "%s",\n' "${run_started_at}"
    printf '  "commit": "%s",\n' "${commit}"
    printf '  "worktree": "%s",\n' "${worktree}"
    printf '  "rustc": "%s",\n' "${rustc_version}"
    printf '  "failedSuites": %d,\n' "${failure_count}"
    printf '  "complexityMetrics": "complexity-metrics.json",\n'
    printf '  "promptAssets": "prompt-assets.json",\n'
    printf '  "goldenFiles": [\n'
    for index in "${!suite_golden_files[@]}"; do
        comma=,
        [[ "${index}" -eq $((${#suite_golden_files[@]} - 1)) ]] && comma=
        printf '    {"path":"%s","sha256":"%s","cases":%s}%s\n' \
            "${suite_golden_files[index]}" "${suite_golden_hashes[index]}" \
            "${suite_case_counts[index]}" "${comma}"
    done
    printf '  ],\n'
    printf '  "suites": [\n'
    for index in "${!suite_names[@]}"; do
        comma=,
        [[ "${index}" -eq $((${#suite_names[@]} - 1)) ]] && comma=
        printf '    {"name":"%s","result":"%s","goldenCases":%s,"rustTests":%s}%s\n' \
            "${suite_names[index]}" "${suite_results[index]}" \
            "${suite_case_counts[index]}" "${suite_test_counts[index]:-0}" "${comma}"
    done
    printf '  ]\n}\n'
} >"${results_dir}/manifest.json"

printf '评测报告：%s/report.html（元数据：manifest.json）\n' "${results_dir}"
(( failure_count == 0 ))
