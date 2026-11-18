use leptos::prelude::*;

use crate::components::dmn_app::team::RollbackPanel;
use crate::models::{
    BehaviorComparisonReport, DecisionVersion, RegressionTestReport, VersionChangeReview,
};

use super::SavedVersionReviewView;

#[component]
pub fn VersionHistoryPanel(
    selected_version_review: ReadSignal<Option<VersionChangeReview>>,
    behavior_comparison: ReadSignal<Option<BehaviorComparisonReport>>,
    regression_report: ReadSignal<Option<RegressionTestReport>>,
    regression_loading: ReadSignal<bool>,
    on_run_regression: Callback<()>,
    selected_version_id: ReadSignal<Option<i64>>,
    versions: ReadSignal<Vec<DecisionVersion>>,
    current_version: ReadSignal<Option<DecisionVersion>>,
    rollback_reason: ReadSignal<String>,
    set_rollback_reason: WriteSignal<String>,
    rollback_confirmed: ReadSignal<bool>,
    set_rollback_confirmed: WriteSignal<bool>,
    rollback_busy: ReadSignal<bool>,
    on_create_rollback: Callback<()>,
    on_close: Callback<()>,
) -> impl IntoView {
    let can_rollback = move || {
        selected_version_id.get().is_some_and(|selected_id| {
            versions.get().iter().any(|version| {
                version.id == selected_id
                    && matches!(version.status.as_str(), "published" | "retired")
                    && current_version
                        .get()
                        .is_none_or(|current| current.id != version.id)
            })
        })
    };
    let can_run_regression = move || {
        selected_version_id.get().is_some_and(|selected_id| {
            versions
                .get()
                .iter()
                .any(|version| version.id == selected_id && version.base_version_id.is_some())
        })
    };

    view! {
        <section class="version-history-panel">
            <div class="version-history-heading">
                <div>
                    <span class="yaml-kicker">"VERSION HISTORY"</span>
                    <strong>"已保存版本审查"</strong>
                </div>
                <button class="button button-quiet" on:click=move |_| on_close.run(())>
                    "关闭"
                </button>
            </div>
            {move || selected_version_review.get().map(|review| view! {
                <SavedVersionReviewView review=review />
            })}
            {move || behavior_comparison.get().map(|report| view! {
                <BehaviorComparisonView report=report />
            })}
            <section class="regression-report">
                <div class="regression-report-heading">
                    <h3>"历史数据回归测试"</h3>
                    <button
                        class="button button-quiet"
                        disabled=move || regression_loading.get() || !can_run_regression()
                        on:click=move |_| on_run_regression.run(())
                    >
                        {move || if regression_loading.get() { "回放中..." } else { "运行历史回归" }}
                    </button>
                </div>
                {move || regression_report.get().map(|report| view! {
                    <RegressionReportView report=report />
                })}
            </section>
            <Show when=can_rollback>
                <RollbackPanel
                    rollback_reason=rollback_reason
                    set_rollback_reason=set_rollback_reason
                    rollback_confirmed=rollback_confirmed
                    set_rollback_confirmed=set_rollback_confirmed
                    rollback_busy=rollback_busy
                    on_create_rollback=on_create_rollback
                />
            </Show>
        </section>
    }
}

#[component]
fn RegressionReportView(report: RegressionTestReport) -> impl IntoView {
    let cases = report.cases;
    view! {
        <div class="regression-result">
            <strong class=format!("review-risk risk-{}", report.risk_level)>
                {format!("{} 风险", report.risk_level)}
            </strong>
            <span>{format!("基线 #{} → 候选 #{}：共 {} 例，通过 {}，失败 {}，错误 {}；去重 {} 例；覆盖历史样本 {}/{}（{:.1}%）；耗时 {} ms。", report.baseline_version_id, report.candidate_version_id, report.generated_count, report.passed, report.failed, report.errors, report.duplicate_count, report.generated_count + report.duplicate_count, report.available_source_count, report.source_coverage_percent, report.elapsed_ms)}</span>
            <For each=move || cases.clone() key=|case| case.source_run_id let:case>
                <div class=format!("regression-case risk-{}", case.risk_level)>
                    <strong>{format!("历史执行 #{} · {}", case.source_run_id, case.status)}</strong>
                    <small>{format!("期望 {} · 实际 {}", case.expected_output, case.actual_output)}</small>
                </div>
            </For>
        </div>
    }
}

#[component]
fn BehaviorComparisonView(report: BehaviorComparisonReport) -> impl IntoView {
    let comparisons = report.comparisons;
    view! {
        <section class="behavior-comparison">
            <h3>{format!("版本行为对比（{} 组相同输入）", report.matched_cases)}</h3>
            <p>{format!("基线版本 #{}：最近抽样 {}/{} 条；候选版本 #{}：最近抽样 {}/{} 条；样本内未配对：基线 {} 条、候选 {} 条。", report.baseline_version_id, report.baseline_sampled_runs, report.baseline_total_runs, report.candidate_version_id, report.candidate_sampled_runs, report.candidate_total_runs, report.unmatched_baseline_runs, report.unmatched_candidate_runs)}</p>
            <For each=move || comparisons.clone() key=|entry| (entry.baseline_run_id, entry.candidate_run_id) let:entry>
                <article class=format!("behavior-diff risk-{}", entry.diff.risk_level)>
                    <strong>{format!("执行记录 {} → {} · {} 风险", entry.baseline_run_id, entry.candidate_run_id, entry.diff.risk_level)}</strong>
                    <Show when=move || entry.diff.execution_order_changed>
                        <span>"决策执行顺序发生变化"</span>
                    </Show>
                    <For each=move || entry.diff.decision_diffs.clone() key=|diff| diff.decision_id.clone() let:diff>
                        <div class="behavior-diff-row">
                            <strong>{diff.decision_name}</strong>
                            <span>{format!("{} · 执行顺序 {} → {}", diff.change_type, diff.baseline_order.map(|order| order.to_string()).unwrap_or_else(|| "无".into()), diff.candidate_order.map(|order| order.to_string()).unwrap_or_else(|| "无".into()))}</span>
                            <small>{format!("{} → {}", diff.baseline_result.map(|value| value.to_string()).unwrap_or_else(|| "无".into()), diff.candidate_result.map(|value| value.to_string()).unwrap_or_else(|| "无".into()))}</small>
                            <small class="behavior-diff-rule-path">
                                "规则命中：" {diff.baseline_matched_rule_ids.join(", ")} " → " {diff.candidate_matched_rule_ids.join(", ")}
                            </small>
                        </div>
                    </For>
                    <For each=move || entry.diff.output_diffs.clone() key=|diff| diff.path.clone() let:diff>
                        <div class="behavior-diff-row">
                            <strong>{diff.path}</strong>
                            <span>{format!("{} · {} 风险", diff.change_type, diff.risk_level)}</span>
                            <small>{format!("{} → {}", diff.baseline_value.map(|value| value.to_string()).unwrap_or_else(|| "无".into()), diff.candidate_value.map(|value| value.to_string()).unwrap_or_else(|| "无".into()))}</small>
                        </div>
                    </For>
                </article>
            </For>
        </section>
    }
}
