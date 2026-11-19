use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::ACCEPT},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::{StreamExt as _, wrappers::ReceiverStream};

use crate::agent_runtime::ExperimentAssignment;
use crate::agent_runtime::orchestration::{
    AgentEvent, AgentStatus, AgentTool, Consensus, PipelineStage, VoteCandidate, parallel_vote_by,
    run_pipeline, run_pipeline_stream,
};
use crate::api::{LlmState, LlmTask};
use crate::knowledge::{citations, effective_project_sources, knowledge_context_for_query};
use crate::nl_dsl::agent::{LlmAgent, StreamEvent};
use crate::nl_dsl::analysis_engine::{
    BusinessLogicPattern, DetectedPattern, PatternDefinition, PatternDetector,
};
use crate::project_access::{self, ProjectPermission};
use crate::requirement_complexity::{RequirementComplexity, classify_requirement_complexity};
use crate::types::{AppState, Claims};
use crate::workflow::{
    AnalyzeRequirementRequest, Clarification, ClarificationAnswer, ConfirmDecisionPlanRequest,
    ConfirmRequirementReviewRequest, ContinueClarificationRequest, CreateDecisionPlanRequest,
    DecisionBrief, DecisionField, DecisionPlan, DecisionWorkflowState, DetectedBusinessPattern,
    OrchestrateDecisionWorkflowRequest, PendingRequirementReview, RequirementAnalysis,
    RequirementItem, RequirementReviewCandidate, SemanticTestProposal, TesterReport,
};
use anyhow::anyhow;

pub(crate) struct WorkflowApiError {
    status: StatusCode,
    message: String,
    orchestration_id: Option<i64>,
    retryable: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct AgentRunHistoryItem {
    id: i64,
    stage: String,
    status: String,
    model: Option<String>,
    prompt_version: Option<String>,
    retry_count: i32,
    usage: Option<Value>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct AgentOrchestrationRow {
    id: i64,
    parent_orchestration_id: Option<i64>,
    retry_of_orchestration_id: Option<i64>,
    trigger_kind: String,
    status: String,
    current_stage: String,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct AgentOrchestrationStep {
    id: i64,
    orchestration_id: i64,
    step_no: i32,
    attempt_no: i32,
    agent_role: String,
    task_kind: String,
    status: String,
    model: Option<String>,
    prompt_version: Option<String>,
    artifact_kind: Option<String>,
    artifact_schema_version: i16,
    artifact_digest: Option<String>,
    usage: Option<Value>,
    error: Option<Value>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct AgentOrchestrationEvent {
    id: i64,
    event_name: String,
    payload: Value,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentOrchestrationHistoryItem {
    #[serde(flatten)]
    orchestration: AgentOrchestrationRow,
    steps: Vec<AgentOrchestrationStep>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentOrchestrationHistoryQuery {
    before_id: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentOrchestrationEventQuery {
    after_event_id: Option<i64>,
    limit: Option<i64>,
}

fn validate_agent_orchestration_event_query(
    query: AgentOrchestrationEventQuery,
) -> Result<(i64, i64), String> {
    if query.after_event_id.is_some_and(|id| id < 0) {
        return Err("afterEventId 不能为负数".into());
    }
    let limit = query.limit.unwrap_or(500);
    if !(1..=1000).contains(&limit) {
        return Err("limit 必须在 1 到 1000 之间".into());
    }
    Ok((query.after_event_id.unwrap_or(0), limit))
}

fn validate_agent_orchestration_history_query(
    query: AgentOrchestrationHistoryQuery,
) -> Result<(Option<i64>, i64), String> {
    if query.before_id.is_some_and(|id| id <= 0) {
        return Err("beforeId 必须为正整数".into());
    }
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err("limit 必须在 1 到 100 之间".into());
    }
    Ok((query.before_id, limit))
}

struct PreparedPlanRun {
    project_id: i64,
    username: String,
    workflow_updated_at: DateTime<Utc>,
    brief: DecisionBrief,
    answers: Vec<ClarificationAnswer>,
    complexity: RequirementComplexity,
    knowledge_text: String,
    model_context: Option<String>,
    agent: LlmAgent,
    run_id: i64,
    orchestration_id: i64,
}

struct PreparedFacilitatorRun {
    state: AppState,
    project_id: i64,
    username: String,
    workflow: DecisionWorkflowState,
    answers: Vec<ClarificationAnswer>,
    base_revision: Option<i64>,
    existing_dsl: Option<String>,
    knowledge_text: String,
    risk_level: String,
    complexity: RequirementComplexity,
    agent: LlmAgent,
    run_id: i64,
    orchestration_id: i64,
}

impl WorkflowApiError {
    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
            orchestration_id: None,
            retryable: false,
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            orchestration_id: None,
            retryable: false,
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "决策项目或需求工作流不存在".to_string(),
            orchestration_id: None,
            retryable: false,
        }
    }

    fn unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "未配置可用的大模型，无法执行智能工作流".to_string(),
            orchestration_id: None,
            retryable: false,
        }
    }

    fn token_budget(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: message.into(),
            orchestration_id: None,
            retryable: false,
        }
    }

    fn model(operation: &str, error: anyhow::Error) -> Self {
        eprintln!("智能工作流失败 [{operation}]: {error}");
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: format!("{operation}失败：{error}"),
            orchestration_id: None,
            retryable: false,
        }
    }

    fn database(operation: &str, error: sqlx::Error) -> Self {
        eprintln!("数据库操作失败 [{operation}]: {error}");
        let production = std::env::var("APP_ENV")
            .or_else(|_| std::env::var("RUST_ENV"))
            .or_else(|_| std::env::var("LEPTOS_ENV"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: if matches!(production.as_str(), "production" | "prod") {
                format!("{operation}失败，请稍后重试")
            } else {
                format!("{operation}失败：{error}")
            },
            orchestration_id: None,
            retryable: false,
        }
    }

    fn with_orchestration(mut self, orchestration_id: i64) -> Self {
        self.retryable = is_retryable_agent_failure(&self.message);
        self.orchestration_id = Some(orchestration_id);
        self
    }
}

impl IntoResponse for WorkflowApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "ok": false, "error": self.message });
        if let Some(orchestration_id) = self.orchestration_id {
            body["orchestrationId"] = json!(orchestration_id);
            body["retryable"] = json!(self.retryable);
        }
        (self.status, Json(body)).into_response()
    }
}

#[derive(sqlx::FromRow)]
struct WorkflowRow {
    project_id: i64,
    base_revision: Option<i64>,
    status: String,
    requirement_text: String,
    original_requirement_text: Option<String>,
    decision_brief_json: Value,
    clarifications_json: Value,
    answers_json: Value,
    decision_plan_json: Option<Value>,
    tester_report_json: Option<Value>,
    confirmed_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

impl WorkflowRow {
    fn into_state(self) -> Result<DecisionWorkflowState, WorkflowApiError> {
        let mut state = DecisionWorkflowState {
            project_id: self.project_id,
            simple_direct_eligible: false,
            base_revision: self.base_revision,
            status: self.status,
            requirement_text: self.requirement_text,
            original_requirement_text: self.original_requirement_text,
            brief: parse_saved(self.decision_brief_json, "需求摘要")?,
            clarifications: parse_saved(self.clarifications_json, "澄清问题")?,
            answers: parse_saved(self.answers_json, "澄清回答")?,
            plan: self
                .decision_plan_json
                .map(|value| parse_saved(value, "决策方案"))
                .transpose()?,
            tester_report: self
                .tester_report_json
                .map(|value| parse_saved(value, "测试提案报告"))
                .transpose()?,
            confirmed_at: self.confirmed_at,
            updated_at: self.updated_at,
        };
        state.simple_direct_eligible = simple_direct_route_eligible(&state);
        Ok(state)
    }
}

fn parse_saved<T: serde::de::DeserializeOwned>(
    value: Value,
    label: &str,
) -> Result<T, WorkflowApiError> {
    serde_json::from_value(value)
        .map_err(|error| WorkflowApiError::bad_request(format!("已保存的{label}无效：{error}")))
}

fn validate_analysis(analysis: &RequirementAnalysis) -> Result<(), WorkflowApiError> {
    if analysis.brief.name.trim().is_empty() || analysis.brief.goal.trim().is_empty() {
        return Err(WorkflowApiError::bad_request(
            "需求分析缺少模型名称或业务目标",
        ));
    }
    if !matches!(
        analysis.brief.risk_level.as_str(),
        "low" | "medium" | "high"
    ) {
        return Err(WorkflowApiError::bad_request("需求风险等级无效"));
    }
    if analysis
        .brief
        .inputs
        .iter()
        .chain(analysis.brief.outputs.iter())
        .any(|field| field.name.trim().is_empty() || field.type_ref.trim().is_empty())
    {
        return Err(WorkflowApiError::bad_request(
            "需求输入输出必须包含名称和类型",
        ));
    }
    let mut ids = HashSet::new();
    if analysis.clarifications.iter().any(|item| {
        item.id.trim().is_empty()
            || item.question.trim().is_empty()
            || !ids.insert(item.id.as_str())
    }) {
        return Err(WorkflowApiError::bad_request("澄清问题包含空白或重复标识"));
    }
    Ok(())
}

const MAX_CLARIFICATIONS_PER_ROUND: usize = 3;
const FACILITATOR_INSTRUCTIONS: &str = "没有回答的可选问题代表用户跳过，不得重新询问。请作为澄清主持人，仅提出仍会影响业务判断且尚未被回答的新歧义。每轮最多提出 3 个问题；若已收敛，请返回空 clarifications。";

fn normalize_clarification_question(question: &str) -> String {
    question
        .chars()
        .filter(|character| {
            !character.is_whitespace() && !matches!(character, '。' | '？' | '?' | '！' | '!')
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn next_clarification_round(
    candidates: Vec<Clarification>,
    existing: &[Clarification],
    round: u8,
) -> Vec<Clarification> {
    let mut known_ids = existing
        .iter()
        .map(|item| item.id.clone())
        .collect::<HashSet<_>>();
    let known_questions = existing
        .iter()
        .map(|item| normalize_clarification_question(&item.question))
        .collect::<HashSet<_>>();
    let mut seen_questions = known_questions;
    let mut next = Vec::new();
    for mut item in candidates {
        let normalized = normalize_clarification_question(&item.question);
        if normalized.is_empty() || !seen_questions.insert(normalized) {
            continue;
        }
        if !known_ids.insert(item.id.clone()) {
            let digest = format!("{:x}", Sha256::digest(item.question.as_bytes()));
            let base_id = format!("clarification-r{round}-{}", &digest[..12]);
            let mut unique_id = base_id.clone();
            let mut suffix = 2;
            while !known_ids.insert(unique_id.clone()) {
                unique_id = format!("{base_id}-{suffix}");
                suffix += 1;
            }
            item.id = unique_id;
        }
        item.round = round;
        next.push(item);
        if next.len() == MAX_CLARIFICATIONS_PER_ROUND {
            break;
        }
    }
    next
}

fn facilitator_review_update(
    existing: &[Clarification],
    proposed: Vec<Clarification>,
    answers: Vec<ClarificationAnswer>,
) -> (&'static str, Vec<Clarification>, Vec<ClarificationAnswer>) {
    let round = existing
        .iter()
        .map(|item| item.round)
        .max()
        .unwrap_or(1)
        .saturating_add(1);
    let next = next_clarification_round(proposed, existing, round);
    let status = if next.is_empty() {
        "analyzed"
    } else {
        "clarification"
    };
    let mut clarifications = existing.to_vec();
    clarifications.extend(next);
    (status, clarifications, answers)
}

fn merge_clarification_answers(
    previous: &[ClarificationAnswer],
    current: &[ClarificationAnswer],
) -> Vec<ClarificationAnswer> {
    let mut merged = previous
        .iter()
        .cloned()
        .map(|answer| (answer.clarification_id.clone(), answer))
        .collect::<std::collections::BTreeMap<_, _>>();
    for answer in current {
        merged.insert(answer.clarification_id.clone(), answer.clone());
    }
    merged.into_values().collect()
}

async fn save_requirement_analysis(
    state: &AppState,
    project_id: i64,
    user_id: &str,
    requirement: &str,
    analysis: &RequirementAnalysis,
    orchestration_id: i64,
    base_revision: Option<i64>,
    previous: Option<&DecisionWorkflowState>,
) -> Result<DecisionWorkflowState, WorkflowApiError> {
    validate_analysis(analysis)?;
    let status = if analysis.clarifications.is_empty() {
        "analyzed"
    } else {
        "clarification"
    };
    let result = sqlx::query(
        r#"INSERT INTO decision_workflows AS workflow (
            project_id, status, requirement_text, decision_brief_json,
            clarifications_json, answers_json, decision_plan_json, tester_report_json,
            confirmed_at, updated_at, base_revision, original_requirement_text,
            analysis_orchestration_id, plan_orchestration_id
        ) SELECT $1, $2, $3, $4, $5, '[]'::JSONB, NULL, NULL, NULL, NOW(), $6, $8, $9, NULL
          FROM decision_projects project WHERE project.id = $1
            AND project.current_draft_version_id IS NOT DISTINCT FROM $6
        ON CONFLICT (project_id) DO UPDATE SET
            status = EXCLUDED.status, requirement_text = EXCLUDED.requirement_text,
            decision_brief_json = EXCLUDED.decision_brief_json,
            clarifications_json = EXCLUDED.clarifications_json,
            answers_json = '[]'::JSONB, decision_plan_json = NULL,
            tester_report_json = NULL,
            analysis_orchestration_id = EXCLUDED.analysis_orchestration_id,
            plan_orchestration_id = NULL,
            confirmed_at = NULL, updated_at = NOW(), base_revision = EXCLUDED.base_revision,
            original_requirement_text = EXCLUDED.original_requirement_text
        WHERE workflow.updated_at = $7"#,
    )
    .bind(project_id)
    .bind(status)
    .bind(requirement)
    .bind(serde_json::to_value(&analysis.brief).unwrap_or(Value::Null))
    .bind(serde_json::to_value(&analysis.clarifications).unwrap_or(Value::Array(vec![])))
    .bind(base_revision)
    .bind(previous.map(|workflow| workflow.updated_at))
    .bind(requirement_origin(requirement, base_revision, previous))
    .bind(orchestration_id)
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("保存需求分析", error))?;
    if result.rows_affected() == 0 {
        return Err(WorkflowApiError::conflict(
            "需求或模型基线已变化，本次分析未覆盖已有结果，请刷新后重新分析",
        ));
    }
    load_workflow(state, project_id, user_id)
        .await?
        .ok_or_else(WorkflowApiError::not_found)
}

fn validate_plan(
    plan: &DecisionPlan,
    brief: Option<&DecisionBrief>,
) -> Result<(), WorkflowApiError> {
    if plan.name.trim().is_empty() || plan.summary.trim().is_empty() || plan.decisions.is_empty() {
        return Err(WorkflowApiError::bad_request(
            "决策方案必须包含名称、摘要和至少一个决策",
        ));
    }
    let mut names = HashSet::new();
    if plan.decisions.iter().any(|decision| {
        decision.name.trim().is_empty()
            || decision.output_type.trim().is_empty()
            || decision.expression_type.trim().is_empty()
            || !names.insert(decision.name.as_str())
    }) {
        return Err(WorkflowApiError::bad_request(
            "决策方案包含缺失字段或重复决策名称",
        ));
    }
    if let Some(brief) = brief.filter(|brief| !brief.requirement_items.is_empty()) {
        let valid_ids = brief
            .requirement_items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        if plan.decisions.iter().any(|decision| {
            decision.requirement_ids.is_empty()
                || decision
                    .requirement_ids
                    .iter()
                    .any(|requirement_id| !valid_ids.contains(requirement_id.as_str()))
        }) {
            return Err(WorkflowApiError::bad_request(
                "每个决策必须关联至少一条有效需求依据",
            ));
        }
    }
    Ok(())
}

fn generate_tester_report(
    brief: &DecisionBrief,
    plan: &DecisionPlan,
    answers: &[ClarificationAnswer],
    complexity: RequirementComplexity,
) -> TesterReport {
    const MAX_PROPOSALS: usize = 100;
    fn push_proposal(
        proposals: &mut Vec<SemanticTestProposal>,
        category: &str,
        title: String,
        rationale: String,
        source_reference: String,
        fields: Vec<String>,
    ) {
        if proposals.len() >= MAX_PROPOSALS {
            return;
        }
        let id = format!("proposal-{:03}", proposals.len() + 1);
        proposals.push(SemanticTestProposal {
            id,
            category: category.into(),
            title,
            rationale,
            source_reference,
            input_fields: fields,
            suggested_input: None,
            expected_output: None,
            review_required: true,
            test_case_id: None,
            execution_status: None,
            judge_review: None,
        });
    }
    let input_fields = brief
        .inputs
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let mut proposals = Vec::new();

    for field in brief.inputs.iter().filter(|field| field.required) {
        push_proposal(
            &mut proposals,
            "required_input",
            format!(
                "缺少必填输入：{}",
                field
                    .label
                    .trim()
                    .is_empty()
                    .then_some(&field.name)
                    .unwrap_or(&field.label)
            ),
            "确认缺少必填输入时的拒绝、澄清或降级行为".into(),
            format!("brief.inputs:{}", field.name),
            vec![field.name.clone()],
        );
    }
    for field in brief
        .inputs
        .iter()
        .filter(|field| !field.required && !field.description.trim().is_empty())
    {
        push_proposal(
            &mut proposals,
            "optional_input",
            format!("可选输入缺省：{}", field.name),
            "验证可选信息缺失时的默认值与输出稳定性".into(),
            format!("brief.inputs:{}", field.name),
            vec![field.name.clone()],
        );
    }
    for (index, answer) in answers.iter().enumerate() {
        push_proposal(
            &mut proposals,
            "clarification",
            format!("复核已确认业务口径：{}", answer.clarification_id),
            format!("验证确认口径“{}”在方案中的落实", answer.answer),
            format!("answers[{}]", index),
            input_fields.clone(),
        );
    }
    if brief.requirement_items.is_empty() {
        if !brief.goal.trim().is_empty() {
            push_proposal(
                &mut proposals,
                "semantic_scenario",
                format!(
                    "业务场景覆盖：{}",
                    brief.goal.chars().take(80).collect::<String>()
                ),
                "围绕需求目标补充真实业务输入，并由业务人员确认期望输出".into(),
                "brief.goal".into(),
                input_fields.clone(),
            );
        }
    } else {
        for item in &brief.requirement_items {
            push_proposal(
                &mut proposals,
                "semantic_requirement",
                format!(
                    "需求场景覆盖：{}",
                    item.text.chars().take(80).collect::<String>()
                ),
                "将该需求条目落实为业务场景输入，并由业务人员确认期望输出".into(),
                format!("requirementItems:{}:{}", item.id, item.source),
                input_fields.clone(),
            );
        }
    }
    for (index, criterion) in plan.acceptance_criteria.iter().enumerate() {
        push_proposal(
            &mut proposals,
            "acceptance_criterion",
            format!(
                "验收标准覆盖：{}",
                criterion.chars().take(80).collect::<String>()
            ),
            "根据验收标准补充业务人员确认的输入和期望输出".into(),
            format!("plan.acceptanceCriteria[{}]", index),
            input_fields.clone(),
        );
    }
    for decision in &plan.decisions {
        push_proposal(
            &mut proposals,
            "decision_contract",
            format!("复核决策输出契约：{}", decision.name),
            format!("确认“{}”的输出类型与依赖满足业务预期", decision.purpose),
            format!("plan.decisions:{}", decision.name),
            input_fields.clone(),
        );
    }
    if !brief.constraints.is_empty() {
        for (index, constraint) in brief.constraints.iter().enumerate() {
            push_proposal(
                &mut proposals,
                "adversarial",
                format!(
                    "约束边界复核：{}",
                    constraint.chars().take(80).collect::<String>()
                ),
                "围绕该约束设计边界、缺省或相互作用场景；输入和期望输出由业务人员确认".into(),
                format!("brief.constraints[{}]", index),
                input_fields.clone(),
            );
        }
    } else if complexity == RequirementComplexity::Complex || brief.risk_level == "high" {
        push_proposal(
            &mut proposals,
            "adversarial",
            "冲突约束与边界组合复核".into(),
            "组合相互冲突的约束、缺省字段和边界条件，期望结果由业务人员确认".into(),
            "requirement.complexity".into(),
            input_fields.clone(),
        );
    }
    if proposals.is_empty() {
        push_proposal(
            &mut proposals,
            "baseline_review",
            "确认基础业务场景".into(),
            "为当前方案补充至少一个由业务人员确认的正常场景".into(),
            "plan".into(),
            input_fields,
        );
    }
    TesterReport {
        schema_version: 1,
        proposals,
    }
}

pub(crate) fn append_static_analysis_proposals(
    report: &mut TesterReport,
    brief: &DecisionBrief,
    draft_version_id: i64,
    findings: &[crate::models::VerificationFinding],
    suggested_inputs: &HashMap<String, Value>,
) -> usize {
    const MAX_PROPOSALS: usize = 100;
    let input_fields = brief
        .inputs
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let mut sources = report
        .proposals
        .iter()
        .map(|proposal| proposal.source_reference.clone())
        .collect::<HashSet<_>>();
    let mut added = 0;
    for finding in findings {
        if report.proposals.len() >= MAX_PROPOSALS
            || !matches!(
                finding.code.as_str(),
                "UNIQUE_RULE_CONFLICT"
                    | "ANY_OUTPUT_CONFLICT"
                    | "UNREACHABLE_RULE"
                    | "MISSING_COVERAGE"
            )
        {
            continue;
        }
        let source_reference = static_analysis_source_reference(draft_version_id, finding);
        if !sources.insert(source_reference.clone()) {
            continue;
        }
        let mut sequence = report.proposals.len() + 1;
        let id = loop {
            let id = format!("proposal-{sequence:03}");
            if report.proposals.iter().all(|proposal| proposal.id != id) {
                break id;
            }
            sequence += 1;
        };
        let title = if finding.code == "MISSING_COVERAGE" {
            format!("模型覆盖缺口复核：{}", finding.element)
        } else {
            format!("模型静态发现复核：{}", finding.element)
        };
        let rule_detail = if finding.rule_ids.is_empty() {
            String::new()
        } else {
            format!(" 涉及规则：{}。", finding.rule_ids.join("、"))
        };
        let suggested_input = suggested_inputs.get(&source_reference).cloned();
        report.proposals.push(SemanticTestProposal {
            id,
            category: "static_analysis".into(),
            title,
            rationale: format!(
                "{}{}{}",
                finding.message,
                rule_detail,
                if suggested_input.is_some() {
                    "已提供一个可执行的参考输入，请核对输入并由业务人员确认期望输出。"
                } else {
                    "请补充针对该模型发现的输入场景，并由业务人员确认期望输出。"
                }
            ),
            source_reference,
            input_fields: input_fields.clone(),
            suggested_input,
            expected_output: None,
            review_required: true,
            test_case_id: None,
            execution_status: None,
            judge_review: None,
        });
        added += 1;
    }
    added
}

pub(crate) fn static_analysis_source_reference(
    draft_version_id: i64,
    finding: &crate::models::VerificationFinding,
) -> String {
    format!(
        "draftVersion:{draft_version_id}/staticFinding:{}:{}:{}",
        finding.code,
        finding.element,
        finding.rule_ids.join(",")
    )
}

fn artifact_digest(value: &Value) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn prompt_digest(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

pub(crate) async fn start_orchestration_step(
    db: &sqlx::PgPool,
    run_id: i64,
    step_no: i32,
    role: &str,
    task_kind: &str,
    model: Option<&str>,
    prompt_version: &str,
    prompt_digest: &str,
    input: &Value,
    previous_step_id: Option<i64>,
) -> Result<i64, String> {
    let digest = artifact_digest(input)?;
    let context_fields = input
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let mut transaction = db.begin().await.map_err(|error| error.to_string())?;
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT orchestration.id
        FROM agent_runs run
        JOIN agent_orchestrations orchestration ON orchestration.id = run.orchestration_id
        WHERE run.id = $1 AND run.orchestration_id IS NOT NULL
          AND run.status = 'running' AND orchestration.status = 'running'
        FOR UPDATE OF orchestration
        "#,
    )
    .bind(run_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;
    let step_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO agent_steps (
            orchestration_id, step_no, agent_role, task_kind, model,
            prompt_version, prompt_digest, artifact_kind, artifact_schema_version
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $4, 1)
        RETURNING id
        "#,
    )
    .bind(orchestration_id)
    .bind(step_no)
    .bind(role)
    .bind(task_kind)
    .bind(model)
    .bind(prompt_version)
    .bind(prompt_digest)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;
    let previous_step_id = match previous_step_id {
        Some(step_id) => Some(step_id),
        None if step_no > 1 => sqlx::query_scalar::<_, i64>(
            "SELECT id FROM agent_steps WHERE orchestration_id = $1 AND step_no = $2",
        )
        .bind(orchestration_id)
        .bind(step_no - 1)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| error.to_string())?,
        None => None,
    };
    if let Some(previous_step_id) = previous_step_id {
        sqlx::query(
            r#"
            INSERT INTO agent_handoffs (
                orchestration_id, from_step_id, to_step_id, context_digest, context_summary_json
            ) VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(orchestration_id)
        .bind(previous_step_id)
        .bind(step_id)
        .bind(digest)
        .bind(json!({ "schemaVersion": 1, "fieldNames": context_fields }))
        .execute(&mut *transaction)
        .await
        .map_err(|error| error.to_string())?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())?;
    Ok(step_id)
}

pub(crate) async fn finish_orchestration_step(
    db: &sqlx::PgPool,
    step_id: i64,
    output: Result<&Value, &str>,
) -> Result<(), String> {
    let (status, digest, error) = match output {
        Ok(value) => ("succeeded", Some(artifact_digest(value)?), None),
        Err(message) => ("failed", None, Some(json!({ "message": message }))),
    };
    sqlx::query(
        "UPDATE agent_steps SET status = $1, artifact_digest = $2, error_json = $3, finished_at = NOW() WHERE id = $4 AND status = 'running'",
    )
    .bind(status)
    .bind(digest)
    .bind(error)
    .bind(step_id)
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

pub(crate) async fn skip_orchestration_step(
    db: &sqlx::PgPool,
    step_id: i64,
    reason: &Value,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE agent_steps SET status = 'skipped', error_json = $1, finished_at = NOW() WHERE id = $2 AND status = 'running'",
    )
    .bind(reason)
    .bind(step_id)
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn decision_plan_validation_stage(brief: DecisionBrief) -> PipelineStage {
    PipelineStage::new("validate_decision_plan", move |value| {
        let brief = brief.clone();
        async move {
            let plan =
                serde_json::from_value::<DecisionPlan>(value).map_err(|error| error.to_string())?;
            validate_plan(&plan, Some(&brief)).map_err(|error| error.message)?;
            serde_json::to_value(plan).map_err(|error| error.to_string())
        }
    })
}

fn persisted_decision_plan_validation_stage(
    state: AppState,
    run_id: i64,
    previous_step: Arc<Mutex<Option<i64>>>,
    brief: DecisionBrief,
) -> PipelineStage {
    PipelineStage::new("validate_decision_plan", move |value| {
        let state = state.clone();
        let previous_step = Arc::clone(&previous_step);
        let brief = brief.clone();
        async move {
            let previous_step_id = previous_step
                .lock()
                .map_err(|_| "读取上一步记录失败".to_string())?
                .take();
            let step_id = start_orchestration_step(
                &state.db,
                run_id,
                3,
                "validator",
                "validate_decision_plan",
                None,
                "deterministic-v1",
                &prompt_digest("deterministic-v1"),
                &value,
                previous_step_id,
            )
            .await?;
            let validation = async {
                let plan = serde_json::from_value::<DecisionPlan>(value)
                    .map_err(|error| error.to_string())?;
                validate_plan(&plan, Some(&brief)).map_err(|error| error.message)?;
                serde_json::to_value(plan).map_err(|error| error.to_string())
            }
            .await;
            match validation {
                Ok(value) => {
                    finish_orchestration_step(&state.db, step_id, Ok(&value)).await?;
                    Ok(value)
                }
                Err(error) => {
                    let _ = finish_orchestration_step(&state.db, step_id, Err(&error)).await;
                    Err(error)
                }
            }
        }
    })
    .with_timeout(Duration::from_secs(30))
}

fn pipeline_agent_role(agent_name: &str) -> &str {
    match agent_name {
        "plan_decision_model" => "architect",
        "validate_decision_plan" => "validator",
        other => other,
    }
}

async fn plan_with_validation_pipeline(
    state: &AppState,
    run_id: i64,
    agent: &LlmAgent,
    brief: DecisionBrief,
    answers: Vec<ClarificationAnswer>,
    knowledge_text: String,
    model_context: Option<String>,
    stream: Option<&mpsc::Sender<StreamEvent>>,
) -> Result<DecisionPlan, WorkflowApiError> {
    let validation_brief = brief.clone();
    let planning_agent = agent.clone();
    let previous_step = Arc::new(Mutex::new(None));
    let planning_previous_step = Arc::clone(&previous_step);
    let planning_state = (*state).clone();
    let planning_stage = PipelineStage::new("plan_decision_model", move |_| {
        let state = planning_state.clone();
        let agent = planning_agent.clone();
        let previous_step = Arc::clone(&planning_previous_step);
        let brief = brief.clone();
        let answers = answers.clone();
        let knowledge_text = knowledge_text.clone();
        let model_context = model_context.clone();
        async move {
            let input_summary = json!({
                "artifactType": "decision_plan_request",
                "schemaVersion": 1,
                "hasBrief": true,
                "answerCount": answers.len(),
                "knowledgeContextIncluded": !knowledge_text.is_empty(),
                "requirementItemIds": brief.requirement_items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            });
            let step_id = start_orchestration_step(
                &state.db,
                run_id,
                2,
                "architect",
                "plan_decision_model",
                Some(agent.model()),
                agent.prompt_asset("decision_planning").version.as_str(),
                &prompt_digest(&agent.prompt_asset("decision_planning").content),
                &input_summary,
                None,
            )
            .await?;
            *previous_step
                .lock()
                .map_err(|_| "保存规划步骤记录失败".to_string())? = Some(step_id);
            let plan = match agent
                .plan_decision_model(
                    &brief,
                    &answers,
                    Some(&knowledge_text),
                    model_context.as_deref(),
                )
                .await
            {
                Ok(plan) => plan,
                Err(error) => {
                    let message = error.to_string();
                    let _ = finish_orchestration_step(&state.db, step_id, Err(&message)).await;
                    return Err(message);
                }
            };
            let value = serde_json::to_value(plan).map_err(|error| error.to_string())?;
            finish_orchestration_step(&state.db, step_id, Ok(&value)).await?;
            Ok(value)
        }
    })
    .with_tools(vec![AgentTool::new(
        "plan_decision_model",
        "根据确认后的需求上下文生成结构化 DMN 决策方案",
    )])
    .with_timeout(Duration::from_secs(180));
    let stages = vec![
        planning_stage,
        persisted_decision_plan_validation_stage(
            state.clone(),
            run_id,
            previous_step,
            validation_brief,
        ),
    ];
    let report = if let Some(stream) = stream {
        let (events_tx, mut events_rx) = mpsc::channel::<AgentEvent>(8);
        let stream = stream.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(event) = events_rx.recv().await {
                let status = match event.status {
                    AgentStatus::Running => "running",
                    AgentStatus::Succeeded => "completed",
                    AgentStatus::Failed => "failed",
                };
                let role = pipeline_agent_role(&event.agent);
                let _ = stream
                    .send(StreamEvent::Agent {
                        role: role.into(),
                        status: status.into(),
                    })
                    .await;
            }
        });
        let report = run_pipeline_stream(stages, Value::Null, events_tx).await;
        let _ = forwarder.await;
        report
    } else {
        run_pipeline(stages, Value::Null).await
    }
    .map_err(|failure| {
        if failure.failed_step == "validate_decision_plan" {
            WorkflowApiError::bad_request(failure.message)
        } else {
            WorkflowApiError::model("决策方案规划", anyhow!(failure))
        }
    })?;
    serde_json::from_value(report.output)
        .map_err(|error| WorkflowApiError::bad_request(format!("规划结果无效：{error}")))
}

async fn persist_tester_report(
    state: &AppState,
    run_id: i64,
    brief: &DecisionBrief,
    plan: &DecisionPlan,
    answers: &[ClarificationAnswer],
    complexity: RequirementComplexity,
) -> Result<TesterReport, WorkflowApiError> {
    let report = generate_tester_report(brief, plan, answers, complexity);
    let input = json!({
        "artifactType": "semantic_test_proposals",
        "schemaVersion": report.schema_version,
        "proposalCount": report.proposals.len(),
        "decisionCount": plan.decisions.len(),
        "acceptanceCriterionCount": plan.acceptance_criteria.len(),
        "allRequireBusinessReview": true,
    });
    let step_id = start_orchestration_step(
        &state.db,
        run_id,
        4,
        "tester",
        "generate_semantic_test_proposals",
        None,
        "deterministic-tester-v1",
        &prompt_digest("deterministic-tester-v1"),
        &input,
        None,
    )
    .await
    .map_err(|error| WorkflowApiError::model("记录测试工程 Agent 步骤", anyhow!(error)))?;
    let artifact = serde_json::to_value(&report)
        .map_err(|error| WorkflowApiError::model("编码测试提案", anyhow!(error)))?;
    finish_orchestration_step(&state.db, step_id, Ok(&artifact))
        .await
        .map_err(|error| WorkflowApiError::model("保存测试工程 Agent 产物摘要", anyhow!(error)))?;
    Ok(report)
}

fn validate_answers(
    clarifications: &[Clarification],
    answers: &[ClarificationAnswer],
) -> Result<(), WorkflowApiError> {
    let clarification_ids = clarifications
        .iter()
        .map(|clarification| clarification.id.as_str())
        .collect::<HashSet<_>>();
    let mut seen_ids = HashSet::new();
    if answers.iter().any(|answer| {
        let id = answer.clarification_id.trim();
        id.is_empty() || !clarification_ids.contains(id) || !seen_ids.insert(id)
    }) {
        return Err(WorkflowApiError::bad_request(
            "澄清回答包含未知、空白或重复标识",
        ));
    }
    let answer_map = answers
        .iter()
        .map(|answer| (answer.clarification_id.trim(), answer.answer.trim()))
        .collect::<HashMap<_, _>>();
    let missing = clarifications
        .iter()
        .filter(|item| item.required)
        .filter(|item| {
            answer_map
                .get(item.id.as_str())
                .is_none_or(|answer| answer.is_empty())
        })
        .map(|item| item.question.as_str())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(WorkflowApiError::bad_request(format!(
            "请先回答必填澄清问题：{}",
            missing.join("；")
        )))
    }
}

async fn ensure_project(
    state: &AppState,
    project_id: i64,
    username: &str,
) -> Result<(), WorkflowApiError> {
    let allowed =
        project_access::has_permission(&state.db, project_id, username, ProjectPermission::View)
            .await
            .map_err(|error| WorkflowApiError::database("读取决策项目", error))?;
    allowed
        .then_some(())
        .ok_or_else(WorkflowApiError::not_found)
}

async fn ensure_model_permission(
    state: &AppState,
    project_id: i64,
    username: &str,
) -> Result<(), WorkflowApiError> {
    let allowed =
        project_access::has_permission(&state.db, project_id, username, ProjectPermission::Model)
            .await
            .map_err(|error| WorkflowApiError::database("校验项目建模权限", error))?;
    allowed
        .then_some(())
        .ok_or_else(WorkflowApiError::not_found)
}

async fn load_workflow(
    state: &AppState,
    project_id: i64,
    _username: &str,
) -> Result<Option<DecisionWorkflowState>, WorkflowApiError> {
    sqlx::query_as::<_, WorkflowRow>(
        r#"
        SELECT workflow.project_id, workflow.base_revision, workflow.status, workflow.requirement_text,
               workflow.original_requirement_text,
               workflow.decision_brief_json, workflow.clarifications_json,
               workflow.answers_json, workflow.decision_plan_json, workflow.tester_report_json,
               workflow.confirmed_at, workflow.updated_at
        FROM decision_workflows workflow
        WHERE workflow.project_id = $1
        "#,
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取需求工作流", error))?
    .map(WorkflowRow::into_state)
    .transpose()
}

async fn begin_agent_run(
    state: &AppState,
    project_id: i64,
    username: &str,
    task: LlmTask,
    agent: &LlmAgent,
    experiment: Option<ExperimentAssignment<'_>>,
    parallel_model_count: Option<usize>,
    parent_orchestration_id: Option<i64>,
    retry_of_orchestration_id: Option<i64>,
    input: Value,
) -> Result<(i64, i64), WorkflowApiError> {
    let reserved_response_count = match parallel_model_count {
        Some(model_count) => agent.maximum_parallel_model_response_attempts(model_count),
        None => agent.maximum_model_response_attempts(),
    };
    let estimated_cost = crate::api::estimated_run_cost_reservation(
        agent.estimated_cost_per_response_usd(),
        reserved_response_count,
    );
    let mut transaction = match crate::api::begin_project_daily_budget_transaction(
        &state.db,
        project_id,
        estimated_cost,
    )
    .await
    {
        Ok(transaction) => transaction,
        Err(crate::api::ProjectTokenBudgetError::Exceeded { used, limit }) => {
            return Err(WorkflowApiError::token_budget(format!(
                "项目今日模型 token 预算已用尽（已用 {used}，上限 {limit}）"
            )));
        }
        Err(crate::api::ProjectTokenBudgetError::OrchestrationTokensExceeded { used, limit }) => {
            return Err(WorkflowApiError::token_budget(format!(
                "本编排树已达到累计观测 Token 门槛（已观测 {used}，门槛 {limit}）；在途模型调用可能使实际用量超出门槛"
            )));
        }
        Err(crate::api::ProjectTokenBudgetError::EstimatedCostExceeded { used, next, limit }) => {
            return Err(WorkflowApiError::token_budget(format!(
                "项目今日模型估算成本预算不足（已用 ${used:.6}，本次至少 ${next:.6}，上限 ${limit:.6}）"
            )));
        }
        Err(crate::api::ProjectTokenBudgetError::InvalidConfiguration(error)) => {
            eprintln!("项目 token 预算配置无效，拒绝模型调用：{error}");
            return Err(WorkflowApiError::token_budget(
                "项目预算配置无效，暂时无法调用模型",
            ));
        }
        Err(crate::api::ProjectTokenBudgetError::Database(error)) => {
            return Err(WorkflowApiError::database("检查项目 token 预算", error));
        }
        Err(crate::api::ProjectTokenBudgetError::RetryAlreadyActive) => {
            return Err(WorkflowApiError::conflict("该编排已有活跃重试"));
        }
    };
    match crate::api::check_orchestration_budgets(
        &mut transaction,
        project_id,
        parent_orchestration_id,
        retry_of_orchestration_id,
        estimated_cost,
    )
    .await
    {
        Ok(()) => {}
        Err(crate::api::ProjectTokenBudgetError::EstimatedCostExceeded { used, next, limit }) => {
            return Err(WorkflowApiError::token_budget(format!(
                "本次智能编排估算成本超出单编排预算（已预留 ${used:.6}，本次至少 ${next:.6}，上限 ${limit:.6}）"
            )));
        }
        Err(crate::api::ProjectTokenBudgetError::OrchestrationTokensExceeded { used, limit }) => {
            return Err(WorkflowApiError::token_budget(format!(
                "本编排树已达到累计观测 Token 门槛（已观测 {used}，门槛 {limit}）；在途模型调用可能使实际用量超出门槛"
            )));
        }
        Err(crate::api::ProjectTokenBudgetError::InvalidConfiguration(error)) => {
            eprintln!("编排成本预算配置无效，拒绝模型调用：{error}");
            return Err(WorkflowApiError::token_budget(
                "编排成本预算配置无效，暂时无法调用模型",
            ));
        }
        Err(crate::api::ProjectTokenBudgetError::Database(error)) => {
            return Err(WorkflowApiError::database("检查编排成本预算", error));
        }
        Err(crate::api::ProjectTokenBudgetError::RetryAlreadyActive) => {
            return Err(WorkflowApiError::conflict("该编排已有活跃重试"));
        }
        Err(crate::api::ProjectTokenBudgetError::Exceeded { .. }) => {
            return Err(WorkflowApiError::token_budget(
                "编排 token 预算已用尽，暂时无法调用模型",
            ));
        }
    }
    let stage = task.audit_stage().to_string();
    let attempt_no = if let Some(retry_of) = retry_of_orchestration_id {
        sqlx::query_scalar::<_, i32>(
            "SELECT COUNT(*)::INTEGER + 2 FROM agent_orchestrations WHERE retry_of_orchestration_id = $1",
        )
        .bind(retry_of)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| WorkflowApiError::database("读取 Agent 重试序号", error))?
    } else {
        1
    };
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO agent_orchestrations (
            project_id, username, trigger_kind, current_stage, parent_orchestration_id,
            retry_of_orchestration_id
        )
        SELECT $1, $2, $3, $3, $4, $5
        WHERE ($4 IS NULL OR EXISTS (
            SELECT 1 FROM agent_orchestrations
            WHERE id = $4 AND project_id = $1 AND status = 'succeeded'
        )) AND ($5 IS NULL OR EXISTS (
            SELECT 1
            FROM agent_orchestrations previous
            WHERE previous.id = $5
              AND previous.project_id = $1
              AND previous.trigger_kind = $3
              AND previous.status = 'failed'
              AND previous.retry_of_orchestration_id IS NULL
              AND EXISTS (
                  SELECT 1
                  FROM agent_runs previous_run
                  JOIN agent_steps previous_step ON previous_step.agent_run_id = previous_run.id
                  WHERE previous_run.orchestration_id = previous.id
                    AND previous_run.stage = $3
                    AND previous_step.error_json->>'retryable' = 'true'
                    AND (previous_run.input_json - 'knowledgeCitations' - 'complexity')
                        = ($6::JSONB - 'knowledgeCitations' - 'complexity')
              )
              AND (SELECT COUNT(*) FROM agent_orchestrations retry
                   WHERE retry.retry_of_orchestration_id = previous.id) < 2
              AND NOT EXISTS (
                  SELECT 1 FROM agent_orchestrations retry
                  WHERE retry.retry_of_orchestration_id = previous.id
                    AND retry.status IN ('pending', 'running')
              )
              AND NOT EXISTS (
                  SELECT 1 FROM agent_orchestrations retry
                  WHERE retry.retry_of_orchestration_id = previous.id
                    AND retry.status = 'succeeded'
              )
        ))
        ON CONFLICT (retry_of_orchestration_id)
            WHERE retry_of_orchestration_id IS NOT NULL
              AND status IN ('pending', 'running')
        DO NOTHING
        RETURNING id
        "#,
    )
    .bind(project_id)
    .bind(username)
    .bind(&stage)
    .bind(parent_orchestration_id)
    .bind(retry_of_orchestration_id)
    .bind(&input)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("创建 Agent 编排记录", error))?
    .ok_or_else(|| {
        if retry_of_orchestration_id.is_some() {
            WorkflowApiError::conflict("该编排已有活跃重试或重试条件不满足")
        } else {
            WorkflowApiError::bad_request("上游编排不可用或重试条件不满足")
        }
    })?;
    let prompt_asset = match task {
        LlmTask::RequirementAnalysis => agent.prompt_asset("requirement_analysis"),
        LlmTask::DecisionPlanning => agent.prompt_asset("decision_planning"),
        _ => return Err(WorkflowApiError::bad_request("该任务没有工作流提示词资产")),
    };
    let prompt_version = prompt_asset.version.as_str();
    let prompt_content_digest = prompt_digest(&prompt_asset.content);
    let run_id = sqlx::query_scalar(
        r#"
        INSERT INTO agent_runs (
            project_id, stage, status, model, prompt_version, prompt_digest, input_json,
            usage_json, experiment_name, experiment_variant,
            experiment_treatment_percent, experiment_bucket, orchestration_id
        )
        VALUES ($1, $2, 'running', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        RETURNING id
        "#,
    )
    .bind(project_id)
    .bind(&stage)
    .bind(agent.model())
    .bind(prompt_version)
    .bind(&prompt_content_digest)
    .bind(input)
    .bind(crate::api::project_budget_reservation(estimated_cost))
    .bind(experiment.map(|assignment| assignment.experiment_name))
    .bind(experiment.map(|assignment| assignment.variant))
    .bind(experiment.map(|assignment| i16::from(assignment.treatment_percent)))
    .bind(experiment.map(|assignment| i16::from(assignment.bucket)))
    .bind(orchestration_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("记录智能运行", error))?;
    sqlx::query(
        "INSERT INTO agent_steps (orchestration_id, agent_run_id, step_no, attempt_no, agent_role, task_kind, model, prompt_version, prompt_digest, artifact_kind) VALUES ($1, $2, 1, $3, $4, $4, $5, $6, $7, $4)",
    )
    .bind(orchestration_id)
    .bind(run_id)
    .bind(attempt_no)
    .bind(&stage)
    .bind(agent.model())
    .bind(prompt_version)
    .bind(&prompt_content_digest)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("记录 Agent 编排步骤", error))?;
    sqlx::query("UPDATE agent_orchestrations SET current_stage = $1 WHERE id = $2")
        .bind(&stage)
        .bind(orchestration_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| WorkflowApiError::database("更新 Agent 编排阶段", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| WorkflowApiError::database("提交智能运行预算占用", error))?;
    Ok((run_id, orchestration_id))
}

async fn project_risk_level(state: &AppState, project_id: i64) -> Result<String, WorkflowApiError> {
    sqlx::query_scalar::<_, String>("SELECT risk_level FROM decision_projects WHERE id = $1")
        .bind(project_id)
        .fetch_one(&state.db)
        .await
        .map_err(|error| WorkflowApiError::database("读取项目风险等级", error))
}

async fn finish_agent_run(
    state: &AppState,
    run_id: i64,
    agent: &LlmAgent,
    output: Result<Value, String>,
) -> Result<(), WorkflowApiError> {
    let artifact_digest = output
        .as_ref()
        .ok()
        .map(artifact_digest)
        .transpose()
        .map_err(|error| WorkflowApiError::model("记录 Agent 产物摘要", anyhow!(error)))?;
    let (status, output, error) = match output {
        Ok(output) => ("succeeded", Some(output), None),
        Err(error) => (
            workflow_run_terminal_status(&error),
            None,
            Some(json!({
                "message": error,
                "retryable": is_retryable_agent_failure(&error),
                "failureKind": if is_retryable_agent_failure(&error) { "transient_provider" } else { "non_retryable_or_unknown" },
            })),
        ),
    };
    let usage = agent.usage_snapshot();
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| WorkflowApiError::database("开始更新 Agent 编排状态", error))?;
    let updated = sqlx::query(
        "UPDATE agent_runs SET status = $1, output_json = $2, error_json = $3, usage_json = $4, finished_at = NOW() WHERE id = $5 AND status = 'running'",
    )
    .bind(status)
    .bind(output)
    .bind(error.clone())
    .bind(usage.clone())
    .bind(run_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("更新智能运行", error))?;
    if updated.rows_affected() > 0 {
        sqlx::query(
            r#"
            UPDATE agent_steps step
            SET status = run.status, usage_json = run.usage_json,
                error_json = run.error_json,
                artifact_digest = COALESCE($2, step.artifact_digest),
                finished_at = run.finished_at
            FROM agent_runs run
            WHERE step.agent_run_id = run.id AND run.id = $1
            "#,
        )
        .bind(run_id)
        .bind(artifact_digest)
        .execute(&mut *transaction)
        .await
        .map_err(|error| WorkflowApiError::database("更新 Agent 编排步骤", error))?;
        sqlx::query(
            r#"
            UPDATE agent_steps
            SET status = $1, error_json = COALESCE(error_json, $2),
                finished_at = COALESCE(finished_at, NOW())
            WHERE orchestration_id = (
                SELECT orchestration_id FROM agent_runs WHERE id = $3
            ) AND status IN ('pending', 'running')
            "#,
        )
        .bind(status)
        .bind(error)
        .bind(run_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| WorkflowApiError::database("收敛剩余 Agent 步骤状态", error))?;
        sqlx::query(
            r#"
            UPDATE agent_orchestrations orchestration
            SET status = $1,
                current_stage = COALESCE((
                    SELECT task_kind FROM agent_steps
                    WHERE orchestration_id = orchestration.id
                    ORDER BY step_no DESC LIMIT 1
                ), orchestration.current_stage),
                finished_at = NOW()
            WHERE orchestration.id = (
                SELECT orchestration_id FROM agent_runs WHERE id = $2
            ) AND orchestration.status = 'running'
            "#,
        )
        .bind(status)
        .bind(run_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| WorkflowApiError::database("完成 Agent 编排", error))?;
    } else if status == "cancelled" {
        sqlx::query(
            "UPDATE agent_runs SET usage_json = $1 WHERE id = $2 AND status = 'cancelled' AND error_json->>'message' = '编排已由用户取消'",
        )
        .bind(usage)
        .bind(run_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| WorkflowApiError::database("补记取消运行用量", error))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| WorkflowApiError::database("提交 Agent 编排状态", error))
}

fn workflow_run_terminal_status(error: &str) -> &'static str {
    if error.contains("编排已由用户取消") {
        "cancelled"
    } else {
        "failed"
    }
}

pub(crate) fn is_retryable_agent_failure(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    if [
        "401",
        "403",
        "unauthorized",
        "forbidden",
        "token 预算",
        "预算已用尽",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        return false;
    }
    [
        "未返回有效需求分析候选",
        "http 429",
        "http 500",
        "http 502",
        "http 503",
        "http 504",
        "[429",
        "[500",
        "[502",
        "[503",
        "[504",
        "timed out",
        "timeout",
        "连接超时",
        "连接失败",
        "connection reset",
        "connection refused",
        "error sending request",
        "sse 客户端连接中断",
        "sse 客户端已断开",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

pub(crate) async fn run_until_stream_disconnect<T, F, C>(
    tx: Option<&mpsc::Sender<StreamEvent>>,
    cancellation: C,
    work: F,
) -> anyhow::Result<T>
where
    F: Future<Output = anyhow::Result<T>>,
    C: Future<Output = anyhow::Result<()>>,
{
    match tx {
        Some(tx) => tokio::select! {
            biased;
            result = work => result,
            _ = tx.closed() => Err(anyhow!("SSE 客户端连接中断")),
            result = cancellation => result.and_then(|()| Err(anyhow!("编排已由用户取消"))),
        },
        None => tokio::select! {
            result = work => result,
            result = cancellation => result.and_then(|()| Err(anyhow!("编排已由用户取消"))),
        },
    }
}

pub(crate) async fn wait_for_orchestration_cancellation(
    db: &sqlx::PgPool,
    orchestration_id: i64,
) -> anyhow::Result<()> {
    loop {
        let status = sqlx::query_scalar::<_, String>(
            "UPDATE agent_orchestrations SET heartbeat_at = NOW() WHERE id = $1 AND status IN ('pending', 'running') RETURNING status",
        )
        .bind(orchestration_id)
        .fetch_optional(db)
        .await?;
        match status.as_deref() {
            Some(_) => tokio::time::sleep(Duration::from_millis(500)).await,
            None => {
                let terminal_status = sqlx::query_scalar::<_, String>(
                    "SELECT status FROM agent_orchestrations WHERE id = $1",
                )
                .bind(orchestration_id)
                .fetch_optional(db)
                .await?;
                match terminal_status.as_deref() {
                    Some("cancelled") => return Ok(()),
                    Some(_) => return Err(anyhow!("编排已结束")),
                    None => return Err(anyhow!("编排记录不存在")),
                }
            }
        }
    }
}

const DEFAULT_ORCHESTRATION_STALE_AFTER_SECONDS: u64 = 120;
const MIN_ORCHESTRATION_STALE_AFTER_SECONDS: u64 = 30;
const MAX_ORCHESTRATION_STALE_AFTER_SECONDS: u64 = 86_400;

fn orchestration_stale_timeout_from_raw(raw: Option<&str>) -> Result<Duration, String> {
    let Some(raw) = raw else {
        return Ok(Duration::from_secs(
            DEFAULT_ORCHESTRATION_STALE_AFTER_SECONDS,
        ));
    };
    let seconds = raw.trim().parse::<u64>().map_err(|_| {
        "AGENT_ORCHESTRATION_STALE_AFTER_SECONDS 必须是 30 到 86400 之间的整数".to_string()
    })?;
    if !(MIN_ORCHESTRATION_STALE_AFTER_SECONDS..=MAX_ORCHESTRATION_STALE_AFTER_SECONDS)
        .contains(&seconds)
    {
        return Err("AGENT_ORCHESTRATION_STALE_AFTER_SECONDS 必须是 30 到 86400 之间的整数".into());
    }
    Ok(Duration::from_secs(seconds))
}

pub(crate) fn orchestration_stale_timeout_from_env() -> Duration {
    match orchestration_stale_timeout_from_raw(
        std::env::var("AGENT_ORCHESTRATION_STALE_AFTER_SECONDS")
            .ok()
            .as_deref(),
    ) {
        Ok(timeout) => timeout,
        Err(error) => {
            eprintln!("{error}，失联编排回收阈值回退到 120 秒");
            Duration::from_secs(DEFAULT_ORCHESTRATION_STALE_AFTER_SECONDS)
        }
    }
}

pub(crate) async fn recover_stale_orchestrations(
    db: &sqlx::PgPool,
    stale_after: Duration,
) -> Result<u64, sqlx::Error> {
    let mut transaction = db.begin().await?;
    let recovered_orchestrations = sqlx::query_as::<_, (i64, i64)>(
        r#"
        UPDATE agent_orchestrations
        SET status = 'failed', finished_at = NOW()
        WHERE status IN ('pending', 'running')
          AND heartbeat_at < NOW() - ($1 * INTERVAL '1 second')
        RETURNING id, project_id
        "#,
    )
    .bind(stale_after.as_secs() as i64)
    .fetch_all(&mut *transaction)
    .await?;
    if recovered_orchestrations.is_empty() {
        transaction.commit().await?;
        return Ok(0);
    }

    let orchestration_ids = recovered_orchestrations
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    let error = orchestration_interrupted_error();
    sqlx::query(
        r#"
        UPDATE agent_runs
        SET status = 'failed', error_json = $1, finished_at = NOW()
        WHERE orchestration_id = ANY($2) AND status = 'running'
        "#,
    )
    .bind(&error)
    .bind(&orchestration_ids)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE agent_steps
        SET status = 'failed', error_json = $1, finished_at = NOW()
        WHERE orchestration_id = ANY($2) AND status IN ('pending', 'running')
        "#,
    )
    .bind(error)
    .bind(&orchestration_ids)
    .execute(&mut *transaction)
    .await?;
    stop_decision_explanation_attempts(
        &mut transaction,
        &orchestration_ids,
        "服务中断，解释作业已停止；可重新发起",
    )
    .await?;
    for (orchestration_id, project_id) in &recovered_orchestrations {
        sqlx::query(
            r#"
            INSERT INTO audit_events (
                project_id, event_type, actor_username, summary, details_json
            ) VALUES ($1, 'agent.orchestration.interrupted', 'system', $2, $3)
            "#,
        )
        .bind(project_id)
        .bind(format!("回收失联 Agent 编排 {orchestration_id}"))
        .bind(json!({
            "orchestrationId": orchestration_id,
            "failureKind": "execution_interrupted"
        }))
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(recovered_orchestrations.len() as u64)
}

/// Cache ownership prevents a cancelled/recovered older attempt from stopping a retry.
pub(crate) async fn stop_decision_explanation_attempts(
    connection: &mut sqlx::PgConnection,
    orchestration_ids: &[i64],
    message: &str,
) -> Result<u64, sqlx::Error> {
    // Older deployments can recover other Agent types before explanation migrations.
    let schema_ready = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid = to_regclass('public.decision_run_explanations') AND attname = 'agent_run_id' AND NOT attisdropped)",
    )
    .fetch_one(&mut *connection)
    .await?;
    if !schema_ready {
        return Ok(0);
    }
    let result = sqlx::query(
        r#"UPDATE decision_run_explanations explanation
           SET status = 'failed', explanation_json = NULL, error_message = $2,
               updated_at = NOW(), completed_at = NOW()
           FROM agent_runs run
           WHERE run.orchestration_id = ANY($1)
             AND run.stage = 'decision_explanation'
             AND explanation.agent_run_id = run.id
             AND explanation.status = 'running'"#,
    )
    .bind(orchestration_ids)
    .bind(message)
    .execute(&mut *connection)
    .await?;
    Ok(result.rows_affected())
}

fn orchestration_interrupted_error() -> Value {
    json!({
        "message": "编排执行实例失联，系统已回收未完成任务",
        "retryable": true,
        "failureKind": "execution_interrupted"
    })
}

pub(crate) async fn record_model_vote<T: Serialize>(
    db: &sqlx::PgPool,
    run_id: i64,
    task_type: &str,
    consensus: &Consensus<T>,
) -> Result<(), sqlx::Error> {
    let models_used: Vec<String> = consensus
        .outcomes
        .iter()
        .map(|outcome| outcome.model.clone())
        .collect();
    sqlx::query(
        r#"
        INSERT INTO agent_model_votes (
            run_id, task_type, models_used, consensus_result,
            divergence_detail, final_decision
        ) VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (run_id, task_type) DO UPDATE SET
            models_used = EXCLUDED.models_used,
            consensus_result = EXCLUDED.consensus_result,
            divergence_detail = EXCLUDED.divergence_detail,
            final_decision = EXCLUDED.final_decision
        "#,
    )
    .bind(run_id)
    .bind(task_type)
    .bind(models_used)
    .bind(json!({
        "confidence": consensus.confidence,
        "requiresHumanReview": consensus.requires_human_review,
        "earlyMajority": consensus.early_majority,
        "stoppedEarly": consensus.stopped_early,
        "stopReason": consensus.stop_reason,
    }))
    .bind(json!({"outcomes": consensus.outcomes}))
    .bind(serde_json::to_value(&consensus.decision).unwrap_or(Value::Null))
    .execute(db)
    .await
    .map(|_| ())
}

#[allow(clippy::too_many_arguments)]
async fn analyze_requirement_with_consensus(
    state: &AppState,
    run_id: i64,
    orchestration_id: i64,
    agent: &LlmAgent,
    agent_role: &str,
    risk_level: &str,
    complexity: RequirementComplexity,
    requirement: &str,
    knowledge_text: &str,
    model_context: Option<&str>,
    tx: Option<&mpsc::Sender<StreamEvent>>,
) -> anyhow::Result<RequirementAnalysis> {
    if let Some(tx) = tx {
        let _ = tx.send(StreamEvent::Stage("analysis".into())).await;
        let _ = tx
            .send(StreamEvent::Agent {
                role: agent_role.into(),
                status: "running".into(),
            })
            .await;
        let _ = tx
            .send(StreamEvent::Step(
                "编排阶段：正在识别业务模式并加载项目知识".into(),
            ))
            .await;
        let route = if risk_level == "high" {
            "项目为高风险，需求将进入多模型交叉验证"
        } else if complexity == RequirementComplexity::Complex {
            "复杂度较高，需求将进入多模型交叉验证"
        } else {
            match complexity {
                RequirementComplexity::Simple => "simple：由单模型分析智能体处理",
                RequirementComplexity::Standard => "standard：由单模型分析智能体处理",
                RequirementComplexity::Complex => unreachable!(),
            }
        };
        let _ = tx
            .send(StreamEvent::Step(format!("编排路由：{route}")))
            .await;
    }
    let pattern_detector = load_pattern_detector(&state.db).await;
    let detected_patterns = pattern_detector.detect_patterns(requirement);
    let enriched_knowledge = if detected_patterns.is_empty() {
        knowledge_text.to_string()
    } else {
        let hints = detected_patterns
            .iter()
            .map(|detection| {
                format!(
                    "- {}（置信度 {:.0}%{}）",
                    detection.pattern.key(),
                    detection.confidence * 100.0,
                    if detection.matched_keywords.is_empty() {
                        String::new()
                    } else {
                        format!("；命中：{}", detection.matched_keywords.join("、"))
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{knowledge_text}\n\n## 本地识别的业务模式候选（启发式线索，并非业务事实）\n{hints}\n请用这些线索辅助拆解；若与原始需求不符，以原始需求为准并提出澄清。"
        )
    };
    if risk_level != "high" && complexity != RequirementComplexity::Complex {
        if let Some(tx) = tx {
            let _ = tx
                .send(StreamEvent::Step(
                    "编排阶段：需求分析智能体正在提取目标、字段和规则".into(),
                ))
                .await;
        }
        let analysis = run_until_stream_disconnect(
            tx,
            wait_for_orchestration_cancellation(&state.db, orchestration_id),
            async {
                let result = match tx {
                    Some(tx) => {
                        agent
                            .analyze_requirement_stream(
                                requirement,
                                Some(&enriched_knowledge),
                                model_context,
                                tx,
                            )
                            .await
                    }
                    None => {
                        agent
                            .analyze_requirement(
                                requirement,
                                Some(&enriched_knowledge),
                                model_context,
                            )
                            .await
                    }
                };
                result.map_err(|error| anyhow!(error.to_string()))
            },
        )
        .await?;
        return postprocess_requirement_analysis(analysis, detected_patterns, tx).await;
    }

    let model_names = agent.parallel_model_names(3);
    if model_names.len() < 2 {
        if let Some(tx) = tx {
            let _ = tx
                .send(StreamEvent::Step(
                    "编排阶段：需求分析智能体正在提取目标、字段和规则".into(),
                ))
                .await;
        }
        let analysis = run_until_stream_disconnect(
            tx,
            wait_for_orchestration_cancellation(&state.db, orchestration_id),
            async {
                let result = match tx {
                    Some(tx) => {
                        agent
                            .analyze_requirement_stream(
                                requirement,
                                Some(&enriched_knowledge),
                                model_context,
                                tx,
                            )
                            .await
                    }
                    None => {
                        agent
                            .analyze_requirement(
                                requirement,
                                Some(&enriched_knowledge),
                                model_context,
                            )
                            .await
                    }
                };
                result.map_err(|error| anyhow!(error.to_string()))
            },
        )
        .await?;
        return postprocess_requirement_analysis(analysis, detected_patterns, tx).await;
    }
    if let Some(tx) = tx {
        let _ = tx
            .send(StreamEvent::Step(
                "编排阶段：需求分析智能体正在并行调用多个模型进行交叉验证".into(),
            ))
            .await;
    }
    let model_agents = agent.parallel_variants(3);
    let candidates = model_agents
        .iter()
        .map(|candidate| VoteCandidate {
            model: candidate.model().to_string(),
            weight: 1.0,
        })
        .collect();
    let requirement = requirement.to_string();
    let knowledge_text = enriched_knowledge;
    let model_context = model_context.map(str::to_string);
    let progress_tx = tx.cloned();
    let consensus_key: fn(&RequirementAnalysis) -> anyhow::Result<String> =
        if agent_role == "facilitator" {
            facilitator_consensus_key
        } else {
            requirement_analysis_consensus_key
        };
    let voting = parallel_vote_by(
        candidates,
        move |model| {
            let agent = model_agents
                .iter()
                .find(|agent| agent.model() == model)
                .cloned();
            let requirement = requirement.clone();
            let knowledge_text = knowledge_text.clone();
            let model_context = model_context.clone();
            let progress_tx = progress_tx.clone();
            async move {
                if let Some(tx) = &progress_tx {
                    let _ = tx
                        .send(StreamEvent::Step(format!("模型 {model} 开始需求分析")))
                        .await;
                }
                let result = match agent {
                    Some(agent) => agent
                        .analyze_requirement(
                            &requirement,
                            Some(&knowledge_text),
                            model_context.as_deref(),
                        )
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|analysis| {
                            validate_analysis(&analysis)
                                .map(|()| analysis)
                                .map_err(|error| error.message)
                        }),
                    None => Err(format!("并行模型不存在：{model}")),
                };
                if let Some(tx) = progress_tx {
                    let status = if result.is_ok() {
                        "完成需求分析"
                    } else {
                        "需求分析失败"
                    };
                    let _ = tx
                        .send(StreamEvent::Step(format!("模型 {model} {status}")))
                        .await;
                }
                result
            }
        },
        consensus_key,
    );
    let consensus = run_until_stream_disconnect(
        tx,
        wait_for_orchestration_cancellation(&state.db, orchestration_id),
        voting,
    )
    .await?;

    record_model_vote(&state.db, run_id, "analyze", &consensus).await?;
    if consensus
        .outcomes
        .iter()
        .all(|outcome| outcome.result.is_err())
    {
        return Err(anyhow!(
            "多模型均未返回有效需求分析候选，请检查模型配置后重试"
        ));
    }
    let strict_majority = consensus.decision.is_some();
    let mut analysis = match consensus.decision {
        Some(analysis) => analysis,
        None => plurality_requirement_analysis(&consensus.outcomes, consensus_key)?
            .ok_or_else(|| anyhow!("多模型均未返回有效需求分析候选，请检查模型配置后重试"))?,
    };
    if !strict_majority && let Some(tx) = tx {
        let _ = tx
            .send(StreamEvent::Step(
                "模型结果未形成严格多数，已按票数选择代表候选（平票按配置顺序），并将分歧转为业务澄清项".into(),
            ))
            .await;
    }
    if agent_role == "facilitator" {
        merge_facilitator_clarifications(&mut analysis, &consensus.outcomes);
    } else {
        add_consensus_divergence_clarifications(&mut analysis, &consensus.outcomes);
    }
    let valid_models = consensus
        .outcomes
        .iter()
        .filter(|outcome| outcome.result.is_ok())
        .count();
    if !strict_majority && valid_models == 1 {
        add_single_model_confirmation(&mut analysis, agent_role == "facilitator");
    }
    let analysis = postprocess_requirement_analysis(analysis, detected_patterns, tx).await?;
    if !strict_majority {
        sqlx::query(
            r#"UPDATE agent_model_votes
               SET divergence_detail = divergence_detail || jsonb_build_object(
                       'resolution', jsonb_build_object(
                           'strategy', $1::TEXT, 'requiresBusinessConfirmation', TRUE
                       )
                   )
               WHERE run_id = $2 AND task_type = 'analyze'"#,
        )
        .bind(if agent_role == "facilitator" {
            "plurality_with_merged_clarifications"
        } else {
            "plurality_with_required_business_clarifications"
        })
        .bind(run_id)
        .execute(&state.db)
        .await?;
    }
    Ok(analysis)
}

fn plurality_requirement_analysis(
    outcomes: &[crate::agent_runtime::orchestration::VoteOutcome],
    key: fn(&RequirementAnalysis) -> anyhow::Result<String>,
) -> anyhow::Result<Option<RequirementAnalysis>> {
    let mut groups = HashMap::<String, (f64, usize, RequirementAnalysis)>::new();
    for (index, outcome) in outcomes.iter().enumerate() {
        let Ok(value) = &outcome.result else {
            continue;
        };
        let Ok(analysis) = serde_json::from_value::<RequirementAnalysis>(value.clone()) else {
            continue;
        };
        validate_analysis(&analysis).map_err(|error| anyhow!(error.message))?;
        let group_key = key(&analysis)?;
        let group = groups
            .entry(group_key)
            .or_insert_with(|| (0.0, index, analysis.clone()));
        group.0 += outcome.weight;
    }
    Ok(groups
        .into_values()
        .max_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| right.1.cmp(&left.1))
        })
        .map(|(_, _, analysis)| analysis))
}

fn add_single_model_confirmation(analysis: &mut RequirementAnalysis, facilitator: bool) {
    let question = if facilitator {
        "本轮只有一个模型返回有效澄清结果。请确认是否还需补充业务问题；如无需补充，请回答“无”。"
    } else {
        "本轮只有一个模型返回有效需求分析，尚未形成交叉验证。请确认当前需求摘要是否准确；如需修正，请说明差异。"
    };
    let mut digest = Sha256::new();
    digest.update(question.as_bytes());
    let id = format!("consensus-{}", &format!("{:x}", digest.finalize())[..12]);
    if analysis.clarifications.iter().any(|item| item.id == id) {
        return;
    }
    analysis.clarifications.insert(
        0,
        Clarification {
            id,
            question: question.into(),
            required: true,
            reason: "仅有一个有效模型结果，需要业务人员确认".into(),
            ..Default::default()
        },
    );
}

fn merge_facilitator_clarifications(
    analysis: &mut RequirementAnalysis,
    outcomes: &[crate::agent_runtime::orchestration::VoteOutcome],
) {
    let mut by_question = HashMap::<String, usize>::new();
    let mut merged = Vec::<Clarification>::new();
    for item in analysis.clarifications.drain(..) {
        let key = normalize_clarification_question(&item.question);
        if key.is_empty() {
            continue;
        }
        by_question.insert(key, merged.len());
        merged.push(item);
    }
    for outcome in outcomes {
        let Ok(value) = &outcome.result else {
            continue;
        };
        let Ok(candidate) = serde_json::from_value::<RequirementAnalysis>(value.clone()) else {
            continue;
        };
        for item in candidate.clarifications {
            let key = normalize_clarification_question(&item.question);
            if key.is_empty() {
                continue;
            }
            if let Some(index) = by_question.get(&key).copied() {
                let current = &mut merged[index];
                let mut options = current.options.iter().cloned().collect::<HashSet<_>>();
                current.options.extend(
                    item.options
                        .into_iter()
                        .filter(|option| options.insert(option.clone())),
                );
                current.required |= item.required;
                if current.reason.trim().is_empty() {
                    current.reason = item.reason;
                }
            } else {
                by_question.insert(key, merged.len());
                merged.push(item);
            }
        }
    }
    analysis.clarifications = merged;
}

fn requirement_analysis_consensus_key(analysis: &RequirementAnalysis) -> anyhow::Result<String> {
    fn normalized(value: &str) -> String {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }

    fn fields(fields: &[DecisionField]) -> Vec<Value> {
        let mut fields = fields
            .iter()
            .map(|field| {
                json!({
                    "name": normalized(&field.name),
                    "type": normalized(&field.type_ref),
                    "unit": normalized(&field.unit),
                    "required": field.required,
                })
            })
            .collect::<Vec<_>>();
        fields.sort_by_key(Value::to_string);
        fields
    }

    Ok(serde_json::to_string(&json!({
        "name": normalized(&analysis.brief.name),
        "riskLevel": normalized(&analysis.brief.risk_level),
        "inputs": fields(&analysis.brief.inputs),
        "outputs": fields(&analysis.brief.outputs),
    }))?)
}

fn facilitator_consensus_key(analysis: &RequirementAnalysis) -> anyhow::Result<String> {
    let mut clarifications = analysis
        .clarifications
        .iter()
        .map(|item| {
            let mut options = item
                .options
                .iter()
                .map(|value| normalize_clarification_question(value))
                .collect::<Vec<_>>();
            options.sort();
            options.dedup();
            json!({
                "question": normalize_clarification_question(&item.question),
                "options": options,
                "required": item.required,
            })
        })
        .collect::<Vec<_>>();
    clarifications.sort_by_key(Value::to_string);
    Ok(serde_json::to_string(&clarifications)?)
}

fn add_consensus_divergence_clarifications(
    analysis: &mut RequirementAnalysis,
    outcomes: &[crate::agent_runtime::orchestration::VoteOutcome],
) {
    fn normalized(value: &str) -> String {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }

    let candidates = outcomes
        .iter()
        .filter_map(|outcome| {
            let value = outcome.result.as_ref().ok()?;
            serde_json::from_value::<RequirementAnalysis>(value.clone())
                .ok()
                .map(|candidate| (outcome.model.as_str(), candidate))
        })
        .collect::<Vec<_>>();
    if candidates.len() < 2 {
        return;
    }

    let mut questions = Vec::new();
    let unique_values = |values: Vec<String>| {
        let mut seen = HashSet::new();
        values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty() && seen.insert(value.to_lowercase()))
            .collect::<Vec<_>>()
    };

    let risks = unique_values(
        candidates
            .iter()
            .map(|(_, candidate)| candidate.brief.risk_level.clone())
            .collect(),
    );
    if risks.len() > 1 {
        let risk_rank = |risk: &str| match risk.to_lowercase().as_str() {
            "high" => 3,
            "medium" => 2,
            _ => 1,
        };
        if let Some(highest) = risks.iter().max_by_key(|risk| risk_rank(risk)) {
            analysis.brief.risk_level = highest.to_lowercase();
        }
        questions.push((
            "请确认该决策的风险等级。模型对风险判断不一致；在确认前按较高风险等级处理。"
                .to_string(),
            risks,
        ));
    }

    let goals = unique_values(
        candidates
            .iter()
            .map(|(_, candidate)| candidate.brief.goal.clone())
            .collect(),
    );
    if goals.len() > 1 {
        questions.push(("请确认本决策需要实现的业务目标。".to_string(), goals));
    }

    let names = unique_values(
        candidates
            .iter()
            .map(|(_, candidate)| candidate.brief.name.clone())
            .collect(),
    );
    let mut business_differences = if names.len() > 1 {
        names
            .into_iter()
            .map(|name| format!("决策名称：{name}"))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let render_fields = |fields: &[DecisionField]| {
        let mut fields = fields
            .iter()
            .map(|field| {
                format!(
                    "{}:{}{}{}",
                    normalized(&field.name),
                    normalized(&field.type_ref),
                    if field.unit.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", normalized(&field.unit))
                    },
                    if field.required { "（必填）" } else { "" }
                )
            })
            .collect::<Vec<_>>();
        fields.sort();
        fields.join("、")
    };
    let selected_schema = format!(
        "输入：{}；输出：{}",
        render_fields(&analysis.brief.inputs),
        render_fields(&analysis.brief.outputs)
    );
    let field_differences = unique_values(
        candidates
            .iter()
            .map(|(model, candidate)| {
                let schema = format!(
                    "输入：{}；输出：{}",
                    render_fields(&candidate.brief.inputs),
                    render_fields(&candidate.brief.outputs)
                );
                if schema == selected_schema {
                    String::new()
                } else {
                    format!("{model}：{schema}")
                }
            })
            .collect(),
    );
    business_differences.extend(field_differences);

    let constraint_sets = candidates
        .iter()
        .map(|(_, candidate)| {
            let mut constraints = candidate
                .brief
                .constraints
                .iter()
                .map(|constraint| normalized(constraint))
                .collect::<Vec<_>>();
            constraints.sort();
            constraints.join("\0")
        })
        .collect::<HashSet<_>>();
    if constraint_sets.len() > 1 {
        business_differences.extend(candidates.iter().flat_map(|(_, candidate)| {
            candidate
                .brief
                .constraints
                .iter()
                .map(|constraint| format!("业务约束：{}", constraint))
                .collect::<Vec<_>>()
        }));
        business_differences = unique_values(business_differences);
    }
    if !business_differences.is_empty() {
        questions.push((
            "模型对输入输出字段或业务规则存在分歧，请逐项确认适用内容。".to_string(),
            business_differences,
        ));
    }

    for (question, options) in questions {
        let key = format!("{}:{}", question, options.join("|"));
        let mut digest = Sha256::new();
        digest.update(key.as_bytes());
        let id = format!("consensus-{}", &format!("{:x}", digest.finalize())[..12]);
        if analysis.clarifications.iter().any(|item| item.id == id) {
            continue;
        }
        analysis.clarifications.insert(
            0,
            Clarification {
                id,
                question,
                options,
                required: true,
                reason: "多模型共识外的差异需要业务确认".into(),
                round: 1,
            },
        );
    }
}

async fn postprocess_requirement_analysis(
    analysis: RequirementAnalysis,
    detected_patterns: Vec<DetectedPattern>,
    tx: Option<&mpsc::Sender<StreamEvent>>,
) -> anyhow::Result<RequirementAnalysis> {
    if let Some(tx) = tx {
        let _ = tx
            .send(StreamEvent::Step(
                "编排阶段：业务模式识别器正在补充可识别的模式线索".into(),
            ))
            .await;
    }
    let analysis = serde_json::to_value(analysis)?;
    let pattern_stage = PipelineStage::new("attach_detected_patterns", move |value| {
        let detected_patterns = detected_patterns.clone();
        async move {
            let analysis = serde_json::from_value::<RequirementAnalysis>(value)
                .map_err(|error| error.to_string())?;
            serde_json::to_value(attach_detected_patterns(analysis, &detected_patterns))
                .map_err(|error| error.to_string())
        }
    });
    let clarification_stage = PipelineStage::new("prioritize_clarifications", |value| async move {
        let analysis = serde_json::from_value::<RequirementAnalysis>(value)
            .map_err(|error| error.to_string())?;
        serde_json::to_value(prioritize_clarifications(analysis)).map_err(|error| error.to_string())
    });
    let first = run_pipeline(vec![pattern_stage], analysis).await?;
    if let Some(tx) = tx {
        let _ = tx
            .send(StreamEvent::Step(
                "编排阶段：澄清排序器正在合并重复问题并排列优先级".into(),
            ))
            .await;
    }
    let report = run_pipeline(vec![clarification_stage], first.output).await?;
    let mut analysis = serde_json::from_value::<RequirementAnalysis>(report.output)?;
    analysis.brief.requirement_items = requirement_items(&analysis.brief);
    analysis
        .clarifications
        .truncate(MAX_CLARIFICATIONS_PER_ROUND);
    for clarification in &mut analysis.clarifications {
        clarification.round = 1;
    }
    Ok(analysis)
}

fn requirement_items(brief: &DecisionBrief) -> Vec<RequirementItem> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |source: String, text: String| {
        if let Some(item) = RequirementItem::new(source, text)
            && seen.insert(item.id.clone())
        {
            items.push(item);
        }
    };
    push("goal".into(), brief.goal.clone());
    if !brief.description.trim().is_empty() {
        push("description".into(), brief.description.clone());
    }
    for field in &brief.inputs {
        let detail = [field.label.trim(), field.description.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("：");
        push(
            format!("input:{}", field.name),
            if detail.is_empty() {
                format!("输入字段 {}", field.name)
            } else {
                format!("输入字段 {}：{detail}", field.name)
            },
        );
    }
    for field in &brief.outputs {
        let detail = [field.label.trim(), field.description.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("：");
        push(
            format!("output:{}", field.name),
            if detail.is_empty() {
                format!("输出字段 {}", field.name)
            } else {
                format!("输出字段 {}：{detail}", field.name)
            },
        );
    }
    for constraint in &brief.constraints {
        push("constraint".into(), constraint.clone());
    }
    for term in &brief.terms {
        push(
            format!("term:{}", term.term),
            format!("业务术语 {}：{}", term.term, term.meaning),
        );
    }
    for assumption in &brief.assumptions {
        push("assumption".into(), assumption.clone());
    }
    items
}

fn attach_detected_patterns(
    mut analysis: RequirementAnalysis,
    detected_patterns: &[DetectedPattern],
) -> RequirementAnalysis {
    analysis.brief.detected_patterns = detected_patterns
        .iter()
        .map(|detection| DetectedBusinessPattern {
            pattern_type: detection.pattern.key().to_string(),
            confidence_percent: (detection.confidence * 100.0).round().clamp(0.0, 100.0) as u8,
            matched_keywords: detection.matched_keywords.clone(),
        })
        .collect();
    analysis
}

fn prioritize_clarifications(mut analysis: RequirementAnalysis) -> RequirementAnalysis {
    let mut unique = Vec::<Clarification>::with_capacity(analysis.clarifications.len());
    let mut positions = HashMap::<String, usize>::new();
    for clarification in analysis.clarifications.drain(..) {
        let key = clarification
            .question
            .chars()
            .filter(|character| !character.is_whitespace() && !character.is_ascii_punctuation())
            .flat_map(char::to_lowercase)
            .collect::<String>();
        if !key.is_empty() && positions.contains_key(&key) {
            let index = positions[&key];
            let existing = &mut unique[index];
            existing.required |= clarification.required;
            if existing.reason.trim().is_empty() {
                existing.reason = clarification.reason;
            }
            if existing.options.is_empty() {
                existing.options = clarification.options;
            }
        } else {
            if !key.is_empty() {
                positions.insert(key, unique.len());
            }
            unique.push(clarification);
        }
    }
    unique.sort_by_key(|clarification| !clarification.required);
    analysis.clarifications = unique;
    analysis
}

async fn load_pattern_detector(db: &sqlx::PgPool) -> PatternDetector {
    let rows = sqlx::query_as::<_, (String, Vec<String>, f64)>(
        "SELECT pattern_type, keywords, COALESCE(success_rate::FLOAT8, 50.0) FROM business_logic_patterns",
    )
    .fetch_all(db)
    .await;
    match rows {
        Ok(rows) => {
            let definitions = rows
                .into_iter()
                .filter_map(|(pattern_type, keywords, success_rate)| {
                    Some(PatternDefinition {
                        pattern: BusinessLogicPattern::parse(&pattern_type)?,
                        keywords,
                        historical_success_rate: (success_rate / 100.0) as f32,
                    })
                })
                .collect();
            PatternDetector::from_definitions(definitions)
        }
        Err(error) => {
            eprintln!("读取业务模式库失败，使用内置规则：{error}");
            PatternDetector::default()
        }
    }
}

async fn get_workflow(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Option<DecisionWorkflowState>>, WorkflowApiError> {
    ensure_project(&state, project_id, &claims.sub).await?;
    Ok(Json(load_workflow(&state, project_id, &claims.sub).await?))
}

async fn model_baseline(
    state: &AppState,
    project_id: i64,
) -> Result<(Option<i64>, Option<String>), WorkflowApiError> {
    let baseline = sqlx::query_as::<_, (Option<i64>, Option<String>)>(
        "SELECT project.current_draft_version_id, artifact.dsl_yaml FROM decision_projects project LEFT JOIN decision_artifacts artifact ON artifact.version_id = project.current_draft_version_id WHERE project.id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取需求分析模型基线", error))?
    .ok_or_else(WorkflowApiError::not_found)?;
    if baseline.0.is_some()
        && baseline
            .1
            .as_deref()
            .is_none_or(|dsl| dsl.trim().is_empty())
    {
        return Err(WorkflowApiError::bad_request(
            "当前草稿缺少 DSL，无法分析变更范围",
        ));
    }
    Ok(baseline)
}

fn original_requirement(workflow: &DecisionWorkflowState) -> Option<&str> {
    workflow.original_requirement_text.as_deref().or_else(|| {
        workflow
            .base_revision
            .is_none()
            .then_some(workflow.requirement_text.as_str())
    })
}

fn requirement_origin(
    requirement: &str,
    base_revision: Option<i64>,
    previous: Option<&DecisionWorkflowState>,
) -> Option<String> {
    if base_revision.is_none() {
        return Some(requirement.to_string());
    }
    previous.and_then(original_requirement).map(str::to_string)
}

fn analysis_model_context(
    existing_dsl: Option<&str>,
    previous: Option<&DecisionWorkflowState>,
) -> Option<String> {
    existing_dsl.map(|dsl| json!({
        "currentDsl": dsl,
        "originalRequirement": previous.and_then(original_requirement),
        "latestRequirement": previous.map(|workflow| &workflow.requirement_text),
        "changePolicy": "结合原始需求和当前模型理解本次需求；明确新增、调整与保留的业务规则，未提出变更的规则必须保留，不得重建或删减整个模型。原始需求仅为业务背景，当前 DSL 是已生效规则基线，本次需求优先，不得将历史规则恢复或覆盖本次调整。",
    }).to_string())
}

pub(crate) async fn confirmed_generation_prompt(
    state: &AppState,
    project_id: i64,
    username: &str,
    base_revision: Option<i64>,
    expected_updated_at: Option<DateTime<Utc>>,
) -> Result<(String, Option<i64>, DecisionPlan), WorkflowApiError> {
    ensure_model_permission(state, project_id, username).await?;
    let (current_revision, _) = model_baseline(state, project_id).await?;
    if current_revision != base_revision {
        return Err(WorkflowApiError::conflict(
            "当前模型基线已变化，请刷新后重新分析",
        ));
    }
    let workflow = load_workflow(state, project_id, username)
        .await?
        .ok_or_else(|| WorkflowApiError::bad_request("请先分析业务需求并规划方案"))?;
    if Some(workflow.updated_at) != expected_updated_at {
        return Err(WorkflowApiError::conflict(
            "方案已变化，请刷新并核对已确认方案后生成模型",
        ));
    }
    let prompt = workflow
        .generation_prompt(base_revision)
        .map_err(WorkflowApiError::conflict)?;
    let plan = workflow
        .plan
        .clone()
        .ok_or_else(|| WorkflowApiError::bad_request("已确认工作流缺少决策方案，请重新确认方案"))?;
    let parent_orchestration_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT plan_orchestration_id FROM decision_workflows WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取方案编排关联", error))?
    .flatten();
    Ok((prompt, parent_orchestration_id, plan))
}

pub(crate) async fn simple_direct_generation_prompt(
    state: &AppState,
    project_id: i64,
    username: &str,
    base_revision: Option<i64>,
    expected_updated_at: Option<DateTime<Utc>>,
) -> Result<(String, Option<i64>, DecisionPlan), WorkflowApiError> {
    ensure_model_permission(state, project_id, username).await?;
    if base_revision.is_some() {
        return Err(WorkflowApiError::conflict(
            "simple 直达生成仅用于没有现有模型的新项目",
        ));
    }
    let workflow = load_workflow(state, project_id, username)
        .await?
        .ok_or_else(WorkflowApiError::not_found)?;
    if Some(workflow.updated_at) != expected_updated_at {
        return Err(WorkflowApiError::conflict(
            "当前需求不满足低风险 simple 直达生成条件，请先完成人工方案规划与确认",
        ));
    }
    let plan = simple_direct_plan(&workflow)?;
    let prompt = format!(
        "## 生成任务\n将低风险 simple 需求转换为可执行 DMN DSL 候选。以下结构依据由 Analyst 需求卡自动整理，未经业务人员确认；不得把它描述为已确认方案，也不得超出需求卡扩展业务规则。生成结果仅供人工检查，保存草稿仍需用户显式确认。\n\n## 临时结构依据\n{}\n\n## 需求摘要\n{}\n\n## 原始需求\n{}\n\n## 需求溯源要求\n每个决策的 requirementIds 必须来自临时结构依据中对应决策的 requirementIds。不得编造、改写或引用结构依据之外的 ID。",
        serde_json::to_string_pretty(&plan).map_err(|error| {
            WorkflowApiError::bad_request(format!("序列化 simple 结构依据失败：{error}"))
        })?,
        serde_json::to_string_pretty(&workflow.brief).map_err(|error| {
            WorkflowApiError::bad_request(format!("序列化需求卡失败：{error}"))
        })?,
        workflow.requirement_text
    );
    let parent_orchestration_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT analysis_orchestration_id FROM decision_workflows WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取 simple 生成的分析编排", error))?
    .flatten();
    Ok((prompt, parent_orchestration_id, plan))
}

async fn load_pending_requirement_review(
    state: &AppState,
    project_id: i64,
) -> Result<Option<PendingRequirementReview>, WorkflowApiError> {
    let row = sqlx::query_as::<_, (i64, Value, Value)>(
        r#"SELECT run.id, run.input_json, vote.divergence_detail
           FROM agent_model_votes vote
           JOIN agent_runs run ON run.id = vote.run_id
           WHERE run.project_id = $1
             AND run.stage = 'requirement_analysis'
             AND run.status IN ('failed', 'running')
             AND vote.task_type = 'analyze'
             AND (vote.final_decision IS NULL OR vote.final_decision = 'null'::JSONB)
             AND vote.consensus_result->>'requiresHumanReview' = 'true'
           ORDER BY run.started_at DESC
           LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取待人工复核的需求分析", error))?;
    let Some((run_id, input, divergence)) = row else {
        return Ok(None);
    };
    let requirement_text = input
        .get("requirement")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let candidates = requirement_review_candidates(&divergence);
    if candidates.is_empty() {
        return Ok(None);
    }
    let purpose = if input
        .get("facilitatorRound")
        .and_then(Value::as_u64)
        .is_some_and(|round| round > 1)
    {
        "facilitator"
    } else {
        "analysis"
    };
    Ok(Some(PendingRequirementReview {
        run_id,
        purpose: purpose.into(),
        requirement_text,
        candidates,
    }))
}

fn requirement_review_candidates(divergence: &Value) -> Vec<RequirementReviewCandidate> {
    let mut seen = HashSet::new();
    divergence
        .get("outcomes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|outcome| {
            let model = outcome.get("model")?.as_str()?.to_string();
            let result = outcome.get("result")?;
            let analysis_value = result
                .get("Ok")
                .or_else(|| result.get("ok"))
                .or_else(|| result.get("success"))
                .or_else(|| result.get("analysis"))
                .unwrap_or(result);
            let analysis =
                serde_json::from_value::<RequirementAnalysis>(analysis_value.clone()).ok()?;
            validate_analysis(&analysis).ok()?;
            if !seen.insert(model.clone()) {
                return None;
            }
            Some(RequirementReviewCandidate { model, analysis })
        })
        .collect()
}

fn requirement_review_baseline_is_current(
    baseline_recorded: bool,
    run_baseline: Option<i64>,
    current_baseline: Option<i64>,
    run_started_at: DateTime<Utc>,
    project_updated_at: DateTime<Utc>,
    current_version_created_at: Option<DateTime<Utc>>,
) -> bool {
    if baseline_recorded {
        return run_baseline == current_baseline;
    }

    if project_updated_at > run_started_at {
        return false;
    }
    match current_version_created_at {
        Some(created_at) => created_at <= run_started_at,
        None => current_baseline.is_none(),
    }
}

async fn pending_requirement_review(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Option<PendingRequirementReview>>, WorkflowApiError> {
    ensure_model_permission(&state, project_id, &claims.sub).await?;
    Ok(Json(
        load_pending_requirement_review(&state, project_id).await?,
    ))
}

async fn confirm_requirement_review(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<ConfirmRequirementReviewRequest>,
) -> Result<Json<DecisionWorkflowState>, WorkflowApiError> {
    ensure_model_permission(&state, project_id, &claims.sub).await?;
    let pending = load_pending_requirement_review(&state, project_id)
        .await?
        .filter(|pending| pending.run_id == payload.run_id)
        .ok_or_else(|| WorkflowApiError::conflict("待复核分析已变化，请刷新后重新分析"))?;
    let candidate = pending
        .candidates
        .iter()
        .find(|candidate| candidate.model == payload.model)
        .ok_or_else(|| WorkflowApiError::bad_request("所选模型结果不在待复核候选中"))?;
    validate_analysis(&candidate.analysis)?;

    let previous = load_workflow(&state, project_id, &claims.sub).await?;
    let (base_revision, _) = model_baseline(&state, project_id).await?;
    let baseline_row = sqlx::query_as::<
        _,
        (
            Value,
            bool,
            Option<i64>,
            DateTime<Utc>,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
        ),
    >(
        r#"SELECT run.input_json,
                  run.input_json ? 'baseRevision',
                  NULLIF(run.input_json->>'baseRevision', '')::BIGINT,
                  run.started_at, project.updated_at, version.created_at
           FROM agent_runs run
           JOIN decision_projects project ON project.id = run.project_id
           LEFT JOIN decision_versions version
             ON version.id = project.current_draft_version_id
           WHERE run.id = $1 AND run.project_id = $2"#,
    )
    .bind(payload.run_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取需求分析基线", error))?
    .ok_or_else(WorkflowApiError::not_found)?;
    let run_input = &baseline_row.0;
    if !requirement_review_baseline_is_current(
        baseline_row.1,
        baseline_row.2,
        base_revision,
        baseline_row.3,
        baseline_row.4,
        baseline_row.5,
    ) {
        return Err(WorkflowApiError::conflict(
            "模型基线已变化，该候选不再适用于当前项目，请重新分析需求",
        ));
    }
    let orchestration_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT orchestration_id FROM agent_runs WHERE id = $1 AND project_id = $2",
    )
    .bind(payload.run_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取需求分析编排记录", error))?
    .flatten()
    .ok_or_else(WorkflowApiError::not_found)?;
    if run_input
        .get("facilitatorRound")
        .and_then(Value::as_u64)
        .is_some_and(|round| round > 1)
    {
        let previous = previous
            .ok_or_else(|| WorkflowApiError::conflict("原澄清工作流已不存在，请重新分析需求"))?;
        return Ok(Json(
            confirm_facilitator_requirement_review(
                &state,
                project_id,
                &claims.sub,
                payload.run_id,
                orchestration_id,
                base_revision,
                run_input,
                &previous,
                candidate,
            )
            .await?,
        ));
    }
    let detector = load_pattern_detector(&state.db).await;
    let detected_patterns = detector.detect_patterns(&pending.requirement_text);
    let analysis =
        postprocess_requirement_analysis(candidate.analysis.clone(), detected_patterns, None)
            .await
            .map_err(|error| WorkflowApiError::bad_request(format!("复核候选处理失败：{error}")))?;
    let workflow = save_requirement_analysis(
        &state,
        project_id,
        &claims.sub,
        &pending.requirement_text,
        &analysis,
        orchestration_id,
        base_revision,
        previous.as_ref(),
    )
    .await?;
    let updated = sqlx::query(
        r#"UPDATE agent_model_votes
           SET final_decision = $1,
               divergence_detail = divergence_detail || jsonb_build_object(
                   'humanReview', jsonb_build_object(
                       'reviewer', $2::TEXT, 'selectedModel', $3::TEXT, 'reviewedAt', NOW()
                   )
               )
           WHERE run_id = $4 AND task_type = 'analyze'
             AND (final_decision IS NULL OR final_decision = 'null'::JSONB)"#,
    )
    .bind(serde_json::to_value(&analysis).unwrap_or(Value::Null))
    .bind(&claims.sub)
    .bind(&payload.model)
    .bind(payload.run_id)
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("记录需求分析人工复核", error))?;
    if updated.rows_affected() == 0 {
        return Err(WorkflowApiError::conflict("该分析已完成复核，请刷新页面"));
    }
    sqlx::query(
        "UPDATE agent_runs SET status = 'succeeded', output_json = $1, error_json = NULL, finished_at = NOW() WHERE id = $2 AND project_id = $3 AND status = 'failed'",
    )
    .bind(serde_json::to_value(&analysis).unwrap_or(Value::Null))
    .bind(payload.run_id)
    .bind(project_id)
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("更新人工复核运行状态", error))?;
    sqlx::query(
        r#"UPDATE agent_steps
           SET status = 'succeeded', error_json = NULL, finished_at = NOW()
           WHERE agent_run_id = $1 AND status = 'failed'"#,
    )
    .bind(payload.run_id)
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("更新人工复核步骤状态", error))?;
    sqlx::query(
        "UPDATE agent_orchestrations SET status = 'succeeded', current_stage = 'requirement_analysis', finished_at = NOW() WHERE id = $1 AND status = 'failed'",
    )
    .bind(orchestration_id)
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("更新人工复核编排状态", error))?;
    Ok(Json(workflow))
}

async fn confirm_facilitator_requirement_review(
    state: &AppState,
    project_id: i64,
    username: &str,
    run_id: i64,
    orchestration_id: i64,
    base_revision: Option<i64>,
    run_input: &Value,
    previous: &DecisionWorkflowState,
    candidate: &RequirementReviewCandidate,
) -> Result<DecisionWorkflowState, WorkflowApiError> {
    if previous.status != "clarification" || previous.base_revision != base_revision {
        return Err(WorkflowApiError::conflict(
            "澄清状态或模型基线已变化，请刷新后重新处理",
        ));
    }
    let answers = run_input
        .get("answeredClarifications")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<ClarificationAnswer>>(value).ok())
        .unwrap_or_else(|| previous.answers.clone());
    validate_answers(&previous.clarifications, &answers)?;
    let (status, clarifications, answers) = facilitator_review_update(
        &previous.clarifications,
        candidate.analysis.clarifications.clone(),
        answers,
    );
    let detector = load_pattern_detector(&state.db).await;
    let detected = detector.detect_patterns(&previous.requirement_text);
    let analysis = postprocess_requirement_analysis(candidate.analysis.clone(), detected, None)
        .await
        .map_err(|error| WorkflowApiError::bad_request(format!("复核候选处理失败：{error}")))?;

    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| WorkflowApiError::database("开始提交澄清主持人工复核", error))?;
    let workflow_update = sqlx::query(
        r#"UPDATE decision_workflows
           SET status = $1, clarifications_json = $2, answers_json = $3,
               decision_plan_json = NULL, tester_report_json = NULL,
               plan_orchestration_id = NULL, confirmed_at = NULL, updated_at = NOW()
           WHERE project_id = $4 AND updated_at = $5 AND status = 'clarification'
             AND base_revision IS NOT DISTINCT FROM $6
             AND EXISTS (SELECT 1 FROM decision_projects project WHERE project.id = $4
                 AND project.current_draft_version_id IS NOT DISTINCT FROM $6)"#,
    )
    .bind(status)
    .bind(serde_json::to_value(&clarifications).unwrap_or(Value::Array(vec![])))
    .bind(serde_json::to_value(&answers).unwrap_or(Value::Array(vec![])))
    .bind(project_id)
    .bind(previous.updated_at)
    .bind(base_revision)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("保存澄清主持人工复核结果", error))?;
    if workflow_update.rows_affected() == 0 {
        return Err(WorkflowApiError::conflict(
            "需求或模型基线已变化，本轮澄清未保存",
        ));
    }

    let final_decision = serde_json::to_value(&analysis).unwrap_or(Value::Null);
    let vote_update = sqlx::query(
        r#"UPDATE agent_model_votes
           SET final_decision = $1,
               divergence_detail = divergence_detail || jsonb_build_object(
                   'humanReview', jsonb_build_object(
                       'reviewer', $2::TEXT, 'selectedModel', $3::TEXT,
                       'reviewedAt', NOW(), 'taskKind', 'clarification_facilitation'
                   )
               )
           WHERE run_id = $4 AND task_type = 'analyze'
             AND (final_decision IS NULL OR final_decision = 'null'::JSONB)"#,
    )
    .bind(&final_decision)
    .bind(username)
    .bind(&candidate.model)
    .bind(run_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("记录澄清主持人工复核", error))?;
    if vote_update.rows_affected() == 0 {
        return Err(WorkflowApiError::conflict("该澄清主持结果已完成复核"));
    }
    let run_update = sqlx::query(
        r#"UPDATE agent_runs
           SET status = 'succeeded', output_json = $1, error_json = NULL, finished_at = NOW()
           WHERE id = $2 AND project_id = $3 AND status IN ('failed', 'running')"#,
    )
    .bind(final_decision)
    .bind(run_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("更新澄清主持运行状态", error))?;
    if run_update.rows_affected() == 0 {
        return Err(WorkflowApiError::conflict(
            "澄清主持运行状态已变化，请刷新后重试",
        ));
    }
    sqlx::query(
        r#"UPDATE agent_steps
           SET status = 'succeeded', error_json = NULL, finished_at = NOW()
           WHERE agent_run_id = $1 AND status IN ('pending', 'running', 'failed')"#,
    )
    .bind(run_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("更新澄清主持步骤状态", error))?;
    let orchestration_update = sqlx::query(
        r#"UPDATE agent_orchestrations
           SET status = 'succeeded', current_stage = 'facilitator', finished_at = NOW()
           WHERE id = $1 AND status IN ('failed', 'running')"#,
    )
    .bind(orchestration_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("更新澄清主持编排状态", error))?;
    if orchestration_update.rows_affected() == 0 {
        return Err(WorkflowApiError::conflict(
            "澄清主持编排状态已变化，请刷新后重试",
        ));
    }
    transaction
        .commit()
        .await
        .map_err(|error| WorkflowApiError::database("提交澄清主持人工复核", error))?;

    load_workflow(state, project_id, username)
        .await?
        .ok_or_else(WorkflowApiError::not_found)
}

async fn analyze_requirement(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<AnalyzeRequirementRequest>,
) -> Result<Json<DecisionWorkflowState>, WorkflowApiError> {
    let requirement = payload.text.trim();
    if requirement.is_empty() || requirement.len() > 20_000 {
        return Err(WorkflowApiError::bad_request(
            "需求描述不能为空且不能超过 20000 字节",
        ));
    }
    ensure_model_permission(&state, project_id, &claims.sub).await?;
    let complexity = classify_requirement_complexity(requirement);
    let risk_level = project_risk_level(&state, project_id).await?;
    let previous = load_workflow(&state, project_id, &claims.sub).await?;
    let (base_revision, existing_dsl) = model_baseline(&state, project_id).await?;
    let model_context = analysis_model_context(existing_dsl.as_deref(), previous.as_ref());
    let knowledge_sources = effective_project_sources(&state.db, project_id, &claims.sub)
        .await
        .map_err(|error| WorkflowApiError::database("读取项目有效知识", error))?;
    let knowledge_text =
        knowledge_context_for_query(&state.db, &knowledge_sources, requirement).await;
    let knowledge_citations = citations(&knowledge_sources);
    let agent_complexity = match complexity {
        RequirementComplexity::Simple => crate::api::AgentComplexity::Simple,
        RequirementComplexity::Standard => crate::api::AgentComplexity::Standard,
        RequirementComplexity::Complex => crate::api::AgentComplexity::Complex,
    };
    let (agent, experiment) = llm_state
        .analysis_agent_for_project(project_id, risk_level != "high", agent_complexity)
        .map(|(agent, experiment)| (agent.fork_for_run(), experiment))
        .ok_or_else(WorkflowApiError::unavailable)?;
    let parallel_model_count =
        if risk_level == "high" || complexity == RequirementComplexity::Complex {
            Some(agent.parallel_model_names(3).len())
        } else {
            None
        };
    let (run_id, orchestration_id) = begin_agent_run(
        &state,
        project_id,
        &claims.sub,
        LlmTask::RequirementAnalysis,
        &agent,
        experiment,
        parallel_model_count,
        None,
        payload.retry_of_orchestration_id,
        json!({
            "requirement": requirement,
            "complexity": complexity.as_str(),
            "baseRevision": base_revision,
            "knowledgeCitations": knowledge_citations,
        }),
    )
    .await?;
    let analysis = match analyze_requirement_with_consensus(
        &state,
        run_id,
        orchestration_id,
        &agent,
        "analyst",
        &risk_level,
        complexity,
        requirement,
        &knowledge_text,
        model_context.as_deref(),
        None,
    )
    .await
    {
        Ok(analysis) => analysis,
        Err(error) => {
            let message = error.to_string();
            let _ = finish_agent_run(&state, run_id, &agent, Err(message.clone())).await;
            return Err(WorkflowApiError::model("需求分析", error));
        }
    };
    if let Err(error) = validate_analysis(&analysis) {
        let _ = finish_agent_run(&state, run_id, &agent, Err(error.message.clone())).await;
        return Err(error);
    }
    let saved_workflow = match save_requirement_analysis(
        &state,
        project_id,
        &claims.sub,
        requirement,
        &analysis,
        orchestration_id,
        base_revision,
        previous.as_ref(),
    )
    .await
    {
        Ok(workflow) => workflow,
        Err(error) => {
            let _ = finish_agent_run(&state, run_id, &agent, Err(error.message.clone())).await;
            return Err(error);
        }
    };
    finish_agent_run(
        &state,
        run_id,
        &agent,
        Ok(serde_json::to_value(&analysis).unwrap_or(Value::Null)),
    )
    .await?;
    Ok(Json(saved_workflow))
}

async fn analyze_requirement_stream(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<AnalyzeRequirementRequest>,
) -> Response {
    let requirement = payload.text.trim().to_string();
    if requirement.is_empty() || requirement.len() > 20_000 {
        return WorkflowApiError::bad_request("需求描述不能为空且不能超过 20000 字节")
            .into_response();
    }
    if let Err(error) = ensure_model_permission(&state, project_id, &claims.sub).await {
        return error.into_response();
    }
    let complexity = classify_requirement_complexity(&requirement);
    let risk_level = match project_risk_level(&state, project_id).await {
        Ok(risk_level) => risk_level,
        Err(error) => return error.into_response(),
    };
    let previous = match load_workflow(&state, project_id, &claims.sub).await {
        Ok(workflow) => workflow,
        Err(error) => return error.into_response(),
    };
    let (base_revision, existing_dsl) = match model_baseline(&state, project_id).await {
        Ok(baseline) => baseline,
        Err(error) => return error.into_response(),
    };
    let model_context = analysis_model_context(existing_dsl.as_deref(), previous.as_ref());
    let knowledge_sources = match effective_project_sources(&state.db, project_id, &claims.sub)
        .await
    {
        Ok(value) => value,
        Err(error) => return WorkflowApiError::database("读取项目有效知识", error).into_response(),
    };
    let knowledge_text =
        knowledge_context_for_query(&state.db, &knowledge_sources, &requirement).await;
    let agent_complexity = match complexity {
        RequirementComplexity::Simple => crate::api::AgentComplexity::Simple,
        RequirementComplexity::Standard => crate::api::AgentComplexity::Standard,
        RequirementComplexity::Complex => crate::api::AgentComplexity::Complex,
    };
    let (agent, experiment) = match llm_state
        .analysis_agent_for_project(project_id, risk_level != "high", agent_complexity)
        .map(|(agent, experiment)| (agent.fork_for_run(), experiment))
    {
        Some(selection) => selection,
        None => return WorkflowApiError::unavailable().into_response(),
    };
    let (run_id, orchestration_id) = match begin_agent_run(
        &state,
        project_id,
        &claims.sub,
        LlmTask::RequirementAnalysis,
        &agent,
        experiment,
        if risk_level == "high" || complexity == RequirementComplexity::Complex {
            Some(agent.parallel_model_names(3).len())
        } else {
            None
        },
        None,
        payload.retry_of_orchestration_id,
        json!({
            "requirement": requirement,
            "complexity": complexity.as_str(),
            "baseRevision": base_revision,
            "knowledgeCitations": citations(&knowledge_sources),
        }),
    )
    .await
    {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let (tx, rx) = mpsc::channel::<StreamEvent>(32);
    let tx2 = tx.clone();
    let user_id = claims.sub.clone();
    let event_db = state.db.clone();
    tokio::spawn(async move {
        let result = analyze_requirement_with_consensus(
            &state,
            run_id,
            orchestration_id,
            &agent,
            "analyst",
            &risk_level,
            complexity,
            &requirement,
            &knowledge_text,
            model_context.as_deref(),
            Some(&tx2),
        )
        .await;
        let event = match result {
            Ok(analysis) => {
                let _ = tx
                    .send(StreamEvent::Step(
                        "编排阶段：工作流协调器正在校验并保存分析结果".into(),
                    ))
                    .await;
                match save_requirement_analysis(
                    &state,
                    project_id,
                    &user_id,
                    &requirement,
                    &analysis,
                    orchestration_id,
                    base_revision,
                    previous.as_ref(),
                )
                .await
                {
                    Ok(workflow) => {
                        if let Ok(brief) = serde_json::to_value(&workflow.brief) {
                            if let Ok(digest) = artifact_digest(&brief) {
                                let _ = tx2
                                    .send(StreamEvent::Artifact {
                                        kind: "decision_brief".into(),
                                        digest: digest.clone(),
                                    })
                                    .await;
                                let _ = tx2
                                    .send(StreamEvent::Handoff {
                                        from_role: "analyst".into(),
                                        to_role: "architect".into(),
                                        artifact_kind: "decision_brief".into(),
                                        digest,
                                    })
                                    .await;
                            }
                        }
                        let _ = tx2
                            .send(StreamEvent::Agent {
                                role: "analyst".into(),
                                status: "completed".into(),
                            })
                            .await;
                        let _ = finish_agent_run(
                            &state,
                            run_id,
                            &agent,
                            Ok(serde_json::to_value(&analysis).unwrap_or(Value::Null)),
                        )
                        .await;
                        StreamEvent::Finished(
                            serde_json::to_value(workflow).map_err(|e| e.to_string()),
                        )
                    }
                    Err(error) => {
                        let _ = tx2
                            .send(StreamEvent::Agent {
                                role: "analyst".into(),
                                status: "failed".into(),
                            })
                            .await;
                        let _ =
                            finish_agent_run(&state, run_id, &agent, Err(error.message.clone()))
                                .await;
                        StreamEvent::Finished(Err(error.message))
                    }
                }
            }
            Err(error) => {
                let message = error.to_string();
                let _ = tx2
                    .send(StreamEvent::Agent {
                        role: "analyst".into(),
                        status: "failed".into(),
                    })
                    .await;
                let _ = finish_agent_run(&state, run_id, &agent, Err(message.clone())).await;
                StreamEvent::Finished(Err(message))
            }
        };
        let _ = tx2.send(event).await;
    });
    workflow_sse_response(rx, orchestration_id, event_db)
}

fn workflow_sse_response(
    rx: mpsc::Receiver<StreamEvent>,
    orchestration_id: i64,
    db: sqlx::PgPool,
) -> Response {
    let initial = tokio_stream::iter([StreamEvent::Orchestration(orchestration_id)]);
    let mut cleanup_expired = true;
    let events = initial.chain(ReceiverStream::new(rx)).then(move |event| {
        let cleanup_this_request = cleanup_expired;
        cleanup_expired = false;
        let db = db.clone();
        async move {
            let (name, data) = match event {
            StreamEvent::Orchestration(id) => ("orchestration", json!({ "id": id })),
            StreamEvent::Agent { role, status } => {
                ("agent", json!({ "role": role, "status": status }))
            }
            StreamEvent::Artifact { kind, digest } => {
                ("artifact", json!({ "kind": kind, "digest": digest }))
            }
            StreamEvent::Handoff {
                from_role,
                to_role,
                artifact_kind,
                digest,
            } => (
                "handoff",
                json!({
                    "fromRole": from_role,
                    "toRole": to_role,
                    "artifactKind": artifact_kind,
                    "digest": digest,
                }),
            ),
            StreamEvent::Stage(name) => ("stage", json!({ "name": name })),
            StreamEvent::Step(message) => ("step", json!({"message": message})),
            StreamEvent::Delta(content) => ("delta", json!({"content": content})),
            StreamEvent::Preview(content) => ("preview", json!({"content": content})),
            StreamEvent::Finished(Ok(payload)) => ("workflow_result", payload),
            StreamEvent::Finished(Err(error)) => (
                "error",
                json!({
                    "ok": false,
                    "error": error,
                    "retryable": is_retryable_agent_failure(&error),
                }),
            ),
            };
            if cleanup_this_request {
                let _ = sqlx::query(
                    "DELETE FROM agent_orchestration_events WHERE expires_at <= NOW()",
                )
                .execute(&db)
                .await;
            }
            let event_id = sqlx::query_scalar::<_, i64>(
                "INSERT INTO agent_orchestration_events (orchestration_id, event_name, payload_json) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(orchestration_id)
            .bind(name)
            .bind(&data)
            .fetch_one(&db)
            .await
            .ok();
            let mut outgoing = Event::default().event(name).data(data.to_string());
            if let Some(event_id) = event_id {
                outgoing = outgoing.id(event_id.to_string());
            }
            Ok::<Event, Infallible>(outgoing)
        }
    });
    let mut response = Sse::new(events)
        .keep_alive(KeepAlive::default())
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&orchestration_id.to_string()) {
        response.headers_mut().insert("x-orchestration-id", value);
    }
    response
}

async fn continue_clarifications(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<ContinueClarificationRequest>,
) -> Result<Response, WorkflowApiError> {
    ensure_model_permission(&state, project_id, &claims.sub).await?;
    let workflow = load_workflow(&state, project_id, &claims.sub)
        .await?
        .ok_or_else(WorkflowApiError::not_found)?;
    if workflow.status != "clarification" || workflow.updated_at != payload.expected_updated_at {
        return Err(WorkflowApiError::conflict("澄清状态已变化，请刷新后继续"));
    }
    let (base_revision, existing_dsl) = model_baseline(&state, project_id).await?;
    if workflow.base_revision != base_revision {
        return Err(WorkflowApiError::conflict(
            "模型基线已变化，请重新分析需求后继续",
        ));
    }
    let current_round = workflow
        .clarifications
        .iter()
        .map(|item| item.round)
        .max()
        .unwrap_or(1);
    let current_questions = workflow
        .clarifications
        .iter()
        .filter(|item| item.round == current_round)
        .cloned()
        .collect::<Vec<_>>();
    validate_answers(&current_questions, &payload.answers)?;
    let answers = merge_clarification_answers(&workflow.answers, &payload.answers);
    let knowledge_sources = effective_project_sources(&state.db, project_id, &claims.sub)
        .await
        .map_err(|error| WorkflowApiError::database("读取项目有效知识", error))?;
    let knowledge_query = format!(
        "{} {}",
        serde_json::to_string(&workflow.brief).unwrap_or_default(),
        serde_json::to_string(&answers).unwrap_or_default()
    );
    let knowledge_text =
        knowledge_context_for_query(&state.db, &knowledge_sources, &knowledge_query).await;
    let complexity = classify_requirement_complexity(&workflow.requirement_text);
    let risk_level = project_risk_level(&state, project_id).await?;
    let agent_complexity = match complexity {
        RequirementComplexity::Simple => crate::api::AgentComplexity::Simple,
        RequirementComplexity::Standard => crate::api::AgentComplexity::Standard,
        RequirementComplexity::Complex => crate::api::AgentComplexity::Complex,
    };
    let (agent, experiment) = llm_state
        .analysis_agent_for_project(project_id, risk_level != "high", agent_complexity)
        .map(|(agent, experiment)| (agent.fork_for_run(), experiment))
        .ok_or_else(WorkflowApiError::unavailable)?;
    let parent_orchestration_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT analysis_orchestration_id FROM decision_workflows WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取澄清上游编排", error))?
    .flatten();
    let (run_id, orchestration_id) = begin_agent_run(
        &state,
        project_id,
        &claims.sub,
        LlmTask::RequirementAnalysis,
        &agent,
        experiment,
        if risk_level == "high" || complexity == RequirementComplexity::Complex {
            Some(agent.parallel_model_names(3).len())
        } else {
            None
        },
        parent_orchestration_id,
        payload.retry_of_orchestration_id,
        json!({
            "requirement": workflow.requirement_text,
            "baseRevision": workflow.base_revision,
            "facilitatorRound": current_round.saturating_add(1),
            "answeredClarifications": answers,
            "previousClarifications": workflow.clarifications,
            "complexity": complexity.as_str(),
        }),
    )
    .await?;
    let base_prompt = agent.prompt_asset("requirement_analysis");
    let facilitator_digest = prompt_digest(&format!(
        "{}\n\n{}",
        base_prompt.content, FACILITATOR_INSTRUCTIONS
    ));
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| WorkflowApiError::database("开始记录澄清主持提示词", error))?;
    sqlx::query(
        "UPDATE agent_runs SET prompt_version = 'facilitator-v1', prompt_digest = $1 WHERE id = $2",
    )
    .bind(&facilitator_digest)
    .bind(run_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("记录澄清主持提示词摘要", error))?;
    sqlx::query(
        "UPDATE agent_steps SET agent_role = 'facilitator', task_kind = 'clarification_facilitation', artifact_kind = 'clarification_batch', prompt_version = 'facilitator-v1', prompt_digest = $1 WHERE agent_run_id = $2",
    )
    .bind(&facilitator_digest)
    .bind(run_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("标记澄清主持步骤", error))?;
    sqlx::query("UPDATE agent_orchestrations SET current_stage = 'facilitator' WHERE id = $1")
        .bind(orchestration_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| WorkflowApiError::database("标记澄清主持阶段", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| WorkflowApiError::database("提交澄清主持提示词摘要", error))?;
    let prepared = PreparedFacilitatorRun {
        state: state.clone(),
        project_id,
        username: claims.sub,
        workflow,
        answers,
        base_revision,
        existing_dsl,
        knowledge_text,
        risk_level,
        complexity,
        agent,
        run_id,
        orchestration_id,
    };
    let db = state.db.clone();
    let app_state = prepared.state.clone();
    let event_db = db.clone();
    let (tx, rx) = mpsc::channel::<StreamEvent>(32);
    let tx2 = tx.clone();
    tokio::spawn(async move {
        let agent = prepared.agent.clone();
        let run_id = prepared.run_id;
        let facilitator_context = serde_json::to_value((
            &prepared.workflow.brief,
            &prepared.answers,
            &prepared.workflow.clarifications,
        ))
        .unwrap_or(Value::Null);
        if let Ok(digest) = artifact_digest(&facilitator_context) {
            let _ = tx2
                .send(StreamEvent::Handoff {
                    from_role: "analyst".into(),
                    to_role: "facilitator".into(),
                    artifact_kind: "clarification_context".into(),
                    digest,
                })
                .await;
        }
        let _ = tx2.send(StreamEvent::Stage("facilitator".into())).await;
        let _ = tx2
            .send(StreamEvent::Agent {
                role: "facilitator".into(),
                status: "running".into(),
            })
            .await;
        let result = run_until_stream_disconnect(
            Some(&tx2),
            wait_for_orchestration_cancellation(&db, prepared.orchestration_id),
            async {
                execute_facilitator_round(prepared, &tx2)
                    .await
                    .map_err(|error| anyhow!(error.message))
            },
        )
        .await;
        let event = match result {
            Ok(workflow) => match finish_agent_run(
                &app_state,
                run_id,
                &agent,
                Ok(serde_json::to_value(&workflow).unwrap_or(Value::Null)),
            )
            .await
            {
                Ok(()) => {
                    let clarification_value =
                        serde_json::to_value(&(&workflow.clarifications, &workflow.answers))
                            .unwrap_or(Value::Null);
                    if let Ok(digest) = artifact_digest(&clarification_value) {
                        let _ = tx2
                            .send(StreamEvent::Artifact {
                                kind: "clarification_state".into(),
                                digest,
                            })
                            .await;
                    }
                    let _ = tx2
                        .send(StreamEvent::Agent {
                            role: "facilitator".into(),
                            status: "completed".into(),
                        })
                        .await;
                    StreamEvent::Finished(Ok(serde_json::to_value(workflow).unwrap_or(Value::Null)))
                }
                Err(error) => {
                    let _ = tx2
                        .send(StreamEvent::Agent {
                            role: "facilitator".into(),
                            status: "failed".into(),
                        })
                        .await;
                    StreamEvent::Finished(Err(error.message))
                }
            },
            Err(error) => {
                let message = error.to_string();
                let _ = tx2
                    .send(StreamEvent::Agent {
                        role: "facilitator".into(),
                        status: "failed".into(),
                    })
                    .await;
                let _ = finish_agent_run(&app_state, run_id, &agent, Err(message.clone())).await;
                StreamEvent::Finished(Err(message))
            }
        };
        let _ = tx2.send(event).await;
    });
    Ok(workflow_sse_response(rx, orchestration_id, event_db))
}

async fn execute_facilitator_round(
    prepared: PreparedFacilitatorRun,
    tx: &mpsc::Sender<StreamEvent>,
) -> Result<DecisionWorkflowState, WorkflowApiError> {
    let PreparedFacilitatorRun {
        state,
        project_id,
        username,
        workflow,
        answers,
        base_revision,
        existing_dsl,
        knowledge_text,
        risk_level,
        complexity,
        agent,
        run_id,
        orchestration_id,
    } = prepared;
    let _ = tx
        .send(StreamEvent::Step(
            "编排阶段：澄清主持人正在检查未决歧义".into(),
        ))
        .await;
    let facilitator_requirement = format!(
        "## 原始需求\n{}\n\n## 已回答的澄清问题\n{}\n\n## 已询问问题（不得重复）\n{}\n\n{}",
        workflow.requirement_text,
        serde_json::to_string_pretty(&answers).unwrap_or_default(),
        serde_json::to_string_pretty(&workflow.clarifications).unwrap_or_default(),
        FACILITATOR_INSTRUCTIONS,
    );
    let context = analysis_model_context(existing_dsl.as_deref(), Some(&workflow));
    let analysis = analyze_requirement_with_consensus(
        &state,
        run_id,
        orchestration_id,
        &agent,
        "facilitator",
        &risk_level,
        complexity,
        &facilitator_requirement,
        &knowledge_text,
        context.as_deref(),
        Some(tx),
    )
    .await;
    let analysis = analysis.map_err(|error| {
        WorkflowApiError::model("继续澄清", error).with_orchestration(orchestration_id)
    })?;
    validate_analysis(&analysis)?;
    let round = workflow
        .clarifications
        .iter()
        .map(|item| item.round)
        .max()
        .unwrap_or(1)
        .saturating_add(1);
    let next = next_clarification_round(
        analysis.clarifications.clone(),
        &workflow.clarifications,
        round,
    );
    let mut clarifications = workflow.clarifications.clone();
    clarifications.extend(next.iter().cloned());
    let status = if next.is_empty() {
        "analyzed"
    } else {
        "clarification"
    };
    let saved = sqlx::query(
        r#"
        UPDATE decision_workflows workflow
        SET status = $1, clarifications_json = $2, answers_json = $3,
            decision_plan_json = NULL, tester_report_json = NULL,
            plan_orchestration_id = NULL, confirmed_at = NULL, updated_at = NOW()
        WHERE workflow.project_id = $4 AND workflow.updated_at = $5
          AND workflow.status = 'clarification'
          AND workflow.base_revision IS NOT DISTINCT FROM $6
          AND EXISTS (SELECT 1 FROM decision_projects project WHERE project.id = $4
              AND project.current_draft_version_id IS NOT DISTINCT FROM $6)
          AND EXISTS (SELECT 1 FROM agent_runs run
              WHERE run.id = $7 AND run.status = 'running')
        "#,
    )
    .bind(status)
    .bind(serde_json::to_value(&clarifications).unwrap_or(Value::Array(vec![])))
    .bind(serde_json::to_value(&answers).unwrap_or(Value::Array(vec![])))
    .bind(project_id)
    .bind(workflow.updated_at)
    .bind(base_revision)
    .bind(run_id)
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("保存澄清主持结果", error))?;
    if saved.rows_affected() == 0 {
        return Err(
            WorkflowApiError::conflict("需求或模型基线已变化，本轮澄清未保存")
                .with_orchestration(orchestration_id),
        );
    }
    load_workflow(&state, project_id, &username)
        .await?
        .ok_or_else(WorkflowApiError::not_found)
}

async fn prepare_plan_run(
    state: &AppState,
    llm_state: &LlmState,
    claims: &Claims,
    project_id: i64,
    payload: CreateDecisionPlanRequest,
) -> Result<PreparedPlanRun, WorkflowApiError> {
    ensure_model_permission(&state, project_id, &claims.sub).await?;
    let workflow = load_workflow(&state, project_id, &claims.sub)
        .await?
        .ok_or_else(WorkflowApiError::not_found)?;
    let (base_revision, existing_dsl) = model_baseline(&state, project_id).await?;
    if workflow.base_revision != base_revision || workflow.updated_at != payload.expected_updated_at
    {
        return Err(WorkflowApiError::conflict(
            "需求或模型基线已变化，请刷新并重新分析后规划",
        ));
    }
    if !matches!(
        workflow.status.as_str(),
        "analyzed" | "clarification" | "planned"
    ) {
        return Err(WorkflowApiError::bad_request(
            "已确认方案如需调整，请先重新分析业务需求",
        ));
    }
    let model_context = analysis_model_context(existing_dsl.as_deref(), Some(&workflow));
    validate_answers(&workflow.clarifications, &payload.answers)?;
    let knowledge_sources = effective_project_sources(&state.db, project_id, &claims.sub)
        .await
        .map_err(|error| WorkflowApiError::database("读取项目有效知识", error))?;
    let knowledge_query = format!(
        "{} {}",
        serde_json::to_string(&workflow.brief).unwrap_or_default(),
        serde_json::to_string(&payload.answers).unwrap_or_default()
    );
    let knowledge_text =
        knowledge_context_for_query(&state.db, &knowledge_sources, &knowledge_query).await;
    let knowledge_citations = citations(&knowledge_sources);
    let complexity = classify_requirement_complexity(&workflow.requirement_text);
    let plan_complexity =
        planning_agent_complexity(complexity, workflow.brief.risk_level == "high");
    let (agent, experiment) = llm_state
        .plan_agent_for_project(
            project_id,
            workflow.brief.risk_level != "high",
            plan_complexity,
        )
        .map(|(agent, experiment)| (agent.fork_for_run(), experiment))
        .ok_or_else(WorkflowApiError::unavailable)?;
    let analysis_orchestration_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT analysis_orchestration_id FROM decision_workflows WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取需求分析编排关联", error))?
    .flatten();
    let (run_id, orchestration_id) = begin_agent_run(
        &state,
        project_id,
        &claims.sub,
        LlmTask::DecisionPlanning,
        &agent,
        experiment,
        None,
        analysis_orchestration_id,
        payload.retry_of_orchestration_id,
        json!({
            "brief": workflow.brief,
            "answers": payload.answers,
            "complexity": complexity.as_str(),
            "knowledgeCitations": knowledge_citations,
        }),
    )
    .await?;
    Ok(PreparedPlanRun {
        project_id,
        username: claims.sub.clone(),
        workflow_updated_at: workflow.updated_at,
        brief: workflow.brief,
        answers: payload.answers,
        complexity,
        knowledge_text,
        model_context,
        agent,
        run_id,
        orchestration_id,
    })
}

async fn execute_plan_run(
    state: &AppState,
    prepared: PreparedPlanRun,
    tx: Option<&mpsc::Sender<StreamEvent>>,
) -> Result<DecisionWorkflowState, WorkflowApiError> {
    let PreparedPlanRun {
        project_id,
        username,
        workflow_updated_at,
        brief,
        answers,
        complexity,
        knowledge_text,
        model_context,
        agent,
        run_id,
        orchestration_id,
    } = prepared;
    let planning = plan_with_validation_pipeline(
        state,
        run_id,
        &agent,
        brief.clone(),
        answers.clone(),
        knowledge_text,
        model_context,
        tx,
    );
    let plan_result = tokio::select! {
        result = planning => result,
        cancellation = wait_for_orchestration_cancellation(&state.db, orchestration_id) => {
            match cancellation {
                Ok(()) => Err(WorkflowApiError::conflict("编排已由用户取消")),
                Err(error) => Err(WorkflowApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("监听 Agent 编排取消失败：{error}"),
                    orchestration_id: None,
                    retryable: false,
                }),
            }
        }
    };
    let plan = match plan_result {
        Ok(plan) => plan,
        Err(error) => {
            let _ = finish_agent_run(&state, run_id, &agent, Err(error.message.clone())).await;
            return Err(error.with_orchestration(orchestration_id));
        }
    };
    if let Some(tx) = tx {
        let _ = tx
            .send(StreamEvent::Agent {
                role: "tester".into(),
                status: "running".into(),
            })
            .await;
    }
    let tester_report =
        match persist_tester_report(&state, run_id, &brief, &plan, &answers, complexity).await {
            Ok(report) => report,
            Err(error) => {
                if let Some(tx) = tx {
                    let _ = tx
                        .send(StreamEvent::Agent {
                            role: "tester".into(),
                            status: "failed".into(),
                        })
                        .await;
                }
                let _ = finish_agent_run(&state, run_id, &agent, Err(error.message.clone())).await;
                return Err(error);
            }
        };
    if let Some(tx) = tx {
        let report_value = serde_json::to_value(&tester_report).unwrap_or(Value::Null);
        if let Ok(digest) = artifact_digest(&report_value) {
            let _ = tx
                .send(StreamEvent::Artifact {
                    kind: "tester_report".into(),
                    digest,
                })
                .await;
        }
        let _ = tx
            .send(StreamEvent::Agent {
                role: "tester".into(),
                status: "completed".into(),
            })
            .await;
    }
    let result = sqlx::query(
        r#"
        UPDATE decision_workflows workflow
        SET status = 'planned', answers_json = $1, decision_plan_json = $2,
            tester_report_json = $3, plan_orchestration_id = $7,
            confirmed_at = NULL, updated_at = NOW()
        WHERE workflow.project_id = $4
          AND workflow.updated_at = $5
          AND EXISTS (SELECT 1 FROM agent_runs run WHERE run.id = $6 AND run.status = 'running')
          AND workflow.status IN ('analyzed', 'clarification', 'planned')
          AND EXISTS (SELECT 1 FROM decision_projects project WHERE project.id = $4
              AND project.current_draft_version_id IS NOT DISTINCT FROM workflow.base_revision)
        "#,
    )
    .bind(serde_json::to_value(&answers).unwrap_or(Value::Array(vec![])))
    .bind(serde_json::to_value(&plan).unwrap_or(Value::Null))
    .bind(serde_json::to_value(&tester_report).unwrap_or(Value::Null))
    .bind(project_id)
    .bind(workflow_updated_at)
    .bind(run_id)
    .bind(orchestration_id)
    .execute(&state.db)
    .await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let workflow_error = WorkflowApiError::database("保存决策方案", error);
            let _ =
                finish_agent_run(&state, run_id, &agent, Err(workflow_error.message.clone())).await;
            return Err(workflow_error);
        }
    };
    if result.rows_affected() == 0 {
        let workflow_error =
            WorkflowApiError::conflict("规划期间需求或模型已变化，旧方案未保存，请刷新后重试");
        let _ = finish_agent_run(&state, run_id, &agent, Err(workflow_error.message.clone())).await;
        return Err(workflow_error);
    }
    let saved_workflow = match load_workflow(state, project_id, &username).await {
        Ok(Some(workflow)) => workflow,
        Ok(None) => {
            let workflow_error = WorkflowApiError::not_found();
            let _ =
                finish_agent_run(&state, run_id, &agent, Err(workflow_error.message.clone())).await;
            return Err(workflow_error);
        }
        Err(workflow_error) => {
            let _ =
                finish_agent_run(&state, run_id, &agent, Err(workflow_error.message.clone())).await;
            return Err(workflow_error);
        }
    };
    finish_agent_run(
        &state,
        run_id,
        &agent,
        Ok(serde_json::to_value(&plan).unwrap_or(Value::Null)),
    )
    .await?;
    Ok(saved_workflow)
}

async fn create_plan(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<CreateDecisionPlanRequest>,
) -> Result<Json<DecisionWorkflowState>, WorkflowApiError> {
    let prepared = prepare_plan_run(&state, &llm_state, &claims, project_id, payload).await?;
    execute_plan_run(&state, prepared, None).await.map(Json)
}

async fn create_plan_stream(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<CreateDecisionPlanRequest>,
) -> Response {
    let prepared = match prepare_plan_run(&state, &llm_state, &claims, project_id, payload).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    let orchestration_id = prepared.orchestration_id;
    let run_id = prepared.run_id;
    let agent = prepared.agent.clone();
    let event_db = state.db.clone();
    let (tx, rx) = mpsc::channel(16);
    tokio::spawn(async move {
        let _ = tx
            .send(StreamEvent::Step("编排阶段：方案规划智能体".into()))
            .await;
        let _ = tx.send(StreamEvent::Stage("architect".into())).await;
        let brief_value = serde_json::to_value(&prepared.brief).unwrap_or(Value::Null);
        if let Ok(digest) = artifact_digest(&brief_value) {
            let _ = tx
                .send(StreamEvent::Handoff {
                    from_role: "analyst".into(),
                    to_role: "architect".into(),
                    artifact_kind: "decision_brief".into(),
                    digest,
                })
                .await;
        }
        let _ = tx
            .send(StreamEvent::Agent {
                role: "architect".into(),
                status: "running".into(),
            })
            .await;
        let result = run_until_stream_disconnect(
            Some(&tx),
            std::future::pending::<anyhow::Result<()>>(),
            async {
                execute_plan_run(&state, prepared, Some(&tx))
                    .await
                    .map_err(|error| anyhow!(error.message))
            },
        )
        .await;
        let event = match result {
            Ok(workflow) => {
                if let Ok(plan_value) = serde_json::to_value(&workflow.plan)
                    && let Ok(digest) = artifact_digest(&plan_value)
                {
                    let _ = tx
                        .send(StreamEvent::Artifact {
                            kind: "decision_plan".into(),
                            digest,
                        })
                        .await;
                }
                let _ = tx
                    .send(StreamEvent::Agent {
                        role: "architect".into(),
                        status: "completed".into(),
                    })
                    .await;
                StreamEvent::Finished(serde_json::to_value(workflow).map_err(|e| e.to_string()))
            }
            Err(error) => {
                let message = error.to_string();
                let _ = tx
                    .send(StreamEvent::Agent {
                        role: "architect".into(),
                        status: "failed".into(),
                    })
                    .await;
                if message.contains("SSE 客户端连接中断") || message.contains("SSE 客户端已断开")
                {
                    let _ = finish_agent_run(&state, run_id, &agent, Err(message.clone())).await;
                }
                StreamEvent::Finished(Err(message))
            }
        };
        let _ = tx.send(event).await;
    });
    workflow_sse_response(rx, orchestration_id, event_db)
}

async fn confirm_plan(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<ConfirmDecisionPlanRequest>,
) -> Result<Json<DecisionWorkflowState>, WorkflowApiError> {
    ensure_model_permission(&state, project_id, &claims.sub).await?;
    let result = sqlx::query(
        r#"
        UPDATE decision_workflows workflow
        SET status = 'confirmed', confirmed_at = NOW(), updated_at = NOW()
        WHERE workflow.project_id = $1 AND workflow.status = 'planned'
          AND workflow.decision_plan_json IS NOT NULL
          AND workflow.updated_at = $2
          AND EXISTS (SELECT 1 FROM decision_projects project WHERE project.id = $1
              AND project.current_draft_version_id IS NOT DISTINCT FROM workflow.base_revision)
        "#,
    )
    .bind(project_id)
    .bind(payload.expected_updated_at)
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("确认决策方案", error))?;
    if result.rows_affected() == 0 {
        return Err(WorkflowApiError::conflict(
            "方案或模型基线已变化，请刷新后重新核对方案",
        ));
    }
    load_workflow(&state, project_id, &claims.sub)
        .await?
        .map(Json)
        .ok_or_else(WorkflowApiError::not_found)
}

async fn list_agent_run_history(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<AgentRunHistoryItem>>, WorkflowApiError> {
    ensure_project(&state, project_id, &claims.sub).await?;
    let history = sqlx::query_as::<_, AgentRunHistoryItem>(
        r#"
        SELECT id, stage, status, model, prompt_version, retry_count,
               usage_json AS usage, started_at, finished_at
        FROM agent_runs
        WHERE project_id = $1
        ORDER BY started_at DESC, id DESC
        LIMIT 100
        "#,
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取 Agent 运行历史", error))?;
    Ok(Json(history))
}

async fn list_orchestration_history(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<AgentOrchestrationHistoryQuery>,
) -> Result<Json<Vec<AgentOrchestrationHistoryItem>>, WorkflowApiError> {
    ensure_project(&state, project_id, &claims.sub).await?;
    let (before_id, limit) =
        validate_agent_orchestration_history_query(query).map_err(WorkflowApiError::bad_request)?;
    let orchestrations = sqlx::query_as::<_, AgentOrchestrationRow>(
        r#"
        SELECT id, parent_orchestration_id, retry_of_orchestration_id,
               trigger_kind, status, current_stage, created_at, finished_at
        FROM agent_orchestrations
        WHERE project_id = $1 AND ($2::BIGINT IS NULL OR id < $2)
        ORDER BY id DESC
        LIMIT $3
        "#,
    )
    .bind(project_id)
    .bind(before_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取 Agent 编排历史", error))?;
    let orchestration_ids = orchestrations
        .iter()
        .map(|orchestration| orchestration.id)
        .collect::<Vec<_>>();
    let steps = sqlx::query_as::<_, AgentOrchestrationStep>(
        r#"
        SELECT id, orchestration_id, step_no, attempt_no, agent_role, task_kind, status,
               model, prompt_version, artifact_kind, artifact_schema_version,
               artifact_digest, usage_json AS usage, error_json AS error,
               started_at, finished_at
        FROM agent_steps
        WHERE orchestration_id = ANY($1)
        ORDER BY orchestration_id, step_no, attempt_no
        "#,
    )
    .bind(&orchestration_ids)
    .fetch_all(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取 Agent 编排步骤", error))?;
    let mut steps_by_orchestration = HashMap::<i64, Vec<AgentOrchestrationStep>>::new();
    for step in steps {
        steps_by_orchestration
            .entry(step.orchestration_id)
            .or_default()
            .push(step);
    }
    Ok(Json(
        orchestrations
            .into_iter()
            .map(|orchestration| AgentOrchestrationHistoryItem {
                steps: steps_by_orchestration
                    .remove(&orchestration.id)
                    .unwrap_or_default(),
                orchestration,
            })
            .collect(),
    ))
}

async fn replay_orchestration_events(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, orchestration_id)): Path<(i64, i64)>,
    Query(query): Query<AgentOrchestrationEventQuery>,
) -> Result<Json<Vec<AgentOrchestrationEvent>>, WorkflowApiError> {
    ensure_project(&state, project_id, &claims.sub).await?;
    if orchestration_id <= 0 {
        return Err(WorkflowApiError::bad_request(
            "orchestrationId 必须为正整数",
        ));
    }
    let (after_event_id, limit) =
        validate_agent_orchestration_event_query(query).map_err(WorkflowApiError::bad_request)?;
    let owned = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM agent_orchestrations WHERE id = $1 AND project_id = $2)",
    )
    .bind(orchestration_id)
    .bind(project_id)
    .fetch_one(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("校验编排事件归属", error))?;
    if !owned {
        return Err(WorkflowApiError::not_found());
    }
    let events = sqlx::query_as::<_, AgentOrchestrationEvent>(
        r#"
        SELECT id, event_name, payload_json AS payload, created_at
        FROM agent_orchestration_events
        WHERE orchestration_id = $1 AND id > $2 AND expires_at > NOW()
        ORDER BY id
        LIMIT $3
        "#,
    )
    .bind(orchestration_id)
    .bind(after_event_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("读取编排事件", error))?;
    Ok(Json(events))
}

async fn cancel_agent_orchestration(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, orchestration_id)): Path<(i64, i64)>,
) -> Result<StatusCode, WorkflowApiError> {
    ensure_model_permission(&state, project_id, &claims.sub).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| WorkflowApiError::database("开始取消 Agent 编排", error))?;
    let cancelled = sqlx::query_scalar::<_, i64>(
        r#"
        UPDATE agent_orchestrations
        SET status = 'cancelled', finished_at = NOW()
        WHERE id = $1 AND project_id = $2 AND status IN ('pending', 'running')
        RETURNING id
        "#,
    )
    .bind(orchestration_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("取消 Agent 编排", error))?;
    if cancelled.is_none() {
        let status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM agent_orchestrations WHERE id = $1 AND project_id = $2",
        )
        .bind(orchestration_id)
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| WorkflowApiError::database("确认 Agent 编排状态", error))?;
        match status.as_deref() {
            Some("cancelled") => {
                transaction
                    .commit()
                    .await
                    .map_err(|error| WorkflowApiError::database("提交 Agent 编排取消", error))?;
                return Ok(StatusCode::NO_CONTENT);
            }
            Some(_) => return Err(WorkflowApiError::conflict("编排已结束，无法取消")),
            None => return Err(WorkflowApiError::not_found()),
        }
    }
    let error = json!({ "message": "编排已由用户取消" });
    sqlx::query(
        "UPDATE agent_runs SET status = 'cancelled', error_json = $1, finished_at = NOW() WHERE orchestration_id = $2 AND status = 'running'",
    )
    .bind(&error)
    .bind(orchestration_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("取消 Agent 运行", error))?;
    sqlx::query(
        "UPDATE agent_steps SET status = 'cancelled', error_json = $1, finished_at = NOW() WHERE orchestration_id = $2 AND status IN ('pending', 'running')",
    )
    .bind(error)
    .bind(orchestration_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| WorkflowApiError::database("取消 Agent 步骤", error))?;
    stop_decision_explanation_attempts(
        &mut transaction,
        &[orchestration_id],
        "解释编排已由用户取消；可重新发起",
    )
    .await
    .map_err(|error| WorkflowApiError::database("取消决策解释缓存", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| WorkflowApiError::database("提交 Agent 编排取消", error))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowNextStage {
    Analyze,
    Clarify,
    Plan,
    AwaitConfirmation,
    Generate,
    ChangeExistingModel,
    Invalid,
}

fn workflow_next_stage(
    has_requirement: bool,
    status: Option<&str>,
    has_existing_model: bool,
) -> WorkflowNextStage {
    if has_requirement {
        return WorkflowNextStage::Analyze;
    }
    match status {
        Some("clarification") => WorkflowNextStage::Clarify,
        Some("analyzed") => WorkflowNextStage::Plan,
        Some("planned") => WorkflowNextStage::AwaitConfirmation,
        Some("confirmed") if has_existing_model => WorkflowNextStage::ChangeExistingModel,
        Some("confirmed") => WorkflowNextStage::Generate,
        _ => WorkflowNextStage::Invalid,
    }
}

fn simple_route_can_skip_facilitator(
    complexity: RequirementComplexity,
    high_risk: bool,
    clarifications: &[Clarification],
) -> bool {
    complexity == RequirementComplexity::Simple
        && !high_risk
        && clarifications
            .iter()
            .all(|clarification| !clarification.required)
}

fn planning_agent_complexity(
    complexity: RequirementComplexity,
    high_risk: bool,
) -> crate::api::AgentComplexity {
    if high_risk {
        return crate::api::AgentComplexity::Standard;
    }
    match complexity {
        RequirementComplexity::Simple => crate::api::AgentComplexity::Simple,
        RequirementComplexity::Standard => crate::api::AgentComplexity::Standard,
        RequirementComplexity::Complex => crate::api::AgentComplexity::Complex,
    }
}

fn simple_direct_route_eligible(workflow: &DecisionWorkflowState) -> bool {
    workflow.status == "analyzed"
        && workflow.base_revision.is_none()
        && workflow.brief.risk_level == "low"
        && workflow.clarifications.is_empty()
        && workflow.brief.outputs.len() == 1
        && classify_requirement_complexity(&workflow.requirement_text)
            == RequirementComplexity::Simple
}

fn simple_direct_plan(workflow: &DecisionWorkflowState) -> Result<DecisionPlan, WorkflowApiError> {
    if !simple_direct_route_eligible(workflow) {
        return Err(WorkflowApiError::conflict(
            "需求不满足低风险 simple 直达生成条件",
        ));
    }
    let output = &workflow.brief.outputs[0];
    let plan = DecisionPlan {
        name: workflow.brief.name.clone(),
        summary: workflow.brief.goal.clone(),
        decisions: vec![crate::workflow::PlannedDecision {
            name: workflow.brief.name.clone(),
            purpose: workflow.brief.goal.clone(),
            output_type: output.type_ref.clone(),
            expression_type: "decisionTable".into(),
            dependencies: Vec::new(),
            requirement_ids: workflow
                .brief
                .requirement_items
                .iter()
                .map(|item| item.id.clone())
                .collect(),
        }],
        ..Default::default()
    };
    validate_plan(&plan, Some(&workflow.brief))?;
    Ok(plan)
}

async fn orchestrate_workflow_stream(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<OrchestrateDecisionWorkflowRequest>,
) -> Response {
    if let Err(error) = ensure_model_permission(&state, project_id, &claims.sub).await {
        return error.into_response();
    }
    let initial_requirement = payload.text;
    if workflow_next_stage(initial_requirement.is_some(), None, false) == WorkflowNextStage::Analyze
    {
        return analyze_requirement_stream(
            State(state),
            Extension(llm_state),
            Extension(claims),
            Path(project_id),
            Json(AnalyzeRequirementRequest {
                text: initial_requirement.unwrap_or_default(),
                retry_of_orchestration_id: payload.retry_of_orchestration_id,
            }),
        )
        .await;
    }

    let workflow = match load_workflow(&state, project_id, &claims.sub).await {
        Ok(Some(workflow)) => workflow,
        Ok(None) => {
            return WorkflowApiError::bad_request("请先提交需求文本").into_response();
        }
        Err(error) => return error.into_response(),
    };

    match workflow_next_stage(
        false,
        Some(&workflow.status),
        workflow.base_revision.is_some(),
    ) {
        WorkflowNextStage::Clarify => {
            let Some(expected_updated_at) = payload.expected_updated_at else {
                return WorkflowApiError::bad_request("继续澄清时必须提供 expectedUpdatedAt")
                    .into_response();
            };
            let complexity = classify_requirement_complexity(&workflow.requirement_text);
            if simple_route_can_skip_facilitator(
                complexity,
                workflow.brief.risk_level == "high",
                &workflow.clarifications,
            ) {
                return create_plan_stream(
                    State(state),
                    Extension(llm_state),
                    Extension(claims),
                    Path(project_id),
                    Json(CreateDecisionPlanRequest {
                        answers: merge_clarification_answers(&workflow.answers, &payload.answers),
                        expected_updated_at,
                        retry_of_orchestration_id: payload.retry_of_orchestration_id,
                    }),
                )
                .await;
            }
            match continue_clarifications(
                State(state),
                Extension(llm_state),
                Extension(claims),
                Path(project_id),
                Json(ContinueClarificationRequest {
                    answers: payload.answers,
                    expected_updated_at,
                    retry_of_orchestration_id: payload.retry_of_orchestration_id,
                }),
            )
            .await
            {
                Ok(response) => response,
                Err(error) => error.into_response(),
            }
        }
        WorkflowNextStage::Plan => {
            let Some(expected_updated_at) = payload.expected_updated_at else {
                return WorkflowApiError::bad_request("开始方案规划时必须提供 expectedUpdatedAt")
                    .into_response();
            };
            if simple_direct_route_eligible(&workflow) {
                let request = json!({
                    "text": workflow.requirement_text,
                    "projectId": project_id,
                    "workflowUpdatedAt": expected_updated_at,
                    "existingDsl": null,
                    "baseVersion": null,
                    "retryOfOrchestrationId": payload.retry_of_orchestration_id,
                    "simpleDirect": true,
                });
                let body = match serde_json::to_vec(&request) {
                    Ok(body) => Bytes::from(body),
                    Err(error) => {
                        return WorkflowApiError::bad_request(format!(
                            "simple 直达请求无效：{error}"
                        ))
                        .into_response();
                    }
                };
                let mut headers = HeaderMap::new();
                headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
                return crate::api::run_pipeline(
                    State(state),
                    Extension(llm_state),
                    Extension(claims),
                    headers,
                    body,
                )
                .await;
            }
            create_plan_stream(
                State(state),
                Extension(llm_state),
                Extension(claims),
                Path(project_id),
                Json(CreateDecisionPlanRequest {
                    answers: workflow.answers,
                    expected_updated_at,
                    retry_of_orchestration_id: payload.retry_of_orchestration_id,
                }),
            )
            .await
        }
        WorkflowNextStage::AwaitConfirmation => {
            WorkflowApiError::conflict("方案已生成，请先由业务人员确认后再继续").into_response()
        }
        WorkflowNextStage::ChangeExistingModel => {
            WorkflowApiError::conflict("当前项目已有模型，请通过模型变更预览流程继续")
                .into_response()
        }
        WorkflowNextStage::Generate => {
            let Some(expected_updated_at) = payload.expected_updated_at else {
                return WorkflowApiError::bad_request(
                    "开始 DSL 生成时必须提供已确认方案的 expectedUpdatedAt",
                )
                .into_response();
            };
            if expected_updated_at != workflow.updated_at {
                return WorkflowApiError::conflict("方案已变化，请刷新后再生成 DSL")
                    .into_response();
            }
            let request = json!({
                "text": workflow.requirement_text,
                "projectId": project_id,
                "workflowUpdatedAt": expected_updated_at,
                "existingDsl": null,
                "baseVersion": null,
                "retryOfOrchestrationId": payload.retry_of_orchestration_id,
            });
            let body = match serde_json::to_vec(&request) {
                Ok(body) => Bytes::from(body),
                Err(error) => {
                    return WorkflowApiError::bad_request(format!("编排请求无效：{error}"))
                        .into_response();
                }
            };
            let mut headers = HeaderMap::new();
            headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
            crate::api::run_pipeline(
                State(state),
                Extension(llm_state),
                Extension(claims),
                headers,
                body,
            )
            .await
        }
        WorkflowNextStage::Analyze | WorkflowNextStage::Invalid => {
            WorkflowApiError::conflict("当前工作流状态不能推进，请刷新工作流后重试").into_response()
        }
    }
}

pub fn routes(llm_state: Arc<LlmState>) -> Router<AppState> {
    Router::new()
        .route("/projects/{project_id}/workflow", get(get_workflow))
        .route(
            "/projects/{project_id}/agent-runs",
            get(list_agent_run_history),
        )
        .route(
            "/projects/{project_id}/orchestrations",
            get(list_orchestration_history),
        )
        .route(
            "/projects/{project_id}/orchestrations/{orchestration_id}/events",
            get(replay_orchestration_events),
        )
        .route(
            "/projects/{project_id}/orchestrations/{orchestration_id}/cancel",
            post(cancel_agent_orchestration),
        )
        .route(
            "/projects/{project_id}/workflow/analyze-stream",
            post(analyze_requirement_stream),
        )
        .route(
            "/projects/{project_id}/workflow/orchestrate-stream",
            post(orchestrate_workflow_stream),
        )
        .route(
            "/projects/{project_id}/workflow/analyze",
            post(analyze_requirement),
        )
        .route(
            "/projects/{project_id}/workflow/pending-review",
            get(pending_requirement_review),
        )
        .route(
            "/projects/{project_id}/workflow/review",
            post(confirm_requirement_review),
        )
        .route(
            "/projects/{project_id}/workflow/clarify",
            post(continue_clarifications),
        )
        .route("/projects/{project_id}/workflow/plan", post(create_plan))
        .route(
            "/projects/{project_id}/workflow/plan-stream",
            post(create_plan_stream),
        )
        .route(
            "/projects/{project_id}/workflow/confirm",
            post(confirm_plan),
        )
        .layer(Extension(llm_state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_requirement_review_reads_successful_vote_outcomes() {
        let divergence = json!({
            "outcomes": [
                {
                    "model": "model-a",
                    "result": {"Ok": {"brief": {"name": "贷款审批", "goal": "判断是否通过", "riskLevel": "medium"}}}
                },
                {"model": "model-b", "result": {"Err": "provider timeout"}}
            ]
        });

        let candidates = requirement_review_candidates(&divergence);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].model, "model-a");
        assert_eq!(candidates[0].analysis.brief.name, "贷款审批");
        assert_eq!(candidates[0].analysis.brief.goal, "判断是否通过");
    }

    #[test]
    fn pending_requirement_review_accepts_wrapped_and_direct_candidates_once_per_model() {
        let analysis =
            json!({"brief": {"name": "贷款审批", "goal": "判断是否通过", "riskLevel": "medium"}});
        let divergence = json!({
            "outcomes": [
                {"model": "model-a", "result": {"success": analysis}},
                {"model": "model-a", "result": {"Ok": analysis}},
                {"model": "model-b", "result": analysis},
                {"model": "model-c", "result": {"Err": "provider timeout"}},
                {"model": "model-d", "result": {"Ok": {"brief": {"name": "", "goal": ""}}}}
            ]
        });

        let candidates = requirement_review_candidates(&divergence);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].model, "model-a");
        assert_eq!(candidates[1].model, "model-b");
    }

    #[test]
    fn requirement_consensus_ignores_prose_but_preserves_dissent_as_required_clarification() {
        let base = RequirementAnalysis {
            brief: DecisionBrief {
                name: "LoanApproval".into(),
                goal: "判断贷款是否通过".into(),
                risk_level: "medium".into(),
                inputs: vec![DecisionField {
                    name: "amount".into(),
                    type_ref: "number".into(),
                    required: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let differently_worded = RequirementAnalysis {
            brief: DecisionBrief {
                goal: "评估贷款申请是否批准".into(),
                description: "模型给出不同的说明文案".into(),
                ..base.brief.clone()
            },
            ..Default::default()
        };
        assert_eq!(
            requirement_analysis_consensus_key(&base).unwrap(),
            requirement_analysis_consensus_key(&differently_worded).unwrap()
        );

        let dissent = RequirementAnalysis {
            brief: DecisionBrief {
                goal: "判断贷款是否通过并检查逾期".into(),
                risk_level: "high".into(),
                inputs: vec![
                    base.brief.inputs[0].clone(),
                    DecisionField {
                        name: "overdueDays".into(),
                        type_ref: "integer".into(),
                        ..Default::default()
                    },
                ],
                constraints: vec!["逾期超过 30 天时拒绝".into()],
                ..base.brief.clone()
            },
            ..Default::default()
        };
        let outcomes = [base.clone(), differently_worded, dissent]
            .into_iter()
            .enumerate()
            .map(
                |(index, analysis)| crate::agent_runtime::orchestration::VoteOutcome {
                    model: format!("model-{index}"),
                    weight: 1.0,
                    result: Ok(serde_json::to_value(analysis).unwrap()),
                },
            )
            .collect::<Vec<_>>();
        let mut selected = base;
        add_consensus_divergence_clarifications(&mut selected, &outcomes);

        assert_eq!(selected.brief.risk_level, "high");
        assert!(
            selected
                .clarifications
                .iter()
                .any(|item| { item.required && item.question.contains("业务目标") })
        );
        assert!(
            selected
                .clarifications
                .iter()
                .any(|item| { item.required && item.question.contains("字段或业务规则") })
        );
    }

    #[test]
    fn requirement_plurality_fallback_selects_a_candidate_when_every_model_differs() {
        let candidate =
            |model: &str, name: &str| crate::agent_runtime::orchestration::VoteOutcome {
                model: model.into(),
                weight: 1.0,
                result: Ok(serde_json::to_value(RequirementAnalysis {
                    brief: DecisionBrief {
                        name: name.into(),
                        goal: "判断申请是否通过".into(),
                        risk_level: "high".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .unwrap()),
            };
        let outcomes = vec![
            candidate("model-a", "LoanApproval"),
            candidate("model-b", "LoanDecision"),
            candidate("model-c", "CreditEligibility"),
        ];

        let selected =
            plurality_requirement_analysis(&outcomes, requirement_analysis_consensus_key)
                .unwrap()
                .unwrap();

        assert_eq!(selected.brief.name, "LoanApproval");
    }

    #[test]
    fn single_valid_requirement_model_adds_required_business_confirmation() {
        let mut analysis = RequirementAnalysis {
            brief: DecisionBrief {
                name: "LoanApproval".into(),
                goal: "判断申请是否通过".into(),
                risk_level: "high".into(),
                ..Default::default()
            },
            ..Default::default()
        };

        add_single_model_confirmation(&mut analysis, false);

        assert_eq!(analysis.clarifications.len(), 1);
        assert!(analysis.clarifications[0].required);
        assert!(
            analysis.clarifications[0]
                .question
                .contains("尚未形成交叉验证")
        );
    }

    #[test]
    fn facilitator_merges_distinct_questions_and_combines_duplicate_options() {
        let first = Clarification {
            id: "a".into(),
            question: "逾期记录是否包含已结清记录？".into(),
            options: vec!["包含".into()],
            required: false,
            ..Default::default()
        };
        let outcomes = vec![
            crate::agent_runtime::orchestration::VoteOutcome {
                model: "model-a".into(),
                weight: 1.0,
                result: Ok(serde_json::to_value(RequirementAnalysis {
                    brief: DecisionBrief {
                        name: "LoanApproval".into(),
                        goal: "判断是否通过".into(),
                        risk_level: "high".into(),
                        ..Default::default()
                    },
                    clarifications: vec![first.clone()],
                })
                .unwrap()),
            },
            crate::agent_runtime::orchestration::VoteOutcome {
                model: "model-b".into(),
                weight: 1.0,
                result: Ok(serde_json::to_value(RequirementAnalysis {
                    brief: DecisionBrief {
                        name: "LoanDecision".into(),
                        goal: "评估申请".into(),
                        risk_level: "high".into(),
                        ..Default::default()
                    },
                    clarifications: vec![
                        Clarification {
                            id: "b".into(),
                            question: " 逾期记录是否包含已结清记录? ".into(),
                            options: vec!["不包含".into()],
                            required: true,
                            ..Default::default()
                        },
                        Clarification {
                            id: "c".into(),
                            question: "逾期次数按哪个时间范围计算？".into(),
                            required: true,
                            ..Default::default()
                        },
                    ],
                })
                .unwrap()),
            },
        ];
        let mut selected = RequirementAnalysis {
            clarifications: vec![first],
            ..Default::default()
        };

        merge_facilitator_clarifications(&mut selected, &outcomes);

        assert_eq!(selected.clarifications.len(), 2);
        assert_eq!(selected.clarifications[0].options, vec!["包含", "不包含"]);
        assert!(selected.clarifications[0].required);
        assert!(selected.clarifications[1].question.contains("时间范围"));
    }

    #[test]
    fn facilitator_consensus_uses_clarification_content_not_repeated_brief_fields() {
        let clarification = Clarification {
            id: "model-a-id".into(),
            question: "逾期记录是否包含已结清记录？".into(),
            options: vec!["包含".into(), "不包含".into()],
            required: true,
            reason: "影响风险判断".into(),
            round: 2,
        };
        let base = RequirementAnalysis {
            brief: DecisionBrief {
                name: "LoanApproval".into(),
                goal: "判断贷款是否通过".into(),
                risk_level: "high".into(),
                ..Default::default()
            },
            clarifications: vec![clarification.clone()],
            ..Default::default()
        };
        let changed_brief = RequirementAnalysis {
            brief: DecisionBrief {
                name: "LoanDecision".into(),
                goal: "评估贷款申请".into(),
                inputs: vec![DecisionField {
                    name: "latePayments".into(),
                    type_ref: "integer".into(),
                    ..Default::default()
                }],
                ..base.brief.clone()
            },
            clarifications: vec![Clarification {
                id: "model-b-id".into(),
                question: " 逾期记录 是否包含已结清记录? ".into(),
                reason: "其他解释".into(),
                ..clarification.clone()
            }],
            ..Default::default()
        };
        let different_question = RequirementAnalysis {
            clarifications: vec![Clarification {
                question: "逾期次数如何计算？".into(),
                ..clarification
            }],
            ..base.clone()
        };

        assert_eq!(
            facilitator_consensus_key(&base).unwrap(),
            facilitator_consensus_key(&changed_brief).unwrap()
        );
        assert_ne!(
            facilitator_consensus_key(&base).unwrap(),
            facilitator_consensus_key(&different_question).unwrap()
        );
    }

    #[test]
    fn facilitator_review_preserves_answers_and_adds_only_new_questions() {
        let existing = vec![Clarification {
            id: "round-1".into(),
            question: "是否包含已结清记录？".into(),
            required: true,
            round: 1,
            ..Default::default()
        }];
        let answers = vec![ClarificationAnswer {
            clarification_id: "round-1".into(),
            answer: "包含".into(),
        }];
        let proposed = vec![
            Clarification {
                id: "round-1".into(),
                question: "是否包含已结清记录？".into(),
                required: true,
                ..Default::default()
            },
            Clarification {
                id: "round-2".into(),
                question: "逾期次数按自然年还是滚动一年计算？".into(),
                required: true,
                ..Default::default()
            },
        ];

        let (status, clarifications, preserved_answers) =
            facilitator_review_update(&existing, proposed, answers.clone());

        assert_eq!(status, "clarification");
        assert_eq!(clarifications.len(), 2);
        assert_eq!(clarifications[0].id, "round-1");
        assert_eq!(clarifications[1].round, 2);
        assert_eq!(preserved_answers, answers);
    }

    #[test]
    fn requirement_review_baseline_supports_legacy_runs_only_when_project_state_is_unchanged() {
        let timestamp = |seconds| DateTime::from_timestamp(seconds, 0).unwrap();
        let run_started = timestamp(200);
        let before_run = timestamp(100);
        let after_run = timestamp(300);

        assert!(requirement_review_baseline_is_current(
            true,
            Some(7),
            Some(7),
            run_started,
            before_run,
            Some(before_run),
        ));
        assert!(!requirement_review_baseline_is_current(
            true,
            None,
            Some(7),
            run_started,
            before_run,
            Some(before_run),
        ));
        assert!(requirement_review_baseline_is_current(
            false,
            None,
            Some(7),
            run_started,
            before_run,
            Some(before_run),
        ));
        assert!(!requirement_review_baseline_is_current(
            false,
            None,
            Some(7),
            run_started,
            after_run,
            Some(before_run),
        ));
        assert!(!requirement_review_baseline_is_current(
            false,
            None,
            Some(7),
            run_started,
            before_run,
            Some(after_run),
        ));
    }

    #[test]
    fn missing_requirement_candidates_are_retryable_but_disagreement_is_reviewable() {
        assert!(is_retryable_agent_failure(
            "多模型均未返回有效需求分析候选，请检查模型配置后重试"
        ));
        assert!(!is_retryable_agent_failure(
            "澄清主持的模型结果未达成多数共识，已标记为需人工复核"
        ));
    }

    #[test]
    fn unified_orchestrator_routes_by_saved_workflow_and_preserves_human_gates() {
        assert_eq!(
            workflow_next_stage(true, Some("planned"), false),
            WorkflowNextStage::Analyze
        );
        assert_eq!(
            workflow_next_stage(false, Some("clarification"), false),
            WorkflowNextStage::Clarify
        );
        assert_eq!(
            workflow_next_stage(false, Some("analyzed"), false),
            WorkflowNextStage::Plan
        );
        assert_eq!(
            workflow_next_stage(false, Some("planned"), false),
            WorkflowNextStage::AwaitConfirmation
        );
        assert_eq!(
            workflow_next_stage(false, Some("confirmed"), false),
            WorkflowNextStage::Generate
        );
        assert_eq!(
            workflow_next_stage(false, Some("confirmed"), true),
            WorkflowNextStage::ChangeExistingModel
        );
        assert_eq!(
            workflow_next_stage(false, Some("cancelled"), false),
            WorkflowNextStage::Invalid
        );
    }

    #[test]
    fn pipeline_task_names_map_to_stable_agent_roles() {
        assert_eq!(pipeline_agent_role("plan_decision_model"), "architect");
        assert_eq!(pipeline_agent_role("validate_decision_plan"), "validator");
        assert_eq!(pipeline_agent_role("custom_agent"), "custom_agent");
    }

    #[test]
    fn unified_orchestrator_request_accepts_existing_stage_payload_shapes() {
        let timestamp = Utc::now();
        let analysis = serde_json::to_value(AnalyzeRequirementRequest {
            text: "需求".into(),
            retry_of_orchestration_id: Some(7),
        })
        .unwrap();
        let clarification = serde_json::to_value(ContinueClarificationRequest {
            answers: vec![ClarificationAnswer {
                clarification_id: "clarification-1".into(),
                answer: "已确认".into(),
            }],
            expected_updated_at: timestamp.clone(),
            retry_of_orchestration_id: Some(8),
        })
        .unwrap();
        let planning = serde_json::to_value(CreateDecisionPlanRequest {
            answers: vec![],
            expected_updated_at: timestamp.clone(),
            retry_of_orchestration_id: Some(9),
        })
        .unwrap();

        let analysis =
            serde_json::from_value::<OrchestrateDecisionWorkflowRequest>(analysis).unwrap();
        assert_eq!(analysis.text.as_deref(), Some("需求"));
        assert_eq!(analysis.retry_of_orchestration_id, Some(7));

        let clarification =
            serde_json::from_value::<OrchestrateDecisionWorkflowRequest>(clarification).unwrap();
        assert_eq!(clarification.answers[0].answer, "已确认");
        assert_eq!(clarification.expected_updated_at, Some(timestamp));
        assert_eq!(clarification.retry_of_orchestration_id, Some(8));

        let planning =
            serde_json::from_value::<OrchestrateDecisionWorkflowRequest>(planning).unwrap();
        assert_eq!(planning.expected_updated_at, Some(timestamp));
        assert_eq!(planning.retry_of_orchestration_id, Some(9));
    }

    #[test]
    fn prompt_digest_tracks_exact_prompt_content() {
        let prompt = crate::nl_dsl::prompts::REQUIREMENT_ANALYSIS_PROMPT;
        assert_eq!(prompt_digest(prompt), prompt_digest(prompt));
        assert_ne!(prompt_digest(prompt), prompt_digest("changed prompt"));
        assert_eq!(
            crate::nl_dsl::prompts::REQUIREMENT_ANALYSIS_PROMPT_VERSION,
            "requirement-analysis-v1"
        );
        assert_eq!(
            crate::nl_dsl::prompts::DECISION_PLANNING_PROMPT_VERSION,
            "decision-planning-v1"
        );
        assert_eq!(
            crate::nl_dsl::prompts::DSL_GENERATION_PROMPT_VERSION,
            "dsl-generation-v1"
        );
        assert_eq!(
            crate::nl_dsl::prompts::CHANGE_INTENT_PROMPT_VERSION,
            "change-intent-v3"
        );
        assert_eq!(
            prompt_digest(crate::nl_dsl::prompts::CHANGE_INTENT_PROMPT),
            prompt_digest(crate::nl_dsl::prompts::CHANGE_INTENT_PROMPT)
        );
    }

    #[test]
    fn orchestration_history_pagination_is_bounded_and_backward_compatible() {
        assert_eq!(
            validate_agent_orchestration_history_query(AgentOrchestrationHistoryQuery {
                before_id: None,
                limit: None,
            }),
            Ok((None, 50))
        );
        assert_eq!(
            validate_agent_orchestration_history_query(AgentOrchestrationHistoryQuery {
                before_id: Some(123),
                limit: Some(100),
            }),
            Ok((Some(123), 100))
        );
        for query in [
            AgentOrchestrationHistoryQuery {
                before_id: Some(0),
                limit: None,
            },
            AgentOrchestrationHistoryQuery {
                before_id: Some(-1),
                limit: Some(50),
            },
            AgentOrchestrationHistoryQuery {
                before_id: None,
                limit: Some(0),
            },
            AgentOrchestrationHistoryQuery {
                before_id: None,
                limit: Some(101),
            },
        ] {
            assert!(validate_agent_orchestration_history_query(query).is_err());
        }
    }

    #[test]
    fn orchestration_event_replay_cursor_is_bounded() {
        assert_eq!(
            validate_agent_orchestration_event_query(AgentOrchestrationEventQuery {
                after_event_id: None,
                limit: None,
            }),
            Ok((0, 500))
        );
        assert_eq!(
            validate_agent_orchestration_event_query(AgentOrchestrationEventQuery {
                after_event_id: Some(42),
                limit: Some(1000),
            }),
            Ok((42, 1000))
        );
        for query in [
            AgentOrchestrationEventQuery {
                after_event_id: Some(-1),
                limit: None,
            },
            AgentOrchestrationEventQuery {
                after_event_id: None,
                limit: Some(0),
            },
            AgentOrchestrationEventQuery {
                after_event_id: None,
                limit: Some(1001),
            },
        ] {
            assert!(validate_agent_orchestration_event_query(query).is_err());
        }
    }

    #[test]
    fn client_disconnect_is_retryable_but_explicit_cancel_is_terminal() {
        let disconnect = "SSE 客户端连接中断";
        assert_eq!(workflow_run_terminal_status(disconnect), "failed");
        assert!(is_retryable_agent_failure(disconnect));
        assert_eq!(
            workflow_run_terminal_status("编排已由用户取消"),
            "cancelled"
        );
        assert_eq!(workflow_run_terminal_status("模型请求失败"), "failed");
    }

    #[test]
    fn orchestration_stale_timeout_defaults_and_rejects_out_of_range_values() {
        assert_eq!(
            orchestration_stale_timeout_from_raw(None),
            Ok(Duration::from_secs(120))
        );
        assert_eq!(
            orchestration_stale_timeout_from_raw(Some("30")),
            Ok(Duration::from_secs(30))
        );
        assert_eq!(
            orchestration_stale_timeout_from_raw(Some("86400")),
            Ok(Duration::from_secs(86_400))
        );
        assert!(orchestration_stale_timeout_from_raw(Some("29")).is_err());
        assert!(orchestration_stale_timeout_from_raw(Some("86401")).is_err());
        assert!(orchestration_stale_timeout_from_raw(Some("invalid")).is_err());
    }

    #[test]
    fn recovered_orchestration_error_is_explicitly_retryable() {
        let error = orchestration_interrupted_error();

        assert_eq!(error["retryable"], true);
        assert_eq!(error["failureKind"], "execution_interrupted");
        assert_eq!(error["message"], "编排执行实例失联，系统已回收未完成任务");
        assert!(!is_retryable_agent_failure(
            error["message"].as_str().unwrap()
        ));
    }

    #[test]
    fn artifact_digest_is_stable_and_content_sensitive() {
        let first = artifact_digest(&json!({ "schemaVersion": 1, "kind": "brief" })).unwrap();
        let same = artifact_digest(&json!({ "schemaVersion": 1, "kind": "brief" })).unwrap();
        let changed = artifact_digest(&json!({ "schemaVersion": 2, "kind": "brief" })).unwrap();
        assert_eq!(first, same);
        assert_ne!(first, changed);
        assert_eq!(first.len(), 64);
    }

    #[tokio::test]
    async fn parallel_work_is_cancelled_when_stream_receiver_disconnects() {
        let (tx, rx) = mpsc::channel(1);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let work_cancelled = Arc::clone(&cancelled);
        let work = tokio::spawn(async move {
            run_until_stream_disconnect(
                Some(&tx),
                std::future::pending::<anyhow::Result<()>>(),
                async move {
                    struct DropSignal(Arc<std::sync::atomic::AtomicBool>);
                    impl Drop for DropSignal {
                        fn drop(&mut self) {
                            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    }

                    let _signal = DropSignal(work_cancelled);
                    let _ = started_tx.send(());
                    std::future::pending::<anyhow::Result<()>>().await
                },
            )
            .await
        });
        started_rx.await.unwrap();
        drop(rx);
        let result = work.await.unwrap();

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("SSE 客户端连接中断")
        );
        assert!(cancelled.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(workflow_run_terminal_status("SSE 客户端连接中断"), "failed");
    }

    #[tokio::test]
    async fn completed_stream_work_wins_over_a_simultaneous_disconnect() {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);

        let result = run_until_stream_disconnect(
            Some(&tx),
            std::future::pending::<anyhow::Result<()>>(),
            async { Ok::<_, anyhow::Error>(42) },
        )
        .await;

        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn orchestration_cancel_signal_drops_active_work() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let work_cancelled = Arc::clone(&cancelled);
        let result = run_until_stream_disconnect(
            None,
            async move {
                started_rx.await.unwrap();
                Ok(())
            },
            async move {
                struct DropSignal(Arc<std::sync::atomic::AtomicBool>);
                impl Drop for DropSignal {
                    fn drop(&mut self) {
                        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                let _signal = DropSignal(work_cancelled);
                let _ = started_tx.send(());
                std::future::pending::<anyhow::Result<()>>().await
            },
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("编排已由用户取消"));
        assert!(cancelled.load(std::sync::atomic::Ordering::SeqCst));
    }

    use crate::workflow::{DecisionBrief, DecisionField, PlannedDecision};

    #[test]
    fn requirement_complexity_routes_simple_standard_and_complex_requests() {
        assert_eq!(
            classify_requirement_complexity("判断申请是否通过"),
            RequirementComplexity::Simple
        );
        assert_eq!(
            classify_requirement_complexity("根据客户等级和申请金额确定审批结果"),
            RequirementComplexity::Standard
        );
        assert_eq!(
            classify_requirement_complexity(
                "如果同时满足收入和评分条件，遇到例外按优先级处理，失败时回退并执行多阶段复核"
            ),
            RequirementComplexity::Complex
        );
    }

    #[test]
    fn simple_low_risk_route_skips_only_optional_facilitator_questions() {
        let optional = vec![Clarification {
            id: "optional-1".into(),
            question: "是否需要额外说明？".into(),
            required: false,
            ..Default::default()
        }];
        let required = vec![Clarification {
            required: true,
            ..optional[0].clone()
        }];

        assert!(simple_route_can_skip_facilitator(
            RequirementComplexity::Simple,
            false,
            &optional,
        ));
        assert!(!simple_route_can_skip_facilitator(
            RequirementComplexity::Simple,
            false,
            &required,
        ));
        assert!(!simple_route_can_skip_facilitator(
            RequirementComplexity::Simple,
            true,
            &optional,
        ));
        assert!(!simple_route_can_skip_facilitator(
            RequirementComplexity::Standard,
            false,
            &optional,
        ));
    }

    #[test]
    fn simple_direct_plan_is_temporary_traceable_and_rejects_noneligible_workflows() {
        let mut state = workflow(None);
        state.status = "analyzed".into();
        state.requirement_text = "判断申请是否通过".into();
        state.brief = DecisionBrief {
            name: "Approval".into(),
            goal: "判断申请是否通过".into(),
            risk_level: "low".into(),
            outputs: vec![crate::workflow::DecisionField {
                name: "approved".into(),
                type_ref: "boolean".into(),
                ..Default::default()
            }],
            requirement_items: vec![crate::workflow::RequirementItem {
                id: "req-goal".into(),
                source: "goal".into(),
                text: "判断申请是否通过".into(),
            }],
            ..Default::default()
        };

        let plan = match simple_direct_plan(&state) {
            Ok(plan) => plan,
            Err(_) => panic!("eligible simple workflow should yield a plan"),
        };
        assert_eq!(plan.decisions.len(), 1);
        assert_eq!(plan.decisions[0].name, "Approval");
        assert_eq!(plan.decisions[0].output_type, "boolean");
        assert_eq!(plan.decisions[0].requirement_ids, vec!["req-goal"]);

        state.brief.risk_level = "high".into();
        assert!(simple_direct_plan(&state).is_err());
        state.brief.risk_level = "low".into();
        state.clarifications.push(Clarification {
            id: "required-1".into(),
            question: "是否允许例外？".into(),
            required: true,
            ..Default::default()
        });
        assert!(simple_direct_plan(&state).is_err());
    }

    #[test]
    fn simple_planning_model_route_is_disabled_for_high_risk_requirements() {
        assert_eq!(
            planning_agent_complexity(RequirementComplexity::Simple, false),
            crate::api::AgentComplexity::Simple
        );
        assert_eq!(
            planning_agent_complexity(RequirementComplexity::Simple, true),
            crate::api::AgentComplexity::Standard
        );
        assert_eq!(
            planning_agent_complexity(RequirementComplexity::Complex, false),
            crate::api::AgentComplexity::Complex
        );
    }

    #[test]
    fn golden_requirement_complexities_match_expected_routes() {
        let corpus = include_str!("../tests/golden/requirement-complexity.tsv");
        let mut case_count = 0;
        let mut class_counts = HashMap::<&str, usize>::new();
        for line in corpus
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let fields = line.splitn(3, '\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 3, "malformed complexity golden row: {line}");
            let [id, expected, requirement] =
                <[&str; 3]>::try_from(fields).expect("three fields are required");
            let actual = classify_requirement_complexity(requirement).as_str();
            assert_eq!(actual, expected, "complexity route mismatch for {id}");
            *class_counts.entry(expected).or_default() += 1;
            case_count += 1;
        }
        assert!(
            case_count >= 28,
            "complexity golden suite only has {case_count} cases"
        );
        for class in ["simple", "standard", "complex"] {
            assert!(
                class_counts.get(class).copied().unwrap_or_default() > 0,
                "missing {class} cases"
            );
        }
    }

    #[test]
    fn tester_report_proposes_reviewable_cases_without_inventing_expected_outputs() {
        let brief = DecisionBrief {
            risk_level: "high".into(),
            goal: "审批申请并解释结果".into(),
            inputs: vec![
                DecisionField {
                    name: "amount".into(),
                    label: "申请金额".into(),
                    type_ref: "number".into(),
                    required: true,
                    ..Default::default()
                },
                DecisionField {
                    name: "customerTier".into(),
                    type_ref: "string".into(),
                    description: "可选客户等级".into(),
                    ..Default::default()
                },
            ],
            constraints: vec!["金额超过上限时必须人工复核".into()],
            requirement_items: vec![crate::workflow::RequirementItem {
                id: "req-limit".into(),
                source: "requirement".into(),
                text: "金额超过上限时转人工复核".into(),
            }],
            ..Default::default()
        };
        let plan = DecisionPlan {
            decisions: vec![PlannedDecision {
                name: "approval".into(),
                purpose: "判断审批结果".into(),
                ..Default::default()
            }],
            acceptance_criteria: vec!["金额边界行为已确认".into()],
            ..Default::default()
        };
        let answers = vec![ClarificationAnswer {
            clarification_id: "boundary".into(),
            answer: "边界包含".into(),
        }];

        let report =
            generate_tester_report(&brief, &plan, &answers, RequirementComplexity::Complex);

        assert_eq!(report.schema_version, 1);
        assert!(
            report
                .proposals
                .iter()
                .any(|item| item.category == "required_input")
        );
        assert!(
            report
                .proposals
                .iter()
                .any(|item| item.category == "optional_input")
        );
        assert!(
            report
                .proposals
                .iter()
                .any(|item| item.category == "clarification")
        );
        assert!(
            report
                .proposals
                .iter()
                .any(|item| item.category == "acceptance_criterion")
        );
        assert!(
            report
                .proposals
                .iter()
                .any(|item| item.category == "decision_contract")
        );
        let semantic = report
            .proposals
            .iter()
            .find(|item| item.category == "semantic_requirement")
            .expect("each stable requirement item should create a semantic scenario");
        assert!(semantic.source_reference.contains("req-limit"));
        let adversarial = report
            .proposals
            .iter()
            .find(|item| item.category == "adversarial")
            .expect("each explicit constraint should create a targeted stress scenario");
        assert!(adversarial.title.contains("金额超过上限"));
        assert_eq!(adversarial.source_reference, "brief.constraints[0]");
        assert!(
            report
                .proposals
                .iter()
                .any(|item| item.category == "adversarial")
        );
        assert!(
            report
                .proposals
                .iter()
                .all(|item| { item.expected_output.is_none() && item.review_required })
        );
        assert_eq!(
            report,
            generate_tester_report(&brief, &plan, &answers, RequirementComplexity::Complex)
        );
    }

    #[test]
    fn tester_report_uses_goal_when_stable_requirement_items_are_missing() {
        let brief = DecisionBrief {
            goal: "判断订单是否满足优惠条件".into(),
            ..Default::default()
        };
        let report = generate_tester_report(
            &brief,
            &DecisionPlan::default(),
            &[],
            RequirementComplexity::Simple,
        );
        assert_eq!(report.proposals.len(), 1);
        assert_eq!(report.proposals[0].category, "semantic_scenario");
        assert_eq!(report.proposals[0].source_reference, "brief.goal");
        assert!(report.proposals[0].expected_output.is_none());
    }

    #[test]
    fn static_findings_create_version_bound_reviewable_tester_proposals() {
        let brief = DecisionBrief {
            inputs: vec![crate::workflow::DecisionField {
                name: "amount".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut report = TesterReport {
            schema_version: 1,
            ..Default::default()
        };
        let findings = vec![
            crate::models::VerificationFinding {
                level: "high".into(),
                code: "UNREACHABLE_RULE".into(),
                message: "规则被前序条件覆盖".into(),
                element: "Approval".into(),
                rule_ids: vec!["r1".into(), "r2".into()],
            },
            crate::models::VerificationFinding {
                level: "low".into(),
                code: "UNSUPPORTED_STATIC_CHECK".into(),
                message: "unknown finding".into(),
                element: "Approval".into(),
                rule_ids: Vec::new(),
            },
        ];

        assert_eq!(
            append_static_analysis_proposals(&mut report, &brief, 52, &findings, &HashMap::new()),
            1
        );
        let proposal = &report.proposals[0];
        assert_eq!(proposal.category, "static_analysis");
        assert!(proposal.source_reference.contains("draftVersion:52"));
        assert!(proposal.source_reference.contains("UNREACHABLE_RULE"));
        assert_eq!(proposal.input_fields, vec!["amount"]);
        assert!(proposal.expected_output.is_none());
        assert!(proposal.review_required);
        assert_eq!(
            append_static_analysis_proposals(&mut report, &brief, 52, &findings, &HashMap::new()),
            0
        );
        assert_eq!(report.proposals.len(), 1);
    }

    #[test]
    fn static_tester_proposal_attaches_only_suggested_input_not_expected_output() {
        let brief = DecisionBrief {
            inputs: vec![crate::workflow::DecisionField {
                name: "amount".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let finding = crate::models::VerificationFinding {
            level: "medium".into(),
            code: "MISSING_COVERAGE".into(),
            message: "决策表存在未覆盖输入".into(),
            element: "Approval".into(),
            rule_ids: Vec::new(),
        };
        let source_reference = static_analysis_source_reference(52, &finding);
        let suggested_inputs =
            HashMap::from([(source_reference, serde_json::json!({"amount": 0}))]);
        let mut report = TesterReport {
            schema_version: 1,
            ..Default::default()
        };

        assert_eq!(
            append_static_analysis_proposals(
                &mut report,
                &brief,
                52,
                &[finding],
                &suggested_inputs,
            ),
            1
        );
        assert_eq!(
            report.proposals[0].suggested_input,
            Some(serde_json::json!({"amount": 0}))
        );
        assert!(report.proposals[0].expected_output.is_none());
        assert!(report.proposals[0].review_required);
    }

    fn workflow(base_revision: Option<i64>) -> DecisionWorkflowState {
        DecisionWorkflowState {
            project_id: 1,
            simple_direct_eligible: false,
            base_revision,
            status: "analyzed".into(),
            requirement_text: "本次将订单门槛调整为 500 元".into(),
            original_requirement_text: Some("订单满 300 元享受优惠".into()),
            brief: DecisionBrief::default(),
            clarifications: Vec::new(),
            answers: Vec::new(),
            plan: None,
            tester_report: None,
            confirmed_at: None,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn modification_context_includes_original_requirement_and_current_dsl() {
        let state = workflow(Some(42));
        let context: Value = serde_json::from_str(
            &analysis_model_context(Some("current model"), Some(&state)).unwrap(),
        )
        .unwrap();
        assert_eq!(context["currentDsl"], "current model");
        assert_eq!(context["originalRequirement"], "订单满 300 元享受优惠");
        assert_eq!(context["latestRequirement"], state.requirement_text);
        assert!(analysis_model_context(None, Some(&state)).is_none());
    }

    #[test]
    fn original_requirement_survives_reanalysis_and_later_model_revisions() {
        let mut state = workflow(Some(42));
        for revision in [42, 43] {
            state.original_requirement_text =
                requirement_origin("再次修改门槛", Some(revision), Some(&state));
            state.requirement_text = "再次修改门槛".into();
            assert_eq!(
                state.original_requirement_text.as_deref(),
                Some("订单满 300 元享受优惠")
            );
        }
        assert_eq!(
            requirement_origin("初始需求", None, None).as_deref(),
            Some("初始需求")
        );
        assert_eq!(
            requirement_origin("重新分析新建需求", None, Some(&state)).as_deref(),
            Some("重新分析新建需求")
        );
    }

    #[test]
    fn imported_model_does_not_treat_change_instructions_as_original_requirement() {
        assert!(requirement_origin("修改导入模型", Some(42), None).is_none());
        let mut state = workflow(Some(42));
        state.original_requirement_text = None;
        assert!(requirement_origin("再次修改", Some(42), Some(&state)).is_none());
        state.base_revision = None;
        assert_eq!(
            requirement_origin("修改旧工作流", Some(42), Some(&state)),
            Some(state.requirement_text)
        );
    }

    #[test]
    fn required_clarifications_must_be_answered() {
        let clarifications = vec![Clarification {
            id: "boundary".into(),
            question: "阈值是否包含边界？".into(),
            required: true,
            ..Default::default()
        }];
        assert!(validate_answers(&clarifications, &[]).is_err());
        assert!(
            validate_answers(
                &clarifications,
                &[ClarificationAnswer {
                    clarification_id: "boundary".into(),
                    answer: "包含".into(),
                }]
            )
            .is_ok()
        );
    }

    #[test]
    fn estimated_response_reservation_scales_with_possible_calls() {
        assert_eq!(
            crate::api::estimated_run_cost_reservation(Some(0.125), 3),
            Some(0.375)
        );
        assert_eq!(
            crate::api::estimated_run_cost_reservation(Some(0.125), 0),
            Some(0.125)
        );
        assert_eq!(crate::api::estimated_run_cost_reservation(None, 3), None);
    }

    #[test]
    fn clarification_answers_reject_unknown_and_duplicate_ids() {
        let clarifications = vec![Clarification {
            id: "boundary".into(),
            question: "阈值是否包含边界？".into(),
            required: true,
            ..Default::default()
        }];
        assert!(
            validate_answers(
                &clarifications,
                &[ClarificationAnswer {
                    clarification_id: "unknown".into(),
                    answer: "包含".into(),
                }]
            )
            .is_err()
        );
        assert!(
            validate_answers(
                &clarifications,
                &[
                    ClarificationAnswer {
                        clarification_id: "boundary".into(),
                        answer: "包含".into(),
                    },
                    ClarificationAnswer {
                        clarification_id: "boundary".into(),
                        answer: "不包含".into(),
                    },
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn facilitator_round_limits_questions_and_suppresses_repeat_wording() {
        let existing = vec![Clarification {
            id: "first".into(),
            question: "申请金额是否包含边界？".into(),
            required: true,
            round: 1,
            ..Default::default()
        }];
        let candidates = [
            "申请金额是否包含边界",
            "例外申请是否需要经理审批？",
            "逾期申请是否允许补交材料？",
            "跨区域申请使用哪个时区？",
            "是否需要保留处理记录？",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, question)| Clarification {
            id: format!("candidate-{index}"),
            question: question.into(),
            required: true,
            ..Default::default()
        })
        .collect();

        let next = next_clarification_round(candidates, &existing, 2);

        assert_eq!(next.len(), MAX_CLARIFICATIONS_PER_ROUND);
        assert!(next.iter().all(|item| item.round == 2));
        assert!(!next.iter().any(|item| item.id == "candidate-0"));
        assert_eq!(next[0].id, "candidate-1");
    }

    #[test]
    fn facilitator_answer_merge_preserves_history_and_replaces_current_values() {
        let previous = vec![
            ClarificationAnswer {
                clarification_id: "first".into(),
                answer: "包含".into(),
            },
            ClarificationAnswer {
                clarification_id: "second".into(),
                answer: "此前回答".into(),
            },
        ];
        let current = vec![ClarificationAnswer {
            clarification_id: "second".into(),
            answer: "已更正".into(),
        }];

        let merged = merge_clarification_answers(&previous, &current);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].clarification_id, "first");
        assert_eq!(merged[1].answer, "已更正");
    }

    #[test]
    fn facilitator_assigns_new_ids_when_model_reuses_an_old_identifier() {
        let existing = vec![Clarification {
            id: "same-id".into(),
            question: "额度是否包含优惠？".into(),
            round: 1,
            ..Default::default()
        }];
        let candidates = vec![Clarification {
            id: "same-id".into(),
            question: "退款是否需要二次审批？".into(),
            ..Default::default()
        }];

        let next = next_clarification_round(candidates, &existing, 2);

        assert_eq!(next.len(), 1);
        assert_ne!(next[0].id, "same-id");
        assert_eq!(next[0].round, 2);
    }

    #[test]
    fn clarification_intelligence_deduplicates_and_prioritizes_required_items() {
        let analysis = RequirementAnalysis {
            clarifications: vec![
                Clarification {
                    id: "optional".into(),
                    question: "是否需要经理审批？".into(),
                    required: false,
                    ..Default::default()
                },
                Clarification {
                    id: "boundary-optional".into(),
                    question: "金额是否包含边界？".into(),
                    required: false,
                    ..Default::default()
                },
                Clarification {
                    id: "boundary-required".into(),
                    question: "金额是否包含边界？!".into(),
                    required: true,
                    reason: "影响规则命中".into(),
                    options: vec!["包含".into(), "不包含".into()],
                    round: 1,
                },
            ],
            ..Default::default()
        };

        let original_count = analysis.clarifications.len();
        let result = prioritize_clarifications(analysis);

        assert_eq!(result.clarifications.len(), 2);
        let reduction_percent =
            (original_count - result.clarifications.len()) as f64 / original_count as f64 * 100.0;
        assert!(reduction_percent >= 30.0);
        assert_eq!(result.clarifications[0].id, "boundary-optional");
        assert!(result.clarifications[0].required);
        assert_eq!(result.clarifications[0].reason, "影响规则命中");
        assert_eq!(result.clarifications[0].options, ["包含", "不包含"]);
        assert_eq!(result.clarifications[1].id, "optional");
    }

    #[test]
    fn detected_business_patterns_are_saved_in_the_analysis_brief() {
        let analysis = RequirementAnalysis::default();
        let detections =
            PatternDetector::default().detect_patterns("如果评分超过阈值，则进入审批流程");
        let enriched = attach_detected_patterns(analysis, &detections);
        assert!(
            enriched
                .brief
                .detected_patterns
                .iter()
                .any(|pattern| pattern.pattern_type == "tree")
        );
        assert!(
            enriched
                .brief
                .detected_patterns
                .iter()
                .all(|pattern| pattern.confidence_percent > 0)
        );
    }

    #[tokio::test]
    async fn requirement_analysis_postprocessing_runs_in_pipeline_order() {
        let analysis = RequirementAnalysis {
            clarifications: vec![
                Clarification {
                    id: "optional".into(),
                    question: "是否需要经理审批？".into(),
                    required: false,
                    ..Default::default()
                },
                Clarification {
                    id: "boundary".into(),
                    question: "金额是否包含边界？".into(),
                    required: true,
                    ..Default::default()
                },
                Clarification {
                    id: "boundary-duplicate".into(),
                    question: "金额是否包含边界？!".into(),
                    required: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let patterns =
            PatternDetector::default().detect_patterns("如果评分超过阈值，则进入审批流程");

        let result = postprocess_requirement_analysis(analysis, patterns, None)
            .await
            .unwrap();

        assert_eq!(result.clarifications.len(), 2);
        assert!(result.clarifications[0].required);
        assert!(
            result
                .brief
                .detected_patterns
                .iter()
                .any(|pattern| pattern.pattern_type == "tree")
        );
    }

    #[test]
    fn analysis_and_plan_require_executable_core_fields() {
        let analysis = RequirementAnalysis {
            brief: DecisionBrief {
                name: "Approval".into(),
                goal: "审批申请".into(),
                inputs: vec![DecisionField {
                    name: "Amount".into(),
                    type_ref: "number".into(),
                    ..Default::default()
                }],
                risk_level: "medium".into(),
                ..Default::default()
            },
            clarifications: vec![],
        };
        assert!(validate_analysis(&analysis).is_ok());
        let plan = DecisionPlan {
            name: "Approval".into(),
            summary: "审批决策".into(),
            decisions: vec![PlannedDecision {
                name: "Result".into(),
                purpose: "输出审批结果".into(),
                output_type: "string".into(),
                expression_type: "decisionTable".into(),
                dependencies: vec!["Amount".into()],
                requirement_ids: vec![],
            }],
            ..Default::default()
        };
        assert!(validate_plan(&plan, None).is_ok());
    }

    #[test]
    fn requirement_items_cover_brief_facts_with_stable_ids() {
        let brief = DecisionBrief {
            goal: "判断是否批准".into(),
            description: "贷款审批规则".into(),
            inputs: vec![crate::workflow::DecisionField {
                name: "Amount".into(),
                label: "申请金额".into(),
                type_ref: "number".into(),
                ..Default::default()
            }],
            constraints: vec!["金额不得为负数".into()],
            ..Default::default()
        };
        let first = requirement_items(&brief);
        let second = requirement_items(&brief);
        let mut reordered_brief = brief.clone();
        reordered_brief
            .constraints
            .insert(0, "申请人必须已成年".into());
        let reordered = requirement_items(&reordered_brief);

        assert_eq!(first, second);
        assert!(first.iter().all(|item| reordered.contains(item)));
        assert!(first.iter().any(|item| item.source == "goal"));
        assert!(first.iter().any(|item| item.source == "input:Amount"));
        assert!(first.iter().any(|item| item.source == "constraint"));
        assert!(first.iter().all(|item| item.id.starts_with("req-")));
    }

    #[test]
    fn plans_with_requirement_items_must_reference_valid_ids_per_decision() {
        let brief = DecisionBrief {
            requirement_items: vec![RequirementItem {
                id: "req-valid".into(),
                source: "goal".into(),
                text: "判断申请是否批准".into(),
            }],
            ..Default::default()
        };
        let mut plan = DecisionPlan {
            name: "审批方案".into(),
            summary: "输出审批结论".into(),
            decisions: vec![PlannedDecision {
                name: "Approval".into(),
                purpose: "判断审批结果".into(),
                output_type: "string".into(),
                expression_type: "decisionTable".into(),
                requirement_ids: vec!["req-valid".into()],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert!(validate_plan(&plan, Some(&brief)).is_ok());
        plan.decisions[0].requirement_ids.clear();
        assert!(validate_plan(&plan, Some(&brief)).is_err());
        plan.decisions[0].requirement_ids = vec!["req-unknown".into()];
        assert!(validate_plan(&plan, Some(&brief)).is_err());
    }

    #[test]
    fn golden_planning_requirements_satisfy_the_plan_contract() {
        let corpus = include_str!("../tests/golden/planning-contracts.tsv");
        let mut case_count = 0;
        for line in corpus
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let fields = line.splitn(6, '\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 6, "malformed planning golden row: {line}");
            let [
                id,
                requirement,
                decision_name,
                output_type,
                expression_type,
                dependencies,
            ] = <[&str; 6]>::try_from(fields).expect("six fields are required");
            assert!(!requirement.trim().is_empty(), "{id} has no requirement");
            let plan = DecisionPlan {
                name: format!("{id} plan"),
                summary: requirement.into(),
                decisions: vec![PlannedDecision {
                    name: decision_name.into(),
                    purpose: requirement.into(),
                    output_type: output_type.into(),
                    expression_type: expression_type.into(),
                    dependencies: dependencies
                        .split(',')
                        .filter(|dependency| !dependency.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    requirement_ids: vec![],
                }],
                ..Default::default()
            };
            validate_plan(&plan, None).unwrap_or_else(|error| panic!("{id}: {}", error.message));
            case_count += 1;
        }
        assert!(
            case_count >= 20,
            "planning golden suite only has {case_count} cases"
        );
    }

    #[tokio::test]
    async fn planning_pipeline_rejects_invalid_plan_at_validation_stage() {
        let failure = run_pipeline(
            vec![decision_plan_validation_stage(DecisionBrief::default())],
            serde_json::to_value(DecisionPlan::default()).unwrap(),
        )
        .await
        .unwrap_err();

        assert_eq!(failure.failed_step, "validate_decision_plan");
        assert!(failure.message.contains("至少一个决策"));
        assert!(failure.completed_steps.is_empty());
    }

    #[test]
    fn only_transient_provider_failures_are_retryable() {
        for error in [
            "LLM API 请求失败 [503 Service Unavailable]",
            "request timed out",
            "连接失败: connection reset",
            "SSE 客户端连接中断",
        ] {
            assert!(is_retryable_agent_failure(error), "{error}");
        }
        for error in [
            "LLM API 请求失败 [401 Unauthorized]",
            "HTTP 400 invalid request",
            "编排 token 预算已用尽",
            "方案校验失败：缺少决策",
        ] {
            assert!(!is_retryable_agent_failure(error), "{error}");
        }
    }

    #[test]
    fn plan_failure_retry_context_requires_a_transient_model_error() {
        let transient = WorkflowApiError::model("方案规划", anyhow!("HTTP 503 unavailable"))
            .with_orchestration(71);
        assert_eq!(transient.orchestration_id, Some(71));
        assert!(transient.retryable);

        let deterministic =
            WorkflowApiError::bad_request("方案缺少决策输出").with_orchestration(72);
        assert_eq!(deterministic.orchestration_id, Some(72));
        assert!(!deterministic.retryable);
    }
}
