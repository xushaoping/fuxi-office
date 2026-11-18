use leptos::prelude::*;
use leptos::task::spawn_local;
use std::collections::BTreeMap;

use crate::components::workspace::{analytics_finding_label, analytics_percent};
use crate::models::{
    DecisionAnalyticsFinding, DecisionAnalyticsReport, DecisionAnalyticsSummary,
    DecisionCopilotQueryResponse, DecisionDailyTrendMetric, DecisionProject, DecisionServiceRun,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleHitData {
    pub rule_id: String,
    pub rule_name: String,
    pub hit_count: u32,
}

fn recent_run_daily_trends(
    report: &DecisionAnalyticsReport,
    runs: &[DecisionServiceRun],
) -> Vec<DecisionDailyTrendMetric> {
    let mut buckets = BTreeMap::<chrono::NaiveDate, DecisionDailyTrendMetric>::new();
    let mut duration_totals = BTreeMap::<chrono::NaiveDate, i128>::new();
    let mut date = report.window_start.date_naive();
    let end_date = report.generated_at.date_naive();
    while date <= end_date {
        buckets.insert(
            date,
            DecisionDailyTrendMetric {
                date,
                ..Default::default()
            },
        );
        let Some(next) = date.succ_opt() else {
            break;
        };
        date = next;
    }

    for run in runs.iter().filter(|run| {
        run.version_id == report.version_id
            && run.executed_at >= report.window_start
            && run.executed_at <= report.generated_at
    }) {
        let day = run.executed_at.date_naive();
        let Some(bucket) = buckets.get_mut(&day) else {
            continue;
        };
        bucket.total_runs += 1;
        *duration_totals.entry(day).or_default() += i128::from(run.duration_ms.max(0));
        if run.status == "succeeded" {
            bucket.succeeded_runs += 1;
            if let Some(assessment) = run.outcome_assessment.as_deref() {
                bucket.outcome_reported_runs += 1;
                if matches!(assessment, "confirmed" | "mismatch") {
                    bucket.conclusive_outcome_runs += 1;
                }
                if assessment == "mismatch" {
                    bucket.mismatch_runs += 1;
                }
            }
        } else if run.status == "failed" {
            bucket.failed_runs += 1;
        }
    }

    buckets
        .into_iter()
        .map(|(date, mut bucket)| {
            if bucket.total_runs > 0 {
                bucket.average_duration_ms = duration_totals.get(&date).copied().unwrap_or_default()
                    as f64
                    / bucket.total_runs as f64;
            }
            bucket
        })
        .collect()
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
            <div class:has-alert={summary.mismatch_runs > 0}><strong>{format!("{:.1}%", summary.mismatch_rate * 100.0)}</strong><span>{format!("偏差 {} / 有效判定 {} 次", summary.mismatch_runs, summary.conclusive_outcome_runs)}</span></div>
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
                        {
                            let diagnostic = finding.numeric_feedback_split_summary();
                            view! {
                                <article class=format!("analytics-finding finding-{}", finding.level)>
                                    <div><strong>{analytics_finding_label(&finding.code)}</strong><code>{finding.code}</code></div>
                                    <span>{finding.message}</span>
                                    {diagnostic.map(|summary| view! {
                                        <small class="analytics-finding-diagnostic">{summary}</small>
                                    })}
                                </article>
                            }
                        }
                    </For>
                </div>
            </div>
        </Show>
    }
}

#[component]
pub fn DecisionCopilotPanel(
    token: RwSignal<Option<String>>,
    current_project: ReadSignal<Option<DecisionProject>>,
    can_route_write: Signal<bool>,
    on_route_write: Callback<String>,
) -> impl IntoView {
    let question = RwSignal::new(String::new());
    let response = RwSignal::new(None::<DecisionCopilotQueryResponse>);
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let request_generation = RwSignal::new(0_u64);
    Effect::new(move |_| {
        let _ = current_project.get();
        request_generation.update(|generation| *generation = generation.wrapping_add(1));
        question.set(String::new());
        response.set(None);
        error.set(None);
        busy.set(false);
    });
    let submit = Callback::new(move |_| {
        let (Some(token_value), Some(project)) = (token.get(), current_project.get()) else {
            return;
        };
        let question_value = question.get().trim().to_string();
        if question_value.is_empty() || busy.get() {
            return;
        }
        let project_id = project.id;
        request_generation.update(|generation| *generation = generation.wrapping_add(1));
        let generation = request_generation.get_untracked();
        busy.set(true);
        error.set(None);
        response.set(None);
        spawn_local(async move {
            match crate::api::query_decision_copilot(&token_value, project_id, &question_value)
                .await
            {
                Ok(result)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project_id)
                        && request_generation.get_untracked() == generation =>
                {
                    response.set(Some(result));
                }
                Ok(_) => {}
                Err(message)
                    if current_project
                        .get_untracked()
                        .is_some_and(|current| current.id == project_id)
                        && request_generation.get_untracked() == generation =>
                {
                    error.set(Some(message));
                }
                Err(_) => {}
            }
            if request_generation.get_untracked() == generation {
                busy.set(false);
            }
        });
    });
    view! {
        <section class="decision-copilot" aria-labelledby="decision-copilot-title">
            <div class="drift-events-heading">
                <div>
                    <strong id="decision-copilot-title">"运营 Copilot"</strong>
                    <small>"只读查询项目运行指标"</small>
                </div>
            </div>
            <div class="decision-copilot-form">
                <input
                    type="text"
                    maxlength="500"
                    placeholder="例如：最近 7 天失败率是多少？"
                    prop:value=move || question.get()
                    on:input=move |event| question.set(event_target_value(&event))
                    on:keydown=move |event: leptos::ev::KeyboardEvent| {
                        if event.key() == "Enter" && !busy.get() {
                            submit.run(());
                        }
                    }
                />
                <button class="button button-secondary" on:click=move |_| submit.run(()) disabled=move || busy.get() || question.get().trim().is_empty()>
                    {move || if busy.get() { "查询中..." } else { "查询" }}
                </button>
            </div>
            <Show when=move || error.get().is_some()>
                <div class="analytics-error" role="alert">{move || error.get().unwrap_or_default()}</div>
            </Show>
            <Show when=move || response.get().is_some()>
                {move || response.get().map(|result| view! {
                    <div class="decision-copilot-answer" role="status">
                        <strong>{result.answer.clone()}</strong>
                        {result.definition.clone().map(|definition| view! { <small>{format!("口径：{definition}")}</small> })}
                        {result.sample_size.map(|count| view! {
                            <small>
                                {format!("样本量：{count} · 时间范围：近 {} 天 · 来源：当前发布版本分析 · ", result.window_days.unwrap_or_default())}
                                <a href="#workspace-panel-analytics">"查看运行分析"</a>
                            </small>
                        })}
                        {result.generated_at.map(|time| view! { <small>{format!("数据生成于 {}", time.format("%Y-%m-%d %H:%M UTC"))}</small> })}
                        <Show when=move || result.status == "routed" && can_route_write.get()>
                            <button
                                class="button button-ghost"
                                on:click=move |_| on_route_write.run(question.get())
                                disabled=move || busy.get()
                            >"带入需求工作流"</button>
                        </Show>
                    </div>
                })}
            </Show>
        </section>
    }
}

#[component]
pub fn DecisionAnalyticsMetrics(
    report: ReadSignal<Option<DecisionAnalyticsReport>>,
    runs: ReadSignal<Vec<DecisionServiceRun>>,
) -> impl IntoView {
    let trend_days = move || {
        report
            .get()
            .map(|report| {
                if report.daily_trends.is_empty() && report.summary.total_runs > 0 {
                    recent_run_daily_trends(&report, &runs.get())
                } else {
                    report.daily_trends
                }
            })
            .unwrap_or_default()
    };
    view! {
        <Show when=move || report.get().is_some_and(|report| report.trace_details_omitted_runs > 0)>
            <div class="analytics-trace-omission" role="status">
                {move || report.get().map(|report| format!(
                    "{} 条执行轨迹超过 1 MiB，已跳过对应的规则与结果明细；运行总数和耗时仍计入，符合明确反馈条件的运行仍占据样本窗口。",
                    report.trace_details_omitted_runs
                )).unwrap_or_default()}
            </div>
        </Show>
        <Show when=move || report.get().is_some_and(|report| report.numeric_input_omitted_runs > 0)>
            <div class="analytics-trace-omission" role="status">
                {move || report.get().map(|report| format!(
                    "{} 条反馈运行的输入超过 1 MiB，未参与数值字段线索；这些运行仍占据反馈样本窗口，不会由更早运行补位。",
                    report.numeric_input_omitted_runs
                )).unwrap_or_default()}
            </div>
        </Show>
        <Show when=move || report.get().is_some_and(|report| report.unknown_result_steps_omitted > 0)>
            <div class="analytics-trace-omission" role="status">
                {move || report.get().map(|report| format!(
                    "{} 条未知决策结果步骤因唯一 ID 数量或标签长度超出上限而省略。",
                    report.unknown_result_steps_omitted
                )).unwrap_or_default()}
            </div>
        </Show>
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
                                                    <span>{if rule.feedback_count == 0 { format!("命中 {} 次", rule.hit_count) } else { format!("命中 {} 次 · 偏差 {}/{}", rule.hit_count, rule.mismatch_count, rule.feedback_count) }}</span>
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
                                        {metric.details_omitted.then(|| view! {
                                            <small>"结果明细过多或过大，已合并展示；总数保持完整。"</small>
                                        })}
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
            <Show when=move || report.get().is_some_and(|report| report.daily_trends.is_empty() && report.summary.total_runs > 0)>
                <div class="analytics-trace-omission" role="status">"接口未提供完整日趋势，当前按页面已加载的最近 50 条运行记录展示部分统计。"</div>
            </Show>
            <div class="analytics-trend-list">
                <For
                    each=trend_days
                    key=|day| day.date
                    let:day
                >
                    <div class="analytics-trend-row">
                        <time>{day.date.format("%m-%d").to_string()}</time>
                        <div><i style=move || {
                            let max_runs = trend_days()
                                .iter()
                                .map(|item| item.total_runs)
                                .max()
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
