#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessLogicPattern {
    DecisionTree,
    ScoringMatrix,
    WorkflowSequence,
    ThresholdRule,
}

impl BusinessLogicPattern {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "tree" | "decision_tree" => Some(Self::DecisionTree),
            "matrix" | "scoring_matrix" => Some(Self::ScoringMatrix),
            "workflow" | "workflow_sequence" => Some(Self::WorkflowSequence),
            "threshold" | "threshold_rule" => Some(Self::ThresholdRule),
            _ => None,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::DecisionTree => "tree",
            Self::ScoringMatrix => "matrix",
            Self::WorkflowSequence => "workflow",
            Self::ThresholdRule => "threshold",
        }
    }

    fn feature_bonus(self, normalized: &str) -> f32 {
        match self {
            Self::DecisionTree => count_any(normalized, &["如果", "若", "否则", "条件", "分支"]),
            Self::ScoringMatrix => count_any(normalized, &["评分", "打分", "权重", "维度", "指标"]),
            Self::WorkflowSequence => count_any(
                normalized,
                &["首先", "然后", "接着", "之后", "流程", "步骤"],
            ),
            Self::ThresholdRule => count_any(
                normalized,
                &["阈值", "超过", "低于", "至少", "不超过", "区间", "门槛"],
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PatternDefinition {
    pub pattern: BusinessLogicPattern,
    pub keywords: Vec<String>,
    pub historical_success_rate: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetectedPattern {
    pub pattern: BusinessLogicPattern,
    pub confidence: f32,
    pub matched_keywords: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PatternDetector {
    definitions: Vec<PatternDefinition>,
}

impl Default for PatternDetector {
    fn default() -> Self {
        Self::default_without_recursion()
    }
}

impl PatternDetector {
    pub fn from_definitions(definitions: Vec<PatternDefinition>) -> Self {
        let definitions: Vec<PatternDefinition> = definitions
            .into_iter()
            .filter_map(|mut definition| {
                definition.keywords = definition
                    .keywords
                    .into_iter()
                    .filter_map(|keyword| {
                        let keyword = keyword.trim();
                        (!normalize(keyword).is_empty()).then(|| keyword.to_string())
                    })
                    .collect();
                if definition.keywords.is_empty() {
                    return None;
                }
                if !definition.historical_success_rate.is_finite() {
                    definition.historical_success_rate = 0.5;
                }
                Some(definition)
            })
            .collect();
        if definitions.is_empty() {
            return Self::default_without_recursion();
        }
        Self { definitions }
    }

    fn default_without_recursion() -> Self {
        Self {
            definitions: vec![
                definition(
                    BusinessLogicPattern::DecisionTree,
                    &["如果", "若", "否则", "条件", "分级"],
                ),
                definition(
                    BusinessLogicPattern::ScoringMatrix,
                    &["评分", "打分", "权重", "维度", "综合得分"],
                ),
                definition(
                    BusinessLogicPattern::WorkflowSequence,
                    &["流程", "步骤", "首先", "然后", "审批"],
                ),
                definition(
                    BusinessLogicPattern::ThresholdRule,
                    &["阈值", "超过", "低于", "至少", "区间"],
                ),
            ],
        }
    }

    pub fn detect_patterns(&self, requirement: &str) -> Vec<DetectedPattern> {
        let normalized = normalize(requirement);
        if normalized.is_empty() {
            return Vec::new();
        }
        let mut results: Vec<_> = self
            .definitions
            .iter()
            .filter_map(|definition| {
                let matched_keywords: Vec<_> = definition
                    .keywords
                    .iter()
                    .filter(|keyword| normalized.contains(&normalize(keyword)))
                    .cloned()
                    .collect();
                let feature_hits = definition.pattern.feature_bonus(&normalized);
                if matched_keywords.is_empty() && feature_hits == 0.0 {
                    return None;
                }
                let keyword_strength = (matched_keywords.len().min(3) as f32) * 0.18;
                let feature_strength = feature_hits.min(2.0) * 0.08;
                let historical_adjustment =
                    (definition.historical_success_rate.clamp(0.0, 1.0) - 0.5) * 0.1;
                Some(DetectedPattern {
                    pattern: definition.pattern,
                    confidence: (0.35
                        + keyword_strength
                        + feature_strength
                        + historical_adjustment)
                        .clamp(0.0, 0.98),
                    matched_keywords,
                })
            })
            .collect();
        results.sort_by(|left, right| right.confidence.total_cmp(&left.confidence));
        results
    }
}

fn definition(pattern: BusinessLogicPattern, keywords: &[&str]) -> PatternDefinition {
    PatternDefinition {
        pattern,
        keywords: keywords.iter().map(|keyword| keyword.to_string()).collect(),
        historical_success_rate: 0.5,
    }
}

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace() && !character.is_ascii_punctuation())
        .flat_map(char::to_lowercase)
        .collect()
}

fn count_any(text: &str, keywords: &[&str]) -> f32 {
    keywords
        .iter()
        .filter(|keyword| text.contains(**keyword))
        .count() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_tree_matrix_workflow_and_threshold_patterns() {
        let detector = PatternDetector::default();
        let cases = [
            (
                "如果信用分低于阈值，否则进入人工复核",
                BusinessLogicPattern::DecisionTree,
            ),
            (
                "根据收入、负债等多个维度加权评分，计算综合得分",
                BusinessLogicPattern::ScoringMatrix,
            ),
            (
                "首先校验资料，然后主管审批，最后归档",
                BusinessLogicPattern::WorkflowSequence,
            ),
            (
                "分数超过80分通过，低于60分拒绝",
                BusinessLogicPattern::ThresholdRule,
            ),
        ];
        for (requirement, expected) in cases {
            assert!(
                detector
                    .detect_patterns(requirement)
                    .iter()
                    .any(|result| result.pattern == expected)
            );
        }
    }

    #[test]
    fn detections_are_ranked_and_include_explanatory_keywords() {
        let detections =
            PatternDetector::default().detect_patterns("如果评分超过80分，则进入审批流程");
        assert!(!detections.is_empty());
        assert!(
            detections
                .windows(2)
                .all(|pair| pair[0].confidence >= pair[1].confidence)
        );
        assert!(
            detections
                .iter()
                .any(|item| !item.matched_keywords.is_empty())
        );
    }

    #[test]
    fn empty_text_and_unknown_library_types_are_safe() {
        assert!(PatternDetector::default().detect_patterns("   ").is_empty());
        assert_eq!(BusinessLogicPattern::parse("unrecognized"), None);
        assert_eq!(
            BusinessLogicPattern::parse("matrix"),
            Some(BusinessLogicPattern::ScoringMatrix)
        );
    }

    #[test]
    fn malformed_library_keywords_cannot_match_every_requirement() {
        let detector = PatternDetector::from_definitions(vec![PatternDefinition {
            pattern: BusinessLogicPattern::DecisionTree,
            keywords: vec!["".into(), "   ".into(), ".,!".into()],
            historical_success_rate: f32::NAN,
        }]);

        assert!(
            detector
                .detect_patterns("请根据普通业务数据生成决策模型")
                .is_empty()
        );
    }

    #[test]
    fn golden_requirement_patterns_match_expected_analysis_categories() {
        let corpus = include_str!("../../tests/golden/business-patterns.tsv");
        let detector = PatternDetector::default();
        let mut case_count = 0;
        for line in corpus
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let fields = line.splitn(3, '\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 3, "malformed analysis golden row: {line}");
            let [id, expected_pattern, requirement] =
                <[&str; 3]>::try_from(fields).expect("three fields are required");
            let expected_pattern = BusinessLogicPattern::parse(expected_pattern)
                .unwrap_or_else(|| panic!("{id}: unknown expected pattern"));
            let detected = detector.detect_patterns(requirement);
            assert!(
                detected
                    .first()
                    .is_some_and(|pattern| pattern.pattern == expected_pattern),
                "{id} did not rank {:?} first: {requirement}",
                expected_pattern
            );
            case_count += 1;
        }
        assert!(
            case_count >= 20,
            "analysis golden suite only has {case_count} cases"
        );
    }
}
