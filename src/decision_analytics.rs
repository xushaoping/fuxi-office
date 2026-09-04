use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::{Map, Value};

use crate::dmn::{DecisionLogic, DmnModel};
use crate::models::{
    DecisionAnalyticsFinding, DecisionAnalyticsReport, DecisionAnalyticsSummary,
    DecisionDailyTrendMetric, DecisionResultDistributionMetric, DecisionResultValueMetric,
    DecisionRuleCoverageMetric, DecisionRuleHitMetric, DecisionServiceRun, ExecutionTrace,
};

pub(crate) const ANALYTICS_WINDOW_DAYS: u32 = 30;
const RESULT_VALUE_LIMIT: usize = 8;

#[derive(Default)]
struct RuleCoverageAccumulator {
    execution_count: usize,
    default_output_count: usize,
    no_match_count: usize,
    hits: HashMap<String, (usize, Option<DateTime<Utc>>)>,
}

#[derive(Default)]
struct DailyAccumulator {
    total_runs: usize,
    succeeded_runs: usize,
    failed_runs: usize,
    outcome_reported_runs: usize,
    mismatch_runs: usize,
    duration_total_ms: i128,
}

pub(crate) fn analytics_window_start(generated_at: DateTime<Utc>) -> DateTime<Utc> {
    let start_date = generated_at.date_naive() - Duration::days(ANALYTICS_WINDOW_DAYS as i64 - 1);
    DateTime::from_naive_utc_and_offset(
        start_date
            .and_hms_opt(0, 0, 0)
            .expect("midnight is a valid time"),
        Utc,
    )
}

pub(crate) fn build_decision_analytics(
    project_id: i64,
    version_id: i64,
    version_number: i32,
    model: &DmnModel,
    runs: &[DecisionServiceRun],
    window_start: DateTime<Utc>,
    generated_at: DateTime<Utc>,
) -> DecisionAnalyticsReport {
    let summary = build_summary(runs);
    let mut coverage = model
        .decisions
        .iter()
        .filter_map(|decision| match &decision.logic {
            DecisionLogic::DecisionTable(table) => Some((
                decision.id.clone(),
                RuleCoverageAccumulator {
                    hits: table
                        .rules
                        .iter()
                        .enumerate()
                        .map(|(index, rule)| {
                            (
                                rule.id
                                    .clone()
                                    .unwrap_or_else(|| format!("rule-{}", index + 1)),
                                (0, None),
                            )
                        })
                        .collect(),
                    ..Default::default()
                },
            )),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let mut result_counts = HashMap::<String, HashMap<String, usize>>::new();
    let mut result_names = HashMap::<String, String>::new();
    let mut daily = initial_daily_buckets(window_start.date_naive(), generated_at.date_naive());

    for run in runs {
        let day = run.executed_at.date_naive();
        if let Some(bucket) = daily.get_mut(&day) {
            bucket.total_runs += 1;
            bucket.duration_total_ms += i128::from(run.duration_ms.max(0));
            if run.status == "succeeded" {
                bucket.succeeded_runs += 1;
            } else if run.status == "failed" {
                bucket.failed_runs += 1;
            }
            if run.outcome_assessment.is_some() {
                bucket.outcome_reported_runs += 1;
            }
            if run.outcome_assessment.as_deref() == Some("mismatch") {
                bucket.mismatch_runs += 1;
            }
        }

        let Ok(trace) = serde_json::from_value::<ExecutionTrace>(run.execution_trace_json.clone())
        else {
            continue;
        };
        for step in trace.decisions {
            if step.logic_type == "decision_table" {
                if let Some(metric) = coverage.get_mut(&step.decision_id) {
                    metric.execution_count += 1;
                    if step.outcome == "default_output" {
                        metric.default_output_count += 1;
                    } else if step.outcome == "no_match" {
                        metric.no_match_count += 1;
                    }
                    for rule_id in &step.matched_rule_ids {
                        if let Some((count, last_hit_at)) = metric.hits.get_mut(rule_id) {
                            *count += 1;
                            if last_hit_at.is_none_or(|current| run.executed_at > current) {
                                *last_hit_at = Some(run.executed_at);
                            }
                        }
                    }
                }
            }
            if step.status == "succeeded" && step.logic_type != "service_input" {
                let value = normalized_json_text(&step.result);
                *result_counts
                    .entry(step.decision_id.clone())
                    .or_default()
                    .entry(value)
                    .or_default() += 1;
                result_names
                    .entry(step.decision_id)
                    .or_insert(step.decision_name);
            }
        }
    }

    let rule_coverage = model
        .decisions
        .iter()
        .filter_map(|decision| {
            let DecisionLogic::DecisionTable(table) = &decision.logic else {
                return None;
            };
            let metric = coverage.remove(&decision.id).unwrap_or_default();
            let rules = table
                .rules
                .iter()
                .enumerate()
                .map(|(index, rule)| {
                    let rule_id = rule
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("rule-{}", index + 1));
                    let (hit_count, last_hit_at) =
                        metric.hits.get(&rule_id).copied().unwrap_or_default();
                    DecisionRuleHitMetric {
                        rule_id,
                        hit_count,
                        last_hit_at,
                    }
                })
                .collect::<Vec<_>>();
            let covered_rules = rules.iter().filter(|rule| rule.hit_count > 0).count();
            let total_rules = rules.len();
            Some(DecisionRuleCoverageMetric {
                decision_id: decision.id.clone(),
                decision_name: decision.name.clone(),
                total_rules,
                covered_rules,
                coverage_rate: ratio(covered_rules, total_rules),
                execution_count: metric.execution_count,
                default_output_count: metric.default_output_count,
                no_match_count: metric.no_match_count,
                rules,
            })
        })
        .collect::<Vec<_>>();
    let result_distributions = build_result_distributions(model, result_counts, result_names);
    let daily_trends = daily
        .into_iter()
        .map(|(date, bucket)| DecisionDailyTrendMetric {
            date,
            total_runs: bucket.total_runs,
            succeeded_runs: bucket.succeeded_runs,
            failed_runs: bucket.failed_runs,
            outcome_reported_runs: bucket.outcome_reported_runs,
            mismatch_runs: bucket.mismatch_runs,
            average_duration_ms: if bucket.total_runs == 0 {
                0.0
            } else {
                bucket.duration_total_ms as f64 / bucket.total_runs as f64
            },
        })
        .collect::<Vec<_>>();
    let findings = build_findings(&summary, &rule_coverage, &daily_trends);

    DecisionAnalyticsReport {
        project_id,
        version_id,
        version_number,
        window_days: ANALYTICS_WINDOW_DAYS,
        window_start,
        generated_at,
        summary,
        rule_coverage,
        result_distributions,
        daily_trends,
        findings,
    }
}

fn build_summary(runs: &[DecisionServiceRun]) -> DecisionAnalyticsSummary {
    let total_runs = runs.len();
    let succeeded_runs = runs.iter().filter(|run| run.status == "succeeded").count();
    let failed_runs = runs.iter().filter(|run| run.status == "failed").count();
    let outcome_reported_runs = runs
        .iter()
        .filter(|run| run.outcome_assessment.is_some())
        .count();
    let confirmed_runs = runs
        .iter()
        .filter(|run| run.outcome_assessment.as_deref() == Some("confirmed"))
        .count();
    let mismatch_runs = runs
        .iter()
        .filter(|run| run.outcome_assessment.as_deref() == Some("mismatch"))
        .count();
    let inconclusive_runs = runs
        .iter()
        .filter(|run| run.outcome_assessment.as_deref() == Some("inconclusive"))
        .count();
    let mut durations = runs
        .iter()
        .map(|run| run.duration_ms.max(0))
        .collect::<Vec<_>>();
    durations.sort_unstable();
    let duration_total = durations
        .iter()
        .map(|duration| i128::from(*duration))
        .sum::<i128>();

    DecisionAnalyticsSummary {
        total_runs,
        succeeded_runs,
        failed_runs,
        outcome_reported_runs,
        confirmed_runs,
        mismatch_runs,
        inconclusive_runs,
        failure_rate: ratio(failed_runs, total_runs),
        feedback_coverage_rate: ratio(outcome_reported_runs, succeeded_runs),
        mismatch_rate: ratio(mismatch_runs, outcome_reported_runs),
        average_duration_ms: if total_runs == 0 {
            0.0
        } else {
            duration_total as f64 / total_runs as f64
        },
        p95_duration_ms: percentile_95(&durations),
    }
}

fn build_result_distributions(
    model: &DmnModel,
    mut counts: HashMap<String, HashMap<String, usize>>,
    names: HashMap<String, String>,
) -> Vec<DecisionResultDistributionMetric> {
    let mut decision_order = model
        .decisions
        .iter()
        .map(|decision| (decision.id.clone(), decision.name.clone()))
        .collect::<Vec<_>>();
    let mut unknown = counts
        .keys()
        .filter(|decision_id| !decision_order.iter().any(|(id, _)| id == *decision_id))
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();
    decision_order.extend(
        unknown
            .into_iter()
            .map(|id| (id.clone(), names.get(&id).cloned().unwrap_or(id))),
    );
    decision_order
        .into_iter()
        .filter_map(|(decision_id, decision_name)| {
            let values = counts.remove(&decision_id)?;
            let total = values.values().sum::<usize>();
            let mut values = values.into_iter().collect::<Vec<_>>();
            values.sort_by(|(left_value, left_count), (right_value, right_count)| {
                right_count
                    .cmp(left_count)
                    .then_with(|| left_value.cmp(right_value))
            });
            let other_count = values
                .iter()
                .skip(RESULT_VALUE_LIMIT)
                .map(|(_, count)| *count)
                .sum::<usize>();
            values.truncate(RESULT_VALUE_LIMIT);
            let mut values = values
                .into_iter()
                .map(|(value, count)| DecisionResultValueMetric {
                    value,
                    count,
                    rate: ratio(count, total),
                })
                .collect::<Vec<_>>();
            if other_count > 0 {
                values.push(DecisionResultValueMetric {
                    value: "其他".into(),
                    count: other_count,
                    rate: ratio(other_count, total),
                });
            }
            Some(DecisionResultDistributionMetric {
                decision_id,
                decision_name,
                total,
                values,
            })
        })
        .collect()
}

fn build_findings(
    summary: &DecisionAnalyticsSummary,
    rule_coverage: &[DecisionRuleCoverageMetric],
    daily_trends: &[DecisionDailyTrendMetric],
) -> Vec<DecisionAnalyticsFinding> {
    let mut findings = Vec::new();
    if summary.total_runs == 0 {
        findings.push(finding(
            "medium",
            "NO_RECENT_RUNS",
            "当前发布版本最近 30 天没有运行数据，无法判断线上表现。",
            "",
        ));
        return findings;
    }
    if summary.total_runs >= 10 && summary.failure_rate >= 0.10 {
        let level = if summary.failure_rate >= 0.20 {
            "high"
        } else {
            "medium"
        };
        findings.push(finding(
            level,
            "HIGH_FAILURE_RATE",
            format!(
                "最近 30 天失败率为 {:.1}%，共 {} 次失败。",
                summary.failure_rate * 100.0,
                summary.failed_runs
            ),
            "",
        ));
    }
    if summary.succeeded_runs >= 10 && summary.feedback_coverage_rate < 0.30 {
        findings.push(finding(
            "medium",
            "LOW_FEEDBACK_COVERAGE",
            format!(
                "成功运行的实际结果回填率仅 {:.1}%，偏差分析样本不足。",
                summary.feedback_coverage_rate * 100.0
            ),
            "",
        ));
    }
    if summary.outcome_reported_runs >= 5 && summary.mismatch_rate >= 0.20 {
        let level = if summary.mismatch_rate >= 0.30 {
            "high"
        } else {
            "medium"
        };
        findings.push(finding(
            level,
            "HIGH_MISMATCH_RATE",
            format!(
                "已回填结果中 {:.1}% 被判定为存在偏差，共 {} 次。",
                summary.mismatch_rate * 100.0,
                summary.mismatch_runs
            ),
            "",
        ));
    }
    if summary.total_runs >= 10 && summary.p95_duration_ms >= 1_000 {
        findings.push(finding(
            "medium",
            "HIGH_P95_LATENCY",
            format!(
                "运行耗时 P95 为 {} ms，高于 1000 ms。",
                summary.p95_duration_ms
            ),
            "",
        ));
    }
    for metric in rule_coverage {
        if metric.execution_count >= 10 && metric.coverage_rate < 0.80 {
            findings.push(finding(
                "medium",
                "LOW_RULE_COVERAGE",
                format!(
                    "决策表“{}”仅覆盖 {}/{} 条规则（{:.1}%）。",
                    metric.decision_name,
                    metric.covered_rules,
                    metric.total_rules,
                    metric.coverage_rate * 100.0
                ),
                metric.decision_id.clone(),
            ));
        }
        let fallback_count = metric.default_output_count + metric.no_match_count;
        let fallback_rate = ratio(fallback_count, metric.execution_count);
        if metric.execution_count >= 10 && fallback_rate >= 0.20 {
            findings.push(finding(
                "medium",
                "FREQUENT_FALLBACK",
                format!(
                    "决策表“{}”有 {:.1}% 的执行未命中规则，其中默认输出 {} 次、无匹配 {} 次。",
                    metric.decision_name,
                    fallback_rate * 100.0,
                    metric.default_output_count,
                    metric.no_match_count
                ),
                metric.decision_id.clone(),
            ));
        }
    }
    add_recent_trend_findings(&mut findings, daily_trends);
    findings.sort_by(|left, right| {
        finding_rank(&left.level)
            .cmp(&finding_rank(&right.level))
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.element.cmp(&right.element))
    });
    findings
}

fn add_recent_trend_findings(
    findings: &mut Vec<DecisionAnalyticsFinding>,
    daily_trends: &[DecisionDailyTrendMetric],
) {
    if daily_trends.len() < 14 {
        return;
    }
    let previous = &daily_trends[daily_trends.len() - 14..daily_trends.len() - 7];
    let recent = &daily_trends[daily_trends.len() - 7..];
    let previous_total = previous.iter().map(|day| day.total_runs).sum::<usize>();
    let recent_total = recent.iter().map(|day| day.total_runs).sum::<usize>();
    let previous_failed = previous.iter().map(|day| day.failed_runs).sum::<usize>();
    let recent_failed = recent.iter().map(|day| day.failed_runs).sum::<usize>();
    let previous_failure_rate = ratio(previous_failed, previous_total);
    let recent_failure_rate = ratio(recent_failed, recent_total);
    if previous_total >= 5
        && recent_total >= 5
        && recent_failure_rate >= 0.10
        && recent_failure_rate >= previous_failure_rate + 0.10
    {
        findings.push(finding(
            "medium",
            "FAILURE_TREND_UP",
            format!(
                "最近 7 天失败率由前 7 天的 {:.1}% 上升到 {:.1}%。",
                previous_failure_rate * 100.0,
                recent_failure_rate * 100.0
            ),
            "",
        ));
    }

    let previous_succeeded = previous.iter().map(|day| day.succeeded_runs).sum::<usize>();
    let recent_succeeded = recent.iter().map(|day| day.succeeded_runs).sum::<usize>();
    let previous_feedback = previous
        .iter()
        .map(|day| day.outcome_reported_runs)
        .sum::<usize>();
    let recent_feedback = recent
        .iter()
        .map(|day| day.outcome_reported_runs)
        .sum::<usize>();
    let previous_feedback_rate = ratio(previous_feedback, previous_succeeded);
    let recent_feedback_rate = ratio(recent_feedback, recent_succeeded);
    if previous_succeeded >= 5
        && recent_succeeded >= 5
        && previous_feedback_rate >= recent_feedback_rate + 0.20
    {
        findings.push(finding(
            "medium",
            "FEEDBACK_TREND_DOWN",
            format!(
                "最近 7 天实际结果回填率由前 7 天的 {:.1}% 降至 {:.1}%。",
                previous_feedback_rate * 100.0,
                recent_feedback_rate * 100.0
            ),
            "",
        ));
    }

    let previous_duration = weighted_average_duration(previous);
    let recent_duration = weighted_average_duration(recent);
    if previous_total >= 5
        && recent_total >= 5
        && recent_duration >= previous_duration * 1.5
        && recent_duration >= previous_duration + 100.0
    {
        findings.push(finding(
            "medium",
            "LATENCY_TREND_UP",
            format!(
                "最近 7 天平均耗时由前 7 天的 {:.0} ms 上升到 {:.0} ms。",
                previous_duration, recent_duration
            ),
            "",
        ));
    }
}

fn initial_daily_buckets(
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> BTreeMap<NaiveDate, DailyAccumulator> {
    let mut buckets = BTreeMap::new();
    let mut date = start_date;
    while date <= end_date {
        buckets.insert(date, DailyAccumulator::default());
        let Some(next) = date.succ_opt() else {
            break;
        };
        date = next;
    }
    buckets
}

fn weighted_average_duration(days: &[DecisionDailyTrendMetric]) -> f64 {
    let total = days.iter().map(|day| day.total_runs).sum::<usize>();
    if total == 0 {
        return 0.0;
    }
    days.iter()
        .map(|day| day.average_duration_ms * day.total_runs as f64)
        .sum::<f64>()
        / total as f64
}

fn normalized_json_text(value: &Value) -> String {
    serde_json::to_string(&normalize_json(value)).unwrap_or_else(|_| "null".into())
}

fn normalize_json(value: &Value) -> Value {
    match value {
        Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), normalize_json(value)))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(normalize_json).collect()),
        _ => value.clone(),
    }
}

fn percentile_95(sorted: &[i64]) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() * 95).div_ceil(100);
    sorted[rank.saturating_sub(1)]
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn finding(
    level: impl Into<String>,
    code: impl Into<String>,
    message: impl Into<String>,
    element: impl Into<String>,
) -> DecisionAnalyticsFinding {
    DecisionAnalyticsFinding {
        level: level.into(),
        code: code.into(),
        message: message.into(),
        element: element.into(),
    }
}

fn finding_rank(level: &str) -> u8 {
    match level {
        "high" => 0,
        "medium" => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use crate::dmn::{DMN_NAMESPACE, parse_dmn};
    use crate::models::{DecisionTraceStep, ExecutionTrace};

    use super::*;

    fn model() -> DmnModel {
        parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}" name="Analytics">
              <decision id="explicit" name="Explicit Table">
                <decisionTable hitPolicy="UNIQUE">
                  <input><inputExpression><text>age</text></inputExpression></input>
                  <output name="result"/>
                  <rule id="adult"><inputEntry><text>&gt;= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule>
                  <rule id="minor"><inputEntry><text>&lt; 18</text></inputEntry><outputEntry><text>"minor"</text></outputEntry></rule>
                </decisionTable>
              </decision>
              <decision id="anonymous" name="Anonymous Table">
                <decisionTable hitPolicy="FIRST">
                  <input><inputExpression><text>score</text></inputExpression></input>
                  <output name="result"><defaultOutputEntry><text>"review"</text></defaultOutputEntry></output>
                  <rule><inputEntry><text>&gt;= 80</text></inputEntry><outputEntry><text>"pass"</text></outputEntry></rule>
                  <rule><inputEntry><text>&lt; 80</text></inputEntry><outputEntry><text>"fail"</text></outputEntry></rule>
                </decisionTable>
              </decision>
            </definitions>"##
        ))
        .unwrap()
    }

    fn run(
        id: i64,
        day: u32,
        status: &str,
        duration_ms: i64,
        assessment: Option<&str>,
        decisions: Vec<DecisionTraceStep>,
    ) -> DecisionServiceRun {
        let executed_at = Utc.with_ymd_and_hms(2026, 8, day, 12, 0, 0).unwrap();
        let trace = ExecutionTrace {
            model_name: "Analytics".into(),
            version_id: Some(7),
            version_number: Some(3),
            status: status.into(),
            normalized_input: json!({}),
            output: json!({}),
            decisions,
            messages: Vec::new(),
            explanation: String::new(),
        };
        DecisionServiceRun {
            id,
            project_id: 5,
            version_id: 7,
            service_id: "service".into(),
            service_name: "Service".into(),
            input_json: json!({}),
            output_json: json!({}),
            execution_trace_json: serde_json::to_value(trace).unwrap(),
            duration_ms,
            status: status.into(),
            executed_by: "tester".into(),
            executed_at,
            business_reference: format!("case-{id}"),
            actual_outcome_json: assessment.map(|_| json!({"done": true})),
            outcome_assessment: assessment.map(str::to_string),
            outcome_note: String::new(),
            outcome_occurred_at: None,
            outcome_reported_by: assessment.map(|_| "tester".into()),
            outcome_reported_at: assessment.map(|_| executed_at),
        }
    }

    fn table_step(
        decision_id: &str,
        decision_name: &str,
        outcome: &str,
        matched_rule_ids: &[&str],
        result: Value,
    ) -> DecisionTraceStep {
        DecisionTraceStep {
            order: 1,
            decision_id: decision_id.into(),
            decision_name: decision_name.into(),
            logic_type: "decision_table".into(),
            status: "succeeded".into(),
            outcome: outcome.into(),
            dependencies: Vec::new(),
            matched_rule_ids: matched_rule_ids.iter().map(|id| (*id).into()).collect(),
            result,
            explanation: String::new(),
        }
    }

    #[test]
    fn aggregates_rule_coverage_results_feedback_and_percentiles() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = vec![
            run(
                1,
                22,
                "succeeded",
                100,
                Some("confirmed"),
                vec![
                    table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!({"b": 2, "a": 1}),
                    ),
                    table_step(
                        "anonymous",
                        "Anonymous Table",
                        "rule_match",
                        &["rule-1"],
                        json!("pass"),
                    ),
                ],
            ),
            run(
                2,
                23,
                "succeeded",
                200,
                Some("mismatch"),
                vec![
                    table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!({"a": 1, "b": 2}),
                    ),
                    table_step(
                        "anonymous",
                        "Anonymous Table",
                        "default_output",
                        &[],
                        json!("review"),
                    ),
                ],
            ),
            run(3, 23, "failed", 1_500, None, Vec::new()),
        ];
        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);

        assert_eq!(report.summary.total_runs, 3);
        assert_eq!(report.summary.succeeded_runs, 2);
        assert_eq!(report.summary.failed_runs, 1);
        assert_eq!(report.summary.outcome_reported_runs, 2);
        assert_eq!(report.summary.mismatch_runs, 1);
        assert_eq!(report.summary.p95_duration_ms, 1_500);
        assert!((report.summary.average_duration_ms - 600.0).abs() < f64::EPSILON);
        let explicit = &report.rule_coverage[0];
        assert_eq!(explicit.covered_rules, 1);
        assert_eq!(explicit.rules[0].rule_id, "adult");
        assert_eq!(explicit.rules[0].hit_count, 2);
        assert_eq!(explicit.rules[1].rule_id, "minor");
        let anonymous = &report.rule_coverage[1];
        assert_eq!(anonymous.rules[0].rule_id, "rule-1");
        assert_eq!(anonymous.default_output_count, 1);
        let distribution = report
            .result_distributions
            .iter()
            .find(|item| item.decision_id == "explicit")
            .unwrap();
        assert_eq!(distribution.total, 2);
        assert_eq!(distribution.values[0].value, r#"{"a":1,"b":2}"#);
        assert_eq!(distribution.values[0].count, 2);
        assert_eq!(report.daily_trends.len(), 30);
    }

    #[test]
    fn caps_high_cardinality_result_distribution() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = (1..=10)
            .map(|id| {
                run(
                    id,
                    23,
                    "succeeded",
                    10,
                    None,
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!(format!("value-{id}")),
                    )],
                )
            })
            .collect::<Vec<_>>();
        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let distribution = report
            .result_distributions
            .iter()
            .find(|item| item.decision_id == "explicit")
            .unwrap();
        assert_eq!(distribution.values.len(), RESULT_VALUE_LIMIT + 1);
        assert_eq!(distribution.values.last().unwrap().value, "其他");
        assert_eq!(distribution.values.last().unwrap().count, 2);
    }

    #[test]
    fn emits_deterministic_threshold_and_recent_trend_findings() {
        let summary = DecisionAnalyticsSummary {
            total_runs: 20,
            succeeded_runs: 16,
            failed_runs: 4,
            outcome_reported_runs: 5,
            confirmed_runs: 3,
            mismatch_runs: 2,
            inconclusive_runs: 0,
            failure_rate: 0.20,
            feedback_coverage_rate: 0.25,
            mismatch_rate: 0.40,
            average_duration_ms: 400.0,
            p95_duration_ms: 1_200,
        };
        let coverage = vec![DecisionRuleCoverageMetric {
            decision_id: "table".into(),
            decision_name: "Table".into(),
            total_rules: 10,
            covered_rules: 5,
            coverage_rate: 0.5,
            execution_count: 20,
            default_output_count: 4,
            no_match_count: 1,
            rules: Vec::new(),
        }];
        let trends = (0..14)
            .map(|index| DecisionDailyTrendMetric {
                date: NaiveDate::from_ymd_opt(2026, 8, index + 1).unwrap(),
                total_runs: 1,
                succeeded_runs: if index < 7 || !matches!(index, 7 | 8) {
                    1
                } else {
                    0
                },
                failed_runs: usize::from(matches!(index, 7 | 8)),
                outcome_reported_runs: if index < 7 { 1 } else { 0 },
                mismatch_runs: 0,
                average_duration_ms: if index < 7 { 100.0 } else { 300.0 },
            })
            .collect::<Vec<_>>();
        let findings = build_findings(&summary, &coverage, &trends);
        let codes = findings
            .iter()
            .map(|finding| finding.code.as_str())
            .collect::<Vec<_>>();

        assert!(codes.contains(&"HIGH_FAILURE_RATE"));
        assert!(codes.contains(&"LOW_FEEDBACK_COVERAGE"));
        assert!(codes.contains(&"HIGH_MISMATCH_RATE"));
        assert!(codes.contains(&"HIGH_P95_LATENCY"));
        assert!(codes.contains(&"LOW_RULE_COVERAGE"));
        assert!(codes.contains(&"FREQUENT_FALLBACK"));
        assert!(codes.contains(&"FAILURE_TREND_UP"));
        assert!(codes.contains(&"FEEDBACK_TREND_DOWN"));
        assert!(codes.contains(&"LATENCY_TREND_UP"));
    }
}
