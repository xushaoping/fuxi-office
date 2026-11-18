use std::collections::HashSet;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoricalRegressionCase {
    pub source_run_id: i64,
    pub input: Value,
    pub expected_output: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeneratedRegressionCases {
    pub cases: Vec<HistoricalRegressionCase>,
    pub duplicate_count: usize,
}

pub struct RegressionTestGenerator;

impl RegressionTestGenerator {
    pub fn generate(
        historical_runs: impl IntoIterator<Item = HistoricalRegressionCase>,
    ) -> GeneratedRegressionCases {
        let mut seen = HashSet::new();
        let mut generated = GeneratedRegressionCases::default();
        for run in historical_runs {
            let key = (
                serde_json::to_string(&run.input).unwrap_or_default(),
                serde_json::to_string(&run.expected_output).unwrap_or_default(),
            );
            if seen.insert(key) {
                generated.cases.push(run);
            } else {
                generated.duplicate_count += 1;
            }
        }
        generated
    }
}

pub fn aggregate_risk(statuses: impl IntoIterator<Item = String>) -> String {
    let mut risk = "low";
    for status in statuses {
        match status.as_str() {
            "failed" | "error" => return "high".into(),
            "unmatched" if risk == "low" => risk = "medium",
            "passed" | "unmatched" => {}
            _ => return "high".into(),
        }
    }
    risk.into()
}

pub fn source_coverage_percent(selected: usize, available: usize) -> f64 {
    if available == 0 {
        0.0
    } else {
        (selected as f64 / available as f64 * 100.0).min(100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn generates_unique_cases_but_preserves_conflicting_expected_outputs() {
        let cases = vec![
            HistoricalRegressionCase {
                source_run_id: 1,
                input: json!({"amount": 100}),
                expected_output: json!({"approved": true}),
            },
            HistoricalRegressionCase {
                source_run_id: 2,
                input: json!({"amount": 100}),
                expected_output: json!({"approved": true}),
            },
            HistoricalRegressionCase {
                source_run_id: 3,
                input: json!({"amount": 100}),
                expected_output: json!({"approved": false}),
            },
        ];
        let generated = RegressionTestGenerator::generate(cases);

        assert_eq!(generated.cases.len(), 2);
        assert_eq!(generated.cases[0].source_run_id, 1);
        assert_eq!(generated.duplicate_count, 1);
    }

    #[test]
    fn aggregates_any_failure_as_high_risk() {
        assert_eq!(aggregate_risk(["passed".into(), "failed".into()]), "high");
        assert_eq!(aggregate_risk(["passed".into()]), "low");
    }

    #[test]
    fn incomplete_or_unknown_results_never_aggregate_as_low_risk() {
        assert_eq!(aggregate_risk(["pending".into()]), "high");
        assert_eq!(aggregate_risk(["cancelled".into()]), "high");
        assert_eq!(
            aggregate_risk(["passed".into(), "unmatched".into()]),
            "medium"
        );
    }

    #[test]
    fn source_coverage_handles_empty_and_capped_history() {
        assert_eq!(source_coverage_percent(0, 0), 0.0);
        assert_eq!(source_coverage_percent(1000, 2000), 50.0);
        assert_eq!(source_coverage_percent(1000, 2), 100.0);
    }
}
