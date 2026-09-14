use crate::api::{ChangeSummary, EvaluateResult, RunResult};
use crate::components::ChatMessage;
use crate::components::workspace::{GenerationMode, RunStatus, WorkspaceView};
use crate::models::{DecisionOptimizationProposal, DecisionServiceRun, PublishedDecisionService};
use leptos::prelude::*;

pub fn apply_result(
    result: RunResult,
    set_dsl: WriteSignal<Option<String>>,
    set_xml: WriteSignal<Option<String>>,
    set_test_xml: WriteSignal<String>,
    set_test_result: WriteSignal<Option<EvaluateResult>>,
    set_test_error: WriteSignal<Option<String>>,
    set_validation_result: WriteSignal<Option<crate::api::ValidateDmnResult>>,
    set_validation_error: WriteSignal<Option<String>>,
    set_error: WriteSignal<Option<String>>,
    set_messages: WriteSignal<Vec<ChatMessage>>,
    set_status: WriteSignal<RunStatus>,
    set_generation_mode: WriteSignal<GenerationMode>,
    set_source_dsl: WriteSignal<String>,
    set_workspace_view: WriteSignal<WorkspaceView>,
) {
    if result.ok {
        let (Some(dsl), Some(xml)) = (result.dsl.clone(), result.xml.clone()) else {
            set_error.set(Some(
                "生成结果不完整，未返回 DSL 或 DMN XML，模型产物未更新".into(),
            ));
            set_status.set(RunStatus::Failed);
            return;
        };

        // Keep every consumer on the generated model snapshot
        set_dsl.set(Some(dsl.clone()));
        set_xml.set(Some(xml.clone()));
        set_source_dsl.set(dsl.clone());
        set_test_xml.set(xml);
        set_test_result.set(None);
        set_test_error.set(None);
        set_validation_result.set(None);
        set_validation_error.set(None);
        set_workspace_view.set(WorkspaceView::Model);

        let message = result
            .change_summary
            .as_ref()
            .map(|summary| super::formatters::incremental_result_message(summary, dsl.len()))
            .unwrap_or_else(|| format!("模型已生成并通过校验。DSL 共 {} 个字符。", dsl.len()));
        set_messages.update(|items| {
            items.push(ChatMessage {
                role: "assistant",
                content: message,
            })
        });
        set_status.set(RunStatus::Succeeded);
        set_generation_mode.set(GenerationMode::Modify);
    } else {
        set_error.set(
            result
                .error
                .or_else(|| Some("智能体未返回有效结果".to_string())),
        );
        set_status.set(RunStatus::Failed);
    }
}

pub fn agent_activity_for_step(step: &str) -> String {
    if step.contains("等待首段输出") {
        "正在等待模型返回首段内容".to_string()
    } else if step.contains("ChangeIntent") || step.contains("变更意图") {
        "正在生成并校验受控模型变更意图".to_string()
    } else if step.contains("调用 LLM") {
        "正在调用语言模型生成 DSL".to_string()
    } else if step.contains("校验") {
        "正在校验 DSL 与 DMN 结构".to_string()
    } else if step.contains("编译") {
        "正在编译 DMN XML".to_string()
    } else if step.contains("重试") {
        "正在修复生成结果并重新尝试".to_string()
    } else {
        step.trim_start_matches(['→', '✓', '⚠', '…', ' '])
            .to_string()
    }
}

pub fn timeline_item_class(step: &str, is_current: bool) -> &'static str {
    if step.starts_with('✓') {
        "is-complete"
    } else if step.starts_with('⚠') {
        "is-warning"
    } else if step.starts_with('…') {
        "is-waiting"
    } else if is_current {
        "is-active"
    } else {
        "is-past"
    }
}

pub fn timeline_item_text(step: &str) -> String {
    step.trim_start_matches(['→', '✓', '⚠', '…', ' '])
        .to_string()
}

pub fn timeline_entries(steps: Vec<String>) -> Vec<(usize, String)> {
    steps.into_iter().enumerate().collect()
}

pub fn workflow_status_label(status: &str) -> &'static str {
    match status {
        "analyzed" => "已分析",
        "clarification" => "待澄清",
        "planned" => "待确认",
        "confirmed" => "已确认",
        _ => "待分析",
    }
}

pub fn service_sample_value(type_ref: &str) -> serde_json::Value {
    match type_ref
        .strip_prefix("feel:")
        .unwrap_or(type_ref)
        .to_ascii_lowercase()
        .as_str()
    {
        "number" => serde_json::json!(1),
        "boolean" => serde_json::json!(false),
        "list" => serde_json::json!([]),
        _ => serde_json::json!(""),
    }
}

pub fn service_input_template(service: &PublishedDecisionService) -> String {
    serde_json::to_string_pretty(&service_input_values(service)).unwrap_or_else(|_| "{}".into())
}

fn service_input_values(
    service: &PublishedDecisionService,
) -> serde_json::Map<String, serde_json::Value> {
    service
        .inputs
        .iter()
        .map(|field| (field.name.clone(), service_sample_value(&field.type_ref)))
        .collect()
}

fn service_request_template(service: &PublishedDecisionService) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "serviceId": service.id,
        "businessReference": "ORDER-2026-0001",
        "inputs": service_input_values(service),
    }))
    .unwrap_or_else(|_| "{}".into())
}

pub fn recent_service_runs(runs: Vec<DecisionServiceRun>) -> Vec<DecisionServiceRun> {
    runs.into_iter().take(10).collect()
}

pub fn outcome_assessment_label(assessment: Option<&str>) -> &'static str {
    match assessment {
        Some("confirmed") => "符合决策",
        Some("mismatch") => "存在偏差",
        Some("inconclusive") => "无法判断",
        _ => "待回填",
    }
}

pub fn analytics_percent(rate: f64) -> String {
    format!("{:.1}%", rate * 100.0)
}

pub fn analytics_finding_label(code: &str) -> &'static str {
    match code {
        "NO_RECENT_RUNS" => "暂无运行数据",
        "HIGH_FAILURE_RATE" => "失败率偏高",
        "LOW_COVERAGE" => "覆盖率不足",
        "OUTDATED_MODEL" => "模型版本过旧",
        "MISSING_KNOWLEDGE" => "缺少知识绑定",
        _ => "其他发现",
    }
}

pub fn optimization_proposal_actions(proposal: &DecisionOptimizationProposal) -> Vec<String> {
    proposal
        .summary
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect()
}

pub fn optimization_proposal_status_label(status: &str) -> &'static str {
    match status {
        "pending" => "待处理",
        "accepted" => "已采纳",
        "rejected" => "已拒绝",
        _ => "未知",
    }
}

pub fn public_service_endpoint(service: &PublishedDecisionService) -> String {
    let path = format!(
        "/api/public/projects/{}/decision-services/execute",
        service.project_id
    );
    #[cfg(target_family = "wasm")]
    if let Some(origin) = web_sys::window().and_then(|window| window.location().origin().ok()) {
        return format!("{origin}{path}");
    }
    path
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn public_service_curl(service: &PublishedDecisionService, secret: &str) -> String {
    format!(
        r#"curl -X POST {endpoint} \
  -H {key_header} \
  -H {content_type} \
  -d {body}"#,
        endpoint = shell_quote(&public_service_endpoint(service)),
        key_header = shell_quote(&format!("X-Decision-Key: {secret}")),
        content_type = shell_quote("Content-Type: application/json"),
        body = shell_quote(&service_request_template(service)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DecisionServiceField;

    fn service() -> PublishedDecisionService {
        PublishedDecisionService {
            project_id: 42,
            version_id: 7,
            version_number: 3,
            id: "pricing".into(),
            name: "Pricing".into(),
            inputs: vec![
                DecisionServiceField {
                    name: "Amount".into(),
                    type_ref: "number".into(),
                    source: "input_data".into(),
                },
                DecisionServiceField {
                    name: "Approved".into(),
                    type_ref: "boolean".into(),
                    source: "input_data".into(),
                },
            ],
            outputs: Vec::new(),
        }
    }

    #[test]
    fn public_endpoint_matches_public_execution_route() {
        assert_eq!(
            public_service_endpoint(&service()),
            "/api/public/projects/42/decision-services/execute"
        );
    }

    #[test]
    fn generated_curl_matches_public_execution_contract() {
        let command = public_service_curl(&service(), "dmn_live_secret");
        assert!(command.contains("X-Decision-Key: dmn_live_secret"));
        assert!(!command.contains("Authorization: Bearer"));
        assert!(command.contains("\"serviceId\": \"pricing\""));
        assert!(command.contains("\"businessReference\": \"ORDER-2026-0001\""));
        assert!(command.contains("\"inputs\": {"));
    }
}
