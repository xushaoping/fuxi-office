use std::collections::{BTreeSet, HashMap};

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::dmn::{
    self, DecisionLogic, DecisionService, DmnModel, DmnResultStatus, HitPolicy, ItemDefinition,
};
use crate::feel::FeelValue;
use crate::models::{
    VerificationFinding, VerificationGate, VerificationTestCase, VersionChangeReview,
    VersionVerificationReport,
};

const MAX_BOUNDARY_CASES: usize = 24;

pub fn verify_version(
    xml: &str,
    version_id: i64,
    review: Option<&VersionChangeReview>,
    reviewed: bool,
) -> Result<VersionVerificationReport, String> {
    let model = dmn::parse_dmn(xml).map_err(|error| format!("DMN XML 解析失败: {error}"))?;
    dmn::compile_dmn(&model).map_err(|error| format!("DMN 模型编译失败: {error}"))?;

    let baseline = input_template(&model)?;
    let mut cases = vec![run_case(
        &model,
        "基础执行",
        "baseline",
        Value::Object(baseline.clone()),
        true,
    )];

    for input in &model.input_data {
        let key = input.variable.as_deref().unwrap_or(&input.name);
        let mut missing = baseline.clone();
        missing.remove(key);
        cases.push(run_case(
            &model,
            &format!("缺失输入：{key}"),
            "contract",
            Value::Object(missing),
            false,
        ));

        if let Some(invalid) = incompatible_value(&model, input.type_ref.as_deref()) {
            let mut wrong_type = baseline.clone();
            wrong_type.insert(key.to_string(), invalid);
            cases.push(run_case(
                &model,
                &format!("错误类型：{key}"),
                "contract",
                Value::Object(wrong_type),
                false,
            ));
        }
    }

    let mut boundary_count = 0;
    for (path, threshold) in numeric_thresholds(&model) {
        if boundary_count >= MAX_BOUNDARY_CASES {
            break;
        }
        let delta = (threshold.abs() * 0.000_001).max(0.000_001);
        for (label, value) in [
            ("下界", threshold - delta),
            ("等值", threshold),
            ("上界", threshold + delta),
        ] {
            if boundary_count >= MAX_BOUNDARY_CASES {
                break;
            }
            let mut input = Value::Object(baseline.clone());
            if set_path_value(&mut input, &path, json!(value)) {
                cases.push(run_case(
                    &model,
                    &format!("阈值{label}：{path} = {value}"),
                    "boundary",
                    input,
                    true,
                ));
                boundary_count += 1;
            }
        }
    }

    let total = cases.len();
    let passed = cases.iter().filter(|case| case.status == "passed").count();
    let failed = total - passed;
    let baseline_passed = cases
        .iter()
        .find(|case| case.category == "baseline")
        .is_some_and(|case| case.status == "passed");
    let contract_cases = cases
        .iter()
        .filter(|case| case.category == "contract")
        .collect::<Vec<_>>();
    let contract_passed = contract_cases.iter().all(|case| case.status == "passed");
    let boundary_cases = cases
        .iter()
        .filter(|case| case.category == "boundary")
        .collect::<Vec<_>>();
    let boundaries_passed = boundary_cases.iter().all(|case| case.status == "passed");
    let boundary_failure_details = boundary_cases
        .iter()
        .filter(|case| case.status != "passed")
        .take(3)
        .map(|case| format!("{}：{}", case.name, case.actual))
        .collect::<Vec<_>>();
    let boundary_failure_message = if boundary_failure_details.is_empty() {
        String::new()
    } else {
        let remaining = boundary_cases
            .iter()
            .filter(|case| case.status != "passed")
            .count()
            .saturating_sub(boundary_failure_details.len());
        let suffix = if remaining == 0 {
            String::new()
        } else {
            format!("；另有 {remaining} 个失败案例")
        };
        format!(
            "部分阈值边界执行失败：{}{}；请查看失败案例并修正模型后重新验收",
            boundary_failure_details.join("；"),
            suffix
        )
    };
    let high_risk_count = review
        .map(|review| {
            review
                .findings
                .iter()
                .filter(|finding| finding.level == "high")
                .count()
        })
        .unwrap_or(0);
    let high_risk_details = review
        .map(|review| {
            review
                .findings
                .iter()
                .filter(|finding| finding.level == "high")
                .map(|finding| {
                    if finding.element.trim().is_empty() {
                        format!("{}：{}", finding.code, finding.message)
                    } else {
                        format!(
                            "{}（{}）：{}",
                            finding.element, finding.code, finding.message
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join("、")
        })
        .unwrap_or_default();
    let review_passed = review.is_some() && (high_risk_count == 0 || reviewed);
    let findings = analyze_rules(&model);
    let blocking_rule_findings = findings
        .iter()
        .filter(|finding| finding.level == "high")
        .count();
    let coverage_findings = findings
        .iter()
        .filter(|finding| finding.code == "MISSING_COVERAGE")
        .count();

    let change_review_message = if review.is_none() {
        "版本缺少变更审查快照".to_string()
    } else if high_risk_count > 0 && !reviewed {
        if high_risk_details.is_empty() {
            "高风险变更尚未确认".to_string()
        } else {
            format!("高风险变更尚未确认：{high_risk_details}")
        }
    } else if high_risk_count > 0 {
        "高风险变更已确认".to_string()
    } else {
        "没有未确认的高风险变更".to_string()
    };

    let mut gates = vec![
        gate(
            "DMN_COMPILED",
            "模型编译",
            true,
            "DSL、DMN 与 FEEL 已通过确定性编译",
        ),
        gate(
            "BASELINE_EXECUTION",
            "基础执行",
            baseline_passed,
            if baseline_passed {
                "默认样本可以完成决策执行"
            } else {
                "默认样本执行失败，需要补充有效输入或修正规则"
            },
        ),
        gate(
            "INPUT_CONTRACT",
            "输入契约",
            contract_passed,
            if contract_cases.is_empty() {
                "模型没有外部输入"
            } else if contract_passed {
                "缺失输入和错误类型均被模型拒绝"
            } else {
                "部分缺失输入或错误类型未被拒绝"
            },
        ),
        gate(
            "BOUNDARY_EXECUTION",
            "阈值边界",
            boundaries_passed,
            if boundary_cases.is_empty() {
                "未识别到可自动测试的数值阈值"
            } else if boundaries_passed {
                "已验证可识别阈值的下界、等值和上界"
            } else {
                &boundary_failure_message
            },
        ),
        gate(
            "CHANGE_REVIEW",
            "变更审查",
            review_passed,
            &change_review_message,
        ),
    ];
    gates.push(gate(
        "RULE_ANALYSIS",
        "规则冲突与可达性",
        blocking_rule_findings == 0,
        if blocking_rule_findings == 0 {
            "未发现阻塞发布的规则冲突或不可达规则"
        } else {
            "发现规则冲突或不可达规则，需要修改模型"
        },
    ));
    gates.push(VerificationGate {
        code: "RULE_COVERAGE".into(),
        name: "规则覆盖".into(),
        status: if coverage_findings == 0 {
            "passed"
        } else {
            "warning"
        }
        .into(),
        message: if coverage_findings == 0 {
            "所有决策表均提供兜底规则或默认输出"
        } else {
            "部分决策表没有兜底规则或默认输出，请确认无命中场景"
        }
        .into(),
    });
    let status = if failed == 0 && gates.iter().all(|gate| gate.status != "failed") {
        "passed"
    } else {
        "failed"
    };

    Ok(VersionVerificationReport {
        version_id,
        status: status.into(),
        generated_at: chrono::Utc::now(),
        total,
        passed,
        failed,
        boundary_cases: boundary_count,
        gates,
        cases,
        findings,
    })
}

fn gate(code: &str, name: &str, passed: bool, message: &str) -> VerificationGate {
    VerificationGate {
        code: code.into(),
        name: name.into(),
        status: if passed { "passed" } else { "failed" }.into(),
        message: message.into(),
    }
}

fn run_case(
    model: &DmnModel,
    name: &str,
    category: &str,
    input: Value,
    expect_success: bool,
) -> VerificationTestCase {
    let expectation = if expect_success {
        "应成功执行"
    } else {
        "应拒绝无效输入"
    };
    let (succeeded, actual) = match json_inputs_to_feel(model, &input) {
        Ok(inputs) => {
            let result = dmn::DmnExecutor::new(model).evaluate_all(inputs);
            let succeeded = result.status != DmnResultStatus::Failed;
            let actual = result
                .first_error()
                .map(|message| format!("{:?}: {}", result.status, message.message))
                .unwrap_or_else(|| format!("{:?}", result.status));
            (succeeded, actual)
        }
        Err(error) => (false, error),
    };
    VerificationTestCase {
        name: name.into(),
        category: category.into(),
        input,
        expectation: expectation.into(),
        status: if succeeded == expect_success {
            "passed"
        } else {
            "failed"
        }
        .into(),
        actual,
    }
}

fn input_template(model: &DmnModel) -> Result<Map<String, Value>, String> {
    model
        .input_data
        .iter()
        .map(|input| {
            let key = input.variable.clone().unwrap_or_else(|| input.name.clone());
            Ok((
                key,
                sample_value(model, input.type_ref.as_deref(), &mut Vec::new())?,
            ))
        })
        .collect()
}

pub(crate) fn sample_input(model: &DmnModel) -> Result<Value, String> {
    input_template(model).map(Value::Object)
}

pub(crate) fn assign_input_path(root: &mut Value, path: &str, value: Value) -> bool {
    set_path_value(root, path, value)
}

fn sample_value(
    model: &DmnModel,
    type_ref: Option<&str>,
    visiting: &mut Vec<String>,
) -> Result<Value, String> {
    let Some(type_ref) = type_ref.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(json!("sample"));
    };
    let builtin = normalize_type(type_ref);
    match builtin {
        // Zero is a valid FEEL number, but it is a poor automatic sample:
        // generated models commonly use an input as a denominator. Use a
        // neutral non-zero value so baseline verification can exercise the
        // model without introducing an artificial division-by-zero failure.
        "number" => return Ok(json!(1)),
        "boolean" => return Ok(json!(false)),
        "string" | "Any" | "any" => return Ok(json!("sample")),
        "date" => return Ok(json!("2024-01-01")),
        "time" => return Ok(json!("12:00:00")),
        "date and time" | "dateTime" => return Ok(json!("2024-01-01T12:00:00")),
        "yearMonthDuration" | "years and months duration" => return Ok(json!("P1M")),
        "duration" | "dayTimeDuration" | "days and time duration" => return Ok(json!("P1D")),
        "list" => return Ok(json!([])),
        _ => {}
    }
    let item = model
        .item_definitions
        .iter()
        .find(|item| item.name == type_ref)
        .ok_or_else(|| format!("input typeRef '{type_ref}' 未在模型中定义"))?;
    sample_item(model, item, visiting)
}

fn sample_item(
    model: &DmnModel,
    item: &ItemDefinition,
    visiting: &mut Vec<String>,
) -> Result<Value, String> {
    if visiting.contains(&item.name) {
        return Err(format!("数据类型 '{}' 存在循环引用", item.name));
    }
    visiting.push(item.name.clone());
    let mut value = if item.is_collection {
        Value::Array(Vec::new())
    } else if !item.item_components.is_empty() {
        Value::Object(
            item.item_components
                .iter()
                .map(|field| Ok((field.name.clone(), sample_item(model, field, visiting)?)))
                .collect::<Result<Map<_, _>, String>>()?,
        )
    } else if let Some(sample) = item.allowed_values.as_deref().and_then(first_allowed_value) {
        sample
    } else {
        sample_value(model, item.type_ref.as_deref(), visiting)?
    };
    visiting.pop();
    if item.is_collection && !item.item_components.is_empty() {
        value = Value::Array(Vec::new());
    }
    Ok(value)
}

fn first_allowed_value(values: &str) -> Option<Value> {
    let quoted = Regex::new(r#"\"([^\"]+)\""#).ok()?;
    if let Some(value) = quoted.captures(values).and_then(|captures| captures.get(1)) {
        return Some(json!(value.as_str()));
    }
    let scalar = values.split(',').next()?.trim();
    if scalar.eq_ignore_ascii_case("true") {
        Some(json!(true))
    } else if scalar.eq_ignore_ascii_case("false") {
        Some(json!(false))
    } else {
        scalar.parse::<f64>().ok().map(|value| json!(value))
    }
}

fn incompatible_value(model: &DmnModel, type_ref: Option<&str>) -> Option<Value> {
    let type_ref = type_ref?.trim();
    match normalize_type(type_ref) {
        "number" | "boolean" | "date" | "time" | "date and time" | "dateTime" | "duration"
        | "yearMonthDuration" | "dayTimeDuration" => Some(json!({ "invalid": true })),
        "string" => Some(json!(12345)),
        "list" => Some(json!("not-a-list")),
        "Any" | "any" => None,
        _ => model
            .item_definitions
            .iter()
            .find(|item| item.name == type_ref)
            .map(|item| {
                if item.is_collection || !item.item_components.is_empty() {
                    json!("not-an-object")
                } else {
                    incompatible_value(model, item.type_ref.as_deref())
                        .unwrap_or_else(|| json!({ "invalid": true }))
                }
            }),
    }
}

fn normalize_type(type_ref: &str) -> &str {
    type_ref
        .strip_prefix("feel:")
        .or_else(|| type_ref.strip_prefix("feel."))
        .unwrap_or(type_ref)
}

#[derive(Clone, Debug)]
enum RuleCondition {
    Any,
    Number(NumericInterval),
    Exact(String),
}

#[derive(Clone, Copy, Debug)]
struct NumericBound {
    value: f64,
    inclusive: bool,
}

#[derive(Clone, Copy, Debug)]
struct NumericInterval {
    lower: Option<NumericBound>,
    upper: Option<NumericBound>,
}

fn analyze_rules(model: &DmnModel) -> Vec<VerificationFinding> {
    let mut findings = Vec::new();
    for decision in &model.decisions {
        let DecisionLogic::DecisionTable(table) = &decision.logic else {
            continue;
        };
        let conditions = table
            .rules
            .iter()
            .map(|rule| {
                rule.input_entries
                    .iter()
                    .map(|entry| parse_rule_condition(entry))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        match table.hit_policy {
            HitPolicy::Unique => {
                for earlier in 0..table.rules.len() {
                    for later in earlier + 1..table.rules.len() {
                        if rules_overlap(&conditions[earlier], &conditions[later]) {
                            findings.push(VerificationFinding {
                                level: "high".into(),
                                code: "UNIQUE_RULE_CONFLICT".into(),
                                message: "UNIQUE 决策表存在可能同时命中的规则".into(),
                                element: decision.name.clone(),
                                rule_ids: vec![
                                    rule_id(table.rules[earlier].id.as_deref(), earlier),
                                    rule_id(table.rules[later].id.as_deref(), later),
                                ],
                            });
                        }
                    }
                }
            }
            HitPolicy::Any => {
                for earlier in 0..table.rules.len() {
                    for later in earlier + 1..table.rules.len() {
                        if rules_overlap(&conditions[earlier], &conditions[later])
                            && normalized_outputs(&table.rules[earlier].output_entries)
                                != normalized_outputs(&table.rules[later].output_entries)
                        {
                            findings.push(VerificationFinding {
                                level: "high".into(),
                                code: "ANY_OUTPUT_CONFLICT".into(),
                                message: "ANY 决策表的重叠规则返回不同结果".into(),
                                element: decision.name.clone(),
                                rule_ids: vec![
                                    rule_id(table.rules[earlier].id.as_deref(), earlier),
                                    rule_id(table.rules[later].id.as_deref(), later),
                                ],
                            });
                        }
                    }
                }
            }
            HitPolicy::First => {
                for later in 1..table.rules.len() {
                    if let Some(earlier) = (0..later)
                        .find(|earlier| rule_subsumes(&conditions[*earlier], &conditions[later]))
                    {
                        findings.push(VerificationFinding {
                            level: "high".into(),
                            code: "UNREACHABLE_RULE".into(),
                            message: "FIRST 决策表中的后续规则被更早规则完全覆盖".into(),
                            element: decision.name.clone(),
                            rule_ids: vec![
                                rule_id(table.rules[earlier].id.as_deref(), earlier),
                                rule_id(table.rules[later].id.as_deref(), later),
                            ],
                        });
                    }
                }
            }
            _ => {}
        }

        let has_catch_all = conditions.iter().any(|rule| {
            rule.iter()
                .all(|condition| matches!(condition, RuleCondition::Any))
        });
        let has_default_outputs = !table.outputs.is_empty()
            && table.outputs.iter().all(|output| {
                output
                    .default_output_entry
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
            });
        if !has_catch_all && !has_default_outputs {
            findings.push(VerificationFinding {
                level: "medium".into(),
                code: "MISSING_COVERAGE".into(),
                message: "决策表没有全匹配兜底规则或完整默认输出，可能返回空结果".into(),
                element: decision.name.clone(),
                rule_ids: Vec::new(),
            });
        }
    }
    findings
}

fn rule_id(id: Option<&str>, index: usize) -> String {
    id.map(str::to_string)
        .unwrap_or_else(|| format!("rule-{}", index + 1))
}

fn normalized_outputs(outputs: &[String]) -> Vec<String> {
    outputs
        .iter()
        .map(|output| output.trim().to_string())
        .collect()
}

fn parse_rule_condition(input: &str) -> RuleCondition {
    let input = input.trim();
    if input.is_empty() || input == "-" {
        return RuleCondition::Any;
    }
    if input.contains(',') || input.starts_with("not(") {
        return RuleCondition::Exact(input.to_string());
    }

    static COMPARISON: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let comparison = COMPARISON.get_or_init(|| {
        Regex::new(r"^(>=|>|<=|<|=)\s*(-?\d+(?:\.\d+)?)$").expect("valid comparison regex")
    });
    if let Some(captures) = comparison.captures(input) {
        let Ok(value) = captures[2].parse::<f64>() else {
            return RuleCondition::Exact(input.to_string());
        };
        let interval = match &captures[1] {
            ">" => NumericInterval {
                lower: Some(NumericBound {
                    value,
                    inclusive: false,
                }),
                upper: None,
            },
            ">=" => NumericInterval {
                lower: Some(NumericBound {
                    value,
                    inclusive: true,
                }),
                upper: None,
            },
            "<" => NumericInterval {
                lower: None,
                upper: Some(NumericBound {
                    value,
                    inclusive: false,
                }),
            },
            "<=" => NumericInterval {
                lower: None,
                upper: Some(NumericBound {
                    value,
                    inclusive: true,
                }),
            },
            _ => NumericInterval {
                lower: Some(NumericBound {
                    value,
                    inclusive: true,
                }),
                upper: Some(NumericBound {
                    value,
                    inclusive: true,
                }),
            },
        };
        return RuleCondition::Number(interval);
    }
    if let Ok(value) = input.parse::<f64>() {
        return RuleCondition::Number(NumericInterval {
            lower: Some(NumericBound {
                value,
                inclusive: true,
            }),
            upper: Some(NumericBound {
                value,
                inclusive: true,
            }),
        });
    }

    static RANGE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let range = RANGE.get_or_init(|| {
        Regex::new(r"^([\[(])\s*(-?\d+(?:\.\d+)?)\s*\.\.\s*(-?\d+(?:\.\d+)?)\s*([\])])$")
            .expect("valid range regex")
    });
    if let Some(captures) = range.captures(input) {
        if let (Ok(lower), Ok(upper)) = (captures[2].parse(), captures[3].parse()) {
            return RuleCondition::Number(NumericInterval {
                lower: Some(NumericBound {
                    value: lower,
                    inclusive: &captures[1] == "[",
                }),
                upper: Some(NumericBound {
                    value: upper,
                    inclusive: &captures[4] == "]",
                }),
            });
        }
    }
    RuleCondition::Exact(input.to_string())
}

fn rules_overlap(left: &[RuleCondition], right: &[RuleCondition]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| conditions_overlap(left, right))
}

fn rule_subsumes(earlier: &[RuleCondition], later: &[RuleCondition]) -> bool {
    earlier.len() == later.len()
        && earlier
            .iter()
            .zip(later)
            .all(|(earlier, later)| condition_subsumes(earlier, later))
}

fn conditions_overlap(left: &RuleCondition, right: &RuleCondition) -> bool {
    match (left, right) {
        (RuleCondition::Any, _) | (_, RuleCondition::Any) => true,
        (RuleCondition::Exact(left), RuleCondition::Exact(right)) => left == right,
        (RuleCondition::Number(left), RuleCondition::Number(right)) => {
            intervals_overlap(*left, *right)
        }
        _ => false,
    }
}

fn condition_subsumes(earlier: &RuleCondition, later: &RuleCondition) -> bool {
    match (earlier, later) {
        (RuleCondition::Any, _) => true,
        (RuleCondition::Exact(earlier), RuleCondition::Exact(later)) => earlier == later,
        (RuleCondition::Number(earlier), RuleCondition::Number(later)) => {
            interval_contains(*earlier, *later)
        }
        _ => false,
    }
}

fn intervals_overlap(left: NumericInterval, right: NumericInterval) -> bool {
    let lower = maximum_lower(left.lower, right.lower);
    let upper = minimum_upper(left.upper, right.upper);
    match (lower, upper) {
        (Some(lower), Some(upper)) if lower.value > upper.value => false,
        (Some(lower), Some(upper)) if lower.value == upper.value => {
            lower.inclusive && upper.inclusive
        }
        _ => true,
    }
}

fn interval_contains(container: NumericInterval, value: NumericInterval) -> bool {
    lower_contains(container.lower, value.lower) && upper_contains(container.upper, value.upper)
}

fn lower_contains(container: Option<NumericBound>, value: Option<NumericBound>) -> bool {
    match (container, value) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(container), Some(value)) if container.value < value.value => true,
        (Some(container), Some(value)) if container.value == value.value => {
            container.inclusive || !value.inclusive
        }
        _ => false,
    }
}

fn upper_contains(container: Option<NumericBound>, value: Option<NumericBound>) -> bool {
    match (container, value) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(container), Some(value)) if container.value > value.value => true,
        (Some(container), Some(value)) if container.value == value.value => {
            container.inclusive || !value.inclusive
        }
        _ => false,
    }
}

fn maximum_lower(left: Option<NumericBound>, right: Option<NumericBound>) -> Option<NumericBound> {
    match (left, right) {
        (None, value) | (value, None) => value,
        (Some(left), Some(right)) if left.value > right.value => Some(left),
        (Some(left), Some(right)) if right.value > left.value => Some(right),
        (Some(left), Some(right)) => Some(NumericBound {
            value: left.value,
            inclusive: left.inclusive && right.inclusive,
        }),
    }
}

fn minimum_upper(left: Option<NumericBound>, right: Option<NumericBound>) -> Option<NumericBound> {
    match (left, right) {
        (None, value) | (value, None) => value,
        (Some(left), Some(right)) if left.value < right.value => Some(left),
        (Some(left), Some(right)) if right.value < left.value => Some(right),
        (Some(left), Some(right)) => Some(NumericBound {
            value: left.value,
            inclusive: left.inclusive && right.inclusive,
        }),
    }
}

fn numeric_thresholds(model: &DmnModel) -> Vec<(String, f64)> {
    let number = Regex::new(r"(?:>=|<=|>|<)\s*(-?\d+(?:\.\d+)?)").expect("valid regex");
    let range =
        Regex::new(r"[\[(]\s*(-?\d+(?:\.\d+)?)\s*\.\.\s*(-?\d+(?:\.\d+)?)").expect("valid regex");
    let mut thresholds = BTreeSet::new();
    for decision in &model.decisions {
        let DecisionLogic::DecisionTable(table) = &decision.logic else {
            continue;
        };
        for rule in &table.rules {
            for (index, entry) in rule.input_entries.iter().enumerate() {
                let Some(input) = table.inputs.get(index) else {
                    continue;
                };
                for captures in number.captures_iter(entry) {
                    if let Ok(value) = captures[1].parse::<f64>() {
                        thresholds.insert((input.expression.clone(), value.to_bits()));
                    }
                }
                for captures in range.captures_iter(entry) {
                    for capture in [&captures[1], &captures[2]] {
                        if let Ok(value) = capture.parse::<f64>() {
                            thresholds.insert((input.expression.clone(), value.to_bits()));
                        }
                    }
                }
            }
        }
    }
    thresholds
        .into_iter()
        .map(|(path, bits)| (path, f64::from_bits(bits)))
        .collect()
}

fn set_path_value(root: &mut Value, path: &str, value: Value) -> bool {
    let parts = path.split('.').map(str::trim).collect::<Vec<_>>();
    if parts.is_empty()
        || parts.iter().any(|part| {
            part.is_empty() || !part.chars().all(|ch| ch == '_' || ch.is_alphanumeric())
        })
    {
        return false;
    }
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        let Value::Object(fields) = current else {
            return false;
        };
        let Some(next) = fields.get_mut(*part) else {
            return false;
        };
        current = next;
    }
    let Value::Object(fields) = current else {
        return false;
    };
    let Some(target) = fields.get_mut(parts[parts.len() - 1]) else {
        return false;
    };
    *target = value;
    true
}

pub(crate) fn json_inputs_to_feel(
    model: &DmnModel,
    value: &Value,
) -> Result<HashMap<String, FeelValue>, String> {
    let Value::Object(values) = value else {
        return Err("输入必须是 JSON 对象".into());
    };
    values
        .iter()
        .map(|(name, value)| {
            let type_ref = model
                .input_data
                .iter()
                .find(|input| input.variable.as_deref().unwrap_or(&input.name) == name)
                .and_then(|input| input.type_ref.as_deref());
            Ok((
                name.clone(),
                typed_json_to_feel(model, type_ref, value, &mut Vec::new())?,
            ))
        })
        .collect()
}

pub(crate) fn decision_service_inputs_to_feel(
    model: &DmnModel,
    service: &DecisionService,
    value: &Value,
) -> Result<HashMap<String, FeelValue>, String> {
    let Value::Object(values) = value else {
        return Err("服务输入必须是 JSON 对象".into());
    };
    let mut fields = HashMap::<String, Option<&str>>::new();
    for id in &service.input_data {
        let input = model
            .input_data
            .iter()
            .find(|input| input.id == *id)
            .ok_or_else(|| format!("决策服务引用了不存在的输入 {id}"))?;
        fields.insert(
            input.variable.as_deref().unwrap_or(&input.name).to_string(),
            input.type_ref.as_deref(),
        );
    }
    for id in &service.input_decisions {
        let decision = model
            .decisions
            .iter()
            .find(|decision| decision.id == *id)
            .ok_or_else(|| format!("决策服务引用了不存在的输入决策 {id}"))?;
        fields.insert(
            decision
                .variable
                .as_deref()
                .unwrap_or(&decision.name)
                .to_string(),
            decision.type_ref.as_deref(),
        );
    }
    for required in fields.keys() {
        if !values.contains_key(required) {
            return Err(format!("缺少服务输入：{required}"));
        }
    }
    if let Some(unknown) = values.keys().find(|name| !fields.contains_key(*name)) {
        return Err(format!("服务未声明输入：{unknown}"));
    }
    values
        .iter()
        .map(|(name, value)| {
            Ok((
                name.clone(),
                typed_json_to_feel(model, fields[name], value, &mut Vec::new())?,
            ))
        })
        .collect()
}

pub(crate) fn feel_value_to_json(value: &FeelValue) -> Result<Value, String> {
    match value {
        FeelValue::Null => Ok(Value::Null),
        FeelValue::Bool(value) => Ok(Value::Bool(*value)),
        FeelValue::Number(value) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .ok_or_else(|| "FEEL 数值不是有限数".to_string()),
        FeelValue::String(value)
        | FeelValue::Date(value)
        | FeelValue::Time(value)
        | FeelValue::DateTime(value)
        | FeelValue::Duration(value) => Ok(Value::String(value.clone())),
        FeelValue::List(values) => values
            .iter()
            .map(feel_value_to_json)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        FeelValue::Context(values) => values
            .iter()
            .map(|(key, value)| Ok((key.clone(), feel_value_to_json(value)?)))
            .collect::<Result<Map<_, _>, String>>()
            .map(Value::Object),
        FeelValue::Range { .. } => value
            .to_feel_string()
            .map(Value::String)
            .map_err(|error| error.to_string()),
        FeelValue::Function(_, _) => Err("函数值不能作为业务验收输出".to_string()),
    }
}

fn typed_json_to_feel(
    model: &DmnModel,
    type_ref: Option<&str>,
    value: &Value,
    visiting: &mut Vec<String>,
) -> Result<FeelValue, String> {
    let Some(type_ref) = type_ref.map(str::trim).filter(|value| !value.is_empty()) else {
        return json_value_to_feel(value);
    };
    match (normalize_type(type_ref), value) {
        // The interactive test form is backed by a JSON textarea, and values
        // entered there often arrive as quoted scalar strings. Normalize only
        // unambiguous representations so declared numeric/boolean inputs stay
        // executable without weakening rejection of incompatible values.
        ("number", Value::String(value)) => {
            if let Ok(number) = value.trim().parse::<f64>()
                && number.is_finite()
            {
                return Ok(FeelValue::Number(number));
            }
        }
        ("boolean", Value::String(value)) if value.trim().eq_ignore_ascii_case("true") => {
            return Ok(FeelValue::Bool(true));
        }
        ("boolean", Value::String(value)) if value.trim().eq_ignore_ascii_case("false") => {
            return Ok(FeelValue::Bool(false));
        }
        ("date", Value::String(value)) => return Ok(FeelValue::Date(value.clone())),
        ("time", Value::String(value)) => return Ok(FeelValue::Time(value.clone())),
        ("date and time" | "dateTime", Value::String(value)) => {
            return Ok(FeelValue::DateTime(value.clone()));
        }
        (
            "duration"
            | "yearMonthDuration"
            | "dayTimeDuration"
            | "years and months duration"
            | "days and time duration",
            Value::String(value),
        ) => return Ok(FeelValue::Duration(value.clone())),
        ("number", Value::Number(_))
        | ("boolean", Value::Bool(_))
        | ("string", Value::String(_))
        | ("list", Value::Array(_))
        | ("Any" | "any", _) => return json_value_to_feel(value),
        _ => {}
    }
    let Some(item) = model
        .item_definitions
        .iter()
        .find(|item| item.name == type_ref)
    else {
        return json_value_to_feel(value);
    };
    if visiting.contains(&item.name) {
        return Err(format!("数据类型 '{}' 存在循环引用", item.name));
    }
    visiting.push(item.name.clone());
    let converted = if item.is_collection {
        let Value::Array(values) = value else {
            visiting.pop();
            return json_value_to_feel(value);
        };
        values
            .iter()
            .map(|value| typed_item_to_feel(model, item, value, visiting))
            .collect::<Result<Vec<_>, _>>()
            .map(FeelValue::List)
    } else {
        typed_item_to_feel(model, item, value, visiting)
    };
    visiting.pop();
    converted
}

fn typed_item_to_feel(
    model: &DmnModel,
    item: &ItemDefinition,
    value: &Value,
    visiting: &mut Vec<String>,
) -> Result<FeelValue, String> {
    if item.item_components.is_empty() {
        return typed_json_to_feel(model, item.type_ref.as_deref(), value, visiting);
    }
    let Value::Object(values) = value else {
        return json_value_to_feel(value);
    };
    item.item_components
        .iter()
        .filter_map(|field| values.get(&field.name).map(|value| (field, value)))
        .map(|(field, value)| {
            Ok((
                field.name.clone(),
                typed_item_to_feel(model, field, value, visiting)?,
            ))
        })
        .collect::<Result<HashMap<_, _>, String>>()
        .map(FeelValue::Context)
}

fn json_value_to_feel(value: &Value) -> Result<FeelValue, String> {
    match value {
        Value::Null => Ok(FeelValue::Null),
        Value::Bool(value) => Ok(FeelValue::Bool(*value)),
        Value::Number(value) => value
            .as_f64()
            .map(FeelValue::Number)
            .ok_or_else(|| "数字超出 FEEL 支持范围".into()),
        Value::String(value) => Ok(FeelValue::String(value.clone())),
        Value::Array(values) => values
            .iter()
            .map(json_value_to_feel)
            .collect::<Result<Vec<_>, _>>()
            .map(FeelValue::List),
        Value::Object(values) => values
            .iter()
            .map(|(name, value)| Ok((name.clone(), json_value_to_feel(value)?)))
            .collect::<Result<HashMap<_, _>, String>>()
            .map(FeelValue::Context),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::VersionRiskFinding;

    const MODEL: &str = r##"
    <definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="model" name="Boundary">
      <inputData id="ageInput" name="Age"><variable name="Age" typeRef="number"/></inputData>
      <decision id="eligibility" name="Eligibility">
        <informationRequirement><requiredInput href="#ageInput"/></informationRequirement>
        <decisionTable id="table" hitPolicy="FIRST">
          <input><inputExpression typeRef="number"><text>Age</text></inputExpression></input>
          <output name="Result" typeRef="string"/>
          <rule id="adult"><inputEntry><text>&gt;= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule>
          <rule id="minor"><inputEntry><text>-</text></inputEntry><outputEntry><text>"minor"</text></outputEntry></rule>
        </decisionTable>
      </decision>
    </definitions>
    "##;

    fn review() -> VersionChangeReview {
        VersionChangeReview {
            confirmation_required: true,
            ..Default::default()
        }
    }

    #[test]
    fn verification_generates_contract_and_three_point_boundary_cases() {
        let report = verify_version(MODEL, 7, Some(&review()), true).unwrap();
        assert_eq!(report.status, "passed");
        assert_eq!(report.boundary_cases, 3);
        assert!(report.cases.iter().any(|case| case.name.contains("等值")));
        assert!(
            report
                .cases
                .iter()
                .any(|case| case.name.contains("错误类型"))
        );
    }

    #[test]
    fn boundary_gate_reports_failed_case_details() {
        let model = r##"
        <definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="boundary-failure" name="BoundaryFailure">
          <inputData id="amount-input" name="Amount"><variable name="Amount" typeRef="number"/></inputData>
          <decision id="ratio" name="Ratio">
            <informationRequirement><requiredInput href="#amount-input"/></informationRequirement>
            <literalExpression><text>Amount / Amount</text></literalExpression>
          </decision>
          <decision id="bucket" name="Bucket">
            <informationRequirement><requiredInput href="#amount-input"/></informationRequirement>
            <decisionTable hitPolicy="FIRST">
              <input><inputExpression typeRef="number"><text>Amount</text></inputExpression></input>
              <output name="Bucket" typeRef="string"/>
              <rule id="positive"><inputEntry><text>&gt;= 0</text></inputEntry><outputEntry><text>"positive"</text></outputEntry></rule>
              <rule id="negative"><inputEntry><text>-</text></inputEntry><outputEntry><text>"negative"</text></outputEntry></rule>
            </decisionTable>
          </decision>
        </definitions>
        "##;
        let report = verify_version(model, 9, Some(&review()), true).unwrap();
        let gate = report
            .gates
            .iter()
            .find(|gate| gate.code == "BOUNDARY_EXECUTION")
            .expect("boundary gate");
        assert_eq!(gate.status, "failed");
        assert!(gate.message.contains("阈值等值：Amount = 0"));
        assert!(gate.message.contains("Division by zero"));
        assert!(
            report
                .cases
                .iter()
                .any(|case| case.category == "boundary" && case.status == "failed")
        );
    }

    #[test]
    fn verification_blocks_missing_review_snapshot() {
        let report = verify_version(MODEL, 7, None, false).unwrap();
        assert_eq!(report.status, "failed");
        assert!(
            report
                .gates
                .iter()
                .any(|gate| gate.code == "CHANGE_REVIEW" && gate.status == "failed")
        );
    }

    #[test]
    fn verification_identifies_unconfirmed_high_risk_change() {
        let review = VersionChangeReview {
            findings: vec![VersionRiskFinding {
                level: "high".into(),
                code: "DECISION_LOGIC_CHANGED".into(),
                message: "决策逻辑发生变化".into(),
                element: "授信决策".into(),
            }],
            ..review()
        };
        let report = verify_version(MODEL, 7, Some(&review), false).unwrap();
        let gate = report
            .gates
            .iter()
            .find(|gate| gate.code == "CHANGE_REVIEW")
            .expect("change review gate");
        assert_eq!(gate.status, "failed");
        assert_eq!(
            gate.message,
            "高风险变更尚未确认：授信决策（DECISION_LOGIC_CHANGED）：决策逻辑发生变化"
        );
    }

    #[test]
    fn automatic_samples_keep_ratio_decisions_runnable() {
        let model = dmn::parse_dmn(
            r##"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="ratio-model" name="Ratio">
                <inputData id="cost-input" name="TotalCost"><variable name="TotalCost" typeRef="number"/></inputData>
                <inputData id="amount-input" name="TotalAmount"><variable name="TotalAmount" typeRef="number"/></inputData>
                <decision id="cost-ratio" name="CostRatio">
                    <informationRequirement><requiredInput href="#cost-input"/></informationRequirement>
                    <informationRequirement><requiredInput href="#amount-input"/></informationRequirement>
                    <literalExpression><text>TotalCost / TotalAmount</text></literalExpression>
                </decision>
                <decision id="ratio-band" name="RatioBand">
                    <informationRequirement><requiredDecision href="#cost-ratio"/></informationRequirement>
                    <decisionTable hitPolicy="FIRST">
                        <input><inputExpression typeRef="number"><text>CostRatio</text></inputExpression></input>
                        <output name="RatioBand" typeRef="string"/>
                        <rule id="safe"><inputEntry><text>&gt;= 1</text></inputEntry><outputEntry><text>"safe"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
        )
        .expect("ratio model should parse");
        dmn::compile_dmn(&model).expect("ratio model should compile");

        let sample = sample_input(&model).expect("sample input should be generated");
        assert_eq!(sample["TotalCost"], json!(1));
        assert_eq!(sample["TotalAmount"], json!(1));
        let inputs = json_inputs_to_feel(&model, &sample).expect("sample input should be typed");
        let result = dmn::DmnExecutor::new(&model).evaluate_all(inputs);
        assert!(
            !result.has_errors(),
            "unexpected ratio sample errors: {:?}",
            result.messages
        );
        assert_eq!(
            result
                .decision_by_name("CostRatio")
                .expect("ratio decision result")
                .status,
            dmn::DecisionStatus::Succeeded
        );

        // The safety default must not hide a real caller error: explicitly
        // supplied zero denominators still fail with the FEEL runtime error.
        let zero_inputs = json!({ "TotalCost": 1, "TotalAmount": 0 });
        let zero_result = dmn::DmnExecutor::new(&model)
            .evaluate_all(json_inputs_to_feel(&model, &zero_inputs).expect("zero input is typed"));
        assert!(zero_result.has_errors());
        assert!(
            zero_result
                .first_error()
                .is_some_and(|error| error.message.contains("Division by zero"))
        );
    }

    #[test]
    fn typed_input_conversion_preserves_temporal_feel_values() {
        let model = dmn::parse_dmn(
            r#"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="m" name="Temporal"><inputData id="date-input" name="Date"><variable name="Date" typeRef="date"/></inputData></definitions>"#,
        )
        .unwrap();
        let values = json_inputs_to_feel(&model, &json!({ "Date": "2024-01-01" })).unwrap();
        assert!(
            matches!(values.get("Date"), Some(FeelValue::Date(value)) if value == "2024-01-01")
        );
    }

    #[test]
    fn typed_input_conversion_normalizes_scalar_strings_for_form_inputs() {
        let model = dmn::parse_dmn(
            r##"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="m" name="ScalarInputs">
                <inputData id="age-input" name="Age"><variable name="Age" typeRef="number"/></inputData>
                <inputData id="device-input" name="UsesNewDevice"><variable name="UsesNewDevice" typeRef="boolean"/></inputData>
                <decision id="check" name="Check">
                    <informationRequirement><requiredInput href="#age-input"/></informationRequirement>
                    <informationRequirement><requiredInput href="#device-input"/></informationRequirement>
                    <literalExpression><text>true</text></literalExpression>
                </decision>
            </definitions>"##,
        )
        .unwrap();
        let values =
            json_inputs_to_feel(&model, &json!({ "Age": " 35 ", "UsesNewDevice": "false" }))
                .unwrap();
        assert!(matches!(values.get("Age"), Some(FeelValue::Number(value)) if *value == 35.0));
        assert!(matches!(
            values.get("UsesNewDevice"),
            Some(FeelValue::Bool(false))
        ));
    }

    #[test]
    fn typed_input_conversion_keeps_incompatible_scalar_strings_invalid() {
        let model = dmn::parse_dmn(
            r##"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="m" name="InvalidScalarInputs">
                <inputData id="age-input" name="Age"><variable name="Age" typeRef="number"/></inputData>
                <inputData id="device-input" name="UsesNewDevice"><variable name="UsesNewDevice" typeRef="boolean"/></inputData>
                <decision id="check" name="Check">
                    <informationRequirement><requiredInput href="#age-input"/></informationRequirement>
                    <informationRequirement><requiredInput href="#device-input"/></informationRequirement>
                    <literalExpression><text>true</text></literalExpression>
                </decision>
            </definitions>"##,
        )
        .unwrap();
        let values = json_inputs_to_feel(
            &model,
            &json!({ "Age": "thirty-five", "UsesNewDevice": "maybe" }),
        )
        .unwrap();
        assert!(matches!(
            values.get("Age"),
            Some(FeelValue::String(value)) if value == "thirty-five"
        ));
        assert!(matches!(
            values.get("UsesNewDevice"),
            Some(FeelValue::String(value)) if value == "maybe"
        ));
        let result = dmn::DmnExecutor::new(&model).evaluate_all(values);
        assert!(result.has_errors());
        assert_eq!(
            result
                .errors()
                .filter(|message| message.code == "DMN-EVAL-TYPE")
                .count(),
            2
        );
    }

    #[test]
    fn decision_service_input_contract_is_strict_and_typed() {
        let model = dmn::parse_dmn(
            r##"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="m" name="Service"><inputData id="date-input" name="Date"><variable name="Date" typeRef="date"/></inputData><decision id="result" name="Result"><informationRequirement><requiredInput href="#date-input"/></informationRequirement><literalExpression><text>Date</text></literalExpression></decision><decisionService id="service" name="Service"><outputDecision href="#result"/><inputData href="#date-input"/></decisionService></definitions>"##,
        )
        .unwrap();
        let service = &model.decision_services[0];
        let values =
            decision_service_inputs_to_feel(&model, service, &json!({ "Date": "2024-01-01" }))
                .unwrap();
        assert!(
            matches!(values.get("Date"), Some(FeelValue::Date(value)) if value == "2024-01-01")
        );
        assert!(decision_service_inputs_to_feel(&model, service, &json!({})).is_err());
        assert!(
            decision_service_inputs_to_feel(
                &model,
                service,
                &json!({ "Date": "2024-01-01", "Unknown": true })
            )
            .is_err()
        );
    }

    fn table_model(
        hit_policy: &str,
        first: &str,
        first_output: &str,
        second: &str,
        second_output: &str,
    ) -> DmnModel {
        dmn::parse_dmn(&format!(
            r##"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="m" name="Rules">
              <inputData id="score-input" name="Score"><variable name="Score" typeRef="number"/></inputData>
              <decision id="result" name="Result">
                <informationRequirement><requiredInput href="#score-input"/></informationRequirement>
                <decisionTable hitPolicy="{hit_policy}">
                  <input><inputExpression typeRef="number"><text>Score</text></inputExpression></input>
                  <output name="Band" typeRef="string"/>
                  <rule id="r1"><inputEntry><text>{first}</text></inputEntry><outputEntry><text>{first_output}</text></outputEntry></rule>
                  <rule id="r2"><inputEntry><text>{second}</text></inputEntry><outputEntry><text>{second_output}</text></outputEntry></rule>
                </decisionTable>
              </decision>
            </definitions>"##
        ))
        .unwrap()
    }

    #[test]
    fn unique_overlap_is_a_blocking_finding() {
        let model = table_model("UNIQUE", "&gt; 10", "\"A\"", "&gt;= 20", "\"B\"");
        let findings = analyze_rules(&model);
        assert!(
            findings.iter().any(|finding| {
                finding.code == "UNIQUE_RULE_CONFLICT" && finding.level == "high"
            })
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.code == "MISSING_COVERAGE")
        );
    }

    #[test]
    fn any_overlap_with_different_outputs_is_a_blocking_finding() {
        let model = table_model("ANY", "&gt; 10", "\"A\"", "&gt;= 20", "\"B\"");
        assert!(
            analyze_rules(&model)
                .iter()
                .any(|finding| finding.code == "ANY_OUTPUT_CONFLICT")
        );
    }

    #[test]
    fn first_rule_that_subsumes_a_later_rule_is_unreachable() {
        let model = table_model("FIRST", "-", "\"A\"", "&gt;= 20", "\"B\"");
        let findings = analyze_rules(&model);
        assert!(findings.iter().any(|finding| {
            finding.code == "UNREACHABLE_RULE" && finding.rule_ids == ["r1", "r2"]
        }));
        assert!(
            !findings
                .iter()
                .any(|finding| finding.code == "MISSING_COVERAGE")
        );
    }
}
