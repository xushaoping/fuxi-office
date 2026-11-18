use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::nl_dsl::analysis_engine::PatternDetector;

const COMPLEXITIES: [&str; 3] = ["simple", "standard", "complex"];
pub const MAX_DATASET_BYTES: usize = 20_000_000;
const MAX_CASES: usize = 100_000;
const MAX_REQUIREMENT_BYTES: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementComplexity {
    Simple,
    Standard,
    Complex,
}

impl RequirementComplexity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Standard => "standard",
            Self::Complex => "complex",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "simple" => Some(Self::Simple),
            "standard" => Some(Self::Standard),
            "complex" => Some(Self::Complex),
            _ => None,
        }
    }
}

pub fn classify_requirement_complexity(requirement: &str) -> RequirementComplexity {
    let character_count = requirement.chars().count();
    let complexity_signals = [
        "否则",
        "例外",
        "除外",
        "优先级",
        "同时满足",
        "依赖",
        "多阶段",
        "互斥",
        "累计",
        "嵌套",
        "交叉校验",
        "回退",
    ]
    .iter()
    .filter(|signal| requirement.contains(**signal))
    .count();
    let standard_signals = [
        "根据", "等级", "金额", "并且", "以及", "分别", "条件", "多项",
    ]
    .iter()
    .filter(|signal| requirement.contains(**signal))
    .count();
    let pattern_count = PatternDetector::default()
        .detect_patterns(requirement)
        .len();
    if character_count > 1800 || complexity_signals >= 3 || pattern_count >= 3 {
        RequirementComplexity::Complex
    } else if character_count <= 350
        && complexity_signals <= 1
        && standard_signals == 0
        && pattern_count <= 1
    {
        RequirementComplexity::Simple
    } else {
        RequirementComplexity::Standard
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComplexityEvaluation {
    pub case_count: usize,
    pub accuracy: Option<f64>,
    pub correct: usize,
    pub confusion_matrix: BTreeMap<String, BTreeMap<String, usize>>,
    pub per_class: BTreeMap<String, ClassMetrics>,
    pub macro_f1: Option<f64>,
    pub dataset_sha256: String,
    pub limitation: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassMetrics {
    pub precision: Option<f64>,
    pub precision_denominator: usize,
    pub precision_wilson_95: Option<[f64; 2]>,
    pub recall: Option<f64>,
    pub recall_denominator: usize,
    pub recall_wilson_95: Option<[f64; 2]>,
    pub f1: Option<f64>,
    pub support: usize,
}

pub fn evaluate_tsv(input: &str) -> Result<ComplexityEvaluation, String> {
    if input.len() > MAX_DATASET_BYTES {
        return Err(format!("dataset exceeds {MAX_DATASET_BYTES} bytes"));
    }
    let mut seen_ids = HashSet::new();
    let mut matrix = empty_matrix();
    let mut case_count = 0usize;
    let mut correct = 0usize;

    for (line_index, line) in input.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.splitn(3, '\t').collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err(format!(
                "line {} must have id, label, and requirement fields",
                line_index + 1
            ));
        }
        let [id, expected, requirement] =
            <[&str; 3]>::try_from(fields).expect("three TSV fields were checked");
        if id.trim().is_empty() || !seen_ids.insert(id) {
            return Err(format!(
                "line {} has an empty or duplicate case ID",
                line_index + 1
            ));
        }
        let expected = RequirementComplexity::parse(expected)
            .ok_or_else(|| format!("line {} has an unknown complexity label", line_index + 1))?;
        if requirement.trim().is_empty() || requirement.len() > MAX_REQUIREMENT_BYTES {
            return Err(format!(
                "line {} requirement must contain 1 to {MAX_REQUIREMENT_BYTES} bytes",
                line_index + 1
            ));
        }
        case_count += 1;
        if case_count > MAX_CASES {
            return Err(format!("dataset exceeds {MAX_CASES} cases"));
        }
        let predicted = classify_requirement_complexity(requirement);
        let expected_name = expected.as_str();
        let predicted_name = predicted.as_str();
        *matrix
            .get_mut(expected_name)
            .expect("all expected labels are initialized")
            .get_mut(predicted_name)
            .expect("all predicted labels are initialized") += 1;
        correct += usize::from(expected == predicted);
    }

    if case_count == 0 {
        return Err("dataset contains no labeled cases".into());
    }

    let mut per_class = BTreeMap::new();
    let mut f1_values = Vec::new();
    for class in COMPLEXITIES {
        let true_positive = matrix[class][class];
        let support = matrix[class].values().sum::<usize>();
        let predicted_count = matrix.values().map(|row| row[class]).sum::<usize>();
        let precision = ratio(true_positive, predicted_count);
        let recall = ratio(true_positive, support);
        let false_positives = predicted_count - true_positive;
        let false_negatives = support - true_positive;
        let f1_denominator = 2 * true_positive + false_positives + false_negatives;
        let f1 = (f1_denominator > 0).then(|| 2.0 * true_positive as f64 / f1_denominator as f64);
        if let Some(value) = f1 {
            f1_values.push(value);
        }
        per_class.insert(
            class.to_string(),
            ClassMetrics {
                precision,
                precision_denominator: predicted_count,
                precision_wilson_95: wilson_interval(true_positive, predicted_count),
                recall,
                recall_denominator: support,
                recall_wilson_95: wilson_interval(true_positive, support),
                f1,
                support,
            },
        );
    }

    Ok(ComplexityEvaluation {
        case_count,
        accuracy: ratio(correct, case_count),
        correct,
        confusion_matrix: matrix,
        per_class,
        macro_f1: (!f1_values.is_empty())
            .then(|| f1_values.iter().sum::<f64>() / f1_values.len() as f64),
        dataset_sha256: format!("{:x}", Sha256::digest(input.as_bytes())),
        limitation: "Offline labeled-set metrics describe this dataset only; they do not establish production routing quality or model cost benefit.",
    })
}

fn empty_matrix() -> BTreeMap<String, BTreeMap<String, usize>> {
    COMPLEXITIES
        .into_iter()
        .map(|expected| {
            (
                expected.to_string(),
                COMPLEXITIES
                    .into_iter()
                    .map(|predicted| (predicted.to_string(), 0))
                    .collect(),
            )
        })
        .collect()
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

fn wilson_interval(successes: usize, denominator: usize) -> Option<[f64; 2]> {
    if denominator == 0 || successes > denominator {
        return None;
    }
    let n = denominator as f64;
    let p = successes as f64 / n;
    let z = 1.959_963_984_540_054;
    let z2 = z * z;
    let center = (p + z2 / (2.0 * n)) / (1.0 + z2 / n);
    let margin = z * ((p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt()) / (1.0 + z2 / n);
    Some([(center - margin).max(0.0), (center + margin).min(1.0)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_labeled_cases_without_returning_requirement_text() {
        let report = evaluate_tsv(concat!(
            "case-1\tsimple\t判断申请是否通过\n",
            "case-2\tcomplex\t同时满足收入、评分和条件，出现例外则回退并交叉校验\n",
        ))
        .unwrap();

        assert_eq!(report.case_count, 2);
        assert_eq!(report.accuracy, Some(1.0));
        assert_eq!(report.confusion_matrix["simple"]["simple"], 1);
        assert_eq!(report.confusion_matrix["complex"]["complex"], 1);
        assert_eq!(report.per_class["standard"].support, 0);
        assert_eq!(report.per_class["standard"].recall_wilson_95, None);
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("判断申请是否通过"));

        let mismatch = evaluate_tsv("case-3\tcomplex\t判断申请是否通过\n").unwrap();
        assert_eq!(mismatch.accuracy, Some(0.0));
        assert_eq!(mismatch.per_class["simple"].precision, Some(0.0));
        assert_eq!(mismatch.per_class["simple"].f1, Some(0.0));
        assert_eq!(mismatch.per_class["complex"].recall, Some(0.0));
        assert_eq!(mismatch.per_class["complex"].f1, Some(0.0));
    }

    #[test]
    fn rejects_invalid_or_oversized_labeled_cases() {
        for (input, expected_error) in [
            ("id\tunknown\trequirement", "unknown complexity label"),
            ("id\tsimple\t", "requirement must contain"),
            (
                "id\tsimple\tvalid\nid\tsimple\tduplicate",
                "empty or duplicate case ID",
            ),
        ] {
            assert!(evaluate_tsv(input).unwrap_err().contains(expected_error));
        }
        let oversized = format!("id\tsimple\t{}", "x".repeat(MAX_REQUIREMENT_BYTES + 1));
        assert!(
            evaluate_tsv(&oversized)
                .unwrap_err()
                .contains("requirement must contain")
        );
    }
}
