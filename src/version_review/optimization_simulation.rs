pub const MIN_SIMULATION_SAMPLE_COUNT: usize = 10;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SimulationMetrics {
    pub test_set_size: usize,
    pub baseline_accuracy: f64,
    pub candidate_accuracy: f64,
    pub accuracy_improvement: f64,
    pub baseline_bias_rate: f64,
    pub candidate_bias_rate: f64,
}

pub fn calculate_metrics(
    baseline_passed: usize,
    candidate_passed: usize,
    total: usize,
) -> SimulationMetrics {
    if total == 0 {
        return SimulationMetrics::default();
    }
    let baseline_accuracy = baseline_passed as f64 / total as f64 * 100.0;
    let candidate_accuracy = candidate_passed as f64 / total as f64 * 100.0;
    SimulationMetrics {
        test_set_size: total,
        baseline_accuracy,
        candidate_accuracy,
        accuracy_improvement: candidate_accuracy - baseline_accuracy,
        baseline_bias_rate: 100.0 - baseline_accuracy,
        candidate_bias_rate: 100.0 - candidate_accuracy,
    }
}

pub fn passes_simulation(metrics: SimulationMetrics) -> bool {
    metrics.test_set_size >= MIN_SIMULATION_SAMPLE_COUNT
        && metrics.candidate_accuracy >= metrics.baseline_accuracy
}

pub fn should_withdraw_proposal(metrics: SimulationMetrics) -> bool {
    metrics.test_set_size >= MIN_SIMULATION_SAMPLE_COUNT && !passes_simulation(metrics)
}

pub fn impact_summary(metrics: SimulationMetrics) -> String {
    if metrics.test_set_size < MIN_SIMULATION_SAMPLE_COUNT {
        return format!(
            "当前只有 {} 个可用样本，至少需要 {} 个样本才能判断候选效果；建议补充业务验收案例后重试。",
            metrics.test_set_size, MIN_SIMULATION_SAMPLE_COUNT
        );
    }
    format!(
        "在 {} 个测试样本上，结果一致率变化 {:+.2} 个百分点；此结果仅代表测试集，不构成业务收益保证。",
        metrics.test_set_size, metrics.accuracy_improvement
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_accuracy_and_mismatch_rate_changes() {
        let metrics = calculate_metrics(8, 9, 10);
        assert_eq!(metrics.baseline_accuracy, 80.0);
        assert_eq!(metrics.candidate_accuracy, 90.0);
        assert_eq!(metrics.accuracy_improvement, 10.0);
        assert_eq!(metrics.baseline_bias_rate, 20.0);
        assert_eq!(metrics.candidate_bias_rate, 10.0);
    }

    #[test]
    fn empty_sample_has_no_false_accuracy_claim() {
        assert_eq!(calculate_metrics(0, 0, 0), SimulationMetrics::default());
        assert!(impact_summary(SimulationMetrics::default()).contains("至少需要 10 个样本"));
        assert!(!passes_simulation(SimulationMetrics::default()));
    }

    #[test]
    fn candidate_must_not_reduce_labeled_case_accuracy() {
        let insufficient = calculate_metrics(8, 8, 9);
        assert!(!passes_simulation(insufficient));
        assert!(!should_withdraw_proposal(insufficient));
        assert!(passes_simulation(calculate_metrics(8, 8, 10)));
        assert!(!should_withdraw_proposal(calculate_metrics(8, 9, 10)));
        let regressed = calculate_metrics(8, 7, 10);
        assert!(!passes_simulation(regressed));
        assert!(should_withdraw_proposal(regressed));
    }
}
