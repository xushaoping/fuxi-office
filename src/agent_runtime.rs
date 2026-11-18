use sha2::{Digest, Sha256};
use std::cmp::Ordering;

pub const MIN_EXPERIMENT_SAMPLE_SIZE: i64 = 30;

#[path = "agent_runtime/orchestration.rs"]
#[allow(dead_code)] // Reusable orchestration primitives are currently exercised by unit tests.
pub mod orchestration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSelectionPolicy {
    CostOptimized,
    QualityFirst,
    BalancedMode,
}

impl ModelSelectionPolicy {
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("cost_optimized") | Some("costoptimized") => Self::CostOptimized,
            Some("quality_first") | Some("qualityfirst") => Self::QualityFirst,
            _ => Self::BalancedMode,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCandidate {
    pub name: String,
    pub estimated_cost_usd: Option<f64>,
    pub success_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelPerformanceMetric {
    pub model_name: String,
    pub task_type: String,
    pub success_rate: f64,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    Analyze,
    Plan,
    Dsl,
    ChangeIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExperimentAssignment<'a> {
    pub experiment_name: &'a str,
    pub variant: &'static str,
    pub treatment_percent: u8,
    pub bucket: u8,
}

pub fn experiment_assignment(
    experiment: &str,
    project_id: i64,
    treatment_percent: u8,
) -> ExperimentAssignment<'_> {
    let mut hasher = Sha256::new();
    hasher.update(experiment.as_bytes());
    hasher.update(b":");
    hasher.update(project_id.to_be_bytes());
    let digest = hasher.finalize();
    let bucket =
        (u64::from_be_bytes(digest[..8].try_into().expect("SHA-256 prefix length")) % 100) as u8;
    let treatment_percent = treatment_percent.min(100);
    ExperimentAssignment {
        experiment_name: experiment,
        variant: if bucket < treatment_percent {
            "treatment"
        } else {
            "control"
        },
        treatment_percent,
        bucket,
    }
}

#[cfg(test)]
pub fn experiment_variant(
    experiment: &str,
    project_id: i64,
    treatment_percent: u8,
) -> &'static str {
    experiment_assignment(experiment, project_id, treatment_percent).variant
}

pub fn wilson_confidence_interval(successes: u64, sample_size: u64) -> Option<(f64, f64)> {
    if sample_size == 0 || successes > sample_size {
        return None;
    }
    let n = sample_size as f64;
    let proportion = successes as f64 / n;
    let z = 1.959_963_984_540_054;
    let z_squared = z * z;
    let denominator = 1.0 + z_squared / n;
    let center = (proportion + z_squared / (2.0 * n)) / denominator;
    let margin = z * ((proportion * (1.0 - proportion) / n + z_squared / (4.0 * n * n)).sqrt())
        / denominator;
    let lower = if successes == 0 {
        0.0
    } else {
        (center - margin).max(0.0)
    };
    let upper = if successes == sample_size {
        1.0
    } else {
        (center + margin).min(1.0)
    };
    Some((lower, upper))
}

pub fn wilson_difference_confidence_interval(
    treatment_successes: u64,
    treatment_size: u64,
    control_successes: u64,
    control_size: u64,
) -> Option<(f64, f64)> {
    let (treatment_lower, treatment_upper) =
        wilson_confidence_interval(treatment_successes, treatment_size)?;
    let (control_lower, control_upper) =
        wilson_confidence_interval(control_successes, control_size)?;
    let treatment_rate = treatment_successes as f64 / treatment_size as f64;
    let control_rate = control_successes as f64 / control_size as f64;
    let difference = treatment_rate - control_rate;
    let lower_margin = ((treatment_rate - treatment_lower).powi(2)
        + (control_upper - control_rate).powi(2))
    .sqrt();
    let upper_margin = ((treatment_upper - treatment_rate).powi(2)
        + (control_rate - control_lower).powi(2))
    .sqrt();
    Some((
        (difference - lower_margin).max(-1.0),
        (difference + upper_margin).min(1.0),
    ))
}

pub fn treatment_effect_is_significant(
    treatment_successes: u64,
    treatment_size: u64,
    control_successes: u64,
    control_size: u64,
) -> bool {
    treatment_size >= MIN_EXPERIMENT_SAMPLE_SIZE as u64
        && control_size >= MIN_EXPERIMENT_SAMPLE_SIZE as u64
        && wilson_difference_confidence_interval(
            treatment_successes,
            treatment_size,
            control_successes,
            control_size,
        )
        .is_some_and(|(lower, upper)| lower > 0.0 || upper < 0.0)
}

impl TaskType {
    pub fn from_config_prefix(prefix: &str) -> Self {
        match prefix {
            "PLAN" => Self::Plan,
            "DSL" => Self::Dsl,
            "CHANGE_INTENT" => Self::ChangeIntent,
            _ => Self::Analyze,
        }
    }

    pub const fn metric_key(self) -> &'static str {
        match self {
            Self::Analyze => "analyze",
            Self::Plan => "plan",
            Self::Dsl => "dsl",
            Self::ChangeIntent => "change_intent",
        }
    }
}

pub fn apply_performance_metrics(
    candidates: &mut [ModelCandidate],
    task: TaskType,
    metrics: &[ModelPerformanceMetric],
) {
    for candidate in candidates {
        let Some(metric) = metrics.iter().find(|metric| {
            metric.model_name == candidate.name && metric.task_type == task.metric_key()
        }) else {
            continue;
        };
        candidate.success_rate = Some(metric.success_rate.clamp(0.0, 1.0));
        if candidate.estimated_cost_usd.is_none() {
            candidate.estimated_cost_usd = metric.estimated_cost_usd;
        }
    }
}

pub fn select_model(
    candidates: &[ModelCandidate],
    policy: ModelSelectionPolicy,
    task: TaskType,
) -> Option<&ModelCandidate> {
    candidates.iter().min_by(|left, right| {
        score(left, policy, task)
            .partial_cmp(&score(right, policy, task))
            .unwrap_or(Ordering::Equal)
    })
}

fn score(candidate: &ModelCandidate, policy: ModelSelectionPolicy, task: TaskType) -> f64 {
    let cost = candidate.estimated_cost_usd.unwrap_or(1.0).max(0.0);
    let quality = candidate.success_rate.unwrap_or(0.5).clamp(0.0, 1.0);
    let quality_weight = match policy {
        ModelSelectionPolicy::CostOptimized => 0.02,
        ModelSelectionPolicy::QualityFirst => 0.9,
        ModelSelectionPolicy::BalancedMode => 0.6,
    } + if task == TaskType::Dsl { 0.1 } else { 0.0 };
    let normalized_cost = cost / (1.0 + cost);
    (1.0 - quality) * quality_weight + normalized_cost * (1.0 - quality_weight)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<ModelCandidate> {
        vec![
            ModelCandidate {
                name: "cheap".into(),
                estimated_cost_usd: Some(0.001),
                success_rate: Some(0.72),
            },
            ModelCandidate {
                name: "accurate".into(),
                estimated_cost_usd: Some(0.02),
                success_rate: Some(0.96),
            },
        ]
    }

    #[test]
    fn policy_changes_selection_tradeoff() {
        let candidates = candidates();
        assert_eq!(
            select_model(
                &candidates,
                ModelSelectionPolicy::CostOptimized,
                TaskType::Analyze
            )
            .unwrap()
            .name,
            "cheap"
        );
        assert_eq!(
            select_model(
                &candidates,
                ModelSelectionPolicy::QualityFirst,
                TaskType::Analyze
            )
            .unwrap()
            .name,
            "accurate"
        );
    }

    #[test]
    fn missing_metrics_receive_neutral_defaults() {
        let candidates = vec![ModelCandidate {
            name: "configured-primary".into(),
            estimated_cost_usd: None,
            success_rate: None,
        }];
        assert_eq!(
            select_model(
                &candidates,
                ModelSelectionPolicy::BalancedMode,
                TaskType::Plan
            )
            .unwrap()
            .name,
            "configured-primary"
        );
        assert!(select_model(&[], ModelSelectionPolicy::BalancedMode, TaskType::Plan).is_none());
    }

    #[test]
    fn policy_configuration_defaults_safely() {
        assert_eq!(
            ModelSelectionPolicy::from_env_value(None),
            ModelSelectionPolicy::BalancedMode
        );
        assert_eq!(
            ModelSelectionPolicy::from_env_value(Some("quality_first")),
            ModelSelectionPolicy::QualityFirst
        );
    }

    #[test]
    fn performance_metrics_enrich_only_matching_model_and_task() {
        let mut candidates = candidates();
        candidates[1].estimated_cost_usd = None;
        let metrics = vec![ModelPerformanceMetric {
            model_name: "accurate".into(),
            task_type: "dsl".into(),
            success_rate: 0.99,
            estimated_cost_usd: Some(0.03),
        }];
        apply_performance_metrics(&mut candidates, TaskType::Dsl, &metrics);
        assert_eq!(candidates[1].success_rate, Some(0.99));
        assert_eq!(candidates[1].estimated_cost_usd, Some(0.03));
        assert_eq!(candidates[0].success_rate, Some(0.72));
    }

    #[test]
    fn experiment_assignment_is_stable_and_scoped_to_experiment() {
        let assignment = experiment_variant("model-choice-v1", 42, 50);
        assert_eq!(experiment_variant("model-choice-v1", 42, 50), assignment);
        assert_eq!(experiment_variant("model-choice-v1", 42, 0), "control");
        assert_eq!(experiment_variant("model-choice-v1", 42, 100), "treatment");
        assert_ne!(
            experiment_variant("model-choice-v1", 42, 0),
            experiment_variant("model-choice-v1", 42, 100)
        );
    }

    #[test]
    fn experiment_assignment_exposes_auditable_bucket_and_split() {
        let assignment = experiment_assignment("model-choice-v1", 42, 50);
        assert_eq!(assignment, experiment_assignment("model-choice-v1", 42, 50));
        assert!(assignment.bucket < 100);
        assert_eq!(assignment.treatment_percent, 50);
        assert_eq!(
            assignment.variant == "treatment",
            assignment.bucket < assignment.treatment_percent
        );
    }

    #[test]
    fn wilson_interval_handles_empty_invalid_and_boundary_samples() {
        assert_eq!(wilson_confidence_interval(0, 0), None);
        assert_eq!(wilson_confidence_interval(11, 10), None);
        let none_succeeded = wilson_confidence_interval(0, 10).unwrap();
        let all_succeeded = wilson_confidence_interval(10, 10).unwrap();
        assert_eq!(none_succeeded.0, 0.0);
        assert_eq!(all_succeeded.1, 1.0);
        assert!(none_succeeded.1 > 0.0);
        assert!(all_succeeded.0 < 1.0);
        let interval = wilson_confidence_interval(7, 10).unwrap();
        assert!(interval.0 < 0.7 && interval.1 > 0.7);
    }

    #[test]
    fn treatment_effect_interval_is_ordered_around_rate_difference() {
        let interval = wilson_difference_confidence_interval(80, 100, 60, 100).unwrap();
        assert!(interval.0 < 0.2 && interval.1 > 0.2);
        assert!(interval.0 > 0.0);
        assert!(wilson_difference_confidence_interval(101, 100, 10, 100).is_none());
    }

    #[test]
    fn treatment_significance_requires_both_minimum_samples_and_nonzero_interval() {
        assert!(!treatment_effect_is_significant(10, 10, 0, 10));
        assert!(!treatment_effect_is_significant(40, 100, 35, 100));
        assert!(treatment_effect_is_significant(80, 100, 60, 100));
        assert!(treatment_effect_is_significant(20, 100, 40, 100));
    }
}
