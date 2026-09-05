use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::{StreamExt as _, wrappers::ReceiverStream};

use crate::api::{LlmState, LlmTask};
use crate::knowledge::{citations, effective_project_sources, knowledge_context};
use crate::nl_dsl::agent::{LlmAgent, StreamEvent};
use crate::project_access::{self, ProjectPermission};
use crate::types::{AppState, Claims};
use crate::workflow::{
    AnalyzeRequirementRequest, Clarification, ClarificationAnswer, CreateDecisionPlanRequest,
    DecisionPlan, DecisionWorkflowState, RequirementAnalysis,
};

struct WorkflowApiError {
    status: StatusCode,
    message: String,
}

impl WorkflowApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "决策项目或需求工作流不存在".to_string(),
        }
    }

    fn unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "未配置可用的大模型，无法执行智能工作流".to_string(),
        }
    }

    fn model(operation: &str, error: anyhow::Error) -> Self {
        eprintln!("智能工作流失败 [{operation}]: {error}");
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: format!("{operation}失败：{error}"),
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
        }
    }
}

impl IntoResponse for WorkflowApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "ok": false, "error": self.message })),
        )
            .into_response()
    }
}

#[derive(sqlx::FromRow)]
struct WorkflowRow {
    project_id: i64,
    status: String,
    requirement_text: String,
    decision_brief_json: Value,
    clarifications_json: Value,
    answers_json: Value,
    decision_plan_json: Option<Value>,
    confirmed_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

impl WorkflowRow {
    fn into_state(self) -> Result<DecisionWorkflowState, WorkflowApiError> {
        Ok(DecisionWorkflowState {
            project_id: self.project_id,
            status: self.status,
            requirement_text: self.requirement_text,
            brief: parse_saved(self.decision_brief_json, "需求摘要")?,
            clarifications: parse_saved(self.clarifications_json, "澄清问题")?,
            answers: parse_saved(self.answers_json, "澄清回答")?,
            plan: self
                .decision_plan_json
                .map(|value| parse_saved(value, "决策方案"))
                .transpose()?,
            confirmed_at: self.confirmed_at,
            updated_at: self.updated_at,
        })
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

async fn save_requirement_analysis(
    state: &AppState,
    project_id: i64,
    user_id: &str,
    requirement: &str,
    analysis: &RequirementAnalysis,
) -> Result<DecisionWorkflowState, WorkflowApiError> {
    validate_analysis(analysis)?;
    let status = if analysis.clarifications.is_empty() {
        "analyzed"
    } else {
        "clarification"
    };
    sqlx::query(
        r#"INSERT INTO decision_workflows (
            project_id, status, requirement_text, decision_brief_json,
            clarifications_json, answers_json, decision_plan_json, confirmed_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, '[]'::JSONB, NULL, NULL, NOW())
        ON CONFLICT (project_id) DO UPDATE SET
            status = EXCLUDED.status, requirement_text = EXCLUDED.requirement_text,
            decision_brief_json = EXCLUDED.decision_brief_json,
            clarifications_json = EXCLUDED.clarifications_json,
            answers_json = '[]'::JSONB, decision_plan_json = NULL,
            confirmed_at = NULL, updated_at = NOW()"#,
    )
    .bind(project_id)
    .bind(status)
    .bind(requirement)
    .bind(serde_json::to_value(&analysis.brief).unwrap_or(Value::Null))
    .bind(serde_json::to_value(&analysis.clarifications).unwrap_or(Value::Array(vec![])))
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("保存需求分析", error))?;
    load_workflow(state, project_id, user_id)
        .await?
        .ok_or_else(WorkflowApiError::not_found)
}

fn validate_plan(plan: &DecisionPlan) -> Result<(), WorkflowApiError> {
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
    Ok(())
}

fn validate_answers(
    clarifications: &[Clarification],
    answers: &[ClarificationAnswer],
) -> Result<(), WorkflowApiError> {
    let answer_map = answers
        .iter()
        .map(|answer| (answer.clarification_id.as_str(), answer.answer.trim()))
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
        SELECT workflow.project_id, workflow.status, workflow.requirement_text,
               workflow.decision_brief_json, workflow.clarifications_json,
               workflow.answers_json, workflow.decision_plan_json,
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
    task: LlmTask,
    agent: &LlmAgent,
    input: Value,
) -> Result<i64, WorkflowApiError> {
    sqlx::query_scalar(
        r#"
        INSERT INTO agent_runs (project_id, stage, status, model, prompt_version, input_json)
        VALUES ($1, $2, 'running', $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(project_id)
    .bind(task.audit_stage())
    .bind(agent.model())
    .bind(format!("workflow-v2:{}", task.audit_stage()))
    .bind(input)
    .fetch_one(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("记录智能运行", error))
}

async fn finish_agent_run(
    state: &AppState,
    run_id: i64,
    output: Result<Value, String>,
) -> Result<(), WorkflowApiError> {
    let result = match output {
        Ok(output) => {
            sqlx::query(
                "UPDATE agent_runs SET status = 'succeeded', output_json = $1, finished_at = NOW() WHERE id = $2",
            )
            .bind(output)
            .bind(run_id)
            .execute(&state.db)
            .await
        }
        Err(error) => {
            sqlx::query(
                "UPDATE agent_runs SET status = 'failed', error_json = $1, finished_at = NOW() WHERE id = $2",
            )
            .bind(json!({ "message": error }))
            .bind(run_id)
            .execute(&state.db)
            .await
        }
    };
    result
        .map(|_| ())
        .map_err(|error| WorkflowApiError::database("更新智能运行", error))
}

async fn get_workflow(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Option<DecisionWorkflowState>>, WorkflowApiError> {
    ensure_project(&state, project_id, &claims.sub).await?;
    Ok(Json(load_workflow(&state, project_id, &claims.sub).await?))
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
    let knowledge_sources = effective_project_sources(&state.db, project_id, &claims.sub)
        .await
        .map_err(|error| WorkflowApiError::database("读取项目有效知识", error))?;
    let knowledge_text = knowledge_context(&knowledge_sources);
    let knowledge_citations = citations(&knowledge_sources);
    let agent = llm_state
        .agent_for(LlmTask::RequirementAnalysis)
        .cloned()
        .ok_or_else(WorkflowApiError::unavailable)?;
    let run_id = begin_agent_run(
        &state,
        project_id,
        LlmTask::RequirementAnalysis,
        &agent,
        json!({
            "requirement": requirement,
            "knowledgeCitations": knowledge_citations,
        }),
    )
    .await?;
    let analysis = match agent
        .analyze_requirement(requirement, Some(&knowledge_text))
        .await
    {
        Ok(analysis) => analysis,
        Err(error) => {
            let message = error.to_string();
            let _ = finish_agent_run(&state, run_id, Err(message.clone())).await;
            return Err(WorkflowApiError::model("需求分析", error));
        }
    };
    if let Err(error) = validate_analysis(&analysis) {
        let _ = finish_agent_run(&state, run_id, Err(error.message.clone())).await;
        return Err(error);
    }
    finish_agent_run(
        &state,
        run_id,
        Ok(serde_json::to_value(&analysis).unwrap_or(Value::Null)),
    )
    .await?;
    save_requirement_analysis(&state, project_id, &claims.sub, requirement, &analysis)
        .await
        .map(Json)
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
    let knowledge_sources = match effective_project_sources(&state.db, project_id, &claims.sub)
        .await
    {
        Ok(value) => value,
        Err(error) => return WorkflowApiError::database("读取项目有效知识", error).into_response(),
    };
    let knowledge_text = knowledge_context(&knowledge_sources);
    let agent = match llm_state.agent_for(LlmTask::RequirementAnalysis).cloned() {
        Some(agent) => agent,
        None => return WorkflowApiError::unavailable().into_response(),
    };
    let run_id = match begin_agent_run(
        &state,
        project_id,
        LlmTask::RequirementAnalysis,
        &agent,
        json!({"requirement": requirement, "knowledgeCitations": citations(&knowledge_sources)}),
    )
    .await
    {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let (tx, rx) = mpsc::channel::<StreamEvent>(32);
    let tx2 = tx.clone();
    let user_id = claims.sub.clone();
    tokio::spawn(async move {
        let result = agent
            .analyze_requirement_stream(&requirement, Some(&knowledge_text), &tx2)
            .await;
        let event = match result {
            Ok(analysis) => match save_requirement_analysis(
                &state,
                project_id,
                &user_id,
                &requirement,
                &analysis,
            )
            .await
            {
                Ok(workflow) => {
                    let _ = finish_agent_run(
                        &state,
                        run_id,
                        Ok(serde_json::to_value(&analysis).unwrap_or(Value::Null)),
                    )
                    .await;
                    StreamEvent::Finished(serde_json::to_value(workflow).map_err(|e| e.to_string()))
                }
                Err(error) => {
                    let _ = finish_agent_run(&state, run_id, Err(error.message.clone())).await;
                    StreamEvent::Finished(Err(error.message))
                }
            },
            Err(error) => {
                let message = error.to_string();
                let _ = finish_agent_run(&state, run_id, Err(message.clone())).await;
                StreamEvent::Finished(Err(message))
            }
        };
        let _ = tx2.send(event).await;
    });
    workflow_sse_response(rx)
}

fn workflow_sse_response(rx: mpsc::Receiver<StreamEvent>) -> Response {
    let stream = ReceiverStream::new(rx).map(|event| {
        let (name, data) = match event {
            StreamEvent::Step(message) => ("step", json!({"message": message})),
            StreamEvent::Delta(content) => ("delta", json!({"content": content})),
            StreamEvent::Preview(content) => ("preview", json!({"content": content})),
            StreamEvent::Finished(Ok(payload)) => ("workflow_result", payload),
            StreamEvent::Finished(Err(error)) => ("error", json!({"ok": false, "error": error})),
        };
        Ok::<Event, Infallible>(Event::default().event(name).data(data.to_string()))
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn create_plan(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<CreateDecisionPlanRequest>,
) -> Result<Json<DecisionWorkflowState>, WorkflowApiError> {
    ensure_model_permission(&state, project_id, &claims.sub).await?;
    let workflow = load_workflow(&state, project_id, &claims.sub)
        .await?
        .ok_or_else(WorkflowApiError::not_found)?;
    validate_answers(&workflow.clarifications, &payload.answers)?;
    let knowledge_sources = effective_project_sources(&state.db, project_id, &claims.sub)
        .await
        .map_err(|error| WorkflowApiError::database("读取项目有效知识", error))?;
    let knowledge_text = knowledge_context(&knowledge_sources);
    let knowledge_citations = citations(&knowledge_sources);
    let agent = llm_state
        .agent_for(LlmTask::DecisionPlanning)
        .cloned()
        .ok_or_else(WorkflowApiError::unavailable)?;
    let run_id = begin_agent_run(
        &state,
        project_id,
        LlmTask::DecisionPlanning,
        &agent,
        json!({
            "brief": workflow.brief,
            "answers": payload.answers,
            "knowledgeCitations": knowledge_citations,
        }),
    )
    .await?;
    let plan = match agent
        .plan_decision_model(&workflow.brief, &payload.answers, Some(&knowledge_text))
        .await
    {
        Ok(plan) => plan,
        Err(error) => {
            let message = error.to_string();
            let _ = finish_agent_run(&state, run_id, Err(message.clone())).await;
            return Err(WorkflowApiError::model("决策方案规划", error));
        }
    };
    if let Err(error) = validate_plan(&plan) {
        let _ = finish_agent_run(&state, run_id, Err(error.message.clone())).await;
        return Err(error);
    }
    finish_agent_run(
        &state,
        run_id,
        Ok(serde_json::to_value(&plan).unwrap_or(Value::Null)),
    )
    .await?;
    sqlx::query(
        r#"
        UPDATE decision_workflows workflow
        SET status = 'planned', answers_json = $1, decision_plan_json = $2,
            confirmed_at = NULL, updated_at = NOW()
        WHERE workflow.project_id = $3
        "#,
    )
    .bind(serde_json::to_value(&payload.answers).unwrap_or(Value::Array(vec![])))
    .bind(serde_json::to_value(&plan).unwrap_or(Value::Null))
    .bind(project_id)
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("保存决策方案", error))?;
    load_workflow(&state, project_id, &claims.sub)
        .await?
        .map(Json)
        .ok_or_else(WorkflowApiError::not_found)
}

async fn confirm_plan(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<DecisionWorkflowState>, WorkflowApiError> {
    ensure_model_permission(&state, project_id, &claims.sub).await?;
    let result = sqlx::query(
        r#"
        UPDATE decision_workflows workflow
        SET status = 'confirmed', confirmed_at = NOW(), updated_at = NOW()
        WHERE workflow.project_id = $1 AND workflow.status = 'planned'
          AND workflow.decision_plan_json IS NOT NULL
        "#,
    )
    .bind(project_id)
    .execute(&state.db)
    .await
    .map_err(|error| WorkflowApiError::database("确认决策方案", error))?;
    if result.rows_affected() == 0 {
        return Err(WorkflowApiError::bad_request(
            "只有已生成的决策方案可以确认",
        ));
    }
    load_workflow(&state, project_id, &claims.sub)
        .await?
        .map(Json)
        .ok_or_else(WorkflowApiError::not_found)
}

pub fn routes(llm_state: Arc<LlmState>) -> Router<AppState> {
    Router::new()
        .route("/projects/{project_id}/workflow", get(get_workflow))
        .route(
            "/projects/{project_id}/workflow/analyze-stream",
            post(analyze_requirement_stream),
        )
        .route(
            "/projects/{project_id}/workflow/analyze",
            post(analyze_requirement),
        )
        .route("/projects/{project_id}/workflow/plan", post(create_plan))
        .route(
            "/projects/{project_id}/workflow/confirm",
            post(confirm_plan),
        )
        .layer(Extension(llm_state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{DecisionBrief, DecisionField, PlannedDecision};

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
            }],
            ..Default::default()
        };
        assert!(validate_plan(&plan).is_ok());
    }
}
