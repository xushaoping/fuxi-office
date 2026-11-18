use serde_json::Value;

use crate::models::{VersionChangeReview, VersionDiffEntry, VersionRiskFinding};
use crate::nl_dsl::Definitions;

pub mod behavior_comparison;
pub mod optimization_simulation;
pub mod regression_testing;

pub fn build_change_review(
    base: Option<&Definitions>,
    updated: &Definitions,
    version_id: Option<i64>,
    base_version_number: Option<i32>,
    target_version_number: Option<i32>,
) -> VersionChangeReview {
    let changes = match base {
        Some(base) => structural_entries(base, updated),
        None => initial_entries(updated),
    };
    let mut findings = changes.iter().filter_map(risk_finding).collect::<Vec<_>>();

    if base.is_none() {
        findings.push(VersionRiskFinding {
            level: "medium".into(),
            code: "INITIAL_MODEL".into(),
            message: "首次生成的模型尚未经过历史版本对照，需要确认输入、输出和默认行为".into(),
            element: updated.name.clone(),
        });
    }
    if changes.is_empty() {
        findings.push(VersionRiskFinding {
            level: "low".into(),
            code: "NO_EFFECTIVE_CHANGE".into(),
            message: "本次修改没有产生可识别的模型变化".into(),
            element: updated.name.clone(),
        });
    }

    let overall_risk = findings
        .iter()
        .map(|finding| finding.level.as_str())
        .max_by_key(|level| risk_rank(level))
        .unwrap_or("low")
        .to_string();
    let added = changes
        .iter()
        .filter(|change| change.change_type == "add")
        .count();
    let updated_count = changes
        .iter()
        .filter(|change| change.change_type == "update")
        .count();
    let removed = changes
        .iter()
        .filter(|change| change.change_type == "remove")
        .count();

    VersionChangeReview {
        version_id,
        base_version_number,
        target_version_number,
        overall_risk,
        summary: format!(
            "{} 个变更：新增 {}、更新 {}、删除 {}",
            changes.len(),
            added,
            updated_count,
            removed
        ),
        changes,
        findings,
        confirmation_required: true,
    }
}

fn initial_entries(updated: &Definitions) -> Vec<VersionDiffEntry> {
    let mut entries = Vec::new();
    macro_rules! add_elements {
        ($kind:literal, $items:expr) => {
            entries.extend($items.iter().map(|item| VersionDiffEntry {
                change_type: "add".into(),
                element_type: $kind.into(),
                element: item.name.clone(),
                path: String::new(),
                before: Value::Null,
                after: serde_json::to_value(item).unwrap_or(Value::Null),
                reason: "首次模型生成".into(),
            }));
        };
    }
    add_elements!("itemDefinition", updated.item_definitions);
    add_elements!("input", updated.inputs);
    add_elements!("decision", updated.decisions);
    add_elements!("bkm", updated.bkms);
    add_elements!("decisionService", updated.decision_services);
    add_elements!("knowledgeSource", updated.knowledge_sources);
    entries
}

fn structural_entries(base: &Definitions, updated: &Definitions) -> Vec<VersionDiffEntry> {
    let mut entries = Vec::new();
    macro_rules! compare_elements {
        ($kind:literal, $before:expr, $after:expr) => {{
            let before = $before
                .iter()
                .map(|item| {
                    (
                        item.name.clone(),
                        serde_json::to_value(item).unwrap_or(Value::Null),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            let after = $after
                .iter()
                .map(|item| {
                    (
                        item.name.clone(),
                        serde_json::to_value(item).unwrap_or(Value::Null),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            for (name, value) in &after {
                match before.get(name) {
                    None => entries.push(VersionDiffEntry {
                        change_type: "add".into(),
                        element_type: $kind.into(),
                        element: name.clone(),
                        path: String::new(),
                        before: Value::Null,
                        after: value.clone(),
                        reason: "根据版本制品计算".into(),
                    }),
                    Some(previous) if previous != value => entries.push(VersionDiffEntry {
                        change_type: "update".into(),
                        element_type: $kind.into(),
                        element: name.clone(),
                        path: String::new(),
                        before: previous.clone(),
                        after: value.clone(),
                        reason: "根据版本制品计算".into(),
                    }),
                    _ => {}
                }
            }
            for (name, value) in &before {
                if !after.contains_key(name) {
                    entries.push(VersionDiffEntry {
                        change_type: "remove".into(),
                        element_type: $kind.into(),
                        element: name.clone(),
                        path: String::new(),
                        before: value.clone(),
                        after: Value::Null,
                        reason: "根据版本制品计算".into(),
                    });
                }
            }
        }};
    }
    compare_elements!("import", base.imports, updated.imports);
    compare_elements!(
        "itemDefinition",
        base.item_definitions,
        updated.item_definitions
    );
    compare_elements!("input", base.inputs, updated.inputs);
    compare_elements!("decision", base.decisions, updated.decisions);
    compare_elements!("bkm", base.bkms, updated.bkms);
    compare_elements!(
        "decisionService",
        base.decision_services,
        updated.decision_services
    );
    compare_elements!(
        "knowledgeSource",
        base.knowledge_sources,
        updated.knowledge_sources
    );

    for (path, before, after) in [
        ("name", json_value(&base.name), json_value(&updated.name)),
        (
            "namespace",
            json_value(&base.namespace),
            json_value(&updated.namespace),
        ),
        (
            "textAnnotations",
            serde_json::to_value(&base.text_annotations).unwrap_or(Value::Null),
            serde_json::to_value(&updated.text_annotations).unwrap_or(Value::Null),
        ),
        (
            "diagram",
            serde_json::to_value(&base.diagram).unwrap_or(Value::Null),
            serde_json::to_value(&updated.diagram).unwrap_or(Value::Null),
        ),
    ] {
        if before != after {
            entries.push(VersionDiffEntry {
                change_type: "update".into(),
                element_type: "model".into(),
                element: updated.name.clone(),
                path: path.into(),
                before,
                after,
                reason: "根据版本制品计算".into(),
            });
        }
    }
    entries
}

fn json_value(value: &str) -> Value {
    Value::String(value.to_string())
}

fn is_numeric_unary_test(value: &str) -> bool {
    let value = value.trim();
    let number = ["<=", ">=", "<", ">", "="]
        .iter()
        .find_map(|operator| value.strip_prefix(operator))
        .unwrap_or(value)
        .trim();
    number.parse::<f64>().is_ok_and(|number| number.is_finite())
}

fn is_pure_numeric_threshold_change(change: &VersionDiffEntry) -> bool {
    if change.change_type != "update" || !change.element_type.eq_ignore_ascii_case("decision") {
        return false;
    }
    let (Some(before), Some(after)) = (change.before.as_object(), change.after.as_object()) else {
        return false;
    };
    let mut before_contract = before.clone();
    let mut after_contract = after.clone();
    let (Some(before_logic), Some(after_logic)) = (
        before_contract.remove("logic"),
        after_contract.remove("logic"),
    ) else {
        return false;
    };
    if before_contract != after_contract {
        return false;
    }
    let (Some(before_logic), Some(after_logic)) =
        (before_logic.as_object(), after_logic.as_object())
    else {
        return false;
    };
    if before_logic.get("type").and_then(Value::as_str) != Some("decisionTable") {
        return false;
    }
    let mut before_table = before_logic.clone();
    let mut after_table = after_logic.clone();
    let (Some(before_rules), Some(after_rules)) = (
        before_table
            .remove("rules")
            .and_then(|rules| rules.as_array().cloned()),
        after_table
            .remove("rules")
            .and_then(|rules| rules.as_array().cloned()),
    ) else {
        return false;
    };
    if before_table != after_table || before_rules.len() != after_rules.len() {
        return false;
    }
    let mut threshold_changed = false;
    for (before_rule, after_rule) in before_rules.iter().zip(&after_rules) {
        let (Some(before_rule), Some(after_rule)) =
            (before_rule.as_object(), after_rule.as_object())
        else {
            return false;
        };
        let mut before_rule_contract = before_rule.clone();
        let mut after_rule_contract = after_rule.clone();
        let (Some(before_inputs), Some(after_inputs)) = (
            before_rule_contract
                .remove("inputEntries")
                .and_then(|inputs| inputs.as_array().cloned()),
            after_rule_contract
                .remove("inputEntries")
                .and_then(|inputs| inputs.as_array().cloned()),
        ) else {
            return false;
        };
        if before_rule_contract != after_rule_contract || before_inputs.len() != after_inputs.len()
        {
            return false;
        }
        for (before_input, after_input) in before_inputs.iter().zip(&after_inputs) {
            if before_input == after_input {
                continue;
            }
            let (Some(before_input), Some(after_input)) =
                (before_input.as_str(), after_input.as_str())
            else {
                return false;
            };
            if !is_numeric_unary_test(before_input) || !is_numeric_unary_test(after_input) {
                return false;
            }
            threshold_changed = true;
        }
    }
    threshold_changed
}

fn risk_finding(change: &VersionDiffEntry) -> Option<VersionRiskFinding> {
    let path = change.path.to_ascii_lowercase();
    let kind = change.element_type.to_ascii_lowercase();
    let (level, code, message) = if change.change_type == "remove" {
        (
            "high",
            "ELEMENT_REMOVED",
            "删除模型元素可能影响现有调用方和决策依赖",
        )
    } else if kind == "model" && (path == "name" || path == "namespace") {
        (
            "high",
            "MODEL_IDENTITY_CHANGED",
            "模型名称或命名空间变化可能破坏外部集成",
        )
    } else if is_pure_numeric_threshold_change(change) {
        (
            "low",
            "NUMERIC_THRESHOLD_CHANGED",
            "仅调整决策表数值阈值，表结构、输出和决策契约保持不变",
        )
    } else if path.contains("logic")
        || path.contains("rules")
        || path.contains("hitpolicy")
        || path.contains("defaultoutput")
        || (path.is_empty()
            && matches!(kind.as_str(), "decision" | "bkm")
            && change.before.get("logic") != change.after.get("logic"))
    {
        (
            "high",
            "DECISION_LOGIC_CHANGED",
            "决策逻辑、命中策略或默认结果发生变化",
        )
    } else if path.contains("typeref") || kind == "itemdefinition" {
        (
            "medium",
            "TYPE_CONTRACT_CHANGED",
            "数据类型变化可能影响输入输出契约",
        )
    } else if path.contains("required") || kind == "decisionservice" || kind == "import" {
        ("medium", "DEPENDENCY_CHANGED", "依赖关系或服务边界发生变化")
    } else if change.change_type == "add" && matches!(kind.as_str(), "decision" | "bkm") {
        (
            "medium",
            "EXECUTABLE_ELEMENT_ADDED",
            "新增可执行决策元素，需要确认其依赖和输出",
        )
    } else if change.change_type == "update" {
        ("low", "ELEMENT_UPDATED", "模型元素属性发生变化")
    } else {
        return None;
    };

    Some(VersionRiskFinding {
        level: level.into(),
        code: code.into(),
        message: message.into(),
        element: change.element.clone(),
    })
}

fn risk_rank(level: &str) -> u8 {
    match level {
        "high" => 3,
        "medium" => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nl_dsl::parse_definitions;
    use serde_json::json;

    fn numeric_threshold_change() -> VersionDiffEntry {
        VersionDiffEntry {
            change_type: "update".into(),
            element_type: "decision".into(),
            element: "Eligibility".into(),
            path: String::new(),
            before: json!({
                "name": "Eligibility",
                "typeRef": "string",
                "requiredInputs": ["Age"],
                "requiredDecisions": [],
                "requiredKnowledge": [],
                "logic": {
                    "type": "decisionTable",
                    "hitPolicy": "unique",
                    "inputs": [{"expression": "Age", "typeRef": "number"}],
                    "outputs": [{"name": "result", "typeRef": "string"}],
                    "rules": [{
                        "inputEntries": [">= 18"],
                        "outputEntries": ["\"Eligible\""],
                        "annotation": "Adult"
                    }]
                }
            }),
            after: json!({
                "name": "Eligibility",
                "typeRef": "string",
                "requiredInputs": ["Age"],
                "requiredDecisions": [],
                "requiredKnowledge": [],
                "logic": {
                    "type": "decisionTable",
                    "hitPolicy": "unique",
                    "inputs": [{"expression": "Age", "typeRef": "number"}],
                    "outputs": [{"name": "result", "typeRef": "string"}],
                    "rules": [{
                        "inputEntries": [">= 21"],
                        "outputEntries": ["\"Eligible\""],
                        "annotation": "Adult"
                    }]
                }
            }),
            reason: String::new(),
        }
    }

    #[test]
    fn numeric_threshold_only_rule_change_is_low_risk() {
        let finding = risk_finding(&numeric_threshold_change()).expect("risk finding");
        assert_eq!(finding.level, "low");
        assert_eq!(finding.code, "NUMERIC_THRESHOLD_CHANGED");
        assert!(is_numeric_unary_test("<= 12.5"));
        assert!(is_numeric_unary_test("-3"));
        assert!(!is_numeric_unary_test("Age >= 18"));
        assert!(!is_numeric_unary_test("-"));
    }

    #[test]
    fn threshold_change_stays_high_risk_when_outputs_or_contracts_change() {
        let mut output_changed = numeric_threshold_change();
        output_changed.after["logic"]["rules"][0]["outputEntries"][0] = json!("\"Ineligible\"");
        assert_eq!(risk_finding(&output_changed).unwrap().level, "high");

        let mut contract_changed = numeric_threshold_change();
        contract_changed.after["requiredInputs"] = json!(["Age", "Region"]);
        assert_eq!(risk_finding(&contract_changed).unwrap().level, "high");

        let mut non_numeric = numeric_threshold_change();
        non_numeric.after["logic"]["rules"][0]["inputEntries"][0] = json!("Age >= minimum_age");
        assert_eq!(risk_finding(&non_numeric).unwrap().level, "high");
    }

    fn model() -> Definitions {
        parse_definitions(
            "name: Approval\ninputs:\n  - name: Age\n    typeRef: number\ndecisions: []\n",
        )
        .unwrap()
    }

    #[test]
    fn rule_updates_are_high_risk() {
        let updated = parse_definitions(
            "name: Approval\ninputs:\n  - name: Age\n    typeRef: number\ndecisions:\n  - name: Approval\n    typeRef: string\n    logic:\n      type: decisionTable\n      inputs:\n        - expression: Age\n      outputs:\n        - name: result\n          typeRef: string\n      rules:\n        - inputEntries: [\"> 21\"]\n          outputEntries: ['\"Approve\"']\n",
        )
        .unwrap();
        let review = build_change_review(Some(&model()), &updated, None, Some(1), Some(2));
        assert_eq!(review.overall_risk, "high");
        assert_eq!(review.changes.len(), 1);
    }

    #[test]
    fn initial_model_lists_created_elements() {
        let review = build_change_review(None, &model(), None, None, Some(1));
        assert_eq!(review.overall_risk, "medium");
        assert!(review.changes.iter().any(|change| change.element == "Age"));
    }

    #[test]
    fn structural_diff_reports_added_elements() {
        let base = model();
        let updated = parse_definitions(
            "name: Approval\ninputs:\n  - name: Age\n    typeRef: number\n  - name: Income\n    typeRef: number\ndecisions: []\n",
        )
        .unwrap();
        let review = build_change_review(Some(&base), &updated, None, Some(1), Some(2));
        assert_eq!(review.changes.len(), 1);
        assert_eq!(review.changes[0].element, "Income");
        assert_eq!(review.changes[0].change_type, "add");
    }
}
