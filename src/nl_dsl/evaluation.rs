use std::collections::HashSet;

use super::dsl::{BoxedExpression, Definitions};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StructureSimilarity {
    pub decision_jaccard: f64,
    pub dependency_f1: f64,
    pub rule_count_deviation: f64,
    pub type_match_rate: f64,
}

impl StructureSimilarity {
    pub fn compute(expected: &Definitions, actual: &Definitions) -> Self {
        let expected_decisions = expected
            .decisions
            .iter()
            .map(|decision| decision.name.as_str())
            .collect::<HashSet<_>>();
        let actual_decisions = actual
            .decisions
            .iter()
            .map(|decision| decision.name.as_str())
            .collect::<HashSet<_>>();
        let intersection = expected_decisions.intersection(&actual_decisions).count();
        let union = expected_decisions.union(&actual_decisions).count();
        let decision_jaccard = set_similarity(intersection, union);

        let expected_dependencies = dependencies(expected);
        let actual_dependencies = dependencies(actual);
        let common_dependencies = expected_dependencies
            .intersection(&actual_dependencies)
            .count();
        let dependency_f1 = if expected_dependencies.is_empty() && actual_dependencies.is_empty() {
            1.0
        } else if expected_dependencies.is_empty() || actual_dependencies.is_empty() {
            0.0
        } else {
            let precision = common_dependencies as f64 / actual_dependencies.len() as f64;
            let recall = common_dependencies as f64 / expected_dependencies.len() as f64;
            harmonic_mean(precision, recall)
        };

        let expected_rule_count = rule_count(expected);
        let actual_rule_count = rule_count(actual);
        let rule_count_deviation = if expected_rule_count == 0 && actual_rule_count == 0 {
            1.0
        } else {
            1.0 - expected_rule_count.abs_diff(actual_rule_count) as f64
                / expected_rule_count.max(actual_rule_count) as f64
        };

        let type_match_rate = type_match_rate(expected, actual);
        Self {
            decision_jaccard,
            dependency_f1,
            rule_count_deviation,
            type_match_rate,
        }
    }

    pub fn overall_score(&self) -> f64 {
        0.3 * self.decision_jaccard
            + 0.3 * self.dependency_f1
            + 0.2 * self.rule_count_deviation
            + 0.2 * self.type_match_rate
    }
}

fn dependencies(definitions: &Definitions) -> HashSet<(String, String)> {
    definitions
        .decisions
        .iter()
        .flat_map(|decision| {
            decision
                .required_decisions
                .iter()
                .chain(&decision.required_inputs)
                .map(|dependency| (decision.name.clone(), dependency.clone()))
        })
        .collect()
}

fn rule_count(definitions: &Definitions) -> usize {
    definitions
        .decisions
        .iter()
        .map(|decision| match &decision.logic {
            BoxedExpression::DecisionTable { rules, .. } => rules.len(),
            _ => 0,
        })
        .sum()
}

fn type_match_rate(expected: &Definitions, actual: &Definitions) -> f64 {
    let expected_types = expected
        .decisions
        .iter()
        .map(|decision| (decision.name.as_str(), decision.type_ref.as_deref()))
        .chain(
            expected
                .inputs
                .iter()
                .map(|input| (input.name.as_str(), input.type_ref.as_deref())),
        )
        .collect::<std::collections::HashMap<_, _>>();
    let actual_types = actual
        .decisions
        .iter()
        .map(|decision| (decision.name.as_str(), decision.type_ref.as_deref()))
        .chain(
            actual
                .inputs
                .iter()
                .map(|input| (input.name.as_str(), input.type_ref.as_deref())),
        )
        .collect::<std::collections::HashMap<_, _>>();
    if expected_types.is_empty() && actual_types.is_empty() {
        return 1.0;
    }
    let union = expected_types
        .keys()
        .chain(actual_types.keys())
        .collect::<HashSet<_>>()
        .len();
    let matches = expected_types
        .iter()
        .filter(|(name, type_ref)| actual_types.get(*name) == Some(*type_ref))
        .count();
    matches as f64 / union as f64
}

fn set_similarity(intersection: usize, union: usize) -> f64 {
    if union == 0 {
        1.0
    } else {
        intersection as f64 / union as f64
    }
}

fn harmonic_mean(left: f64, right: f64) -> f64 {
    if left + right == 0.0 {
        0.0
    } else {
        2.0 * left * right / (left + right)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::StructureSimilarity;
    use crate::nl_dsl::dsl::parse_definitions;

    fn model(yaml: &str) -> crate::nl_dsl::dsl::Definitions {
        parse_definitions(&format!("name: Golden\n{yaml}")).expect("golden DSL should parse")
    }

    #[test]
    fn identical_models_score_perfectly_including_empty_structures() {
        let empty = model("inputs: []\ndecisions: []\n");
        assert_eq!(
            StructureSimilarity::compute(&empty, &empty).overall_score(),
            1.0
        );

        let populated = model(
            "inputs:\n  - name: Age\n    typeRef: number\ndecisions:\n  - name: Eligibility\n    typeRef: boolean\n    requiredInputs: [Age]\n    logic:\n      type: literalExpression\n      expression: Age >= 18\n",
        );
        assert_eq!(
            StructureSimilarity::compute(&populated, &populated).overall_score(),
            1.0
        );
    }

    #[test]
    fn missing_dependency_and_type_are_reflected_in_their_metrics() {
        let expected = model(
            "inputs:\n  - name: Age\n    typeRef: number\ndecisions:\n  - name: Eligibility\n    typeRef: boolean\n    requiredInputs: [Age]\n    logic:\n      type: literalExpression\n      expression: Age >= 18\n",
        );
        let actual = model(
            "inputs:\n  - name: Age\n    typeRef: string\ndecisions:\n  - name: Eligibility\n    typeRef: string\n    logic:\n      type: literalExpression\n      expression: '\"eligible\"'\n",
        );
        let score = StructureSimilarity::compute(&expected, &actual);
        assert_eq!(score.decision_jaccard, 1.0);
        assert_eq!(score.dependency_f1, 0.0);
        assert_eq!(score.type_match_rate, 0.0);
        assert!(score.overall_score() < 0.7);
    }

    #[test]
    fn golden_requirement_dsl_cases_compile_and_match_expected_outputs() {
        let corpus = include_str!("../../tests/golden/age-thresholds.tsv");
        let mut seen_ids = HashSet::new();
        let mut case_count = 0;

        for line in corpus
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let fields = line.splitn(5, '\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 5, "malformed golden row: {line}");
            let [id, threshold, probe_age, expected, requirement] =
                <[&str; 5]>::try_from(fields).expect("five fields are required");
            assert!(seen_ids.insert(id), "duplicate golden case id {id}");

            let threshold: f64 = threshold.parse().expect("threshold should be numeric");
            let probe_age: f64 = probe_age.parse().expect("probe age should be numeric");
            assert!(requirement.contains(&(threshold as i64).to_string()));
            let dsl = format!(
                "name: Golden_{id}\ninputs:\n  - name: age\n    typeRef: number\ndecisions:\n  - name: Eligibility\n    typeRef: string\n    requiredInputs: [age]\n    logic:\n      type: literalExpression\n      expression: 'if age >= {threshold} then \"eligible\" else \"ineligible\"'\n"
            );
            let definitions = parse_definitions(&dsl)
                .unwrap_or_else(|error| panic!("{id}: generated DSL did not parse: {error}"));
            let xml = crate::nl_dsl::dmn::generate_dmn_xml(&definitions)
                .unwrap_or_else(|error| panic!("{id}: DMN generation failed: {error}"));
            let model = crate::dmn::parse_dmn(&xml)
                .unwrap_or_else(|error| panic!("{id}: generated DMN did not parse: {error}"));
            let result = crate::dmn::DmnExecutor::new(&model).evaluate_all(HashMap::from([(
                "age".into(),
                crate::feel::FeelValue::Number(probe_age),
            )]));
            assert_eq!(
                result.status,
                crate::dmn::DmnResultStatus::Succeeded,
                "{id} ({requirement}) failed: {:?}",
                result.messages
            );
            let actual = result
                .decision_by_name("Eligibility")
                .expect("golden decision result should exist");
            match &actual.value {
                crate::feel::FeelValue::String(value) => {
                    assert_eq!(value, expected, "{id} ({requirement})");
                }
                value => panic!("{id} returned an unexpected value: {value:?}"),
            }
            case_count += 1;
        }

        assert!(case_count >= 50, "golden suite only has {case_count} cases");
    }
}
