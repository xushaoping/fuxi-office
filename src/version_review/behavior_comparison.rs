use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use crate::models::{BehaviorDiff, DecisionDiff, DecisionTraceStep, ExecutionTrace, ValueDiff};

pub struct ExecutionTraceComparator<'a> {
    pub baseline_trace: &'a ExecutionTrace,
    pub candidate_trace: &'a ExecutionTrace,
}

impl ExecutionTraceComparator<'_> {
    pub fn compare(&self) -> BehaviorDiff {
        let baseline_by_id = self
            .baseline_trace
            .decisions
            .iter()
            .map(|step| (step.decision_id.as_str(), step))
            .collect::<HashMap<_, _>>();
        let candidate_by_id = self
            .candidate_trace
            .decisions
            .iter()
            .map(|step| (step.decision_id.as_str(), step))
            .collect::<HashMap<_, _>>();
        let mut decision_ids = BTreeSet::new();
        decision_ids.extend(baseline_by_id.keys().copied());
        decision_ids.extend(candidate_by_id.keys().copied());

        let decision_diffs = decision_ids
            .into_iter()
            .filter_map(|decision_id| {
                let baseline = baseline_by_id.get(decision_id).copied();
                let candidate = candidate_by_id.get(decision_id).copied();
                decision_diff(decision_id, baseline, candidate)
            })
            .collect::<Vec<_>>();
        let execution_order_changed =
            execution_order(self.baseline_trace) != execution_order(self.candidate_trace);
        let output_diffs = compare_values(
            "",
            &self.baseline_trace.output,
            &self.candidate_trace.output,
        );
        let status_changed = self.baseline_trace.status != self.candidate_trace.status;
        let same_input =
            self.baseline_trace.normalized_input == self.candidate_trace.normalized_input;

        let risk_level = if !same_input || execution_order_changed || status_changed {
            "high".to_string()
        } else {
            decision_diffs
                .iter()
                .map(|diff| diff.risk_level.as_str())
                .chain(output_diffs.iter().map(|diff| diff.risk_level.as_str()))
                .max_by_key(|level| risk_rank(level))
                .unwrap_or("low")
                .to_string()
        };

        BehaviorDiff {
            baseline_version_id: self.baseline_trace.version_id,
            candidate_version_id: self.candidate_trace.version_id,
            same_input,
            status_changed,
            execution_order_changed,
            decision_diffs,
            output_diffs,
            risk_level,
        }
    }
}

fn decision_diff(
    decision_id: &str,
    baseline: Option<&DecisionTraceStep>,
    candidate: Option<&DecisionTraceStep>,
) -> Option<DecisionDiff> {
    let change_type = match (baseline, candidate) {
        (None, Some(_)) => "added",
        (Some(_), None) => "removed",
        (Some(before), Some(after))
            if before.result != after.result || before.status != after.status =>
        {
            "result_changed"
        }
        _ => "unchanged",
    };
    let matched_rules_changed = match (baseline, candidate) {
        (Some(before), Some(after)) => before.matched_rule_ids != after.matched_rule_ids,
        (None, Some(after)) => !after.matched_rule_ids.is_empty(),
        (Some(before), None) => !before.matched_rule_ids.is_empty(),
        (None, None) => false,
    };
    if change_type == "unchanged" && !matched_rules_changed {
        return None;
    }
    let step = candidate
        .or(baseline)
        .expect("at least one trace step exists");
    let risk_level = match (baseline, candidate) {
        (Some(before), Some(after)) if before.status != after.status => "high",
        (Some(_), Some(_)) if matched_rules_changed => "high",
        (Some(before), Some(after)) if before.result != after.result => {
            if numeric_deviation(&before.result, &after.result)
                .is_some_and(|percent| percent <= 5.0)
            {
                "low"
            } else {
                "high"
            }
        }
        (None, Some(_)) | (Some(_), None) => "medium",
        _ => "low",
    };
    Some(DecisionDiff {
        decision_id: decision_id.into(),
        decision_name: step.decision_name.clone(),
        change_type: change_type.into(),
        baseline_order: baseline.map(|step| step.order),
        candidate_order: candidate.map(|step| step.order),
        matched_rules_changed,
        baseline_matched_rule_ids: baseline
            .map(|step| step.matched_rule_ids.clone())
            .unwrap_or_default(),
        candidate_matched_rule_ids: candidate
            .map(|step| step.matched_rule_ids.clone())
            .unwrap_or_default(),
        baseline_result: baseline.map(|step| step.result.clone()),
        candidate_result: candidate.map(|step| step.result.clone()),
        risk_level: risk_level.into(),
    })
}

fn risk_rank(level: &str) -> u8 {
    match level {
        "high" => 3,
        "medium" => 2,
        _ => 1,
    }
}

fn execution_order(trace: &ExecutionTrace) -> Vec<&str> {
    let mut decisions = trace.decisions.iter().collect::<Vec<_>>();
    decisions.sort_by_key(|step| step.order);
    decisions
        .into_iter()
        .map(|step| step.decision_id.as_str())
        .collect()
}

fn compare_values(path: &str, baseline: &Value, candidate: &Value) -> Vec<ValueDiff> {
    match (baseline, candidate) {
        (Value::Object(before), Value::Object(after)) => {
            let mut keys = BTreeSet::new();
            keys.extend(before.keys());
            keys.extend(after.keys());
            keys.into_iter()
                .flat_map(|key| {
                    let child_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    match (before.get(key), after.get(key)) {
                        (Some(before), Some(after)) => compare_values(&child_path, before, after),
                        (Some(before), None) => vec![value_diff(
                            child_path,
                            "removed",
                            Some(before.clone()),
                            None,
                            None,
                            "high",
                        )],
                        (None, Some(after)) => vec![value_diff(
                            child_path,
                            "added",
                            None,
                            Some(after.clone()),
                            None,
                            "medium",
                        )],
                        (None, None) => Vec::new(),
                    }
                })
                .collect()
        }
        _ if baseline == candidate => Vec::new(),
        _ => {
            let deviation = numeric_deviation(baseline, candidate);
            let risk = if deviation.is_some_and(|percent| percent <= 5.0) {
                "low"
            } else {
                "high"
            };
            vec![value_diff(
                path.to_string(),
                "changed",
                Some(baseline.clone()),
                Some(candidate.clone()),
                deviation,
                risk,
            )]
        }
    }
}

fn numeric_deviation(baseline: &Value, candidate: &Value) -> Option<f64> {
    let before = baseline.as_f64()?;
    let after = candidate.as_f64()?;
    if before == 0.0 {
        return (after == 0.0).then_some(0.0);
    }
    Some(((after - before).abs() / before.abs()) * 100.0)
}

fn value_diff(
    path: String,
    change_type: &str,
    baseline_value: Option<Value>,
    candidate_value: Option<Value>,
    deviation_percent: Option<f64>,
    risk_level: &str,
) -> ValueDiff {
    ValueDiff {
        path,
        change_type: change_type.into(),
        baseline_value,
        candidate_value,
        deviation_percent,
        risk_level: risk_level.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(version_id: i64, input: Value, output: Value) -> ExecutionTrace {
        ExecutionTrace {
            version_id: Some(version_id),
            status: "success".into(),
            normalized_input: input,
            output,
            ..Default::default()
        }
    }

    fn step(id: &str, order: usize, result: Value, rules: &[&str]) -> DecisionTraceStep {
        DecisionTraceStep {
            order,
            decision_id: id.into(),
            decision_name: id.into(),
            status: "evaluated".into(),
            matched_rule_ids: rules.iter().map(|rule| (*rule).into()).collect(),
            result,
            ..Default::default()
        }
    }

    #[test]
    fn compares_order_rule_hits_intermediate_and_final_results() {
        let mut baseline = trace(
            1,
            serde_json::json!({"amount": 100}),
            serde_json::json!({"approved": true, "score": 100}),
        );
        baseline.decisions = vec![
            step("score", 1, serde_json::json!(100), &["rule-1"]),
            step("approval", 2, serde_json::json!(true), &["rule-a"]),
        ];
        let mut candidate = trace(
            2,
            serde_json::json!({"amount": 100}),
            serde_json::json!({"approved": false, "score": 104}),
        );
        candidate.decisions = vec![
            step("approval", 1, serde_json::json!(false), &["rule-b"]),
            step("score", 2, serde_json::json!(104), &["rule-1"]),
        ];

        let diff = ExecutionTraceComparator {
            baseline_trace: &baseline,
            candidate_trace: &candidate,
        }
        .compare();

        assert!(diff.same_input);
        assert!(diff.execution_order_changed);
        assert_eq!(diff.risk_level, "high");
        assert_eq!(diff.decision_diffs.len(), 2);
        let approval_diff = diff
            .decision_diffs
            .iter()
            .find(|item| item.decision_id == "approval")
            .unwrap();
        assert_eq!(approval_diff.baseline_matched_rule_ids, ["rule-a"]);
        assert_eq!(approval_diff.candidate_matched_rule_ids, ["rule-b"]);
        assert!(diff.output_diffs.iter().any(|item| item.path == "approved"));
        let score_diff = diff
            .output_diffs
            .iter()
            .find(|item| item.path == "score")
            .unwrap();
        assert_eq!(score_diff.deviation_percent, Some(4.0));
        assert_eq!(score_diff.risk_level, "low");
    }

    #[test]
    fn reports_added_and_removed_outputs_and_input_mismatch() {
        let baseline = trace(
            1,
            serde_json::json!({"id": 1}),
            serde_json::json!({"old": "x"}),
        );
        let candidate = trace(
            2,
            serde_json::json!({"id": 2}),
            serde_json::json!({"new": "y"}),
        );
        let diff = ExecutionTraceComparator {
            baseline_trace: &baseline,
            candidate_trace: &candidate,
        }
        .compare();

        assert!(!diff.same_input);
        assert_eq!(diff.output_diffs.len(), 2);
        assert!(
            diff.output_diffs
                .iter()
                .all(|item| item.change_type != "changed")
        );
        assert_eq!(diff.risk_level, "high");
        assert_eq!(
            diff.output_diffs
                .iter()
                .find(|item| item.change_type == "removed")
                .unwrap()
                .risk_level,
            "high"
        );
        assert_eq!(
            diff.output_diffs
                .iter()
                .find(|item| item.change_type == "added")
                .unwrap()
                .risk_level,
            "medium"
        );
    }

    #[test]
    fn input_mismatch_is_high_risk_even_when_outputs_match() {
        let baseline = trace(
            1,
            serde_json::json!({"id": 1}),
            serde_json::json!({"ok": true}),
        );
        let candidate = trace(
            2,
            serde_json::json!({"id": 2}),
            serde_json::json!({"ok": true}),
        );

        let diff = ExecutionTraceComparator {
            baseline_trace: &baseline,
            candidate_trace: &candidate,
        }
        .compare();

        assert!(!diff.same_input);
        assert!(diff.output_diffs.is_empty());
        assert_eq!(diff.risk_level, "high");
    }

    #[test]
    fn numeric_deviation_over_five_percent_is_high_risk() {
        let diff = compare_values("value", &serde_json::json!(100), &serde_json::json!(106));
        assert_eq!(diff[0].deviation_percent, Some(6.0));
        assert_eq!(diff[0].risk_level, "high");
    }

    #[test]
    fn small_numeric_decision_change_stays_low_risk() {
        let mut baseline = trace(
            1,
            serde_json::json!({"x": 1}),
            serde_json::json!({"score": 100}),
        );
        baseline.decisions = vec![step("score", 1, serde_json::json!(100), &[])];
        let mut candidate = trace(
            2,
            serde_json::json!({"x": 1}),
            serde_json::json!({"score": 104}),
        );
        candidate.decisions = vec![step("score", 1, serde_json::json!(104), &[])];
        let diff = ExecutionTraceComparator {
            baseline_trace: &baseline,
            candidate_trace: &candidate,
        }
        .compare();

        assert_eq!(diff.risk_level, "low");
        assert_eq!(diff.decision_diffs[0].risk_level, "low");
    }

    #[test]
    fn reports_rule_hit_order_changes_even_when_the_same_rules_match() {
        let mut baseline = trace(1, serde_json::json!({"x": 1}), serde_json::json!({"v": 1}));
        baseline.decisions = vec![step(
            "decision",
            1,
            serde_json::json!(1),
            &["rule-a", "rule-b"],
        )];
        let mut candidate = trace(2, serde_json::json!({"x": 1}), serde_json::json!({"v": 1}));
        candidate.decisions = vec![step(
            "decision",
            1,
            serde_json::json!(1),
            &["rule-b", "rule-a"],
        )];

        let diff = ExecutionTraceComparator {
            baseline_trace: &baseline,
            candidate_trace: &candidate,
        }
        .compare();

        assert_eq!(diff.risk_level, "high");
        assert!(diff.decision_diffs[0].matched_rules_changed);
        assert_eq!(
            diff.decision_diffs[0].baseline_matched_rule_ids,
            ["rule-a", "rule-b"]
        );
        assert_eq!(
            diff.decision_diffs[0].candidate_matched_rule_ids,
            ["rule-b", "rule-a"]
        );
    }
}
