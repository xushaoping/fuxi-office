use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::{Map, Value};

use crate::dmn::{DecisionLogic, DmnModel};
use crate::models::{
    DecisionAnalyticsDriftSignal, DecisionAnalyticsFinding, DecisionAnalyticsReport,
    DecisionAnalyticsSummary, DecisionAnalyticsThresholds, DecisionDailyTrendMetric,
    DecisionResultDistributionMetric, DecisionResultValueMetric, DecisionRuleCoverageMetric,
    DecisionRuleHitMetric, DecisionServiceRun, ExecutionTrace, NumericFeedbackSplitHint,
};

pub(crate) const ANALYTICS_WINDOW_DAYS: u32 = 30;
const RESULT_VALUE_LIMIT: usize = 8;
const MAX_RESULT_DISTINCT_VALUES: usize = 256;
const MAX_RESULT_DECISION_BYTES: usize = 64 * 1_024;
const MAX_RESULT_REPORT_BYTES: usize = 1_024 * 1_024;
const MAX_UNKNOWN_RESULT_DECISIONS: usize = 256;
const MAX_UNKNOWN_RESULT_LABEL_BYTES: usize = 256;
const MAX_RESULT_VALUE_NODES: usize = 4_096;
const MAX_RESULT_VALUE_DEPTH: usize = 16;
const RULE_FEEDBACK_MIN_SAMPLE_COUNT: usize = 5;
const RULE_FEEDBACK_MISMATCH_THRESHOLD: f64 = 0.30;
const MAX_FEEDBACK_SAMPLES: usize = 2_000;
pub(crate) const MAX_ANALYTICS_TRACE_BYTES: i64 = 1_048_576;
const MAX_FISHER_CACHE_ENTRIES: usize = 100_000;
const MAX_NUMERIC_INPUT_FIELDS: usize = 64;
const MAX_NUMERIC_INPUT_DEPTH: usize = 4;
const MAX_NUMERIC_INPUT_NODES: usize = 4_096;
const MAX_NUMERIC_INPUT_PATH_BYTES: usize = 64 * 1_024;
type FisherContingency = (usize, usize, usize, usize);
type NumericFeedbackInputs = HashMap<usize, Vec<(String, f64)>>;

#[derive(Debug)]
#[cfg_attr(not(target_family = "wasm"), derive(sqlx::FromRow))]
pub(crate) struct DecisionAnalyticsRun {
    pub(crate) project_id: i64,
    pub(crate) input_json: Value,
    pub(crate) numeric_input_omitted: bool,
    pub(crate) execution_trace_json: Value,
    pub(crate) trace_details_omitted: bool,
    pub(crate) duration_ms: i64,
    pub(crate) status: String,
    pub(crate) executed_at: DateTime<Utc>,
    pub(crate) business_reference: String,
    pub(crate) outcome_assessment: Option<String>,
}

impl From<&DecisionServiceRun> for DecisionAnalyticsRun {
    fn from(run: &DecisionServiceRun) -> Self {
        Self {
            project_id: run.project_id,
            input_json: run.input_json.clone(),
            numeric_input_omitted: false,
            execution_trace_json: run.execution_trace_json.clone(),
            trace_details_omitted: false,
            duration_ms: run.duration_ms,
            status: run.status.clone(),
            executed_at: run.executed_at,
            business_reference: run.business_reference.clone(),
            outcome_assessment: run.outcome_assessment.clone(),
        }
    }
}

#[derive(Clone)]
struct FeedbackSample {
    project_id: i64,
    business_reference: String,
    mismatch: bool,
    numeric_fields: Vec<(String, f64)>,
    missingness_observed: bool,
    matched_rules: HashSet<(String, String)>,
    executed_at: DateTime<Utc>,
}

pub(crate) struct DecisionAnalyticsBuilder<'a> {
    project_id: i64,
    version_id: i64,
    version_number: i32,
    model: &'a DmnModel,
    known_decision_ids: HashSet<&'a str>,
    window_start: DateTime<Utc>,
    generated_at: DateTime<Utc>,
    thresholds: DecisionAnalyticsThresholds,
    summary_counts: SummaryCounts,
    numeric_input_omitted_runs: usize,
    trace_details_omitted_runs: usize,
    unknown_result_decisions: usize,
    unknown_result_steps_omitted: usize,
    duration_total_ms: i128,
    durations: Vec<i64>,
    exact_p95_duration_ms: Option<i64>,
    coverage: HashMap<String, RuleCoverageAccumulator>,
    result_counts: HashMap<String, ResultDistributionAccumulator>,
    retained_result_bytes: usize,
    result_names: HashMap<String, String>,
    daily: BTreeMap<NaiveDate, DailyAccumulator>,
    feedback_window: VecDeque<FeedbackSample>,
}

#[derive(Default)]
struct SummaryCounts {
    total_runs: usize,
    succeeded_runs: usize,
    failed_runs: usize,
    outcome_reported_runs: usize,
    confirmed_runs: usize,
    mismatch_runs: usize,
    inconclusive_runs: usize,
}

struct FisherTestContext {
    maximum_p: f64,
    log_factorials: Vec<f64>,
    significance_cache: HashMap<FisherContingency, bool>,
}

impl FisherTestContext {
    fn new(maximum_p: f64) -> Self {
        let mut log_factorials = Vec::with_capacity(MAX_FEEDBACK_SAMPLES + 1);
        log_factorials.push(0.0);
        for value in 1..=MAX_FEEDBACK_SAMPLES {
            let previous = log_factorials[value - 1];
            log_factorials.push(previous + (value as f64).ln());
        }
        Self {
            maximum_p,
            log_factorials,
            significance_cache: HashMap::new(),
        }
    }

    fn is_significant(&mut self, contingency: FisherContingency) -> bool {
        if let Some(result) = self.significance_cache.get(&contingency) {
            return *result;
        }
        let maximum_p = self.maximum_p;
        let result = fisher_exact_is_significant(contingency, &self.log_factorials, maximum_p);
        if self.significance_cache.len() < MAX_FISHER_CACHE_ENTRIES {
            self.significance_cache.insert(contingency, result);
        }
        result
    }
}

#[derive(Clone, Default)]
struct RuleHitAccumulator {
    hit_count: usize,
    last_hit_at: Option<DateTime<Utc>>,
    feedback_count: usize,
    mismatch_count: usize,
    daily_hits: BTreeMap<NaiveDate, usize>,
}

#[derive(Clone, Default)]
struct RuleCoverageAccumulator {
    execution_count: usize,
    default_output_count: usize,
    no_match_count: usize,
    daily_executions: BTreeMap<NaiveDate, usize>,
    hits: HashMap<String, RuleHitAccumulator>,
}

#[derive(Default)]
struct DailyAccumulator {
    total_runs: usize,
    succeeded_runs: usize,
    failed_runs: usize,
    outcome_reported_runs: usize,
    conclusive_outcome_runs: usize,
    mismatch_runs: usize,
    duration_total_ms: i128,
}

#[derive(Default)]
struct ResultDistributionAccumulator {
    total: usize,
    counts: HashMap<String, usize>,
    retained_bytes: usize,
    details_omitted: bool,
}

impl ResultDistributionAccumulator {
    fn record(&mut self, value: &Value, report_bytes: &mut usize) {
        self.total += 1;
        if self.details_omitted {
            return;
        }
        let Some(value) = normalized_json_text(value) else {
            self.omit_details(report_bytes);
            return;
        };
        if let Some(count) = self.counts.get_mut(&value) {
            *count += 1;
            return;
        }
        if self.counts.len() == MAX_RESULT_DISTINCT_VALUES
            || value.len() > MAX_RESULT_DECISION_BYTES - self.retained_bytes
            || value.len() > MAX_RESULT_REPORT_BYTES - *report_bytes
        {
            self.omit_details(report_bytes);
            return;
        }
        self.retained_bytes += value.len();
        *report_bytes += value.len();
        self.counts.insert(value, 1);
    }

    fn omit_details(&mut self, report_bytes: &mut usize) {
        // Release keys and map capacity; never resume with incomplete frequencies.
        self.counts = HashMap::new();
        *report_bytes -= self.retained_bytes;
        self.retained_bytes = 0;
        self.details_omitted = true;
    }
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

#[cfg(test)]
pub(crate) fn build_decision_analytics(
    project_id: i64,
    version_id: i64,
    version_number: i32,
    model: &DmnModel,
    runs: &[DecisionServiceRun],
    window_start: DateTime<Utc>,
    generated_at: DateTime<Utc>,
) -> DecisionAnalyticsReport {
    let mut builder = DecisionAnalyticsBuilder::new(
        project_id,
        version_id,
        version_number,
        model,
        window_start,
        generated_at,
    );
    for run in runs {
        builder.record(DecisionAnalyticsRun::from(run));
    }
    builder.finish()
}

impl<'a> DecisionAnalyticsBuilder<'a> {
    pub(crate) fn new(
        project_id: i64,
        version_id: i64,
        version_number: i32,
        model: &'a DmnModel,
        window_start: DateTime<Utc>,
        generated_at: DateTime<Utc>,
    ) -> Self {
        let coverage = model
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
                                    RuleHitAccumulator::default(),
                                )
                            })
                            .collect(),
                        ..Default::default()
                    },
                )),
                _ => None,
            })
            .collect();
        let known_decision_ids = model
            .decisions
            .iter()
            .map(|decision| decision.id.as_str())
            .collect();
        Self {
            project_id,
            version_id,
            version_number,
            model,
            known_decision_ids,
            window_start,
            generated_at,
            thresholds: DecisionAnalyticsThresholds::default(),
            summary_counts: SummaryCounts::default(),
            numeric_input_omitted_runs: 0,
            trace_details_omitted_runs: 0,
            unknown_result_decisions: 0,
            unknown_result_steps_omitted: 0,
            duration_total_ms: 0,
            durations: Vec::new(),
            exact_p95_duration_ms: None,
            coverage,
            result_counts: HashMap::new(),
            retained_result_bytes: 0,
            result_names: HashMap::new(),
            daily: initial_daily_buckets(window_start.date_naive(), generated_at.date_naive()),
            feedback_window: VecDeque::with_capacity(MAX_FEEDBACK_SAMPLES),
        }
    }

    pub(crate) fn record(&mut self, run: DecisionAnalyticsRun) {
        let duration = run.duration_ms.max(0);
        self.duration_total_ms += i128::from(duration);
        if self.exact_p95_duration_ms.is_none() {
            self.durations.push(duration);
        }
        self.summary_counts.total_runs += 1;
        if run.status == "succeeded" {
            self.summary_counts.succeeded_runs += 1;
            match run.outcome_assessment.as_deref() {
                Some("confirmed") => self.summary_counts.confirmed_runs += 1,
                Some("mismatch") => self.summary_counts.mismatch_runs += 1,
                Some("inconclusive") => self.summary_counts.inconclusive_runs += 1,
                Some(_) => self.summary_counts.outcome_reported_runs += 1,
                None => {}
            }
            if matches!(
                run.outcome_assessment.as_deref(),
                Some("confirmed" | "mismatch" | "inconclusive")
            ) {
                self.summary_counts.outcome_reported_runs += 1;
            }
        } else if run.status == "failed" {
            self.summary_counts.failed_runs += 1;
        }

        let day = run.executed_at.date_naive();
        if let Some(bucket) = self.daily.get_mut(&day) {
            bucket.total_runs += 1;
            bucket.duration_total_ms += i128::from(duration);
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

        let conclusive = run.status == "succeeded"
            && matches!(
                run.outcome_assessment.as_deref(),
                Some("confirmed" | "mismatch")
            );
        if conclusive {
            if self.feedback_window.len() == MAX_FEEDBACK_SAMPLES {
                self.feedback_window.pop_front();
            }
            self.feedback_window.push_back(FeedbackSample {
                project_id: run.project_id,
                business_reference: run.business_reference,
                mismatch: run.outcome_assessment.as_deref() == Some("mismatch"),
                numeric_fields: Vec::new(),
                missingness_observed: false,
                matched_rules: HashSet::new(),
                executed_at: run.executed_at,
            });
        }
        let feedback_index = conclusive.then_some(self.feedback_window.len().saturating_sub(1));

        if run.numeric_input_omitted {
            self.numeric_input_omitted_runs += 1;
        }
        if run.trace_details_omitted {
            self.trace_details_omitted_runs += 1;
            return;
        }
        let Ok(trace) = serde_json::from_value::<ExecutionTrace>(run.execution_trace_json) else {
            return;
        };
        let mut numeric_input_needed = false;
        for step in trace.decisions {
            if step.logic_type == "decision_table"
                && let Some(metric) = self.coverage.get_mut(&step.decision_id)
            {
                metric.execution_count += 1;
                *metric.daily_executions.entry(day).or_default() += 1;
                if step.outcome == "default_output" {
                    metric.default_output_count += 1;
                } else if step.outcome == "no_match" {
                    metric.no_match_count += 1;
                }
                for rule_id in &step.matched_rule_ids {
                    if let Some(rule) = metric.hits.get_mut(rule_id) {
                        rule.hit_count += 1;
                        *rule.daily_hits.entry(day).or_default() += 1;
                        if rule
                            .last_hit_at
                            .is_none_or(|current| run.executed_at > current)
                        {
                            rule.last_hit_at = Some(run.executed_at);
                        }
                        if conclusive {
                            rule.feedback_count += 1;
                            if run.outcome_assessment.as_deref() == Some("mismatch") {
                                rule.mismatch_count += 1;
                            }
                            if let Some(index) = feedback_index {
                                let sample = &mut self.feedback_window[index];
                                sample
                                    .matched_rules
                                    .insert((step.decision_id.clone(), rule_id.clone()));
                                numeric_input_needed |=
                                    !sample.business_reference.trim().is_empty();
                            }
                        }
                    }
                }
            }
            if step.status == "succeeded" && step.logic_type != "service_input" {
                let known_decision = self.known_decision_ids.contains(step.decision_id.as_str());
                let already_retained = self.result_counts.contains_key(&step.decision_id);
                let retain_unknown = known_decision
                    || already_retained
                    || (step.decision_id.len() <= MAX_UNKNOWN_RESULT_LABEL_BYTES
                        && step.decision_name.len() <= MAX_UNKNOWN_RESULT_LABEL_BYTES
                        && self.unknown_result_decisions < MAX_UNKNOWN_RESULT_DECISIONS);
                if retain_unknown {
                    if !known_decision && !already_retained {
                        self.unknown_result_decisions += 1;
                        self.result_names
                            .insert(step.decision_id.clone(), step.decision_name);
                    }
                    self.result_counts
                        .entry(step.decision_id)
                        .or_default()
                        .record(&step.result, &mut self.retained_result_bytes);
                } else {
                    self.unknown_result_steps_omitted =
                        self.unknown_result_steps_omitted.saturating_add(1);
                }
            }
        }
        if numeric_input_needed
            && !run.numeric_input_omitted
            && let Some(index) = feedback_index
        {
            let mut collector = NumericInputCollector::new();
            if collector.collect(&run.input_json, "", 0) {
                self.feedback_window[index].missingness_observed =
                    collector.fields.len() < MAX_NUMERIC_INPUT_FIELDS;
                self.feedback_window[index].numeric_fields = collector.fields.into_iter().collect();
            }
        }
    }

    pub(crate) fn set_exact_p95_duration_ms(&mut self, duration_ms: Option<i64>) {
        self.exact_p95_duration_ms = duration_ms.map(|duration| duration.max(0));
        if self.exact_p95_duration_ms.is_some() {
            self.durations = Vec::new();
        }
    }

    pub(crate) fn set_thresholds(&mut self, thresholds: DecisionAnalyticsThresholds) {
        self.thresholds = thresholds;
    }

    pub(crate) fn finish(self) -> DecisionAnalyticsReport {
        let mut durations = self.durations;
        let exact_p95_duration_ms = self.exact_p95_duration_ms.unwrap_or_else(|| {
            durations.sort_unstable();
            percentile_95(&durations)
        });
        let total_runs = self.summary_counts.total_runs;
        let succeeded_runs = self.summary_counts.succeeded_runs;
        let failed_runs = self.summary_counts.failed_runs;
        let confirmed_runs = self.summary_counts.confirmed_runs;
        let mismatch_runs = self.summary_counts.mismatch_runs;
        let outcome_reported_runs = self.summary_counts.outcome_reported_runs;
        let conclusive_outcome_runs = confirmed_runs + mismatch_runs;
        let summary = DecisionAnalyticsSummary {
            total_runs,
            succeeded_runs,
            failed_runs,
            outcome_reported_runs,
            conclusive_outcome_runs,
            confirmed_runs,
            mismatch_runs,
            inconclusive_runs: self.summary_counts.inconclusive_runs,
            failure_rate: ratio(failed_runs, total_runs),
            feedback_coverage_rate: ratio(outcome_reported_runs, succeeded_runs),
            mismatch_rate: ratio(mismatch_runs, conclusive_outcome_runs),
            average_duration_ms: if total_runs == 0 {
                0.0
            } else {
                self.duration_total_ms as f64 / total_runs as f64
            },
            p95_duration_ms: exact_p95_duration_ms,
        };

        let rule_coverage = self
            .model
            .decisions
            .iter()
            .filter_map(|decision| {
                let DecisionLogic::DecisionTable(table) = &decision.logic else {
                    return None;
                };
                let metric = self.coverage.get(&decision.id).cloned().unwrap_or_default();
                let rules = table
                    .rules
                    .iter()
                    .enumerate()
                    .map(|(index, rule)| {
                        let rule_id = rule
                            .id
                            .clone()
                            .unwrap_or_else(|| format!("rule-{}", index + 1));
                        let rule_metrics = metric.hits.get(&rule_id).cloned().unwrap_or_default();
                        DecisionRuleHitMetric {
                            rule_id,
                            hit_count: rule_metrics.hit_count,
                            last_hit_at: rule_metrics.last_hit_at,
                            feedback_count: rule_metrics.feedback_count,
                            mismatch_count: rule_metrics.mismatch_count,
                            mismatch_rate: ratio(
                                rule_metrics.mismatch_count,
                                rule_metrics.feedback_count,
                            ),
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
        let result_distributions =
            build_result_distributions(self.model, self.result_counts, self.result_names);
        let daily_trends = self
            .daily
            .into_iter()
            .map(|(date, bucket)| DecisionDailyTrendMetric {
                date,
                total_runs: bucket.total_runs,
                succeeded_runs: bucket.succeeded_runs,
                failed_runs: bucket.failed_runs,
                outcome_reported_runs: bucket.outcome_reported_runs,
                conclusive_outcome_runs: bucket.conclusive_outcome_runs,
                mismatch_runs: bucket.mismatch_runs,
                average_duration_ms: if bucket.total_runs == 0 {
                    0.0
                } else {
                    bucket.duration_total_ms as f64 / bucket.total_runs as f64
                },
            })
            .collect::<Vec<_>>();

        let samples = self.feedback_window.into_iter().collect::<Vec<_>>();
        let mut rule_feedback_indices = HashMap::<(String, String), Vec<usize>>::new();
        let mut runs = Vec::with_capacity(samples.len());
        let mut numeric_inputs = NumericFeedbackInputs::new();
        for (index, sample) in samples.iter().enumerate() {
            runs.push(DecisionServiceRun {
                id: index as i64,
                project_id: sample.project_id,
                version_id: self.version_id,
                service_id: String::new(),
                service_name: String::new(),
                input_json: Value::Null,
                output_json: Value::Null,
                execution_trace_json: Value::Null,
                duration_ms: 0,
                status: "succeeded".into(),
                executed_by: String::new(),
                executed_at: sample.executed_at,
                business_reference: sample.business_reference.clone(),
                actual_outcome_json: None,
                outcome_assessment: Some(
                    if sample.mismatch {
                        "mismatch"
                    } else {
                        "confirmed"
                    }
                    .into(),
                ),
                outcome_note: String::new(),
                outcome_occurred_at: None,
                outcome_reported_by: None,
                outcome_reported_at: None,
            });
            if !sample.numeric_fields.is_empty() {
                numeric_inputs.insert(index, sample.numeric_fields.clone());
            }
            for (decision_id, rule_id) in &sample.matched_rules {
                rule_feedback_indices
                    .entry((decision_id.clone(), rule_id.clone()))
                    .or_default()
                    .push(index);
            }
        }
        let rule_feedback_indices = rule_feedback_indices
            .into_iter()
            .map(|(key, indices)| {
                let newest_first = indices.iter().rev().copied().collect::<Vec<_>>();
                (key, distinct_business_case_indices(&runs, &newest_first))
            })
            .collect::<HashMap<_, _>>();
        let report_hypothesis_count = rule_feedback_indices
            .values()
            .map(|indices| numeric_feedback_hypothesis_count(&numeric_inputs, indices))
            .fold(0usize, usize::saturating_add);
        let mut fisher_tests = FisherTestContext::new(0.05 / report_hypothesis_count.max(1) as f64);
        let mut findings =
            build_findings(&summary, &rule_coverage, &daily_trends, &self.thresholds);
        add_rule_hit_drift_findings(
            &mut findings,
            self.model,
            &self.coverage,
            self.window_start.date_naive(),
            self.generated_at.date_naive(),
        );
        let drift_signals = add_input_distribution_drift_findings(
            &mut findings,
            &samples,
            self.window_start.date_naive(),
            self.generated_at.date_naive(),
        );
        for finding in &mut findings {
            if finding.code != "RULE_FEEDBACK_MISMATCH" {
                continue;
            }
            let Some((decision_id, rule_id)) = finding.element.rsplit_once('/') else {
                continue;
            };
            let sample_indices = rule_feedback_indices
                .get(&(decision_id.to_string(), rule_id.to_string()))
                .map(Vec::as_slice)
                .unwrap_or_default();
            finding.numeric_feedback_split = infer_numeric_feedback_split_for_indices(
                &runs,
                &numeric_inputs,
                sample_indices,
                &mut fisher_tests,
            );
        }
        DecisionAnalyticsReport {
            project_id: self.project_id,
            version_id: self.version_id,
            version_number: self.version_number,
            window_days: ANALYTICS_WINDOW_DAYS,
            window_start: self.window_start,
            generated_at: self.generated_at,
            summary,
            numeric_input_omitted_runs: self.numeric_input_omitted_runs,
            trace_details_omitted_runs: self.trace_details_omitted_runs,
            unknown_result_steps_omitted: self.unknown_result_steps_omitted,
            rule_coverage,
            result_distributions,
            daily_trends,
            findings,
            drift_signals,
        }
    }
}

#[cfg(test)]
fn is_conclusive_feedback_run(run: &DecisionServiceRun) -> bool {
    run.status == "succeeded"
        && matches!(
            run.outcome_assessment.as_deref(),
            Some("confirmed" | "mismatch")
        )
}

#[cfg(test)]
fn latest_conclusive_feedback_indices(runs: &[DecisionServiceRun]) -> Vec<usize> {
    runs.iter()
        .enumerate()
        .rev()
        .filter(|(_, run)| is_conclusive_feedback_run(run))
        .take(MAX_FEEDBACK_SAMPLES)
        .map(|(index, _)| index)
        .collect()
}

// API keys execute as their creator, so the operator is not a business sample identity.
fn distinct_business_case_indices(
    runs: &[DecisionServiceRun],
    newest_first_indices: &[usize],
) -> Vec<usize> {
    let mut seen_cases = HashSet::new();
    newest_first_indices
        .iter()
        .copied()
        .filter(|index| {
            runs.get(*index).is_some_and(|run| {
                let reference = run.business_reference.trim();
                !reference.is_empty() && seen_cases.insert((run.project_id, reference))
            })
        })
        .collect()
}

fn build_result_distributions(
    model: &DmnModel,
    mut counts: HashMap<String, ResultDistributionAccumulator>,
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
            let accumulator = counts.remove(&decision_id)?;
            let total = accumulator.total;
            let mut values = accumulator.counts.into_iter().collect::<Vec<_>>();
            values.sort_by(|(left_value, left_count), (right_value, right_count)| {
                right_count
                    .cmp(left_count)
                    .then_with(|| left_value.cmp(right_value))
            });
            let other_count = if accumulator.details_omitted {
                total
            } else {
                values
                    .iter()
                    .skip(RESULT_VALUE_LIMIT)
                    .map(|(_, count)| *count)
                    .sum::<usize>()
            };
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
                details_omitted: accumulator.details_omitted,
                values,
            })
        })
        .collect()
}

const RULE_HIT_DRIFT_MIN_EXECUTIONS: usize = 20;
const RULE_HIT_DRIFT_MIN_RATE_GAP: f64 = 0.15;
const RULE_HIT_DRIFT_ALLOWANCE: f64 = 0.10;
const RULE_HIT_DRIFT_CUSUM_THRESHOLD: f64 = 0.50;
const INPUT_DISTRIBUTION_MIN_SAMPLES: usize = 20;
const INPUT_DISTRIBUTION_PSI_THRESHOLD: f64 = 0.25;
const INPUT_DISTRIBUTION_MAX_FIELDS: usize = 64;
const INPUT_DISTRIBUTION_FAMILY_ALPHA: f64 = 0.05;
const INPUT_MISSING_RATE_GAP_THRESHOLD: f64 = 0.15;

fn rule_hit_drift_score(
    rule: &RuleHitAccumulator,
    daily_executions: &BTreeMap<NaiveDate, usize>,
    window_start: NaiveDate,
    recent_start: NaiveDate,
    window_end: NaiveDate,
) -> Option<(f64, f64, f64, usize, usize)> {
    let mut baseline_executions = 0usize;
    let mut baseline_hits = 0usize;
    let mut recent_executions = 0usize;
    let mut recent_hits = 0usize;
    let mut positive_cusum = 0.0_f64;
    let mut negative_cusum = 0.0_f64;

    for (day, executions) in daily_executions {
        if *day < window_start || *day > window_end {
            continue;
        }
        let hits = rule.daily_hits.get(day).copied().unwrap_or_default();
        if *day < recent_start {
            baseline_executions += *executions;
            baseline_hits += hits;
        } else {
            recent_executions += *executions;
            recent_hits += hits;
        }
    }
    if baseline_executions < RULE_HIT_DRIFT_MIN_EXECUTIONS
        || recent_executions < RULE_HIT_DRIFT_MIN_EXECUTIONS
    {
        return None;
    }

    let baseline_rate = baseline_hits as f64 / baseline_executions as f64;
    let recent_rate = recent_hits as f64 / recent_executions as f64;
    for (day, executions) in daily_executions {
        if *day < recent_start || *day > window_end || *executions == 0 {
            continue;
        }
        let daily_hits = rule.daily_hits.get(day).copied().unwrap_or_default();
        let daily_rate = daily_hits as f64 / *executions as f64;
        let deviation = daily_rate - baseline_rate;
        positive_cusum = (positive_cusum + deviation - RULE_HIT_DRIFT_ALLOWANCE).max(0.0);
        negative_cusum = (negative_cusum + deviation + RULE_HIT_DRIFT_ALLOWANCE).min(0.0);
    }
    let score = positive_cusum.max(-negative_cusum);
    ((recent_rate - baseline_rate).abs() >= RULE_HIT_DRIFT_MIN_RATE_GAP
        && score >= RULE_HIT_DRIFT_CUSUM_THRESHOLD)
        .then_some((
            baseline_rate,
            recent_rate,
            score,
            baseline_executions,
            recent_executions,
        ))
}

fn add_rule_hit_drift_findings(
    findings: &mut Vec<DecisionAnalyticsFinding>,
    model: &DmnModel,
    coverage: &HashMap<String, RuleCoverageAccumulator>,
    window_start: NaiveDate,
    window_end: NaiveDate,
) {
    let recent_start = window_end - Duration::days(6);
    for decision in &model.decisions {
        if !matches!(&decision.logic, DecisionLogic::DecisionTable(_)) {
            continue;
        }
        let Some(metric) = coverage.get(&decision.id) else {
            continue;
        };
        for (rule_id, rule) in &metric.hits {
            let Some((baseline_rate, recent_rate, score, baseline_n, recent_n)) =
                rule_hit_drift_score(
                    rule,
                    &metric.daily_executions,
                    window_start,
                    recent_start,
                    window_end,
                )
            else {
                continue;
            };
            let level = if score >= 1.0 && (recent_rate - baseline_rate).abs() >= 0.30 {
                "high"
            } else {
                "medium"
            };
            findings.push(finding(
                level,
                "RULE_HIT_DRIFT",
                format!(
                    "规则“{rule_id}”最近 7 天命中率为 {:.1}%，前期基线为 {:.1}%；CUSUM {:.2}，基线/近期执行样本 {baseline_n}/{recent_n}。",
                    recent_rate * 100.0,
                    baseline_rate * 100.0,
                    score
                ),
                format!("{}/{}", decision.id, rule_id),
            ));
        }
    }
}

fn population_stability_index(baseline: &[f64], recent: &[f64]) -> Option<(f64, f64)> {
    if baseline.len() < INPUT_DISTRIBUTION_MIN_SAMPLES
        || recent.len() < INPUT_DISTRIBUTION_MIN_SAMPLES
        || baseline
            .iter()
            .chain(recent)
            .any(|value| !value.is_finite())
    {
        return None;
    }

    let mut sorted_baseline = baseline.to_vec();
    sorted_baseline.sort_by(f64::total_cmp);
    let mut boundaries = Vec::new();
    for quantile in 1..10 {
        let split = sorted_baseline.len() * quantile / 10;
        if split == 0 || split >= sorted_baseline.len() {
            continue;
        }
        let lower = sorted_baseline[split - 1];
        let upper = sorted_baseline[split];
        if lower < upper {
            let boundary = lower + (upper - lower) / 2.0;
            if boundaries
                .last()
                .is_none_or(|previous| *previous < boundary)
            {
                boundaries.push(boundary);
            }
        }
    }
    if boundaries.is_empty() {
        boundaries.push(sorted_baseline[sorted_baseline.len() / 2]);
    }

    let bin_count = boundaries.len() + 1;
    let mut baseline_bins = vec![0usize; bin_count];
    let mut recent_bins = vec![0usize; bin_count];
    for value in baseline {
        baseline_bins[boundaries.partition_point(|boundary| value > boundary)] += 1;
    }
    for value in recent {
        recent_bins[boundaries.partition_point(|boundary| value > boundary)] += 1;
    }

    let baseline_denominator = baseline.len() as f64 + 0.5 * bin_count as f64;
    let recent_denominator = recent.len() as f64 + 0.5 * bin_count as f64;
    let psi = baseline_bins
        .iter()
        .zip(recent_bins.iter())
        .map(|(baseline_count, recent_count)| {
            let baseline_rate = (*baseline_count as f64 + 0.5) / baseline_denominator;
            let recent_rate = (*recent_count as f64 + 0.5) / recent_denominator;
            (recent_rate - baseline_rate) * (recent_rate / baseline_rate).ln()
        })
        .sum();

    // Merge sparse adjacent bins before the two-sample homogeneity test.
    let minimum_expected = 5.0;
    let minimum_pooled = minimum_expected * (baseline.len() + recent.len()) as f64
        / baseline.len().min(recent.len()) as f64;
    let mut grouped = Vec::<(usize, usize)>::new();
    let mut pending_baseline = 0usize;
    let mut pending_recent = 0usize;
    for (baseline_count, recent_count) in baseline_bins.into_iter().zip(recent_bins) {
        pending_baseline += baseline_count;
        pending_recent += recent_count;
        if (pending_baseline + pending_recent) as f64 >= minimum_pooled {
            grouped.push((pending_baseline, pending_recent));
            pending_baseline = 0;
            pending_recent = 0;
        }
    }
    if pending_baseline + pending_recent > 0 {
        if let Some(last) = grouped.last_mut() {
            last.0 += pending_baseline;
            last.1 += pending_recent;
        } else {
            return Some((psi, 1.0));
        }
    }
    if grouped.len() < 2 {
        return Some((psi, 1.0));
    }

    let total = (baseline.len() + recent.len()) as f64;
    let mut chi_square = 0.0;
    for (baseline_count, recent_count) in grouped.iter().copied() {
        let pooled = (baseline_count + recent_count) as f64;
        let expected_baseline = pooled * baseline.len() as f64 / total;
        let expected_recent = pooled * recent.len() as f64 / total;
        chi_square += (baseline_count as f64 - expected_baseline).powi(2) / expected_baseline;
        chi_square += (recent_count as f64 - expected_recent).powi(2) / expected_recent;
    }
    Some((psi, chi_square_survival(chi_square, grouped.len() - 1)))
}

fn chi_square_survival(statistic: f64, degrees_of_freedom: usize) -> f64 {
    regularized_gamma_q(degrees_of_freedom as f64 / 2.0, statistic / 2.0)
}

fn bonferroni_adjusted_p(p_value: f64, hypothesis_count: usize) -> f64 {
    (p_value * hypothesis_count.max(1) as f64).min(1.0)
}

fn binary_distribution_p_value(
    baseline_present: usize,
    baseline_missing: usize,
    recent_present: usize,
    recent_missing: usize,
) -> Option<f64> {
    let baseline_total = baseline_present + baseline_missing;
    let recent_total = recent_present + recent_missing;
    if baseline_total < INPUT_DISTRIBUTION_MIN_SAMPLES
        || recent_total < INPUT_DISTRIBUTION_MIN_SAMPLES
    {
        return None;
    }
    let total = (baseline_total + recent_total) as f64;
    let present_total = (baseline_present + recent_present) as f64;
    let missing_total = (baseline_missing + recent_missing) as f64;
    let expected = [
        present_total * baseline_total as f64 / total,
        missing_total * baseline_total as f64 / total,
        present_total * recent_total as f64 / total,
        missing_total * recent_total as f64 / total,
    ];
    if expected.iter().any(|value| *value < 5.0) {
        return None;
    }
    let observed = [
        baseline_present as f64,
        baseline_missing as f64,
        recent_present as f64,
        recent_missing as f64,
    ];
    let statistic = observed
        .iter()
        .zip(expected)
        .map(|(observed, expected)| (observed - expected).powi(2) / expected)
        .sum();
    Some(chi_square_survival(statistic, 1))
}

fn regularized_gamma_q(shape: f64, value: f64) -> f64 {
    if !shape.is_finite() || shape <= 0.0 || value.is_nan() || value < 0.0 {
        return 1.0;
    }
    if value == 0.0 {
        return 1.0;
    }
    let log_scale = -value + shape * value.ln() - log_gamma(shape);
    if value < shape + 1.0 {
        let mut term = 1.0 / shape;
        let mut sum = term;
        let mut denominator = shape;
        for _ in 0..10_000 {
            denominator += 1.0;
            term *= value / denominator;
            sum += term;
            if term.abs() <= sum.abs() * 1e-14 {
                break;
            }
        }
        (1.0 - sum * log_scale.exp()).clamp(0.0, 1.0)
    } else {
        let tiny = 1e-300;
        let mut b = value + 1.0 - shape;
        let mut c = 1.0 / tiny;
        let mut d = 1.0 / b.max(tiny);
        let mut fraction = d;
        for index in 1..=10_000 {
            let index = index as f64;
            let a = -index * (index - shape);
            b += 2.0;
            d = (a * d + b).max(tiny);
            c = (b + a / c).max(tiny);
            d = 1.0 / d;
            let delta = d * c;
            fraction *= delta;
            if (delta - 1.0).abs() <= 1e-14 {
                break;
            }
        }
        (fraction * log_scale.exp()).clamp(0.0, 1.0)
    }
}

fn log_gamma(value: f64) -> f64 {
    const COEFFICIENTS: [f64; 8] = [
        676.5203681218851,
        -1259.1392167224028,
        771.3234287776531,
        -176.6150291621406,
        12.507343278686905,
        -0.13857109526572012,
        9.984369578019572e-6,
        1.5056327351493116e-7,
    ];
    if value < 0.5 {
        return std::f64::consts::PI.ln()
            - (std::f64::consts::PI * value).sin().ln()
            - log_gamma(1.0 - value);
    }
    let shifted = value - 1.0;
    let mut series = 0.9999999999998099;
    for (index, coefficient) in COEFFICIENTS.iter().enumerate() {
        series += coefficient / (shifted + index as f64 + 1.0);
    }
    let t = shifted + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (shifted + 0.5) * t.ln() - t + series.ln()
}

fn add_input_distribution_drift_findings(
    findings: &mut Vec<DecisionAnalyticsFinding>,
    samples: &[FeedbackSample],
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Vec<DecisionAnalyticsDriftSignal> {
    let recent_start = window_end - Duration::days(6);
    let field_names = samples
        .iter()
        .filter(|sample| {
            let day = sample.executed_at.date_naive();
            day >= window_start && day <= window_end
        })
        .flat_map(|sample| sample.numeric_fields.iter().map(|(field, _)| field.clone()))
        .collect::<BTreeSet<_>>();
    let selected_fields = field_names
        .iter()
        .take(INPUT_DISTRIBUTION_MAX_FIELDS)
        .cloned()
        .collect::<HashSet<_>>();
    let omitted_fields = field_names.len().saturating_sub(selected_fields.len());
    let mut fields = HashMap::<String, (Vec<f64>, Vec<f64>, usize, usize)>::new();
    let mut baseline_observed_total = 0usize;
    let mut recent_observed_total = 0usize;
    for sample in samples {
        let day = sample.executed_at.date_naive();
        if day < window_start || day > window_end {
            continue;
        }
        let baseline = day < recent_start;
        if sample.missingness_observed {
            if baseline {
                baseline_observed_total += 1;
            } else {
                recent_observed_total += 1;
            }
        }
        for (field, value) in &sample.numeric_fields {
            if !selected_fields.contains(field) {
                continue;
            }
            let bucket = fields.entry(field.clone()).or_default();
            if baseline {
                bucket.0.push(*value);
                if sample.missingness_observed {
                    bucket.2 += 1;
                }
            } else {
                bucket.1.push(*value);
                if sample.missingness_observed {
                    bucket.3 += 1;
                }
            }
        }
    }

    let mut fields = fields.into_iter().collect::<Vec<_>>();
    fields.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hypotheses = Vec::<(
        f64,
        bool,
        DecisionAnalyticsFinding,
        DecisionAnalyticsDriftSignal,
    )>::new();
    for (field, (baseline, recent, baseline_present, recent_present)) in fields {
        if let Some((psi, p_value)) = population_stability_index(&baseline, &recent) {
            let severity = if psi >= 0.50 { "high" } else { "medium" };
            hypotheses.push((
                p_value,
                psi >= INPUT_DISTRIBUTION_PSI_THRESHOLD,
                finding(
                    severity,
                    "INPUT_DISTRIBUTION_DRIFT",
                    format!(
                        "数值输入字段“{field}”最近 7 天相对前期分布 PSI 为 {psi:.3}（非缺失样本 {}/{})；该信号只描述有值反馈样本的分布变化，不代表全量流量或业务结果退化。",
                        recent.len(),
                        baseline.len()
                    ),
                    field.clone(),
                ),
                DecisionAnalyticsDriftSignal {
                    code: "INPUT_DISTRIBUTION_DRIFT".into(),
                    element: field.clone(),
                    metric_value: psi,
                    p_value,
                    baseline_sample_count: baseline.len(),
                    recent_sample_count: recent.len(),
                },
            ));
        }
        if let Some(p_value) = binary_distribution_p_value(
            baseline_present,
            baseline_observed_total.saturating_sub(baseline_present),
            recent_present,
            recent_observed_total.saturating_sub(recent_present),
        ) {
            let baseline_missing_rate = (baseline_observed_total - baseline_present) as f64
                / baseline_observed_total as f64;
            let recent_missing_rate =
                (recent_observed_total - recent_present) as f64 / recent_observed_total as f64;
            let gap = (recent_missing_rate - baseline_missing_rate).abs();
            hypotheses.push((
                p_value,
                gap >= INPUT_MISSING_RATE_GAP_THRESHOLD,
                finding(
                    "medium",
                    "INPUT_FIELD_MISSINGNESS_DRIFT",
                    format!(
                        "数值输入字段“{field}”最近 7 天缺失率为 {:.1}%（已完整扫描样本 n={recent_observed_total}），前期为 {:.1}%（已完整扫描样本 n={baseline_observed_total}）；未扫描或触及字段上限的输入不计入该比较。",
                        recent_missing_rate * 100.0,
                        baseline_missing_rate * 100.0
                    ),
                    field.clone(),
                ),
                DecisionAnalyticsDriftSignal {
                    code: "INPUT_FIELD_MISSINGNESS_DRIFT".into(),
                    element: field,
                    metric_value: gap,
                    p_value,
                    baseline_sample_count: baseline_observed_total,
                    recent_sample_count: recent_observed_total,
                },
            ));
        }
    }
    let hypothesis_count = hypotheses.len();
    let mut signals = Vec::with_capacity(hypothesis_count);
    for (p_value, effect_threshold_met, mut candidate, signal) in hypotheses {
        let adjusted_p = bonferroni_adjusted_p(p_value, hypothesis_count);
        signals.push(signal);
        if effect_threshold_met && adjusted_p <= INPUT_DISTRIBUTION_FAMILY_ALPHA {
            candidate.message.push_str(&format!(
                " Bonferroni 校正 p={adjusted_p:.4}，本报告检验 {hypothesis_count} 个假设。"
            ));
            findings.push(candidate);
        }
    }
    if omitted_fields > 0 {
        findings.push(finding(
            "medium",
            "INPUT_DISTRIBUTION_FIELDS_CAPPED",
            format!(
                "本次 PSI 检查按字段名排序，仅评估前 {INPUT_DISTRIBUTION_MAX_FIELDS} 个数值输入字段；另有 {omitted_fields} 个字段未评估。"
            ),
            "",
        ));
    }
    signals
}

fn build_findings(
    summary: &DecisionAnalyticsSummary,
    rule_coverage: &[DecisionRuleCoverageMetric],
    daily_trends: &[DecisionDailyTrendMetric],
    thresholds: &DecisionAnalyticsThresholds,
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
    if summary.total_runs >= thresholds.failure_min_runs.max(1) as usize
        && summary.failure_rate >= thresholds.failure_warning_rate
    {
        let level = if summary.failure_rate >= thresholds.failure_high_rate {
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
    if summary.conclusive_outcome_runs >= thresholds.mismatch_min_samples.max(1) as usize
        && summary.mismatch_rate >= thresholds.mismatch_warning_rate
    {
        let level = if summary.mismatch_rate >= thresholds.mismatch_high_rate {
            "high"
        } else {
            "medium"
        };
        findings.push(finding(
            level,
            "HIGH_MISMATCH_RATE",
            format!(
                "有效业务判定中 {:.1}% 被判定为存在偏差，共 {} 次。",
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
        for rule in &metric.rules {
            if rule.feedback_count >= RULE_FEEDBACK_MIN_SAMPLE_COUNT
                && rule.mismatch_rate >= RULE_FEEDBACK_MISMATCH_THRESHOLD
            {
                let level = if rule.mismatch_rate >= 0.50 {
                    "high"
                } else {
                    "medium"
                };
                findings.push(finding(
                    level,
                    "RULE_FEEDBACK_MISMATCH",
                    format!(
                        "规则“{}”命中后有 {}/{} 次业务反馈为偏差（{:.1}%）。",
                        rule.rule_id,
                        rule.mismatch_count,
                        rule.feedback_count,
                        rule.mismatch_rate * 100.0
                    ),
                    format!("{}/{}", metric.decision_id, rule.rule_id),
                ));
            }
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

#[cfg(test)]
fn infer_numeric_feedback_split(
    runs: &[DecisionServiceRun],
    decision_id: &str,
    rule_id: &str,
) -> Option<NumericFeedbackSplitHint> {
    let mut sample_indices = Vec::new();
    for index in latest_conclusive_feedback_indices(runs) {
        let run = &runs[index];
        let Ok(trace) = serde_json::from_value::<ExecutionTrace>(run.execution_trace_json.clone())
        else {
            continue;
        };
        if trace.decisions.iter().any(|step| {
            step.decision_id == decision_id && step.matched_rule_ids.iter().any(|id| id == rule_id)
        }) {
            sample_indices.push(index);
        }
    }
    let sample_indices = distinct_business_case_indices(runs, &sample_indices);
    let numeric_inputs = numeric_feedback_inputs(runs, &sample_indices);
    let hypothesis_count = numeric_feedback_hypothesis_count(&numeric_inputs, &sample_indices);
    let mut fisher_tests = FisherTestContext::new(0.05 / hypothesis_count.max(1) as f64);
    infer_numeric_feedback_split_for_indices(
        runs,
        &numeric_inputs,
        &sample_indices,
        &mut fisher_tests,
    )
}

#[cfg(test)]
fn numeric_feedback_inputs(
    runs: &[DecisionServiceRun],
    sample_indices: &[usize],
) -> NumericFeedbackInputs {
    sample_indices
        .iter()
        .take(MAX_FEEDBACK_SAMPLES)
        .filter_map(|index| {
            let run = runs.get(*index)?;
            let mut collector = NumericInputCollector::new();
            // Never use a partially scanned input in either the family size or inference.
            if !collector.collect(&run.input_json, "", 0) {
                return None;
            }
            Some((*index, collector.fields.into_iter().collect()))
        })
        .collect()
}

fn numeric_feedback_hypothesis_count(
    numeric_inputs: &NumericFeedbackInputs,
    sample_indices: &[usize],
) -> usize {
    const MIN_GROUP_SIZE: usize = 3;
    let mut observations = BTreeMap::<&str, Vec<f64>>::new();
    for index in sample_indices.iter().take(MAX_FEEDBACK_SAMPLES) {
        if let Some(fields) = numeric_inputs.get(index) {
            for (field, value) in fields {
                observations.entry(field.as_str()).or_default().push(*value);
            }
        }
    }

    // Keep the family size independent of the mismatch/confirmed label balance.
    observations
        .into_values()
        .map(|mut values| {
            if values.len() < MIN_GROUP_SIZE * 2 {
                return 0;
            }
            values.sort_by(f64::total_cmp);
            (MIN_GROUP_SIZE..=values.len() - MIN_GROUP_SIZE)
                .filter(|split| values[split - 1] < values[*split])
                .count()
        })
        .fold(0usize, usize::saturating_add)
}

fn infer_numeric_feedback_split_for_indices(
    runs: &[DecisionServiceRun],
    numeric_inputs: &NumericFeedbackInputs,
    sample_indices: &[usize],
    fisher_tests: &mut FisherTestContext,
) -> Option<NumericFeedbackSplitHint> {
    const MIN_GROUP_SIZE: usize = 3;
    const MIN_RATE_GAP: f64 = 0.25;

    let mut observations = BTreeMap::<&str, Vec<(f64, bool)>>::new();
    for index in sample_indices.iter().take(MAX_FEEDBACK_SAMPLES) {
        let Some(run) = runs.get(*index) else {
            continue;
        };
        let Some(fields) = numeric_inputs.get(index) else {
            continue;
        };
        let is_mismatch = run.outcome_assessment.as_deref() == Some("mismatch");
        for (field, value) in fields {
            observations
                .entry(field.as_str())
                .or_default()
                .push((*value, is_mismatch));
        }
    }

    let mut fields = Vec::new();
    let mut hypothesis_count = 0usize;
    for (field, mut values) in observations {
        if values.len() < MIN_GROUP_SIZE * 2 {
            continue;
        }
        values.sort_by(|left, right| left.0.total_cmp(&right.0));
        // Count every valid cut before applying the observed effect-size filter.
        let valid_splits = (MIN_GROUP_SIZE..=values.len() - MIN_GROUP_SIZE)
            .filter(|split| values[split - 1].0 < values[*split].0)
            .count();
        hypothesis_count += valid_splits;
        let mismatch_count = values.iter().filter(|(_, mismatch)| *mismatch).count();
        if mismatch_count < MIN_GROUP_SIZE || values.len() - mismatch_count < MIN_GROUP_SIZE {
            continue;
        }
        fields.push((field, values, mismatch_count));
    }
    if hypothesis_count == 0 || fields.is_empty() {
        return None;
    }

    let mut best: Option<(f64, String, f64, usize, usize, usize, usize)> = None;
    for (field, values, mismatch_count) in fields {
        let mut mismatch_prefix = Vec::with_capacity(values.len() + 1);
        mismatch_prefix.push(0usize);
        for (_, mismatch) in &values {
            mismatch_prefix
                .push(mismatch_prefix.last().copied().unwrap_or_default() + usize::from(*mismatch));
        }
        for split in MIN_GROUP_SIZE..=values.len() - MIN_GROUP_SIZE {
            let lower = &values[..split];
            let upper = &values[split..];
            if lower.last()?.0 >= upper.first()?.0 {
                continue;
            }
            let lower_mismatches = mismatch_prefix[split];
            let upper_mismatches = mismatch_count - lower_mismatches;
            let lower_rate = ratio(lower_mismatches, lower.len());
            let upper_rate = ratio(upper_mismatches, upper.len());
            let gap = (lower_rate - upper_rate).abs();
            if gap < MIN_RATE_GAP || best.as_ref().is_some_and(|candidate| gap <= candidate.0) {
                continue;
            }
            let contingency = (
                lower_mismatches,
                lower.len() - lower_mismatches,
                upper_mismatches,
                upper.len() - upper_mismatches,
            );
            if !fisher_tests.is_significant(contingency) {
                continue;
            }
            let lower_value = lower.last()?.0;
            let upper_value = upper.first()?.0;
            let threshold = lower_value / 2.0 + upper_value / 2.0;
            if threshold.is_finite() {
                best = Some((
                    gap,
                    field.to_string(),
                    threshold,
                    lower_mismatches,
                    lower.len(),
                    upper_mismatches,
                    upper.len(),
                ));
            }
        }
    }

    best.map(
        |(
            _,
            input_field,
            threshold,
            lower_mismatch_count,
            lower_sample_count,
            upper_mismatch_count,
            upper_sample_count,
        )| {
            NumericFeedbackSplitHint {
                business_case_deduplicated: true,
                input_field,
                threshold,
                lower_mismatch_count,
                lower_sample_count,
                upper_mismatch_count,
                upper_sample_count,
            }
        },
    )
}

// Two-sided Fisher probability ordering; stop summing once the adjusted cutoff is exceeded.
fn fisher_exact_is_significant(
    (lower_mismatches, lower_confirmed, upper_mismatches, upper_confirmed): (
        usize,
        usize,
        usize,
        usize,
    ),
    log_factorials: &[f64],
    maximum_p: f64,
) -> bool {
    let lower_count = lower_mismatches + lower_confirmed;
    let upper_count = upper_mismatches + upper_confirmed;
    let mismatch_count = lower_mismatches + upper_mismatches;
    let total_count = lower_count + upper_count;
    let observed_log_probability = log_combination(log_factorials, lower_count, lower_mismatches)
        + log_combination(log_factorials, upper_count, upper_mismatches)
        - log_combination(log_factorials, total_count, mismatch_count);
    if observed_log_probability.exp() > maximum_p {
        return false;
    }

    let minimum_lower_mismatches = mismatch_count.saturating_sub(upper_count);
    let maximum_lower_mismatches = mismatch_count.min(lower_count);
    let mut two_sided_p = 0.0;
    for lower_mismatch_count in minimum_lower_mismatches..=maximum_lower_mismatches {
        let upper_mismatch_count = mismatch_count - lower_mismatch_count;
        let log_probability = log_combination(log_factorials, lower_count, lower_mismatch_count)
            + log_combination(log_factorials, upper_count, upper_mismatch_count)
            - log_combination(log_factorials, total_count, mismatch_count);
        if log_probability <= observed_log_probability + 1e-9 {
            two_sided_p += log_probability.exp();
            if two_sided_p > maximum_p {
                return false;
            }
        }
    }
    true
}

fn log_combination(log_factorials: &[f64], count: usize, selected: usize) -> f64 {
    log_factorials[count] - log_factorials[selected] - log_factorials[count - selected]
}

struct NumericInputCollector {
    fields: BTreeMap<String, f64>,
    remaining_nodes: usize,
    remaining_path_bytes: usize,
}

impl NumericInputCollector {
    fn new() -> Self {
        Self {
            fields: BTreeMap::new(),
            remaining_nodes: MAX_NUMERIC_INPUT_NODES,
            remaining_path_bytes: MAX_NUMERIC_INPUT_PATH_BYTES,
        }
    }

    // A false result invalidates the entire input, including fields already collected.
    fn collect(&mut self, value: &Value, prefix: &str, depth: usize) -> bool {
        let Some(remaining) = self.remaining_nodes.checked_sub(1) else {
            return false;
        };
        self.remaining_nodes = remaining;
        match value {
            Value::Number(number) => {
                if !prefix.is_empty() {
                    if let Some(value) = number.as_f64().filter(|value| value.is_finite()) {
                        // Object traversal order differs from encoded full-path ordering.
                        // Keep only the smallest paths without accumulating every field.
                        if self.fields.len() == MAX_NUMERIC_INPUT_FIELDS {
                            if self
                                .fields
                                .last_key_value()
                                .is_some_and(|(last, _)| prefix >= last.as_str())
                            {
                                return true;
                            }
                            self.fields.pop_last();
                        }
                        self.fields.insert(prefix.to_owned(), value);
                    }
                }
            }
            Value::Object(object) if depth < MAX_NUMERIC_INPUT_DEPTH => {
                for (key, value) in object {
                    // Check the raw-byte lower bound before scanning or escaping a huge key.
                    if self.remaining_nodes == 0
                        || key.len() > self.remaining_path_bytes.saturating_sub(prefix.len())
                        || prefix.len() > self.remaining_path_bytes
                    {
                        return false;
                    }
                    let path = numeric_field_path(prefix, key);
                    let Some(remaining) = self.remaining_path_bytes.checked_sub(path.len()) else {
                        return false;
                    };
                    self.remaining_path_bytes = remaining;
                    if !self.collect(value, &path, depth + 1) {
                        return false;
                    }
                }
            }
            _ => {}
        }
        true
    }
}

// Preserve familiar dotted identifiers while quoting literal keys unambiguously.
fn numeric_field_path(prefix: &str, key: &str) -> String {
    let mut characters = key.chars();
    let identifier = characters
        .next()
        .is_some_and(|character| character.is_alphabetic() || matches!(character, '_' | '$'))
        && characters
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '$'));
    if identifier {
        if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        }
    } else {
        let quoted_key =
            serde_json::to_string(key).expect("a string key can be serialized as JSON");
        format!("{prefix}[{quoted_key}]")
    }
}

// Decode only canonical paths emitted by the bounded numeric input collector.
pub(crate) fn numeric_field_segments(path: &str) -> Option<Vec<String>> {
    if path.is_empty() || path.len() > MAX_NUMERIC_INPUT_PATH_BYTES {
        return None;
    }
    let mut remaining = path;
    let mut segments = Vec::new();
    let mut canonical = String::new();
    while !remaining.is_empty() {
        if !segments.is_empty() && !remaining.starts_with('[') {
            remaining = remaining.strip_prefix('.')?;
        }
        let key = if let Some(quoted) = remaining.strip_prefix('[') {
            let mut stream = serde_json::Deserializer::from_str(quoted).into_iter::<String>();
            let key = stream.next()?.ok()?;
            remaining = quoted.get(stream.byte_offset()..)?.strip_prefix(']')?;
            key
        } else {
            let end = remaining.find(['.', '[']).unwrap_or(remaining.len());
            let key = remaining.get(..end)?;
            if key.is_empty() {
                return None;
            }
            remaining = remaining.get(end..)?;
            key.to_string()
        };
        canonical = numeric_field_path(&canonical, &key);
        segments.push(key);
        if segments.len() > MAX_NUMERIC_INPUT_DEPTH {
            return None;
        }
    }
    (canonical == path).then_some(segments)
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

fn normalized_json_text(value: &Value) -> Option<String> {
    let mut remaining_nodes = MAX_RESULT_VALUE_NODES;
    let mut remaining_text_bytes = MAX_RESULT_DECISION_BYTES;
    if !result_value_within_budget(value, 0, &mut remaining_nodes, &mut remaining_text_bytes) {
        return None;
    }
    // Preflight bounds the canonical tree clone; the writer also bounds escaped output.
    let mut buffer = ResultJsonBuffer(Vec::new());
    serde_json::to_writer(&mut buffer, &normalize_json(value)).ok()?;
    String::from_utf8(buffer.0).ok()
}

fn result_value_within_budget(
    value: &Value,
    depth: usize,
    remaining_nodes: &mut usize,
    remaining_text_bytes: &mut usize,
) -> bool {
    if depth > MAX_RESULT_VALUE_DEPTH || *remaining_nodes == 0 {
        return false;
    }
    *remaining_nodes -= 1;
    match value {
        Value::String(text) => {
            let Some(remaining) = remaining_text_bytes.checked_sub(text.len()) else {
                return false;
            };
            *remaining_text_bytes = remaining;
            true
        }
        Value::Object(entries) => {
            if entries.len() > *remaining_nodes {
                return false;
            }
            for (key, child) in entries {
                let Some(remaining) = remaining_text_bytes.checked_sub(key.len()) else {
                    return false;
                };
                *remaining_text_bytes = remaining;
                if !result_value_within_budget(
                    child,
                    depth + 1,
                    remaining_nodes,
                    remaining_text_bytes,
                ) {
                    return false;
                }
            }
            true
        }
        Value::Array(entries) => {
            entries.len() <= *remaining_nodes
                && entries.iter().all(|child| {
                    result_value_within_budget(
                        child,
                        depth + 1,
                        remaining_nodes,
                        remaining_text_bytes,
                    )
                })
        }
        _ => true,
    }
}

struct ResultJsonBuffer(Vec<u8>);

impl std::io::Write for ResultJsonBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_RESULT_DECISION_BYTES - self.0.len() {
            return Err(std::io::Error::other(
                "result JSON exceeds analytics budget",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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
        numeric_feedback_split: None,
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
            requirement_ids: Vec::new(),
            matched_rule_requirements: Vec::new(),
            result,
            explanation: String::new(),
        }
    }

    // Run explicitly with --ignored --nocapture --test-threads=1; excludes fixture construction.
    #[test]
    #[ignore = "local analytics CPU scale benchmark; no database or LLM required"]
    fn benchmark_decision_analytics_scale() {
        const SAMPLE_COUNT: usize = 2_000;
        const FIELD_COUNT: usize = 64;
        const REPETITIONS: usize = 3;
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let mix = |mut value: u64| {
            value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
            value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            value ^ (value >> 31)
        };
        for rule_count in [1, 8, 32] {
            let rule_ids = (0..rule_count)
                .map(|id| format!("rule-{id}"))
                .collect::<Vec<_>>();
            let rules_xml = rule_ids.iter().map(|id| format!(
                r#"<rule id="{id}"><inputEntry><text>-</text></inputEntry><outputEntry><text>"approve"</text></outputEntry></rule>"#
            )).collect::<String>();
            let model = parse_dmn(&format!(
                r#"<definitions xmlns="{DMN_NAMESPACE}" name="Scale"><decision id="scale" name="Scale"><decisionTable hitPolicy="COLLECT"><input><inputExpression><text>field_00</text></inputExpression></input><output name="result"/>{rules_xml}</decisionTable></decision></definitions>"#
            )).unwrap();
            crate::dmn::compile_dmn(&model).unwrap();
            let rule_refs = rule_ids.iter().map(String::as_str).collect::<Vec<_>>();
            for signal in [false, true] {
                let runs = (0..SAMPLE_COUNT)
                    .map(|sample| {
                        let mut record = run(
                            sample as i64 + 1,
                            23,
                            "succeeded",
                            100,
                            Some(if sample < SAMPLE_COUNT / 2 {
                                "mismatch"
                            } else {
                                "confirmed"
                            }),
                            vec![table_step(
                                "scale",
                                "Scale",
                                "rule_match",
                                &rule_refs,
                                json!("approve"),
                            )],
                        );
                        let input = (0..FIELD_COUNT)
                            .map(|field| {
                                let value = if signal && field == FIELD_COUNT - 1 {
                                    sample as f64
                                } else {
                                    (mix((sample * FIELD_COUNT + field) as u64) >> 11) as f64
                                };
                                (format!("field_{field:02}"), json!(value))
                            })
                            .collect::<Map<_, _>>();
                        record.input_json = Value::Object(input);
                        record
                    })
                    .collect::<Vec<_>>();
                let mut elapsed_ms = Vec::new();
                let indices = latest_conclusive_feedback_indices(&runs);
                let numeric_inputs = numeric_feedback_inputs(&runs, &indices);
                assert_eq!(numeric_inputs.len(), SAMPLE_COUNT);
                assert!(
                    numeric_inputs
                        .values()
                        .all(|fields| fields.len() == FIELD_COUNT)
                );
                assert_eq!(
                    numeric_feedback_hypothesis_count(&numeric_inputs, &indices),
                    FIELD_COUNT * (SAMPLE_COUNT - 5)
                );
                for _ in 0..REPETITIONS {
                    let started = std::time::Instant::now();
                    let report = std::hint::black_box(build_decision_analytics(
                        5,
                        7,
                        3,
                        &model,
                        &runs,
                        start,
                        generated_at,
                    ));
                    elapsed_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
                    assert_eq!(report.summary.total_runs, SAMPLE_COUNT);
                    assert_eq!(report.rule_coverage[0].covered_rules, rule_count);
                    assert!(report.rule_coverage[0].rules.iter().all(|rule| {
                        rule.feedback_count == SAMPLE_COUNT
                            && rule.mismatch_count == SAMPLE_COUNT / 2
                    }));
                    assert_eq!(
                        report
                            .findings
                            .iter()
                            .filter(|finding| { finding.code == "RULE_FEEDBACK_MISMATCH" })
                            .count(),
                        rule_count
                    );
                    let hints = report
                        .findings
                        .iter()
                        .filter_map(|finding| finding.numeric_feedback_split.as_ref())
                        .collect::<Vec<_>>();
                    assert_eq!(hints.len(), if signal { rule_count } else { 0 });
                    for hint in hints {
                        assert_eq!(hint.input_field, "field_63");
                        assert_eq!(
                            hint.lower_sample_count + hint.upper_sample_count,
                            SAMPLE_COUNT
                        );
                        assert!(hint.business_case_deduplicated);
                    }
                }
                println!(
                    "analytics_scale {}",
                    json!({
                        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
                        "runs": SAMPLE_COUNT,
                        "numeric_fields": FIELD_COUNT,
                        "matched_rules": rule_count,
                        "scenario": if signal { "late_strong_signal" } else { "deterministic_noise" },
                        "elapsed_ms": elapsed_ms,
                    })
                );
            }
        }
    }

    #[test]
    fn numeric_feedback_window_keeps_only_the_latest_conclusive_runs() {
        let runs = (0..MAX_FEEDBACK_SAMPLES + 5)
            .map(|index| {
                run(
                    index as i64 + 1,
                    23,
                    "succeeded",
                    100,
                    Some("confirmed"),
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>();

        let indices = latest_conclusive_feedback_indices(&runs);

        assert_eq!(indices.len(), MAX_FEEDBACK_SAMPLES);
        assert_eq!(indices.first(), Some(&(MAX_FEEDBACK_SAMPLES + 4)));
        assert_eq!(indices.last(), Some(&5));
    }

    #[test]
    fn numeric_feedback_sampling_uses_project_business_case_not_operator() {
        let mut runs = (0..7)
            .map(|index| {
                run(
                    index + 1,
                    23,
                    "succeeded",
                    100,
                    Some("confirmed"),
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>();
        // Different operators can rerun the same case; surrounding spaces are ignored.
        runs[0].business_reference = " CASE-1 ".into();
        runs[0].executed_by = "another-operator".into();
        runs[1].business_reference = "CASE-1".into();
        // The same reference in another project denotes a different case.
        runs[2].business_reference = "CASE-1".into();
        runs[2].project_id = 6;
        runs[3].business_reference = "   ".into();
        runs[4].business_reference = String::new();
        // Cases 6 and 7 share an operator, but both must contribute.

        assert_eq!(
            distinct_business_case_indices(&runs, &[6, 5, 4, 3, 2, 1, 0]),
            [6, 5, 2, 1]
        );
    }

    #[test]
    fn cached_numeric_inputs_preserve_field_limits_and_run_identity() {
        let mut runs = vec![
            run(1, 23, "succeeded", 100, Some("confirmed"), Vec::new()),
            run(2, 23, "succeeded", 100, Some("mismatch"), Vec::new()),
        ];
        runs[0].input_json = Value::Object(
            (0..70)
                .map(|field| (format!("field_{field:02}"), json!(field)))
                .collect(),
        );
        runs[1].input_json = json!({
            "nested": {"score": 12},
            "array": [5],
            "flag": true,
            "missing": null,
            "deep": {"a": {"b": {"c": {"score": 9}}}}
        });

        let inputs = numeric_feedback_inputs(&runs, &[1, 0, usize::MAX]);

        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[&0].len(), 64);
        assert_eq!(inputs[&0].first(), Some(&("field_00".to_string(), 0.0)));
        assert_eq!(inputs[&0].last(), Some(&("field_63".to_string(), 63.0)));
        assert_eq!(inputs[&1], [("nested.score".to_string(), 12.0)]);
    }

    #[test]
    fn bounded_numeric_fields_preserve_encoded_path_order_across_branches() {
        let mut input = Map::new();
        input.insert("a".into(), json!({"z.z": -1}));
        let mut expected = vec![(r#"a["z.z"]"#.to_string(), -1.0)];
        for index in 0..80 {
            let key = format!("aZ{index:02}");
            input.insert(key.clone(), json!(index));
            expected.push((key, index as f64));
        }
        // Visited after aZ*, but its quoted representation sorts before them.
        input.insert("z.z".into(), json!(999));
        expected.push((r#"["z.z"]"#.to_string(), 999.0));
        expected.sort_by(|left, right| left.0.cmp(&right.0));
        expected.truncate(MAX_NUMERIC_INPUT_FIELDS);

        let mut collector = NumericInputCollector::new();
        assert!(collector.collect(&Value::Object(input), "", 0));
        assert_eq!(collector.fields.into_iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn numeric_input_node_budget_includes_root_and_ignored_values() {
        let mut input = (0..MAX_NUMERIC_INPUT_NODES - 1)
            .map(|index| (format!("f{index:04}"), Value::Null))
            .collect::<Map<_, _>>();
        input.insert("f0000".into(), json!(42));
        let mut collector = NumericInputCollector::new();
        assert!(collector.collect(&Value::Object(input.clone()), "", 0));
        assert_eq!(collector.remaining_nodes, 0);
        assert_eq!(collector.fields["f0000"], 42.0);

        input.insert("z".into(), Value::Null);
        let mut collector = NumericInputCollector::new();
        assert!(!collector.collect(&Value::Object(input), "", 0));
        assert_eq!(collector.remaining_nodes, 0);
    }

    #[test]
    fn numeric_input_path_budget_counts_encoded_bytes_and_all_paths() {
        let first = "a".repeat(MAX_NUMERIC_INPUT_PATH_BYTES / 2);
        let second = "b".repeat(MAX_NUMERIC_INPUT_PATH_BYTES / 2);
        let mut input = Map::from_iter([(first, json!(1)), (second, json!(2))]);
        let mut collector = NumericInputCollector::new();
        assert!(collector.collect(&Value::Object(input.clone()), "", 0));
        assert_eq!(collector.remaining_path_bytes, 0);
        assert_eq!(collector.fields.len(), 2);
        input.insert("z".into(), Value::Null);
        let mut collector = NumericInputCollector::new();
        assert!(!collector.collect(&Value::Object(input), "", 0));

        for key in [
            "a".repeat(MAX_NUMERIC_INPUT_PATH_BYTES + 1),
            // Raw key fits the budget; its JSON-escaped path does not.
            "\u{0001}".repeat(MAX_NUMERIC_INPUT_PATH_BYTES / 6 + 1),
        ] {
            let mut collector = NumericInputCollector::new();
            assert!(!collector.collect(&json!({(key): 1}), "", 0));
            assert!(collector.fields.is_empty());
        }
    }

    #[test]
    fn numeric_input_budget_respects_ignored_arrays_and_depth_boundary() {
        let input = json!({
            "number": 1,
            "null": null,
            "flag": true,
            "array": vec![1; MAX_NUMERIC_INPUT_NODES + 1],
            "deep": {"a": {"b": {"c": {"too_deep": 2}}}}
        });
        let mut collector = NumericInputCollector::new();
        assert!(collector.collect(&input, "", 0));
        assert_eq!(collector.remaining_nodes, MAX_NUMERIC_INPUT_NODES - 9);
        assert_eq!(
            collector.fields.into_iter().collect::<Vec<_>>(),
            [("number".into(), 1.0)]
        );
    }

    #[test]
    fn oversized_latest_case_does_not_backfill_an_older_numeric_input() {
        let mut runs = vec![
            run(1, 23, "succeeded", 100, Some("confirmed"), Vec::new()),
            run(2, 23, "succeeded", 100, Some("mismatch"), Vec::new()),
            run(3, 23, "succeeded", 100, Some("confirmed"), Vec::new()),
        ];
        runs[0].business_reference = "same-case".into();
        runs[1].business_reference = "same-case".into();
        runs[0].input_json = json!({"age": 1});
        runs[1].input_json = Value::Object(Map::from_iter([
            ("age".into(), json!(2)),
            ("z".repeat(MAX_NUMERIC_INPUT_PATH_BYTES + 1), Value::Null),
        ]));
        runs[2].input_json = json!({"age": 3});
        let indices =
            distinct_business_case_indices(&runs, &latest_conclusive_feedback_indices(&runs));
        assert_eq!(indices, [2, 1]);
        let inputs = numeric_feedback_inputs(&runs, &indices);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[&2], [("age".into(), 3.0)]);
    }

    #[test]
    fn oversized_numeric_inputs_are_excluded_without_changing_descriptive_feedback() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = (1..=16)
            .map(|id| {
                let mut sample = run(
                    id,
                    23,
                    "succeeded",
                    100,
                    Some(if id <= 8 { "mismatch" } else { "confirmed" }),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("approve"),
                    )],
                );
                let mut input = (0..MAX_NUMERIC_INPUT_NODES)
                    .map(|index| (format!("z{index:04}"), Value::Null))
                    .collect::<Map<_, _>>();
                // A partial scan would retain this apparently perfect signal.
                input.insert("age".into(), json!(id));
                sample.input_json = Value::Object(input);
                sample
            })
            .collect::<Vec<_>>();
        let indices = latest_conclusive_feedback_indices(&runs);
        let inputs = numeric_feedback_inputs(&runs, &indices);
        assert!(inputs.is_empty());
        assert_eq!(numeric_feedback_hypothesis_count(&inputs, &indices), 0);

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let finding = report
            .findings
            .iter()
            .find(|finding| {
                finding.code == "RULE_FEEDBACK_MISMATCH" && finding.element == "explicit/adult"
            })
            .unwrap();
        assert!(finding.numeric_feedback_split.is_none());
        let coverage = report
            .rule_coverage
            .iter()
            .find(|coverage| coverage.decision_id == "explicit")
            .unwrap();
        let rule = coverage
            .rules
            .iter()
            .find(|rule| rule.rule_id == "adult")
            .unwrap();
        assert_eq!(rule.feedback_count, 16);
        assert_eq!(rule.mismatch_count, 8);
    }

    #[test]
    fn numeric_field_paths_decode_canonically_without_literal_key_collisions() {
        for keys in [
            vec!["applicant", "age"],
            vec!["applicant.age"],
            vec!["申请人", "annual-income"],
            vec!["", "a\"b", "c\\d", "x]y"],
            vec!["x", "[0]"],
        ] {
            let path = keys.iter().fold(String::new(), |prefix, key| {
                numeric_field_path(&prefix, key)
            });
            assert_eq!(
                numeric_field_segments(&path),
                Some(keys.iter().map(|key| key.to_string()).collect())
            );
        }
        for invalid in [
            "",
            "applicant..age",
            ".age",
            "applicant.",
            "applicant[0]",
            "[\"age\"]",
            "applicant.[\"age\"]",
            "[\"x\"]tail",
            "[\"bad]",
            "a.b.c.d.e",
        ] {
            assert!(numeric_field_segments(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn numeric_paths_preserve_literal_keys_without_overwriting_nested_fields() {
        let input = json!({
            "applicant": {"age": 20, "a.b": 2},
            "applicant.age": 40,
            "": {"age": 3},
            "[\"applicant.age\"]": 4,
            "quote\"slash\\": 5,
            "0": 6,
            "客户": {"年龄": 7}
        });
        let mut collector = NumericInputCollector::new();
        assert!(collector.collect(&input, "", 0));
        let fields = collector.fields;

        assert_eq!(fields.len(), 8);
        assert_eq!(fields["applicant.age"], 20.0);
        assert_eq!(fields[r#"["applicant.age"]"#], 40.0);
        assert_eq!(fields[r#"applicant["a.b"]"#], 2.0);
        assert_eq!(fields[r#"[""].age"#], 3.0);
        assert_eq!(fields[r#"["[\"applicant.age\"]"]"#], 4.0);
        assert_eq!(fields[r#"["quote\"slash\\"]"#], 5.0);
        assert_eq!(fields[r#"["0"]"#], 6.0);
        assert_eq!(fields["客户.年龄"], 7.0);
    }

    #[test]
    fn literal_and_nested_paths_do_not_pool_samples_into_false_numeric_evidence() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = (1..=10)
            .map(|id| {
                let mut sample = run(
                    id,
                    23,
                    "succeeded",
                    100,
                    Some(if id <= 5 { "mismatch" } else { "confirmed" }),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("approve"),
                    )],
                );
                sample.input_json = if id <= 5 {
                    json!({"applicant.age": id})
                } else {
                    json!({"applicant": {"age": id}})
                };
                sample
            })
            .collect::<Vec<_>>();
        let indices = latest_conclusive_feedback_indices(&runs);
        let inputs = numeric_feedback_inputs(&runs, &indices);
        // Each distinct field has only five samples and cannot form a valid split.
        assert_eq!(numeric_feedback_hypothesis_count(&inputs, &indices), 0);
        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let finding = report
            .findings
            .iter()
            .find(|finding| {
                finding.code == "RULE_FEEDBACK_MISMATCH" && finding.element == "explicit/adult"
            })
            .unwrap();
        assert!(finding.numeric_feedback_split.is_none());
    }

    #[test]
    fn numeric_hint_identifies_the_literal_dotted_key_with_json_quoting() {
        let runs = (1..=16)
            .map(|id| {
                let mut sample = run(
                    id,
                    23,
                    "succeeded",
                    100,
                    Some(if id <= 8 { "mismatch" } else { "confirmed" }),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("approve"),
                    )],
                );
                sample.input_json = json!({"applicant.age": id, "applicant": {"age": 100}});
                sample
            })
            .collect::<Vec<_>>();

        let hint = infer_numeric_feedback_split(&runs, "explicit", "adult").unwrap();
        assert_eq!(hint.input_field, r#"["applicant.age"]"#);
        assert_eq!(hint.threshold, 8.5);
        assert_eq!(hint.lower_sample_count + hint.upper_sample_count, 16);
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
        assert_eq!(explicit.rules[0].feedback_count, 2);
        assert_eq!(explicit.rules[0].mismatch_count, 1);
        assert_eq!(explicit.rules[0].mismatch_rate, 0.5);
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
    fn inconclusive_feedback_is_excluded_from_mismatch_rate_denominator() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = vec![
            run(1, 23, "succeeded", 10, Some("confirmed"), Vec::new()),
            run(2, 23, "succeeded", 10, Some("mismatch"), Vec::new()),
            run(3, 23, "succeeded", 10, Some("inconclusive"), Vec::new()),
            run(4, 23, "succeeded", 10, Some("inconclusive"), Vec::new()),
        ];

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);

        assert_eq!(report.summary.outcome_reported_runs, 4);
        assert_eq!(report.summary.conclusive_outcome_runs, 2);
        assert_eq!(report.summary.mismatch_rate, 0.5);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.code != "HIGH_MISMATCH_RATE")
        );
        let latest_day = report.daily_trends.last().unwrap();
        assert_eq!(latest_day.outcome_reported_runs, 4);
        assert_eq!(latest_day.conclusive_outcome_runs, 2);
    }

    #[test]
    fn feedback_on_failed_runs_is_ignored_by_analytics() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = vec![run(1, 23, "failed", 10, Some("mismatch"), Vec::new())];

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);

        assert_eq!(report.summary.outcome_reported_runs, 0);
        assert_eq!(report.summary.conclusive_outcome_runs, 0);
        assert_eq!(report.summary.mismatch_runs, 0);
        assert_eq!(report.daily_trends.last().unwrap().outcome_reported_runs, 0);
    }

    #[test]
    fn reports_rule_hit_drift_after_sustained_well_sampled_change() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let mut runs = Vec::new();
        let mut id = 1;
        for day in 1..=23 {
            for sample in 0..4 {
                let hit = if day < 17 { sample < 2 } else { true };
                runs.push(run(
                    id,
                    day,
                    "succeeded",
                    10,
                    None,
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        if hit { &["adult"] } else { &["minor"] },
                        json!(hit),
                    )],
                ));
                id += 1;
            }
        }

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let drift = report
            .findings
            .iter()
            .find(|finding| finding.code == "RULE_HIT_DRIFT" && finding.element == "explicit/adult")
            .expect("sustained rule hit change should be detected");

        assert!(drift.message.contains("CUSUM"));
        assert!(drift.message.contains("64/28"));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "RULE_HIT_DRIFT" && finding.element == "explicit/minor"
        }));
    }

    #[test]
    fn steady_rule_hit_rates_do_not_trigger_cusum_drift() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let mut runs = Vec::new();
        let mut id = 1;
        for day in 1..=23 {
            for sample in 0..4 {
                let hit = sample < 2;
                runs.push(run(
                    id,
                    day,
                    "succeeded",
                    10,
                    None,
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        if hit { &["adult"] } else { &["minor"] },
                        json!(hit),
                    )],
                ));
                id += 1;
            }
        }

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.code != "RULE_HIT_DRIFT")
        );
    }

    #[test]
    fn reports_numeric_input_psi_for_changed_feedback_sample_distribution() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let mut runs = Vec::new();
        let mut id = 1;
        for day in 1..=23 {
            for sample in 0..4 {
                let mut execution = run(
                    id,
                    day,
                    "succeeded",
                    10,
                    Some("confirmed"),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("qualified"),
                    )],
                );
                let amount = if day < 17 {
                    10.0 + (sample % 2) as f64
                } else {
                    100.0 + sample as f64
                };
                execution.input_json = json!({"Amount": amount});
                runs.push(execution);
                id += 1;
            }
        }

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let drift = report
            .findings
            .iter()
            .find(|finding| finding.code == "INPUT_DISTRIBUTION_DRIFT")
            .expect("changed numeric feedback distribution should be detected");
        assert_eq!(drift.element, "Amount");
        assert!(drift.message.contains("PSI"));
        assert!(drift.message.contains("28/64"));
        assert!(report.drift_signals.iter().any(|signal| {
            signal.code == "INPUT_DISTRIBUTION_DRIFT"
                && signal.element == "Amount"
                && signal.metric_value >= INPUT_DISTRIBUTION_PSI_THRESHOLD
        }));
    }

    #[test]
    fn stable_numeric_feedback_distribution_does_not_trigger_psi_drift() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let mut runs = Vec::new();
        let mut id = 1;
        for day in 1..=23 {
            for sample in 0..4 {
                let mut execution = run(
                    id,
                    day,
                    "succeeded",
                    10,
                    Some("confirmed"),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("qualified"),
                    )],
                );
                execution.input_json = json!({"Amount": 10.0 + (sample % 2) as f64});
                runs.push(execution);
                id += 1;
            }
        }

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        assert!(report.findings.iter().all(|finding| {
            finding.code != "INPUT_DISTRIBUTION_DRIFT"
                && finding.code != "INPUT_FIELD_MISSINGNESS_DRIFT"
        }));
    }

    #[test]
    fn reports_numeric_field_missingness_drift_and_rejects_sparse_tests() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = (1..=23)
            .flat_map(|day| {
                (0..10).map(move |sample| {
                    let mut execution = run(
                        i64::from(day * 10 + sample),
                        day,
                        "succeeded",
                        10,
                        Some("confirmed"),
                        vec![table_step(
                            "explicit",
                            "Explicit Table",
                            "rule_match",
                            &["adult"],
                            json!("qualified"),
                        )],
                    );
                    if day < 17 || sample < 7 {
                        execution.input_json = json!({"Amount": 10.0});
                    }
                    execution
                })
            })
            .collect::<Vec<_>>();

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.code == "INPUT_FIELD_MISSINGNESS_DRIFT")
            .expect("feedback sample field availability changes should be reported");
        assert_eq!(finding.element, "Amount");
        assert!(finding.message.contains("Bonferroni 校正 p="));
        assert!(report.drift_signals.iter().any(|signal| {
            signal.code == "INPUT_FIELD_MISSINGNESS_DRIFT"
                && signal.element == "Amount"
                && signal.metric_value >= INPUT_MISSING_RATE_GAP_THRESHOLD
        }));
        assert!(binary_distribution_p_value(19, 1, 19, 1).is_none());
    }

    #[test]
    fn missingness_rates_exclude_unobserved_and_field_capped_inputs() {
        let window_start = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let window_end = NaiveDate::from_ymd_opt(2026, 8, 30).unwrap();
        let mut samples = Vec::new();
        for (day, present_count) in [(10, 20), (25, 10)] {
            for index in 0..20 {
                samples.push(FeedbackSample {
                    project_id: 1,
                    business_reference: format!("observed-{day}-{index}"),
                    mismatch: false,
                    numeric_fields: (index < present_count)
                        .then(|| ("amount".to_string(), 10.0))
                        .into_iter()
                        .collect(),
                    missingness_observed: true,
                    matched_rules: HashSet::new(),
                    executed_at: Utc.with_ymd_and_hms(2026, 8, day, 12, 0, 0).unwrap(),
                });
            }
        }
        for index in 0..100 {
            samples.push(FeedbackSample {
                project_id: 1,
                business_reference: format!("unobserved-{index}"),
                mismatch: false,
                numeric_fields: Vec::new(),
                missingness_observed: false,
                matched_rules: HashSet::new(),
                executed_at: Utc.with_ymd_and_hms(2026, 8, 25, 12, 0, 0).unwrap(),
            });
        }

        let mut findings = Vec::new();
        add_input_distribution_drift_findings(&mut findings, &samples, window_start, window_end);
        let missingness = findings
            .iter()
            .find(|finding| finding.code == "INPUT_FIELD_MISSINGNESS_DRIFT")
            .expect("unobserved inputs must not dilute the observed missingness change");
        assert!(missingness.message.contains("已完整扫描样本 n=20"));

        for sample in &mut samples {
            if !sample.numeric_fields.is_empty() {
                sample.numeric_fields = (0..MAX_NUMERIC_INPUT_FIELDS)
                    .map(|index| (format!("field-{index:02}"), 1.0))
                    .collect();
                sample.missingness_observed = false;
            }
        }
        let mut findings = Vec::new();
        add_input_distribution_drift_findings(&mut findings, &samples, window_start, window_end);
        assert!(
            findings
                .iter()
                .all(|finding| finding.code != "INPUT_FIELD_MISSINGNESS_DRIFT")
        );
    }

    #[test]
    fn psi_drift_uses_report_wide_bonferroni_correction() {
        let baseline = (0..100)
            .map(|index| (index % 10) as f64)
            .collect::<Vec<_>>();
        let recent = vec![100.0; 100];
        let (psi, p_value) = population_stability_index(&baseline, &recent).unwrap();

        assert!(psi >= INPUT_DISTRIBUTION_PSI_THRESHOLD);
        assert!(p_value <= INPUT_DISTRIBUTION_FAMILY_ALPHA / 64.0);
        assert!(bonferroni_adjusted_p(p_value, 64) <= INPUT_DISTRIBUTION_FAMILY_ALPHA);
        assert!(bonferroni_adjusted_p(0.01, 6) > INPUT_DISTRIBUTION_FAMILY_ALPHA);
    }

    #[test]
    fn chi_square_tail_probability_matches_known_reference_values() {
        assert!((chi_square_survival(2.0, 2) - (-1.0_f64).exp()).abs() < 1e-12);
        assert!((chi_square_survival(3.841_458_820_694_124, 1) - 0.05).abs() < 1e-8);
        assert_eq!(bonferroni_adjusted_p(0.02, 0), 0.02);
    }

    #[test]
    fn numeric_input_psi_caps_multiple_field_checks_and_reports_omissions() {
        let window_start = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let window_end = NaiveDate::from_ymd_opt(2026, 8, 30).unwrap();
        let mut samples = Vec::new();
        for index in 0..40 {
            for (day, value) in [(10, 10.0 + (index % 2) as f64), (25, 100.0 + index as f64)] {
                let executed_at = Utc.with_ymd_and_hms(2026, 8, day, 12, 0, 0).unwrap();
                samples.push(FeedbackSample {
                    project_id: 1,
                    business_reference: format!("CASE-{index}"),
                    mismatch: false,
                    numeric_fields: (0..INPUT_DISTRIBUTION_MAX_FIELDS + 6)
                        .map(|field_index| (format!("field_{field_index:03}"), value))
                        .collect(),
                    missingness_observed: false,
                    matched_rules: HashSet::new(),
                    executed_at,
                });
            }
        }

        let mut findings = Vec::new();
        add_input_distribution_drift_findings(&mut findings, &samples, window_start, window_end);

        assert_eq!(
            findings
                .iter()
                .filter(|finding| finding.code == "INPUT_DISTRIBUTION_DRIFT")
                .count(),
            INPUT_DISTRIBUTION_MAX_FIELDS
        );
        let capped = findings
            .iter()
            .find(|finding| finding.code == "INPUT_DISTRIBUTION_FIELDS_CAPPED")
            .expect("truncated PSI field checks should be reported");
        assert!(capped.message.contains("6 个字段未评估"));
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
        assert!(!distribution.details_omitted);
    }

    #[test]
    fn result_cardinality_budget_discards_partial_ranking_and_stays_omitted() {
        let mut accumulator = ResultDistributionAccumulator::default();
        let mut report_bytes = 0;
        for value in 0..MAX_RESULT_DISTINCT_VALUES {
            accumulator.record(&json!(value), &mut report_bytes);
        }
        assert_eq!(accumulator.counts.len(), MAX_RESULT_DISTINCT_VALUES);
        assert!(!accumulator.details_omitted);
        let retained_bytes = report_bytes;
        accumulator.record(&json!(0), &mut report_bytes);
        assert_eq!(report_bytes, retained_bytes);
        assert_eq!(accumulator.counts["0"], 2);

        accumulator.record(&json!(MAX_RESULT_DISTINCT_VALUES), &mut report_bytes);
        assert!(accumulator.details_omitted);
        assert!(accumulator.counts.is_empty());
        assert_eq!(accumulator.counts.capacity(), 0);
        assert_eq!(report_bytes, 0);
        accumulator.record(&Value::Null, &mut report_bytes);
        assert!(accumulator.counts.is_empty());
        assert_eq!(accumulator.total, MAX_RESULT_DISTINCT_VALUES + 3);
    }

    #[test]
    fn result_decision_byte_budget_accepts_exact_limit_and_refunds_on_overflow() {
        let mut accumulator = ResultDistributionAccumulator::default();
        let mut report_bytes = 0;
        let first = json!("a".repeat(MAX_RESULT_DECISION_BYTES / 2 - 2));
        let second = json!("b".repeat(MAX_RESULT_DECISION_BYTES / 2 - 2));
        accumulator.record(&first, &mut report_bytes);
        accumulator.record(&second, &mut report_bytes);
        assert_eq!(accumulator.retained_bytes, MAX_RESULT_DECISION_BYTES);
        assert_eq!(report_bytes, MAX_RESULT_DECISION_BYTES);
        accumulator.record(&first, &mut report_bytes);
        assert_eq!(report_bytes, MAX_RESULT_DECISION_BYTES);
        assert!(!accumulator.details_omitted);

        accumulator.record(&json!(0), &mut report_bytes);
        assert!(accumulator.details_omitted);
        assert_eq!(accumulator.total, 4);
        assert_eq!(accumulator.retained_bytes, 0);
        assert_eq!(report_bytes, 0);
    }

    #[test]
    fn result_report_byte_budget_is_shared_and_released_for_other_decisions() {
        let value = json!("a".repeat(MAX_RESULT_DECISION_BYTES / 2 - 2));
        let mut report_bytes = 0;
        let mut accumulators = (0..MAX_RESULT_REPORT_BYTES / (MAX_RESULT_DECISION_BYTES / 2))
            .map(|_| {
                let mut accumulator = ResultDistributionAccumulator::default();
                accumulator.record(&value, &mut report_bytes);
                accumulator
            })
            .collect::<Vec<_>>();
        assert_eq!(report_bytes, MAX_RESULT_REPORT_BYTES);
        accumulators[0].record(&value, &mut report_bytes);
        assert_eq!(report_bytes, MAX_RESULT_REPORT_BYTES);

        // This decision still has its own byte capacity; the shared budget rejects it.
        accumulators[0].record(&json!(0), &mut report_bytes);
        assert!(accumulators[0].details_omitted);
        assert_eq!(
            report_bytes,
            MAX_RESULT_REPORT_BYTES - MAX_RESULT_DECISION_BYTES / 2
        );
        let mut later = ResultDistributionAccumulator::default();
        later.record(&value, &mut report_bytes);
        assert!(!later.details_omitted);
        assert_eq!(report_bytes, MAX_RESULT_REPORT_BYTES);
    }

    #[test]
    fn result_json_serialization_bounds_nodes_depth_and_escaped_bytes() {
        assert!(
            normalized_json_text(&json!(vec![Value::Null; MAX_RESULT_VALUE_NODES - 1])).is_some()
        );
        assert!(normalized_json_text(&json!(vec![Value::Null; MAX_RESULT_VALUE_NODES])).is_none());
        let mut nested = json!(0);
        for _ in 0..MAX_RESULT_VALUE_DEPTH {
            nested = json!([nested]);
        }
        assert!(normalized_json_text(&nested).is_some());
        assert!(normalized_json_text(&json!([nested])).is_none());
        assert_eq!(
            normalized_json_text(&json!("a".repeat(MAX_RESULT_DECISION_BYTES - 2)))
                .unwrap()
                .len(),
            MAX_RESULT_DECISION_BYTES
        );
        for value in [
            json!("a".repeat(MAX_RESULT_DECISION_BYTES + 1)),
            json!("\u{0001}".repeat(MAX_RESULT_DECISION_BYTES / 6 + 1)),
            json!({("a".repeat(MAX_RESULT_DECISION_BYTES)): 0}),
        ] {
            assert!(normalized_json_text(&value).is_none());
            let mut accumulator = ResultDistributionAccumulator::default();
            let mut report_bytes = 0;
            accumulator.record(&Value::Null, &mut report_bytes);
            accumulator.record(&value, &mut report_bytes);
            assert!(accumulator.details_omitted);
            assert!(accumulator.counts.is_empty());
            assert_eq!(accumulator.total, 2);
            assert_eq!(report_bytes, 0);
        }
    }

    #[test]
    fn result_report_preserves_full_total_when_details_are_omitted() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = (1..=MAX_RESULT_DISTINCT_VALUES + 1)
            .map(|id| {
                run(
                    id as i64,
                    23,
                    "succeeded",
                    10,
                    None,
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!(id),
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
        assert!(distribution.details_omitted);
        assert_eq!(distribution.total, runs.len());
        assert_eq!(
            distribution.values,
            [DecisionResultValueMetric {
                value: "其他".into(),
                count: runs.len(),
                rate: 1.0,
            }]
        );
        assert_eq!(report.summary.succeeded_runs, runs.len());
    }

    #[test]
    fn incremental_builder_matches_slice_report_for_mixed_run_data() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let mut runs = (1..=20)
            .map(|id| {
                run(
                    id,
                    23,
                    if id % 7 == 0 { "failed" } else { "succeeded" },
                    if id % 5 == 0 { -10 } else { id * 100 },
                    match id % 4 {
                        0 => Some("mismatch"),
                        1 => Some("confirmed"),
                        2 => Some("inconclusive"),
                        _ => None,
                    },
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!({"outcome": id % 3}),
                    )],
                )
            })
            .collect::<Vec<_>>();
        runs[3].execution_trace_json = Value::Null;
        runs[8].business_reference = "".into();
        runs[12].input_json = json!({"age": 12});

        let expected = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let model = model();
        let mut builder = DecisionAnalyticsBuilder::new(5, 7, 3, &model, start, generated_at);
        builder.set_exact_p95_duration_ms(Some(expected.summary.p95_duration_ms));
        for run in &runs {
            builder.record(DecisionAnalyticsRun::from(run));
        }
        assert!(builder.durations.is_empty());
        assert_eq!(builder.finish(), expected);
    }

    #[test]
    fn omitted_trace_keeps_run_totals_and_is_backward_compatible() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let model = model();
        let source = run(
            1,
            23,
            "succeeded",
            120,
            Some("confirmed"),
            vec![table_step(
                "explicit",
                "Explicit Table",
                "rule_match",
                &["adult"],
                json!("approved"),
            )],
        );
        let mut analytics_run = DecisionAnalyticsRun::from(&source);
        analytics_run.trace_details_omitted = true;
        analytics_run.numeric_input_omitted = true;
        analytics_run.execution_trace_json = Value::Null;

        let mut builder = DecisionAnalyticsBuilder::new(5, 7, 3, &model, start, generated_at);
        builder.record(analytics_run);
        assert_eq!(builder.feedback_window.len(), 1);
        assert_eq!(builder.feedback_window[0].business_reference, "case-1");
        let report = builder.finish();

        assert_eq!(report.summary.total_runs, 1);
        assert_eq!(report.summary.succeeded_runs, 1);
        assert_eq!(report.summary.confirmed_runs, 1);
        assert_eq!(report.trace_details_omitted_runs, 1);
        assert_eq!(report.numeric_input_omitted_runs, 1);
        assert_eq!(report.rule_coverage[0].execution_count, 0);
        assert!(report.result_distributions.is_empty());

        let mut legacy_snapshot = serde_json::to_value(report).unwrap();
        legacy_snapshot
            .as_object_mut()
            .unwrap()
            .remove("traceDetailsOmittedRuns");
        legacy_snapshot
            .as_object_mut()
            .unwrap()
            .remove("numericInputOmittedRuns");
        legacy_snapshot
            .as_object_mut()
            .unwrap()
            .remove("unknownResultStepsOmitted");
        legacy_snapshot
            .as_object_mut()
            .unwrap()
            .remove("driftSignals");
        let restored: DecisionAnalyticsReport = serde_json::from_value(legacy_snapshot).unwrap();
        assert_eq!(restored.trace_details_omitted_runs, 0);
        assert_eq!(restored.numeric_input_omitted_runs, 0);
        assert_eq!(restored.unknown_result_steps_omitted, 0);
        assert!(restored.drift_signals.is_empty());
    }

    #[test]
    fn oversized_numeric_input_still_consumes_feedback_window() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let model = model();
        let source = run(
            2,
            23,
            "succeeded",
            160,
            Some("mismatch"),
            vec![table_step(
                "explicit",
                "Explicit Table",
                "rule_match",
                &["adult"],
                json!("approved"),
            )],
        );
        let mut analytics_run = DecisionAnalyticsRun::from(&source);
        analytics_run.numeric_input_omitted = true;
        analytics_run.input_json = Value::Null;

        let mut builder = DecisionAnalyticsBuilder::new(5, 7, 3, &model, start, generated_at);
        builder.record(analytics_run);
        assert_eq!(builder.feedback_window.len(), 1);
        assert_eq!(builder.feedback_window[0].business_reference, "case-2");
        let report = builder.finish();

        assert_eq!(report.summary.total_runs, 1);
        assert_eq!(report.summary.mismatch_runs, 1);
        assert_eq!(report.numeric_input_omitted_runs, 1);
        assert_eq!(report.rule_coverage[0].execution_count, 1);
    }

    #[test]
    fn unknown_result_decisions_have_bounded_ids_and_labels() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let mut steps = (0..MAX_UNKNOWN_RESULT_DECISIONS)
            .map(|index| {
                table_step(
                    &format!("external-{index}"),
                    &format!("External decision {index}"),
                    "rule_match",
                    &[],
                    json!(index),
                )
            })
            .collect::<Vec<_>>();
        steps.push(table_step(
            "oversized-label",
            &"n".repeat(MAX_UNKNOWN_RESULT_LABEL_BYTES + 1),
            "rule_match",
            &[],
            json!("omitted-label"),
        ));
        steps.push(table_step(
            "external-over-limit",
            "External decision over limit",
            "rule_match",
            &[],
            json!("omitted-id"),
        ));
        let runs = vec![run(1, 23, "succeeded", 100, None, steps)];

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);

        assert_eq!(
            report.result_distributions.len(),
            MAX_UNKNOWN_RESULT_DECISIONS
        );
        assert_eq!(report.unknown_result_steps_omitted, 2);
        assert!(report.result_distributions.iter().all(|metric| {
            metric.decision_id.len() <= MAX_UNKNOWN_RESULT_LABEL_BYTES
                && metric.decision_name.len() <= MAX_UNKNOWN_RESULT_LABEL_BYTES
        }));
    }

    #[test]
    fn malformed_feedback_traces_still_consume_the_latest_sample_window() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let mut runs = (1..=MAX_FEEDBACK_SAMPLES + 8)
            .map(|id| {
                run(
                    id as i64,
                    23,
                    "succeeded",
                    100,
                    Some(if id <= 8 { "mismatch" } else { "confirmed" }),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("approve"),
                    )],
                )
            })
            .collect::<Vec<_>>();
        for (index, sample) in runs.iter_mut().enumerate().skip(8) {
            sample.input_json = json!({"age": index});
            sample.business_reference = format!("case-{index}");
            sample.execution_trace_json = Value::Null;
        }

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let finding = report
            .findings
            .iter()
            .find(|finding| {
                finding.code == "RULE_FEEDBACK_MISMATCH" && finding.element == "explicit/adult"
            })
            .unwrap();
        assert!(finding.numeric_feedback_split.is_none());
        assert_eq!(report.summary.total_runs, MAX_FEEDBACK_SAMPLES + 8);
        assert_eq!(report.summary.mismatch_runs, 8);
        let coverage = report
            .rule_coverage
            .iter()
            .find(|coverage| coverage.decision_id == "explicit")
            .unwrap();
        assert_eq!(coverage.rules[0].feedback_count, 8);
    }

    #[test]
    fn emits_deterministic_threshold_and_recent_trend_findings() {
        let summary = DecisionAnalyticsSummary {
            total_runs: 20,
            succeeded_runs: 16,
            failed_runs: 4,
            outcome_reported_runs: 5,
            conclusive_outcome_runs: 5,
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
                conclusive_outcome_runs: if index < 7 { 1 } else { 0 },
                mismatch_runs: 0,
                average_duration_ms: if index < 7 { 100.0 } else { 300.0 },
            })
            .collect::<Vec<_>>();
        let findings = build_findings(
            &summary,
            &coverage,
            &trends,
            &DecisionAnalyticsThresholds::default(),
        );
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

    #[test]
    fn configured_project_thresholds_control_rate_findings_and_severity() {
        let summary = DecisionAnalyticsSummary {
            total_runs: 20,
            succeeded_runs: 17,
            failed_runs: 3,
            conclusive_outcome_runs: 20,
            mismatch_runs: 5,
            failure_rate: 0.15,
            mismatch_rate: 0.25,
            ..Default::default()
        };
        let thresholds = DecisionAnalyticsThresholds {
            failure_warning_rate: 0.14,
            failure_high_rate: 0.16,
            mismatch_warning_rate: 0.24,
            mismatch_high_rate: 0.26,
            ..Default::default()
        };
        let findings = build_findings(&summary, &[], &[], &thresholds);
        let failure = findings
            .iter()
            .find(|finding| finding.code == "HIGH_FAILURE_RATE")
            .unwrap();
        let mismatch = findings
            .iter()
            .find(|finding| finding.code == "HIGH_MISMATCH_RATE")
            .unwrap();

        assert_eq!(failure.level, "medium");
        assert_eq!(mismatch.level, "medium");

        let elevated = DecisionAnalyticsThresholds {
            failure_high_rate: 0.15,
            mismatch_high_rate: 0.25,
            ..thresholds
        };
        let findings = build_findings(&summary, &[], &[], &elevated);
        assert_eq!(
            findings
                .iter()
                .find(|finding| finding.code == "HIGH_FAILURE_RATE")
                .unwrap()
                .level,
            "high"
        );
        assert_eq!(
            findings
                .iter()
                .find(|finding| finding.code == "HIGH_MISMATCH_RATE")
                .unwrap()
                .level,
            "high"
        );
    }

    #[test]
    fn configured_minimum_samples_suppress_rate_findings() {
        let summary = DecisionAnalyticsSummary {
            total_runs: 11,
            failed_runs: 5,
            failure_rate: 5.0 / 11.0,
            conclusive_outcome_runs: 6,
            mismatch_runs: 4,
            mismatch_rate: 4.0 / 6.0,
            ..Default::default()
        };
        let thresholds = DecisionAnalyticsThresholds {
            failure_min_runs: 12,
            mismatch_min_samples: 7,
            ..Default::default()
        };

        let findings = build_findings(&summary, &[], &[], &thresholds);
        assert!(
            findings
                .iter()
                .all(|finding| finding.code != "HIGH_FAILURE_RATE")
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding.code != "HIGH_MISMATCH_RATE")
        );
    }

    #[test]
    fn detects_rule_level_business_feedback_anomalies_after_minimum_sample() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = (1..=5)
            .map(|id| {
                run(
                    id,
                    23,
                    "succeeded",
                    100,
                    Some(if id <= 2 { "confirmed" } else { "mismatch" }),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("approve"),
                    )],
                )
            })
            .collect::<Vec<_>>();

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let rule = &report.rule_coverage[0].rules[0];
        assert_eq!(rule.feedback_count, 5);
        assert_eq!(rule.mismatch_count, 3);
        assert_eq!(rule.mismatch_rate, 0.6);
        assert!(report.findings.iter().any(|finding| {
            finding.code == "RULE_FEEDBACK_MISMATCH" && finding.element == "explicit/adult"
        }));

        let under_threshold = runs.iter().take(4).cloned().collect::<Vec<_>>();
        let report =
            build_decision_analytics(5, 7, 3, &model(), &under_threshold, start, generated_at);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.code != "RULE_FEEDBACK_MISMATCH")
        );
    }

    #[test]
    fn infers_numeric_feedback_split_only_when_both_sides_have_enough_samples() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = (1..=10)
            .map(|id| {
                let age = id + 19;
                let assessment = if age <= 24 { "mismatch" } else { "confirmed" };
                let mut run = run(
                    id,
                    23,
                    "succeeded",
                    100,
                    Some(assessment),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("approve"),
                    )],
                );
                run.input_json = json!({"applicant": {"age": age}});
                run
            })
            .collect::<Vec<_>>();

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.code == "RULE_FEEDBACK_MISMATCH")
            .unwrap();
        let hint = finding.numeric_feedback_split.as_ref().unwrap();
        assert!(hint.business_case_deduplicated);
        assert_eq!(hint.input_field, "applicant.age");
        assert_eq!(hint.threshold, 24.5);
        assert_eq!(hint.lower_mismatch_count, 5);
        assert_eq!(hint.lower_sample_count, 5);
        assert_eq!(hint.upper_mismatch_count, 0);
        assert_eq!(hint.upper_sample_count, 5);
        assert!(
            finding
                .numeric_feedback_split_summary()
                .unwrap()
                .contains("不能证明因果")
        );
        assert!(
            finding
                .numeric_feedback_split_summary()
                .unwrap()
                .contains("业务关联号去重")
        );

        let weak_signal_runs = runs
            .iter()
            .map(|run| {
                let mut run = run.clone();
                run.input_json = json!({"applicant": {"age": 20}});
                run
            })
            .collect::<Vec<_>>();
        let report =
            build_decision_analytics(5, 7, 3, &model(), &weak_signal_runs, start, generated_at);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.code == "RULE_FEEDBACK_MISMATCH")
            .unwrap();
        assert!(finding.numeric_feedback_split.is_none());
    }

    #[test]
    fn repeated_business_case_runs_do_not_create_numeric_feedback_evidence() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = (1..=10)
            .map(|id| {
                let age = id + 19;
                let assessment = if age <= 24 { "mismatch" } else { "confirmed" };
                let mut run = run(
                    id,
                    23,
                    "succeeded",
                    100,
                    Some(assessment),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("approve"),
                    )],
                );
                run.executed_by = format!("operator-{id}");
                run.business_reference = "same-business-case".into();
                run.input_json = json!({"applicant": {"age": age}});
                run
            })
            .collect::<Vec<_>>();

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let finding = report
            .findings
            .iter()
            .find(|finding| {
                finding.code == "RULE_FEEDBACK_MISMATCH" && finding.element == "explicit/adult"
            })
            .unwrap();

        assert!(finding.numeric_feedback_split.is_none());
    }

    #[test]
    fn business_case_sampling_and_test_helper_use_the_same_corrected_family() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let mut runs = Vec::new();
        for id in 1..=10 {
            for retry in 0..2 {
                let mut sample = run(
                    id * 2 + retry,
                    23,
                    "succeeded",
                    100,
                    Some(if id <= 5 { "mismatch" } else { "confirmed" }),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("approve"),
                    )],
                );
                sample.business_reference = format!("case-{id}");
                sample.input_json = json!({"age": id + 19});
                runs.push(sample);
            }
        }

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let finding = report
            .findings
            .iter()
            .find(|finding| {
                finding.code == "RULE_FEEDBACK_MISMATCH" && finding.element == "explicit/adult"
            })
            .unwrap();
        let hint = finding.numeric_feedback_split.as_ref().unwrap();
        assert_eq!(hint.lower_sample_count + hint.upper_sample_count, 10);
        assert_eq!(
            infer_numeric_feedback_split(&runs, "explicit", "adult"),
            finding.numeric_feedback_split
        );
        assert_eq!(report.rule_coverage[0].rules[0].feedback_count, 20);

        // Legacy feedback without case identity remains descriptive evidence only.
        for sample in &mut runs {
            sample.business_reference = "  ".into();
        }
        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.numeric_feedback_split.is_none())
        );
        assert_eq!(report.rule_coverage[0].rules[0].feedback_count, 20);
    }

    #[test]
    fn report_wide_split_correction_includes_rules_without_anomaly_findings() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let runs = (1..=20)
            .map(|id| {
                let is_adult_rule = id <= 10;
                let sample = if is_adult_rule { id } else { id - 10 };
                let assessment = if is_adult_rule {
                    if sample <= 5 { "mismatch" } else { "confirmed" }
                } else if sample <= 2 {
                    "mismatch"
                } else {
                    "confirmed"
                };
                let rule_id = if is_adult_rule { "adult" } else { "minor" };
                let mut run = run(
                    id,
                    23,
                    "succeeded",
                    100,
                    Some(assessment),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &[rule_id],
                        json!("approve"),
                    )],
                );
                run.input_json = json!({"age": sample + if is_adult_rule { 19 } else { 39 }});
                run
            })
            .collect::<Vec<_>>();

        let report = build_decision_analytics(5, 7, 3, &model(), &runs, start, generated_at);
        let adult_finding = report
            .findings
            .iter()
            .find(|finding| {
                finding.code == "RULE_FEEDBACK_MISMATCH" && finding.element == "explicit/adult"
            })
            .unwrap();
        assert!(adult_finding.numeric_feedback_split.is_none());
        assert!(report.findings.iter().all(|finding| {
            finding.code != "RULE_FEEDBACK_MISMATCH" || finding.element != "explicit/minor"
        }));
    }

    #[test]
    fn report_split_hypothesis_count_uses_distinct_input_cuts_not_outcome_balance() {
        let values = [1, 1, 1, 2, 2, 3, 3, 4, 4, 4];
        let runs = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let mut run = run(
                    index as i64 + 1,
                    23,
                    "succeeded",
                    100,
                    Some("confirmed"),
                    Vec::new(),
                );
                run.input_json = json!({"score": value});
                run
            })
            .collect::<Vec<_>>();

        let indices = (0..runs.len()).rev().collect::<Vec<_>>();

        let numeric_inputs = numeric_feedback_inputs(&runs, &indices);
        assert_eq!(
            numeric_feedback_hypothesis_count(&numeric_inputs, &indices),
            3
        );
    }

    #[test]
    fn fisher_exact_split_uses_two_sided_probability_ordering() {
        let mut log_factorials = vec![0.0];
        for value in 1..=10 {
            let previous = log_factorials[value - 1];
            log_factorials.push(previous + (value as f64).ln());
        }

        assert!(fisher_exact_is_significant(
            (5, 0, 0, 5),
            &log_factorials,
            0.008
        ));
        assert!(!fisher_exact_is_significant(
            (5, 0, 0, 5),
            &log_factorials,
            0.007
        ));
    }

    #[test]
    fn report_fisher_context_reuses_equivalent_contingency_tests() {
        let mut context = FisherTestContext::new(0.008);

        assert!(context.is_significant((5, 0, 0, 5)));
        assert!(context.is_significant((5, 0, 0, 5)));

        assert_eq!(context.significance_cache.len(), 1);
    }

    #[test]
    fn numeric_feedback_split_corrects_for_scanned_fields_and_thresholds() {
        let mix = |mut value: u64| {
            value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
            value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            value ^ (value >> 31)
        };
        let runs = (0..80)
            .map(|sample| {
                let mut input = serde_json::Map::new();
                for field in 0..16 {
                    let seed = (sample * 17 + field * 131) as u64;
                    input.insert(
                        format!("field_{field}"),
                        json!((mix(seed) % 1_000_003) as f64),
                    );
                }
                let mut run = run(
                    sample as i64 + 1,
                    sample % 28 + 1,
                    "succeeded",
                    100,
                    Some(if sample % 2 == 0 {
                        "mismatch"
                    } else {
                        "confirmed"
                    }),
                    vec![table_step(
                        "explicit",
                        "Explicit Table",
                        "rule_match",
                        &["adult"],
                        json!("approve"),
                    )],
                );
                run.input_json = Value::Object(input);
                run
            })
            .collect::<Vec<_>>();

        assert!(infer_numeric_feedback_split(&runs, "explicit", "adult").is_none());
    }

    #[test]
    fn rule_feedback_anomaly_threshold_is_inclusive_at_thirty_percent() {
        let generated_at = Utc.with_ymd_and_hms(2026, 8, 23, 18, 0, 0).unwrap();
        let start = analytics_window_start(generated_at);
        let build_runs = |mismatch_count| {
            (1..=10)
                .map(|id| {
                    run(
                        id,
                        23,
                        "succeeded",
                        100,
                        Some(if id <= mismatch_count {
                            "mismatch"
                        } else {
                            "confirmed"
                        }),
                        vec![table_step(
                            "explicit",
                            "Explicit Table",
                            "rule_match",
                            &["adult"],
                            json!("approve"),
                        )],
                    )
                })
                .collect::<Vec<_>>()
        };

        for (mismatch_count, expected) in [(3, true), (2, false)] {
            let report = build_decision_analytics(
                5,
                7,
                3,
                &model(),
                &build_runs(mismatch_count),
                start,
                generated_at,
            );
            assert_eq!(
                report
                    .findings
                    .iter()
                    .any(|finding| finding.code == "RULE_FEEDBACK_MISMATCH"),
                expected
            );
        }
    }
}
