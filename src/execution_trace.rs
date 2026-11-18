use serde_json::{Map, Value};

use crate::dmn::{DecisionLogic, DecisionService, DecisionStatus, DmnModel, DmnResult};
use crate::models::{
    DecisionTraceStep, ExecutionTrace, ExecutionTraceMessage, MatchedRuleRequirementTrace,
};
use crate::version_verification::feel_value_to_json;

pub fn build_execution_trace(
    model: &DmnModel,
    version_id: Option<i64>,
    version_number: Option<i32>,
    input: &Value,
    result: &DmnResult,
) -> ExecutionTrace {
    let model_name = model.name.as_deref().unwrap_or("未命名模型");
    let decisions = result
        .decision_results
        .iter()
        .enumerate()
        .map(|(index, evaluated)| {
            let definition = model
                .decisions
                .iter()
                .find(|decision| decision.id == evaluated.decision_id);
            let logic_type = definition
                .map(|decision| logic_type(&decision.logic))
                .unwrap_or("unknown");
            let dependencies = evaluated
                .evaluated_dependencies
                .iter()
                .map(|id| {
                    model
                        .decisions
                        .iter()
                        .find(|decision| decision.id == *id)
                        .map(|decision| decision.name.clone())
                        .unwrap_or_else(|| id.clone())
                })
                .collect::<Vec<_>>();
            let value = feel_value_to_json(&evaluated.value)
                .unwrap_or_else(|error| serde_json::json!({ "serializationError": error }));
            let outcome = trace_outcome(
                definition.map(|decision| &decision.logic),
                evaluated.status,
                &evaluated.matched_rule_ids,
            );
            let explanation = explain_decision(
                &evaluated.decision_name,
                evaluated.status,
                outcome,
                &dependencies,
                &evaluated.matched_rule_ids,
                &value,
            );
            let matched_rule_requirements = definition
                .and_then(|decision| match &decision.logic {
                    DecisionLogic::DecisionTable(table) => Some(table),
                    _ => None,
                })
                .map(|table| {
                    evaluated
                        .matched_rule_ids
                        .iter()
                        .filter_map(|rule_id| {
                            table
                                .rules
                                .iter()
                                .enumerate()
                                .find(|(index, rule)| {
                                    rule.id.as_deref().unwrap_or("") == rule_id
                                        || (rule.id.is_none()
                                            && format!("rule-{}", index + 1) == *rule_id)
                                })
                                .map(|(_, rule)| MatchedRuleRequirementTrace {
                                    rule_id: rule_id.clone(),
                                    requirement_ids: rule.requirement_ids.clone(),
                                })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            DecisionTraceStep {
                order: index + 1,
                decision_id: evaluated.decision_id.clone(),
                decision_name: evaluated.decision_name.clone(),
                logic_type: logic_type.into(),
                status: decision_status(evaluated.status).into(),
                outcome: outcome.into(),
                dependencies,
                matched_rule_ids: evaluated.matched_rule_ids.clone(),
                requirement_ids: definition
                    .map(|decision| decision.requirement_ids.clone())
                    .unwrap_or_default(),
                matched_rule_requirements,
                result: value,
                explanation,
            }
        })
        .collect::<Vec<_>>();

    let output = Value::Object(
        decisions
            .iter()
            .map(|decision| (decision.decision_name.clone(), decision.result.clone()))
            .collect::<Map<_, _>>(),
    );
    let messages = result
        .messages
        .iter()
        .map(|message| ExecutionTraceMessage {
            severity: format!("{:?}", message.severity).to_ascii_lowercase(),
            code: message.code.to_string(),
            message: message.message.clone(),
            decision: message.source.decision_name.clone(),
        })
        .collect::<Vec<_>>();
    let version = version_number
        .map(|number| format!(" v{number}"))
        .unwrap_or_default();
    let explanation = if decisions.is_empty() {
        format!("模型 {model_name}{version} 未产生可用的决策结果。")
    } else {
        format!(
            "模型 {}{version} 按依赖顺序执行了 {} 个决策。{}",
            model_name,
            decisions.len(),
            decisions
                .iter()
                .map(|decision| decision.explanation.as_str())
                .collect::<Vec<_>>()
                .join("；")
        )
    };

    ExecutionTrace {
        model_name: model_name.into(),
        version_id,
        version_number,
        status: format!("{:?}", result.status).to_ascii_lowercase(),
        normalized_input: normalize_json(input),
        output,
        decisions,
        messages,
        explanation,
    }
}

pub fn build_rejected_execution_trace(
    model: &DmnModel,
    version_id: Option<i64>,
    version_number: Option<i32>,
    input: &Value,
    code: &str,
    message: &str,
) -> ExecutionTrace {
    let model_name = model.name.as_deref().unwrap_or("未命名模型");
    ExecutionTrace {
        model_name: model_name.into(),
        version_id,
        version_number,
        status: "failed".into(),
        normalized_input: normalize_json(input),
        output: Value::Object(Map::new()),
        decisions: Vec::new(),
        messages: vec![ExecutionTraceMessage {
            severity: "error".into(),
            code: code.into(),
            message: message.into(),
            decision: None,
        }],
        explanation: format!("模型 {model_name} 未开始决策执行：{message}"),
    }
}

pub fn build_decision_service_trace(
    model: &DmnModel,
    service: &DecisionService,
    version_id: i64,
    version_number: i32,
    input: &Value,
    result: &DmnResult,
) -> ExecutionTrace {
    let mut trace =
        build_execution_trace(model, Some(version_id), Some(version_number), input, result);
    for step in &mut trace.decisions {
        if service.input_decisions.contains(&step.decision_id) {
            step.logic_type = "service_input".into();
            step.outcome = "service_input".into();
            step.explanation = format!(
                "决策 {} 的值由调用方作为决策服务输入提供，值为 {}",
                step.decision_name,
                serde_json::to_string(&step.result).unwrap_or_else(|_| "null".into())
            );
        }
    }
    trace.output = Value::Object(
        service
            .output_decisions
            .iter()
            .filter_map(|id| {
                trace
                    .decisions
                    .iter()
                    .find(|step| step.decision_id == *id)
                    .map(|step| (step.decision_name.clone(), step.result.clone()))
            })
            .collect(),
    );
    trace.explanation = format!(
        "已调用决策服务 {}。{}",
        service.name,
        trace
            .decisions
            .iter()
            .map(|step| step.explanation.as_str())
            .collect::<Vec<_>>()
            .join("；")
    );
    trace
}

fn logic_type(logic: &DecisionLogic) -> &'static str {
    match logic {
        DecisionLogic::DecisionTable(_) => "decision_table",
        DecisionLogic::LiteralExpression(_) => "literal_expression",
        DecisionLogic::Context(_) => "context",
        DecisionLogic::List(_) => "list",
        DecisionLogic::Relation(_) => "relation",
        DecisionLogic::Conditional(_) => "conditional",
        DecisionLogic::Filter(_) => "filter",
        DecisionLogic::For(_) => "for",
        DecisionLogic::Some(_) => "some",
        DecisionLogic::Every(_) => "every",
        DecisionLogic::Invocation(_) => "invocation",
        DecisionLogic::FunctionDefinition(_) => "function",
        DecisionLogic::Unsupported(_) => "unsupported",
        DecisionLogic::None => "none",
    }
}

fn trace_outcome(
    logic: Option<&DecisionLogic>,
    status: DecisionStatus,
    matched_rule_ids: &[String],
) -> &'static str {
    match status {
        DecisionStatus::Failed => "failed",
        DecisionStatus::Skipped => "skipped",
        DecisionStatus::Succeeded => match logic {
            Some(DecisionLogic::DecisionTable(_)) if !matched_rule_ids.is_empty() => "rule_match",
            Some(DecisionLogic::DecisionTable(table))
                if table
                    .outputs
                    .iter()
                    .any(|output| output.default_output_entry.is_some()) =>
            {
                "default_output"
            }
            Some(DecisionLogic::DecisionTable(_)) => "no_match",
            _ => "evaluated",
        },
    }
}

fn decision_status(status: DecisionStatus) -> &'static str {
    match status {
        DecisionStatus::Succeeded => "succeeded",
        DecisionStatus::Failed => "failed",
        DecisionStatus::Skipped => "skipped",
    }
}

fn explain_decision(
    name: &str,
    status: DecisionStatus,
    outcome: &str,
    dependencies: &[String],
    matched_rule_ids: &[String],
    result: &Value,
) -> String {
    let dependency_text = if dependencies.is_empty() {
        String::new()
    } else {
        format!("，依赖 {}", dependencies.join("、"))
    };
    let result_text = serde_json::to_string(result).unwrap_or_else(|_| "null".into());
    match (status, outcome) {
        (DecisionStatus::Succeeded, "rule_match") => format!(
            "决策 {name}{dependency_text}，命中规则 {}，结果为 {result_text}",
            matched_rule_ids.join("、")
        ),
        (DecisionStatus::Succeeded, "default_output") => {
            format!("决策 {name}{dependency_text} 未命中规则，使用默认输出 {result_text}")
        }
        (DecisionStatus::Succeeded, "no_match") => {
            format!("决策 {name}{dependency_text} 未命中规则且没有默认输出，结果为空")
        }
        (DecisionStatus::Succeeded, _) => {
            format!("决策 {name}{dependency_text} 完成表达式求值，结果为 {result_text}")
        }
        (DecisionStatus::Skipped, _) => {
            format!("决策 {name}{dependency_text} 因前置依赖失败而跳过")
        }
        (DecisionStatus::Failed, _) => format!("决策 {name}{dependency_text} 执行失败"),
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
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(normalize_json).collect()),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::dmn::{DMN_NAMESPACE, DmnExecutor, parse_dmn};
    use crate::feel::FeelValue;

    use super::*;

    #[test]
    fn trace_preserves_dependency_order_and_rule_explanation() {
        let model = parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}" xmlns:fuxi="urn:fuxi:dmn:traceability:v1" name="EligibilityModel">
            <inputData id="age" name="age"><variable name="age" typeRef="number"/></inputData>
            <decision id="segment" name="Segment"><informationRequirement><requiredInput href="#age"/></informationRequirement><literalExpression><text>if age >= 18 then "adult" else "minor"</text></literalExpression></decision>
            <decision id="eligibility" name="Eligibility"><extensionElements><fuxi:traceability><fuxi:requirementRef id="req-goal-a1"/></fuxi:traceability></extensionElements><informationRequirement><requiredDecision href="#segment"/></informationRequirement><decisionTable hitPolicy="UNIQUE"><input><inputExpression><text>Segment</text></inputExpression></input><output name="result"/><rule id="adult-rule"><extensionElements><fuxi:traceability><fuxi:requirementRef id="req-rule-b2"/></fuxi:traceability></extensionElements><inputEntry><text>"adult"</text></inputEntry><outputEntry><text>"approved"</text></outputEntry></rule></decisionTable></decision>
            </definitions>"##
        ))
        .unwrap();
        let result = DmnExecutor::new(&model)
            .evaluate_all(HashMap::from([("age".into(), FeelValue::Number(30.0))]));
        let trace = build_execution_trace(
            &model,
            Some(7),
            Some(3),
            &serde_json::json!({ "age": 30 }),
            &result,
        );

        assert_eq!(trace.model_name, "EligibilityModel");
        assert_eq!(trace.version_number, Some(3));
        assert_eq!(trace.decisions.len(), 2);
        assert_eq!(trace.decisions[0].decision_name, "Segment");
        assert_eq!(trace.decisions[1].dependencies, ["Segment"]);
        assert_eq!(trace.decisions[1].outcome, "rule_match");
        assert_eq!(trace.decisions[1].matched_rule_ids, ["adult-rule"]);
        assert_eq!(trace.decisions[1].requirement_ids, ["req-goal-a1"]);
        assert_eq!(
            trace.decisions[1].matched_rule_requirements[0].rule_id,
            "adult-rule"
        );
        assert_eq!(
            trace.decisions[1].matched_rule_requirements[0].requirement_ids,
            ["req-rule-b2"]
        );
        assert!(trace.explanation.contains("命中规则 adult-rule"));
        assert_eq!(trace.output["Eligibility"], "approved");
    }

    #[test]
    fn trace_distinguishes_default_output_and_rejected_input() {
        let model = parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}" name="DefaultModel"><inputData id="age" name="age"><variable name="age"/></inputData><decision id="result" name="Result"><informationRequirement><requiredInput href="#age"/></informationRequirement><decisionTable><input><inputExpression><text>age</text></inputExpression></input><output name="result"><defaultOutputEntry><text>"unknown"</text></defaultOutputEntry></output><rule id="adult"><inputEntry><text>>= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule></decisionTable></decision></definitions>"##
        ))
        .unwrap();
        let result = DmnExecutor::new(&model)
            .evaluate_all(HashMap::from([("age".into(), FeelValue::Number(10.0))]));
        let trace = build_execution_trace(
            &model,
            Some(4),
            Some(2),
            &serde_json::json!({ "age": 10 }),
            &result,
        );
        assert_eq!(trace.decisions[0].outcome, "default_output");
        assert!(trace.decisions[0].explanation.contains("使用默认输出"));

        let rejected = build_rejected_execution_trace(
            &model,
            Some(4),
            Some(2),
            &serde_json::json!({ "age": "invalid" }),
            "INPUT_CONVERSION",
            "age 类型错误",
        );
        assert_eq!(rejected.status, "failed");
        assert!(rejected.decisions.is_empty());
        assert!(rejected.explanation.contains("未开始决策执行"));
    }

    #[test]
    fn decision_service_trace_marks_caller_supplied_decisions() {
        let model = parse_dmn(&format!(
            r##"<definitions xmlns="{DMN_NAMESPACE}" name="ServiceModel"><decision id="external" name="External"><variable name="external" typeRef="number"/><literalExpression><text>100</text></literalExpression></decision><decision id="result" name="Result"><informationRequirement><requiredDecision href="#external"/></informationRequirement><literalExpression><text>external + 1</text></literalExpression></decision><decisionService id="service" name="Published Service"><outputDecision href="#result"/><inputDecision href="#external"/></decisionService></definitions>"##
        ))
        .unwrap();
        let service = &model.decision_services[0];
        let result = DmnExecutor::new(&model).evaluate_decision_service_with_details(
            "service",
            HashMap::from([("external".into(), FeelValue::Number(5.0))]),
        );
        let trace = build_decision_service_trace(
            &model,
            service,
            8,
            2,
            &serde_json::json!({ "external": 5 }),
            &result,
        );
        let input = trace
            .decisions
            .iter()
            .find(|step| step.decision_id == "external")
            .unwrap();
        assert_eq!(input.outcome, "service_input");
        assert!(input.explanation.contains("由调用方"));
        assert_eq!(trace.output["Result"].as_f64(), Some(6.0));
        assert!(trace.output.get("External").is_none());
    }
}
