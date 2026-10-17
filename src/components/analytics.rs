use leptos::prelude::*;

use crate::components::workspace::{analytics_finding_label, analytics_percent};
use crate::models::{DecisionAnalyticsFinding, DecisionAnalyticsReport, DecisionAnalyticsSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleHitData {
    pub rule_id: String,
    pub rule_name: String,
    pub hit_count: u32,
}

/// Compact, responsive rule-hit heatmap for analytics pages.
#[component]
pub fn ExecutionHeatmap(rule_hits: Vec<RuleHitData>) -> impl IntoView {
    let max_hits = rule_hits
        .iter()
        .map(|rule| rule.hit_count)
        .max()
        .unwrap_or(1);
    view! {
        <section class="heatmap-container" aria-labelledby="heatmap-title">
            <h3 id="heatmap-title">"规则命中热图"</h3>
            <div class="heatmap-grid">
                <For each=move || rule_hits.clone() key=|rule| rule.rule_id.clone() children=move |rule| {
                    let intensity = (rule.hit_count.saturating_mul(100) / max_hits).min(100);
                    let name = rule.rule_name.clone();
                    view! {
                        <div class="heatmap-cell" style=format!("--heat-intensity: {intensity}%") title=format!("{}: {} 次命中", name, rule.hit_count)>
                            <span class="cell-label">{name.clone()}</span><span class="cell-count">{rule.hit_count}</span>
                        </div>
                    }
                }/>
            </div>
            <div class="heatmap-scale" aria-label="命中次数从低到高"><span>"低"</span><span class="scale-gradient"></span><span>"高"</span></div>
        </section>
    }
}

#[component]
pub fn DecisionAnalyticsHighlights(
    summary: DecisionAnalyticsSummary,
    findings: Vec<DecisionAnalyticsFinding>,
) -> impl IntoView {
    let findings = RwSignal::new(findings);
    view! {
        <div class="analytics-summary">
            <div><strong>{summary.total_runs}</strong><span>"运行总量"</span></div>
            <div><strong>{format!("{:.1}%", summary.failure_rate * 100.0)}</strong><span>{format!("失败 {} 次", summary.failed_runs)}</span></div>
            <div><strong>{format!("{:.1}%", summary.feedback_coverage_rate * 100.0)}</strong><span>{format!("反馈 {} 次", summary.outcome_reported_runs)}</span></div>
            <div class:has-alert={summary.mismatch_runs > 0}><strong>{format!("{:.1}%", summary.mismatch_rate * 100.0)}</strong><span>{format!("偏差 {} 次", summary.mismatch_runs)}</span></div>
            <div><strong>{format!("{:.0} ms", summary.average_duration_ms)}</strong><span>"平均耗时"</span></div>
            <div class:has-alert={summary.p95_duration_ms >= 1_000}><strong>{format!("{} ms", summary.p95_duration_ms)}</strong><span>"P95 耗时"</span></div>
        </div>
        <Show when=move || !findings.get().is_empty()>
            <div class="analytics-findings">
                <strong>"确定性异常"</strong>
                <div>
                    <For
                        each=move || findings.get()
                        key=|finding| format!("{}:{}", finding.code, finding.element)
                        let:finding
                    >
                        <article class=format!("analytics-finding finding-{}", finding.level)>
                            <div><strong>{analytics_finding_label(&finding.code)}</strong><code>{finding.code}</code></div>
                            <span>{finding.message}</span>
                        </article>
                    </For>
                </div>
            </div>
        </Show>
    }
}

#[component]
pub fn DecisionAnalyticsMetrics(
    report: ReadSignal<Option<DecisionAnalyticsReport>>,
) -> impl IntoView {
    view! {
        <div class="analytics-grid">
            <div class="analytics-block">
                <div class="analytics-block-heading"><strong>"规则覆盖"</strong><small>"按决策表统计真实命中"</small></div>
                <Show
                    when=move || report.get().is_some_and(|report| !report.rule_coverage.is_empty())
                    fallback=|| view! { <div class="analytics-empty">"当前发布版本没有决策表"</div> }
                >
                    <div class="analytics-coverage-list">
                        <For
                            each=move || report.get().map(|report| report.rule_coverage).unwrap_or_default()
                            key=|metric| metric.decision_id.clone()
                            let:metric
                        >
                            {
                                let rules = metric.rules.clone();
                                let coverage_width = format!("width: {:.1}%", metric.coverage_rate * 100.0);
                                view! {
                                    <details class="analytics-coverage-row">
                                        <summary>
                                            <div><strong>{metric.decision_name}</strong><small>{format!("执行 {} 次 · 默认 {} · 无匹配 {}", metric.execution_count, metric.default_output_count, metric.no_match_count)}</small></div>
                                            <span>{format!("{}/{}", metric.covered_rules, metric.total_rules)}</span>
                                        </summary>
                                        <div class="analytics-progress"><i style=coverage_width></i></div>
                                        <div class="analytics-rule-list">
                                            <For each=move || rules.clone() key=|rule| rule.rule_id.clone() let:rule>
                                                <div class:uncovered=rule.hit_count == 0>
                                                    <code>{rule.rule_id}</code>
                                                    <span>{format!("命中 {} 次", rule.hit_count)}</span>
                                                    <small>{rule.last_hit_at.map(|time| time.format("%m-%d %H:%M").to_string()).unwrap_or_else(|| "从未命中".into())}</small>
                                                </div>
                                            </For>
                                        </div>
                                    </details>
                                }
                            }
                        </For>
                    </div>
                </Show>
            </div>
            <div class="analytics-block">
                <div class="analytics-block-heading"><strong>"结果分布"</strong><small>"每个决策最多展示 8 个高频值"</small></div>
                <Show
                    when=move || report.get().is_some_and(|report| !report.result_distributions.is_empty())
                    fallback=|| view! { <div class="analytics-empty">"尚无可聚合的成功决策结果"</div> }
                >
                    <div class="analytics-distribution-list">
                        <For
                            each=move || report.get().map(|report| report.result_distributions).unwrap_or_default()
                            key=|metric| metric.decision_id.clone()
                            let:metric
                        >
                            {
                                let values = metric.values.clone();
                                view! {
                                    <article class="analytics-distribution">
                                        <div><strong>{metric.decision_name}</strong><small>{format!("{} 个结果", metric.total)}</small></div>
                                        <For each=move || values.clone() key=|value| value.value.clone() let:value>
                                            <div class="analytics-value-row">
                                                <code>{value.value}</code><span>{value.count}</span><small>{analytics_percent(value.rate)}</small>
                                                <i style=format!("width: {:.1}%", value.rate * 100.0)></i>
                                            </div>
                                        </For>
                                    </article>
                                }
                            }
                        </For>
                    </div>
                </Show>
            </div>
        </div>
        <div class="analytics-block analytics-trends">
            <div class="analytics-block-heading"><strong>"30 日趋势"</strong><small>"运行量、失败、反馈、偏差与平均耗时"</small></div>
            <div class="analytics-trend-list">
                <For
                    each=move || report.get().map(|report| report.daily_trends).unwrap_or_default()
                    key=|day| day.date
                    let:day
                >
                    <div class="analytics-trend-row">
                        <time>{day.date.format("%m-%d").to_string()}</time>
                        <div><i style=move || {
                            let max_runs = report
                                .get()
                                .and_then(|report| report.daily_trends.iter().map(|item| item.total_runs).max())
                                .unwrap_or(1)
                                .max(1);
                            format!("width: {:.1}%", day.total_runs as f64 / max_runs as f64 * 100.0)
                        }></i></div>
                        <strong>{day.total_runs}</strong>
                        <span>{format!("失败 {} · 反馈 {} · 偏差 {}", day.failed_runs, day.outcome_reported_runs, day.mismatch_runs)}</span>
                        <small>{format!("{:.0} ms", day.average_duration_ms)}</small>
                    </div>
                </For>
            </div>
        </div>
    }
}
