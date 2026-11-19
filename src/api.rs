#[cfg(target_family = "wasm")]
use crate::models::{
    AdminBusinessQualityMetricView, AdminUser, AgentOrchestrationHistoryItem,
    ApproveDecisionTestCase, AuditEvent, CreateDecisionProject, CreateDecisionTestCase,
    CreateDecisionVersion, CreateUser, CreatedDecisionServiceApiKey, DecisionAnalyticsReport,
    DecisionAnalyticsThresholds, DecisionCopilotQueryRequest, DecisionCopilotQueryResponse,
    DecisionDriftEvent, DecisionDriftEventActionResult, DecisionDriftEventReviewResult,
    DecisionDriftImpactReport, DecisionDriftScanWindow, DecisionDriftScanWindowReviewResult,
    DecisionExplanationAdminMetricView, DecisionOptimizationProposal, DecisionProject,
    DecisionRunExplanationReviewRequest, DecisionRunExplanationReviewView,
    DecisionRunExplanationView, DecisionServiceApiKey, DecisionServiceRun, DecisionTestCase,
    DecisionVersion, DecisionWorkspace, DeviationAnalysis, DeviationAnalysisReviewResult,
    ExecuteDecisionService, ExecutionTrace, GenerateDecisionTestCasesResult,
    InstantiateModelLibraryItem, KnowledgeIndexStatus, KnowledgeSearchDiagnostics, KnowledgeSource,
    ModelChangeIntent, ModelChangePreview, ModelExperimentMetricView, ModelLibraryItem,
    ModelPerformanceMetricView, ModelUsageMetricView, OptimizationAutodraftShadowEvaluation,
    PatternLibraryAnalytics, PatternLineageItem, PatternRecommendation,
    PatternRecommendationFeedback, PatternSearchDiagnostics, ProjectMember, PublishDecisionVersion,
    PublishedDecisionService, RollbackDecisionVersion, SaveDecisionRunOutcome,
    SaveDecisionTemplate, SaveKnowledgeSource, SaveProjectMember, UpdateDecisionDriftAutomation,
    UpdateDecisionDriftEventReview, UpdateDecisionDriftScanWindowReview,
    UpdateDeviationAnalysisReview, UpdateUser, VersionChangeReview, VersionVerificationReport,
};
#[cfg(target_family = "wasm")]
use crate::workflow::{
    AnalyzeRequirementRequest, ClarificationAnswer, ConfirmDecisionPlanRequest,
    ConfirmRequirementReviewRequest, ContinueClarificationRequest, CreateDecisionPlanRequest,
    DecisionWorkflowState, PendingRequirementReview,
};
#[cfg(target_family = "wasm")]
use js_sys::{Reflect, Uint8Array};
use serde::Deserialize;
#[cfg(target_family = "wasm")]
use serde::Serialize;
#[cfg(target_family = "wasm")]
use wasm_bindgen::{JsCast, JsValue};
#[cfg(target_family = "wasm")]
use wasm_bindgen_futures::JsFuture;
// 前端 API 客户端：封装所有后端接口调用。
#[cfg(target_family = "wasm")]
const AUTH_EXPIRED_EVENT: &str = "auth-expired";

#[cfg(target_family = "wasm")]
fn notify_auth_expired() {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Ok(event) = web_sys::Event::new(AUTH_EXPIRED_EVENT) {
        let _ = window.dispatch_event(&event);
    }
}

#[cfg(target_family = "wasm")]
fn handle_auth_status(status: reqwest::StatusCode) {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        notify_auth_expired();
    }
}

#[cfg(not(target_family = "wasm"))]
use sha2::Digest as _;
#[cfg(not(target_family = "wasm"))]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_family = "wasm"))]
use std::convert::Infallible;
#[cfg(not(target_family = "wasm"))]
use std::sync::Arc;

#[cfg(not(target_family = "wasm"))]
use crate::agent_runtime::orchestration::{
    Consensus, VoteCandidate, VoteOutcome, parallel_vote_until_majority_by,
};
#[cfg(not(target_family = "wasm"))]
use crate::agent_runtime::{
    ExperimentAssignment, ModelCandidate, ModelPerformanceMetric, ModelSelectionPolicy, TaskType,
    apply_performance_metrics, experiment_assignment, select_model,
};
#[cfg(not(target_family = "wasm"))]
use crate::dmn as executable_dmn;
#[cfg(not(target_family = "wasm"))]
use crate::execution_trace::build_execution_trace;
#[cfg(not(target_family = "wasm"))]
use crate::knowledge::{citations, effective_project_sources, knowledge_context};
#[cfg(not(target_family = "wasm"))]
use crate::nl_dsl;
#[cfg(not(target_family = "wasm"))]
use crate::nl_dsl::agent::{LlmAgent, ModelConcurrencyLimiter, StreamEvent};
#[cfg(not(target_family = "wasm"))]
use crate::nl_dsl::dmn;
#[cfg(not(target_family = "wasm"))]
use crate::requirement_complexity::{RequirementComplexity, classify_requirement_complexity};
#[cfg(not(target_family = "wasm"))]
use crate::types::{AppState, Claims};
#[cfg(not(target_family = "wasm"))]
use crate::version_review::build_change_review;
#[cfg(not(target_family = "wasm"))]
use crate::version_verification::{
    feel_value_to_json, json_inputs_to_feel as typed_json_inputs_to_feel,
};
#[cfg(not(target_family = "wasm"))]
use axum::body::Bytes;
#[cfg(not(target_family = "wasm"))]
use axum::extract::{Extension, State};
#[cfg(not(target_family = "wasm"))]
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
#[cfg(not(target_family = "wasm"))]
use axum::response::sse::{Event, KeepAlive, Sse};
#[cfg(not(target_family = "wasm"))]
use axum::response::{IntoResponse, Response};
#[cfg(not(target_family = "wasm"))]
use axum::{
    Json, Router,
    routing::{get, post},
};
#[cfg(not(target_family = "wasm"))]
use serde_json::{Map, Value, json};
#[cfg(not(target_family = "wasm"))]
use tokio::sync::mpsc;
#[cfg(not(target_family = "wasm"))]
use tokio_stream::{StreamExt as _, wrappers::ReceiverStream};

// 登录请求体 / 响应（前端专用）
#[cfg(target_family = "wasm")]
#[derive(Debug, Serialize, Deserialize)]
pub struct LoginPayload {
    pub username: String,
    pub password: String,
    #[serde(rename = "captchaId")]
    pub captcha_id: String,
    #[serde(rename = "captchaCode")]
    pub captcha_code: String,
    #[serde(rename = "captchaWorkNonce")]
    pub captcha_work_nonce: String,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Deserialize)]
pub struct CaptchaResponse {
    #[serde(rename = "captchaId")]
    pub captcha_id: String,
    pub image: String,
    #[serde(rename = "expiresIn")]
    pub expires_in: u64,
    #[serde(rename = "workChallenge")]
    pub work_challenge: String,
    #[serde(rename = "workDifficulty")]
    pub work_difficulty: u8,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Deserialize)]
struct DslRequest {
    dsl: String,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Deserialize)]
struct EvaluateRequest {
    xml: String,
    #[serde(default)]
    inputs: Value,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Deserialize)]
struct XmlRequest {
    xml: String,
}

#[cfg(not(target_family = "wasm"))]
struct DmnInputsAnalysis {
    template: serde_json::Map<String, Value>,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Deserialize, Clone)]
struct GenerateRequest {
    text: String,
    #[serde(default, rename = "workflowUpdatedAt")]
    workflow_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, rename = "projectId")]
    project_id: Option<i64>,
    /// 已有的 DSL YAML（增量修改模式），可选
    #[serde(default, rename = "existingDsl")]
    existing_dsl: Option<String>,
    #[serde(default, rename = "baseVersion")]
    base_version: Option<i32>,
    /// Immutable database revision used by the ChangeIntent preview flow.
    #[serde(default, rename = "baseRevision")]
    base_revision: Option<i64>,
    /// Compiler feedback from the preceding candidate preview. This is prompt
    /// context only; the server always derives the actual baseline itself.
    #[serde(default, rename = "repairError")]
    repair_error: Option<String>,
    #[serde(default, rename = "repairAttempt")]
    repair_attempt: Option<i32>,
    #[serde(default, rename = "previousIntent")]
    previous_intent: Option<crate::models::ModelChangeIntent>,
    #[serde(default, rename = "retryOfOrchestrationId")]
    retry_of_orchestration_id: Option<i64>,
    #[serde(default, rename = "simpleDirect")]
    simple_direct: bool,
}

#[cfg(not(target_family = "wasm"))]
fn generation_request_fingerprint(req: &GenerateRequest, base_revision: Option<i64>) -> String {
    let payload = json!({
        "text": req.text,
        "workflowUpdatedAt": req.workflow_updated_at,
        "projectId": req.project_id,
        "existingDsl": req.existing_dsl,
        "baseRevision": base_revision.or(req.base_revision).or_else(|| req.base_version.map(i64::from)),
        "repairError": req.repair_error,
        "repairAttempt": req.repair_attempt,
        "previousIntent": req.previous_intent,
        "simpleDirect": req.simple_direct,
    });
    let bytes = serde_json::to_vec(&payload).unwrap_or_default();
    format!("{:x}", sha2::Sha256::digest(bytes))
}

#[cfg(not(target_family = "wasm"))]
async fn apply_effective_knowledge(
    state: &AppState,
    claims: &Claims,
    req: &mut GenerateRequest,
) -> Result<Vec<crate::models::KnowledgeCitation>, (StatusCode, Json<Value>)> {
    let Some(project_id) = req.project_id else {
        return Ok(Vec::new());
    };
    let allowed = crate::project_access::has_permission(
        &state.db,
        project_id,
        &claims.sub,
        crate::project_access::ProjectPermission::Model,
    )
    .await
    .map_err(|error| {
        eprintln!("数据库操作失败 [校验模型生成权限]: {error}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": format!("校验模型生成权限失败：{error}") })),
        )
    })?;
    if !allowed {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "决策项目不存在" })),
        ));
    }
    let sources = effective_project_sources(&state.db, project_id, &claims.sub)
        .await
        .map_err(|error| {
            eprintln!("数据库操作失败 [读取项目有效知识]: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": format!("读取项目有效知识失败：{error}") })),
            )
        })?;
    let context = knowledge_context(&sources);
    if !context.is_empty() {
        req.text = format!(
            "{}\n\n## 当前有效的企业知识依据（服务端治理）\n{}\n## 知识使用约束\n以上内容仅作为术语、字段和制度依据；如与已确认需求冲突，应保持需求结构并显式反映冲突，不得绕过测试、审批或发布门禁。",
            req.text, context
        );
    }
    Ok(citations(&sources))
}

#[cfg(not(target_family = "wasm"))]
fn validate_generate_request(req: &GenerateRequest) -> Result<(), (StatusCode, Json<Value>)> {
    if req.retry_of_orchestration_id.is_some_and(|id| id <= 0) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "retryOfOrchestrationId 必须为正整数" })),
        ));
    }
    if req.text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "请求描述不能为空" })),
        ));
    }
    if req.text.len() > 20_000 {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "ok": false, "error": "请求描述不能超过 20000 字节" })),
        ));
    }
    if req
        .existing_dsl
        .as_deref()
        .is_some_and(|dsl| dsl.len() > 200_000)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "ok": false, "error": "已有 DSL 不能超过 200000 字节" })),
        ));
    }
    if req.workflow_updated_at.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"ok": false, "error": "缺少已确认方案的 workflowUpdatedAt，请重新加载工作流"}),
            ),
        ));
    }
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
fn validate_change_intent_request(req: &GenerateRequest) -> Result<(), (StatusCode, Json<Value>)> {
    validate_generate_request(req)?;
    if req
        .repair_attempt
        .is_some_and(|attempt| !(0..=crate::workflow::MAX_MODEL_CHANGE_REPAIRS).contains(&attempt))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "模型修复轮次必须在 0 到 2 之间"})),
        ));
    }
    if req.repair_error.is_some() != req.repair_attempt.is_some_and(|attempt| attempt > 0) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "模型修复错误信息和修复轮次必须同时提供"})),
        ));
    }
    if req
        .repair_error
        .as_deref()
        .is_some_and(|error| error.len() > 12_000)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "ok": false, "error": "模型修复错误信息不能超过 12000 字节" })),
        ));
    }
    if let Some(intent) = &req.previous_intent {
        if intent.changes.len() > 64
            || serde_json::to_vec(intent)
                .map(|value| value.len() > 30_000)
                .unwrap_or(true)
        {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({ "ok": false, "error": "上一轮模型变更意图过大" })),
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
pub struct LlmState {
    analysis_agent: Option<LlmAgent>,
    analysis_treatment_agent: Option<LlmAgent>,
    analysis_simple_agent: Option<LlmAgent>,
    analysis_complex_agent: Option<LlmAgent>,
    analysis_treatment_simple_agent: Option<LlmAgent>,
    analysis_treatment_complex_agent: Option<LlmAgent>,
    analysis_experiment_name: Option<String>,
    analysis_treatment_percent: u8,
    plan_agent: Option<LlmAgent>,
    plan_simple_agent: Option<LlmAgent>,
    plan_treatment_agent: Option<LlmAgent>,
    plan_experiment_name: Option<String>,
    plan_treatment_percent: u8,
    dsl_agent: Option<LlmAgent>,
    dsl_simple_agent: Option<LlmAgent>,
    dsl_treatment_agent: Option<LlmAgent>,
    dsl_experiment_name: Option<String>,
    dsl_treatment_percent: u8,
    change_intent_agent: Option<LlmAgent>,
    change_intent_treatment_agent: Option<LlmAgent>,
    change_intent_experiment_name: Option<String>,
    change_intent_treatment_percent: u8,
    optimization_proposal_agent: Option<LlmAgent>,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LlmTask {
    RequirementAnalysis,
    DecisionPlanning,
    DslGeneration,
    ChangeIntent,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentComplexity {
    Simple,
    Standard,
    Complex,
}

#[cfg(not(target_family = "wasm"))]
impl LlmTask {
    const fn config_prefix(self) -> &'static str {
        match self {
            Self::RequirementAnalysis => "ANALYSIS",
            Self::DecisionPlanning => "PLAN",
            Self::DslGeneration => "DSL",
            Self::ChangeIntent => "CHANGE_INTENT",
        }
    }

    pub(crate) const fn audit_stage(self) -> &'static str {
        match self {
            Self::RequirementAnalysis => "requirement_analysis",
            Self::DecisionPlanning => "decision_planning",
            Self::DslGeneration => "dsl_generation",
            Self::ChangeIntent => "change_intent",
        }
    }
}

#[cfg(not(target_family = "wasm"))]
#[derive(Debug, PartialEq)]
struct LlmTaskSettings {
    model: String,
    fallback_models: Vec<String>,
    estimated_cost_per_response_usd: Option<f64>,
    max_completion_tokens: Option<u32>,
    request_options: Map<String, Value>,
}

#[cfg(not(target_family = "wasm"))]
impl LlmState {
    pub async fn from_env(db: &sqlx::PgPool) -> Self {
        dotenvy::dotenv().ok();
        let api_key = std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty());
        let api_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let default_model =
            std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
        let model_concurrency_limit = std::env::var("OPENAI_MODEL_MAX_CONCURRENCY")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .map(|value| value.min(256))
            .unwrap_or(4);
        let model_limiter = Arc::new(ModelConcurrencyLimiter::new(model_concurrency_limit));
        println!("==== OPENAI_MODEL: {default_model}");
        let default_options = llm_request_options_from_env();
        let fallback_prompt_catalog = nl_dsl::prompts::PromptCatalog::builtins();
        let prompt_catalog = match crate::prompt_registry::schema_ready(db).await {
            Ok(true) => match crate::prompt_registry::ensure_builtin_assets(db).await {
                Ok(catalog) => crate::prompt_registry::load_active_catalog(db, catalog).await,
                Err(error) => {
                    eprintln!("初始化提示词注册表失败，继续使用代码内置提示词: {error}");
                    fallback_prompt_catalog
                }
            },
            Ok(false) => {
                eprintln!(
                    "提示词注册表未安装，继续使用代码内置提示词；运行 script/upgrade-prompt-registry.sh 并重启服务以启用注册表"
                );
                fallback_prompt_catalog
            }
            Err(error) => {
                eprintln!("检查提示词注册表失败，继续使用代码内置提示词: {error}");
                fallback_prompt_catalog
            }
        };
        let performance_metrics = load_model_performance_metrics(db).await;
        let experiment_config = match analysis_experiment_config_from_env() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("需求分析 A/B 实验配置无效，已禁用：{error}");
                None
            }
        };
        let plan_experiment_config = match plan_experiment_config_from_env() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("决策规划 A/B 实验配置无效，已禁用：{error}");
                None
            }
        };
        let dsl_experiment_config = experiment_config_from_env("OPENAI_DSL_EXPERIMENT");
        let change_intent_experiment_config =
            experiment_config_from_env("OPENAI_CHANGE_INTENT_EXPERIMENT");
        let agent_for = |task, model_override: Option<&str>| {
            let mut settings = match llm_task_settings_from_env(
                task,
                &default_model,
                &default_options,
                &performance_metrics,
                model_override,
            ) {
                Ok(settings) => settings,
                Err(error) => {
                    eprintln!("{task:?} 模型配置无效：{error}");
                    return None;
                }
            };
            if task == LlmTask::ChangeIntent {
                disable_thinking(&mut settings.request_options);
            }
            api_key.as_ref().map(|key| {
                let agent = LlmAgent::new(
                    key.clone(),
                    api_url.clone(),
                    settings.model,
                    settings.request_options,
                )
                .with_shared_model_limiter(Arc::clone(&model_limiter))
                .with_prompt_catalog(prompt_catalog.clone())
                .with_fallback_models(settings.fallback_models)
                .with_estimated_cost_per_response_usd(settings.estimated_cost_per_response_usd)
                .with_max_completion_tokens(settings.max_completion_tokens);
                if task == LlmTask::DslGeneration {
                    agent.with_repair_memory_db(db.clone())
                } else {
                    agent
                }
            })
        };
        let analysis_agent = agent_for(LlmTask::RequirementAnalysis, None);
        let configured_treatment_agent = experiment_config
            .as_ref()
            .and_then(|(_, model, _)| agent_for(LlmTask::RequirementAnalysis, Some(model)));
        let analysis_treatment_agent = match configured_treatment_agent {
            Some(treatment)
                if analysis_agent
                    .as_ref()
                    .is_some_and(|control| control.model() != treatment.model()) =>
            {
                Some(treatment)
            }
            Some(_) => {
                eprintln!("需求分析 A/B 实验 control 与 treatment 模型相同，已禁用");
                None
            }
            None => None,
        };
        let analysis_simple_model = configured_model_from_env("OPENAI_ANALYSIS_SIMPLE_MODEL");
        let analysis_complex_model = configured_model_from_env("OPENAI_ANALYSIS_COMPLEX_MODEL");
        let analysis_simple_agent = analysis_simple_model
            .as_deref()
            .and_then(|model| agent_for(LlmTask::RequirementAnalysis, Some(model)));
        let analysis_complex_agent = analysis_complex_model
            .as_deref()
            .and_then(|model| agent_for(LlmTask::RequirementAnalysis, Some(model)));
        let analysis_treatment_simple_model = analysis_simple_model.as_ref().and_then(|_| {
            experiment_config.as_ref().map(|(_, experiment_model, _)| {
                configured_model_from_env("OPENAI_ANALYSIS_EXPERIMENT_SIMPLE_MODEL")
                    .unwrap_or_else(|| experiment_model.clone())
            })
        });
        let analysis_treatment_complex_model = analysis_complex_model.as_ref().and_then(|_| {
            experiment_config.as_ref().map(|(_, experiment_model, _)| {
                configured_model_from_env("OPENAI_ANALYSIS_EXPERIMENT_COMPLEX_MODEL")
                    .unwrap_or_else(|| experiment_model.clone())
            })
        });
        let analysis_treatment_simple_agent = analysis_treatment_simple_model
            .as_deref()
            .and_then(|model| agent_for(LlmTask::RequirementAnalysis, Some(model)));
        let analysis_treatment_complex_agent = analysis_treatment_complex_model
            .as_deref()
            .and_then(|model| agent_for(LlmTask::RequirementAnalysis, Some(model)));
        let plan_agent = agent_for(LlmTask::DecisionPlanning, None);
        let plan_simple_agent = configured_model_from_env("OPENAI_PLAN_SIMPLE_MODEL")
            .as_deref()
            .and_then(|model| agent_for(LlmTask::DecisionPlanning, Some(model)));
        let configured_plan_treatment_agent = plan_experiment_config
            .as_ref()
            .and_then(|(_, model, _)| agent_for(LlmTask::DecisionPlanning, Some(model)));
        let plan_treatment_agent = match configured_plan_treatment_agent {
            Some(treatment)
                if plan_agent
                    .as_ref()
                    .is_some_and(|control| control.model() != treatment.model()) =>
            {
                Some(treatment)
            }
            Some(_) => {
                eprintln!("决策规划 A/B 实验 control 与 treatment 模型相同，已禁用");
                None
            }
            None => None,
        };
        let dsl_agent = agent_for(LlmTask::DslGeneration, None);
        let dsl_simple_agent = configured_model_from_env("OPENAI_DSL_SIMPLE_MODEL")
            .as_deref()
            .and_then(|model| agent_for(LlmTask::DslGeneration, Some(model)));
        let configured_dsl_treatment_agent = dsl_experiment_config
            .as_ref()
            .and_then(|(_, model, _)| agent_for(LlmTask::DslGeneration, Some(model)));
        let dsl_treatment_agent = match configured_dsl_treatment_agent {
            Some(treatment)
                if dsl_agent
                    .as_ref()
                    .is_some_and(|control| control.model() != treatment.model()) =>
            {
                Some(treatment)
            }
            Some(_) => {
                eprintln!("DSL 生成 A/B 实验 control 与 treatment 模型相同，已禁用");
                None
            }
            None => None,
        };
        let change_intent_agent = agent_for(LlmTask::ChangeIntent, None);
        let configured_change_intent_treatment_agent = change_intent_experiment_config
            .as_ref()
            .and_then(|(_, model, _)| agent_for(LlmTask::ChangeIntent, Some(model)));
        let change_intent_treatment_agent = match configured_change_intent_treatment_agent {
            Some(treatment)
                if change_intent_agent
                    .as_ref()
                    .is_some_and(|control| control.model() != treatment.model()) =>
            {
                Some(treatment)
            }
            Some(_) => {
                eprintln!("变更意图 A/B 实验 control 与 treatment 模型相同，已禁用");
                None
            }
            None => None,
        };
        let optimization_proposal_model = configured_model_from_env("OPENAI_OPTIMIZATION_MODEL");
        let optimization_proposal_agent = agent_for(
            LlmTask::DecisionPlanning,
            optimization_proposal_model.as_deref(),
        );
        Self {
            analysis_agent,
            analysis_treatment_agent,
            analysis_simple_agent,
            analysis_complex_agent,
            analysis_treatment_simple_agent,
            analysis_treatment_complex_agent,
            analysis_experiment_name: experiment_config.as_ref().map(|(name, _, _)| name.clone()),
            analysis_treatment_percent: experiment_config
                .as_ref()
                .map(|(_, _, percent)| *percent)
                .unwrap_or(0),
            plan_agent,
            plan_simple_agent,
            plan_treatment_agent,
            plan_experiment_name: plan_experiment_config
                .as_ref()
                .map(|(name, _, _)| name.clone()),
            plan_treatment_percent: plan_experiment_config
                .as_ref()
                .map(|(_, _, percent)| *percent)
                .unwrap_or(0),
            dsl_agent,
            dsl_simple_agent,
            dsl_treatment_agent,
            dsl_experiment_name: dsl_experiment_config
                .as_ref()
                .map(|(name, _, _)| name.clone()),
            dsl_treatment_percent: dsl_experiment_config
                .as_ref()
                .map(|(_, _, percent)| *percent)
                .unwrap_or(0),
            change_intent_agent,
            change_intent_treatment_agent,
            change_intent_experiment_name: change_intent_experiment_config
                .as_ref()
                .map(|(name, _, _)| name.clone()),
            change_intent_treatment_percent: change_intent_experiment_config
                .as_ref()
                .map(|(_, _, percent)| *percent)
                .unwrap_or(0),
            optimization_proposal_agent,
        }
    }

    pub(crate) fn analysis_agent_for_project(
        &self,
        project_id: i64,
        allow_experiment: bool,
        complexity: AgentComplexity,
    ) -> Option<(&LlmAgent, Option<ExperimentAssignment<'_>>)> {
        let control = self.analysis_agent.as_ref()?;
        let assignment = if allow_experiment && self.analysis_treatment_agent.is_some() {
            self.analysis_experiment_name.as_deref().map(|name| {
                experiment_assignment(name, project_id, self.analysis_treatment_percent)
            })
        } else {
            None
        };
        let treatment_selected = assignment
            .as_ref()
            .is_some_and(|assignment| assignment.variant == "treatment")
            && self.analysis_treatment_agent.is_some();
        let agent = match (complexity, treatment_selected) {
            (AgentComplexity::Simple, true) => self
                .analysis_treatment_simple_agent
                .as_ref()
                .or(self.analysis_treatment_agent.as_ref())?,
            (AgentComplexity::Complex, true) => self
                .analysis_treatment_complex_agent
                .as_ref()
                .or(self.analysis_treatment_agent.as_ref())?,
            (AgentComplexity::Simple, false) => {
                self.analysis_simple_agent.as_ref().unwrap_or(control)
            }
            (AgentComplexity::Complex, false) => {
                self.analysis_complex_agent.as_ref().unwrap_or(control)
            }
            (AgentComplexity::Standard, true) => self.analysis_treatment_agent.as_ref()?,
            (AgentComplexity::Standard, false) => control,
        };
        if let Some(assignment) = assignment {
            Some((agent, Some(assignment)))
        } else {
            Some((agent, None))
        }
    }

    pub(crate) fn prompt_evaluation_agent(&self, prompt_key: &str) -> Option<&LlmAgent> {
        match prompt_key {
            "requirement_analysis" => self.analysis_agent.as_ref(),
            "decision_planning" => self.plan_agent.as_ref(),
            "dsl_generation" => self.dsl_agent.as_ref(),
            "change_intent" => self.change_intent_agent.as_ref(),
            "optimization_proposal" => self.optimization_proposal_agent.as_ref(),
            "decision_explanation" => self.plan_agent.as_ref(),
            "tester_judge" => self.plan_agent.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn plan_agent_for_project(
        &self,
        project_id: i64,
        allow_experiment: bool,
        complexity: AgentComplexity,
    ) -> Option<(&LlmAgent, Option<ExperimentAssignment<'_>>)> {
        let control = self.plan_agent.as_ref()?;
        let assignment = if allow_experiment && self.plan_treatment_agent.is_some() {
            self.plan_experiment_name
                .as_deref()
                .map(|name| experiment_assignment(name, project_id, self.plan_treatment_percent))
        } else {
            None
        };
        let treatment_selected = assignment
            .as_ref()
            .is_some_and(|assignment| assignment.variant == "treatment")
            && self.plan_treatment_agent.is_some();
        let agent = match (complexity, treatment_selected) {
            (_, true) => self.plan_treatment_agent.as_ref()?,
            (AgentComplexity::Simple, false) => self.plan_simple_agent.as_ref().unwrap_or(control),
            (AgentComplexity::Standard | AgentComplexity::Complex, false) => control,
        };
        Some((agent, assignment))
    }

    pub(crate) fn dsl_agent_for_project(
        &self,
        project_id: i64,
        allow_experiment: bool,
        complexity: AgentComplexity,
    ) -> Option<(&LlmAgent, Option<ExperimentAssignment<'_>>)> {
        let control = self.dsl_agent.as_ref()?;
        let assignment = if allow_experiment && self.dsl_treatment_agent.is_some() {
            self.dsl_experiment_name
                .as_deref()
                .map(|name| experiment_assignment(name, project_id, self.dsl_treatment_percent))
        } else {
            None
        };
        let treatment_selected = assignment
            .as_ref()
            .is_some_and(|assignment| assignment.variant == "treatment")
            && self.dsl_treatment_agent.is_some();
        let agent = match (complexity, treatment_selected) {
            (_, true) => self.dsl_treatment_agent.as_ref()?,
            (AgentComplexity::Simple, false) => self.dsl_simple_agent.as_ref().unwrap_or(control),
            (AgentComplexity::Standard | AgentComplexity::Complex, false) => control,
        };
        Some((agent, assignment))
    }

    pub(crate) fn change_intent_agent_for_project(
        &self,
        project_id: i64,
        allow_experiment: bool,
    ) -> Option<(&LlmAgent, Option<ExperimentAssignment<'_>>)> {
        experiment_agent_for_project(
            self.change_intent_agent.as_ref(),
            self.change_intent_treatment_agent.as_ref(),
            self.change_intent_experiment_name.as_deref(),
            self.change_intent_treatment_percent,
            project_id,
            allow_experiment,
        )
    }

    pub(crate) fn optimization_proposal_agent(&self) -> Option<LlmAgent> {
        self.optimization_proposal_agent
            .as_ref()
            .map(LlmAgent::fork_for_run)
    }

    pub(crate) fn decision_explanation_agent(&self) -> Option<LlmAgent> {
        self.plan_agent.as_ref().map(LlmAgent::fork_for_run)
    }
}

#[cfg(not(target_family = "wasm"))]
fn experiment_agent_for_project<'a>(
    control: Option<&'a LlmAgent>,
    treatment: Option<&'a LlmAgent>,
    name: Option<&'a str>,
    treatment_percent: u8,
    project_id: i64,
    allow_experiment: bool,
) -> Option<(&'a LlmAgent, Option<ExperimentAssignment<'a>>)> {
    let control = control?;
    if !allow_experiment {
        return Some((control, None));
    }
    let (Some(name), Some(treatment)) = (name, treatment) else {
        return Some((control, None));
    };
    let assignment = experiment_assignment(name, project_id, treatment_percent);
    if assignment.variant == "treatment" {
        Some((treatment, Some(assignment)))
    } else {
        Some((control, Some(assignment)))
    }
}

#[cfg(not(target_family = "wasm"))]
fn analysis_experiment_config_from_env() -> Result<Option<(String, String, u8)>, String> {
    let name = std::env::var("OPENAI_ANALYSIS_EXPERIMENT_NAME").ok();
    let model = std::env::var("OPENAI_ANALYSIS_EXPERIMENT_MODEL").ok();
    let percent = std::env::var("OPENAI_ANALYSIS_EXPERIMENT_TREATMENT_PERCENT").ok();
    analysis_experiment_config_from_values(name.as_deref(), model.as_deref(), percent.as_deref())
}

#[cfg(not(target_family = "wasm"))]
fn analysis_experiment_config_from_values(
    name: Option<&str>,
    model: Option<&str>,
    percent: Option<&str>,
) -> Result<Option<(String, String, u8)>, String> {
    experiment_config_from_values("OPENAI_ANALYSIS_EXPERIMENT", name, model, percent)
}

#[cfg(not(target_family = "wasm"))]
fn plan_experiment_config_from_env() -> Result<Option<(String, String, u8)>, String> {
    let name = std::env::var("OPENAI_PLAN_EXPERIMENT_NAME").ok();
    let model = std::env::var("OPENAI_PLAN_EXPERIMENT_MODEL").ok();
    let percent = std::env::var("OPENAI_PLAN_EXPERIMENT_TREATMENT_PERCENT").ok();
    plan_experiment_config_from_values(name.as_deref(), model.as_deref(), percent.as_deref())
}

#[cfg(not(target_family = "wasm"))]
fn experiment_config_from_env(prefix: &str) -> Option<(String, String, u8)> {
    let config = experiment_config_from_values(
        prefix,
        std::env::var(format!("{prefix}_NAME")).ok().as_deref(),
        std::env::var(format!("{prefix}_MODEL")).ok().as_deref(),
        std::env::var(format!("{prefix}_TREATMENT_PERCENT"))
            .ok()
            .as_deref(),
    );
    match config {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{prefix} A/B 实验配置无效，已禁用：{error}");
            None
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn plan_experiment_config_from_values(
    name: Option<&str>,
    model: Option<&str>,
    percent: Option<&str>,
) -> Result<Option<(String, String, u8)>, String> {
    experiment_config_from_values("OPENAI_PLAN_EXPERIMENT", name, model, percent)
}

#[cfg(not(target_family = "wasm"))]
fn experiment_config_from_values(
    prefix: &str,
    name: Option<&str>,
    model: Option<&str>,
    percent: Option<&str>,
) -> Result<Option<(String, String, u8)>, String> {
    if name.is_none() && model.is_none() && percent.is_none() {
        return Ok(None);
    }
    let name = name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("实验启用时必须配置 {prefix}_NAME"))?;
    let model = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("实验启用时必须配置 {prefix}_MODEL"))?;
    if name.len() > 128 || model.len() > 255 {
        return Err("实验名称最多 128 字节，模型名称最多 255 字节".into());
    }
    let percent = percent
        .unwrap_or("50")
        .parse::<u8>()
        .map_err(|_| format!("{prefix}_TREATMENT_PERCENT 必须为 0-100 的整数"))?;
    if percent > 100 {
        return Err(format!("{prefix}_TREATMENT_PERCENT 必须为 0-100 的整数"));
    }
    Ok(Some((name, model, percent)))
}

#[cfg(not(target_family = "wasm"))]
#[cfg_attr(not(test), allow(dead_code))] // 当前仅在单元测试中直接断言，产品路径经 disable_thinking 生效
fn is_deepseek_reasoning_model(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    // DeepSeek-compatible gateways expose thinking under several model
    // aliases (for example `deepseek-chat`, `deepseek-v3.2` and provider-
    // prefixed names). PATCH needs a tool call, so all DeepSeek variants use
    // the non-thinking default unless the operator explicitly opts in.
    model.contains("deepseek") || model.ends_with("-reasoner")
}

#[cfg(not(target_family = "wasm"))]
#[cfg_attr(not(test), allow(dead_code))] // 仅在单元测试中直接断言，产品路径经 disable_thinking 生效
fn has_thinking_option(options: &Map<String, Value>) -> bool {
    options.contains_key("enable_thinking") || options.contains_key("thinking")
}

#[cfg(not(target_family = "wasm"))]
fn disable_thinking(options: &mut Map<String, Value>) {
    // Remove provider-specific thinking configuration.  Sending both
    // `enable_thinking` and a `thinking` object is rejected by several
    // OpenAI-compatible gateways, so PATCH requests use one portable flag.
    options.remove("thinking");
    options.remove("enable_thinking");
    options.insert("enable_thinking".into(), Value::Bool(false));
}

#[cfg(not(target_family = "wasm"))]
fn llm_request_options_from_env() -> Map<String, Value> {
    request_options_from_raw(
        "OPENAI_REQUEST_OPTIONS_JSON",
        std::env::var("OPENAI_REQUEST_OPTIONS_JSON").ok().as_deref(),
    )
    .unwrap_or_default()
}

#[cfg(not(target_family = "wasm"))]
async fn load_model_performance_metrics(db: &sqlx::PgPool) -> Vec<ModelPerformanceMetric> {
    let rows = sqlx::query_as::<_, (String, String, f64, Option<f64>)>(
        r#"
        SELECT model_name, task_type, success_rate::FLOAT8,
               estimated_cost_usd::FLOAT8
        FROM model_performance_metrics
        WHERE sample_count > 0
        "#,
    )
    .fetch_all(db)
    .await;

    match rows {
        Ok(rows) => rows
            .into_iter()
            .map(
                |(model_name, task_type, success_rate_percent, estimated_cost_usd)| {
                    ModelPerformanceMetric {
                        model_name,
                        task_type,
                        success_rate: success_rate_percent / 100.0,
                        estimated_cost_usd,
                    }
                },
            )
            .collect(),
        Err(error) => {
            eprintln!("读取模型性能指标失败，将仅使用静态模型配置：{error}");
            Vec::new()
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn request_options_from_raw(variable: &str, raw: Option<&str>) -> Option<Map<String, Value>> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(options)) => Some(options),
        Ok(_) => {
            eprintln!("{variable} 必须是 JSON 对象，已忽略该配置");
            None
        }
        Err(error) => {
            eprintln!("{variable} 不是有效 JSON，已忽略：{error}");
            None
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn max_completion_tokens_from_env() -> Result<Option<u32>, String> {
    max_completion_tokens_from_raw(
        std::env::var("OPENAI_MAX_COMPLETION_TOKENS")
            .ok()
            .as_deref(),
    )
}

#[cfg(not(target_family = "wasm"))]
fn max_completion_tokens_from_raw(raw: Option<&str>) -> Result<Option<u32>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let tokens = raw
        .trim()
        .parse::<u32>()
        .map_err(|_| "OPENAI_MAX_COMPLETION_TOKENS 必须是 1 到 1000000 的整数".to_string())?;
    if !(1..=1_000_000).contains(&tokens) {
        return Err("OPENAI_MAX_COMPLETION_TOKENS 必须是 1 到 1000000 的整数".into());
    }
    Ok(Some(tokens))
}

#[cfg(not(target_family = "wasm"))]
fn llm_task_settings_from_env(
    task: LlmTask,
    default_model: &str,
    default_options: &Map<String, Value>,
    performance_metrics: &[ModelPerformanceMetric],
    model_override: Option<&str>,
) -> Result<LlmTaskSettings, String> {
    let prefix = task.config_prefix();
    let model_variable = format!("OPENAI_{prefix}_MODEL");
    let options_variable = format!("OPENAI_{prefix}_REQUEST_OPTIONS_JSON");
    let stage_model = model_override
        .map(str::to_string)
        .or_else(|| std::env::var(&model_variable).ok());
    let stage_options = std::env::var(&options_variable).ok();
    let mut candidates = model_candidates_from_raw(
        default_model,
        std::env::var("OPENAI_MODEL_CANDIDATES_JSON")
            .ok()
            .as_deref(),
    );
    let task_type = TaskType::from_config_prefix(prefix);
    apply_performance_metrics(&mut candidates, task_type, performance_metrics);
    let policy = model_selection_policy_from_env(prefix);
    let max_estimated_cost = estimated_call_cost_limit_from_env()?;
    let fallback_variable = format!("OPENAI_{prefix}_FALLBACK_MODELS");
    let fallback_models = fallback_models_from_raw(
        std::env::var(&fallback_variable)
            .ok()
            .or_else(|| std::env::var("OPENAI_FALLBACK_MODELS").ok())
            .as_deref(),
    );
    llm_task_settings_with_cost_limit(
        &options_variable,
        default_model,
        default_options,
        stage_model.as_deref(),
        stage_options.as_deref(),
        &candidates,
        policy,
        task_type,
        fallback_models,
        max_estimated_cost,
    )
}

#[cfg(not(target_family = "wasm"))]
fn configured_model_from_env(variable: &str) -> Option<String> {
    std::env::var(variable)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(all(not(target_family = "wasm"), test))]
fn llm_task_settings(
    options_variable: &str,
    default_model: &str,
    default_options: &Map<String, Value>,
    stage_model: Option<&str>,
    stage_options: Option<&str>,
) -> LlmTaskSettings {
    llm_task_settings_with_candidates(
        options_variable,
        default_model,
        default_options,
        stage_model,
        stage_options,
        &[],
        ModelSelectionPolicy::BalancedMode,
        TaskType::Analyze,
        Vec::new(),
    )
}

#[cfg(all(not(target_family = "wasm"), test))]
#[allow(clippy::too_many_arguments)]
fn llm_task_settings_with_candidates(
    options_variable: &str,
    default_model: &str,
    default_options: &Map<String, Value>,
    stage_model: Option<&str>,
    stage_options: Option<&str>,
    candidates: &[ModelCandidate],
    policy: ModelSelectionPolicy,
    task: TaskType,
    fallback_models: Vec<String>,
) -> LlmTaskSettings {
    llm_task_settings_with_cost_limit(
        options_variable,
        default_model,
        default_options,
        stage_model,
        stage_options,
        candidates,
        policy,
        task,
        fallback_models,
        None,
    )
    .expect("no cost ceiling is configured")
}

#[cfg(not(target_family = "wasm"))]
#[allow(clippy::too_many_arguments)]
fn llm_task_settings_with_cost_limit(
    options_variable: &str,
    default_model: &str,
    default_options: &Map<String, Value>,
    stage_model: Option<&str>,
    stage_options: Option<&str>,
    candidates: &[ModelCandidate],
    policy: ModelSelectionPolicy,
    task: TaskType,
    fallback_models: Vec<String>,
    max_estimated_cost: Option<f64>,
) -> Result<LlmTaskSettings, String> {
    let eligible_candidates: Vec<ModelCandidate> = candidates
        .iter()
        .filter(|candidate| {
            max_estimated_cost.is_none_or(|limit| {
                candidate
                    .estimated_cost_usd
                    .is_some_and(|cost| cost.is_finite() && cost >= 0.0 && cost <= limit)
            })
        })
        .cloned()
        .collect();
    let requested_model = stage_model.map(str::trim).filter(|value| !value.is_empty());
    if max_estimated_cost.is_some() {
        if let Some(model) = requested_model {
            if !eligible_candidates
                .iter()
                .any(|candidate| candidate.name == model)
            {
                return Err(format!(
                    "模型 {model} 缺少符合上限的成本估算，或超过单次估算上限"
                ));
            }
        }
    }
    let selected_model = select_model(&eligible_candidates, policy, task)
        .map(|candidate| candidate.name.as_str())
        .or_else(|| max_estimated_cost.is_none().then_some(default_model))
        .ok_or_else(|| "没有具备有效成本估算且符合上限的候选模型".to_string())?;
    let model = requested_model.unwrap_or(selected_model).to_string();
    let fallback_models = if max_estimated_cost.is_some() {
        fallback_models
            .into_iter()
            .filter(|model| {
                eligible_candidates
                    .iter()
                    .any(|candidate| candidate.name == *model)
            })
            .collect()
    } else {
        fallback_models
    };
    let estimated_cost_per_response_usd = std::iter::once(model.as_str())
        .chain(fallback_models.iter().map(String::as_str))
        .map(|name| {
            eligible_candidates
                .iter()
                .find(|candidate| candidate.name == name)
                .and_then(|candidate| candidate.estimated_cost_usd)
        })
        .collect::<Option<Vec<_>>>()
        .and_then(|prices| prices.into_iter().reduce(f64::max));
    let request_options = request_options_from_raw(options_variable, stage_options)
        .unwrap_or_else(|| default_options.clone());
    Ok(LlmTaskSettings {
        model,
        fallback_models,
        estimated_cost_per_response_usd,
        max_completion_tokens: max_completion_tokens_from_env()?,
        request_options,
    })
}

#[cfg(not(target_family = "wasm"))]
fn estimated_call_cost_limit_from_env() -> Result<Option<f64>, String> {
    estimated_call_cost_limit_from_raw(
        std::env::var("OPENAI_MAX_ESTIMATED_CALL_COST_USD")
            .ok()
            .as_deref(),
    )
}

#[cfg(not(target_family = "wasm"))]
fn estimated_call_cost_limit_from_raw(raw: Option<&str>) -> Result<Option<f64>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let limit = raw
        .trim()
        .parse::<f64>()
        .map_err(|_| "OPENAI_MAX_ESTIMATED_CALL_COST_USD 必须是非负有限数字".to_string())?;
    if !limit.is_finite() || limit < 0.0 {
        return Err("OPENAI_MAX_ESTIMATED_CALL_COST_USD 必须是非负有限数字".into());
    }
    Ok(Some(limit))
}

#[cfg(not(target_family = "wasm"))]
fn orchestration_estimated_cost_budget_from_env() -> Result<Option<f64>, String> {
    orchestration_estimated_cost_budget_from_raw(
        std::env::var("OPENAI_MAX_ORCHESTRATION_ESTIMATED_COST_USD")
            .ok()
            .as_deref(),
    )
}

#[cfg(not(target_family = "wasm"))]
fn orchestration_estimated_cost_budget_from_raw(raw: Option<&str>) -> Result<Option<f64>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let limit = raw.trim().parse::<f64>().map_err(|_| {
        "OPENAI_MAX_ORCHESTRATION_ESTIMATED_COST_USD 必须是非负有限数字".to_string()
    })?;
    if !limit.is_finite() || limit < 0.0 {
        return Err("OPENAI_MAX_ORCHESTRATION_ESTIMATED_COST_USD 必须是非负有限数字".into());
    }
    Ok(Some(limit))
}

#[cfg(not(target_family = "wasm"))]
fn orchestration_observed_token_budget_from_raw(raw: Option<&str>) -> Result<Option<i64>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let limit = raw
        .trim()
        .parse::<i64>()
        .map_err(|_| "OPENAI_MAX_ORCHESTRATION_OBSERVED_TOKENS 必须是正整数".to_string())?;
    if limit <= 0 {
        return Err("OPENAI_MAX_ORCHESTRATION_OBSERVED_TOKENS 必须是正整数".into());
    }
    Ok(Some(limit))
}

#[cfg(not(target_family = "wasm"))]
fn orchestration_observed_token_budget_from_env() -> Result<Option<i64>, String> {
    orchestration_observed_token_budget_from_raw(
        std::env::var("OPENAI_MAX_ORCHESTRATION_OBSERVED_TOKENS")
            .ok()
            .as_deref(),
    )
}

#[cfg(not(target_family = "wasm"))]
pub(crate) enum ProjectTokenBudgetError {
    InvalidConfiguration(String),
    Database(sqlx::Error),
    RetryAlreadyActive,
    Exceeded { used: i64, limit: i64 },
    OrchestrationTokensExceeded { used: i64, limit: i64 },
    EstimatedCostExceeded { used: f64, next: f64, limit: f64 },
}

#[cfg(not(target_family = "wasm"))]
pub(crate) async fn check_orchestration_budgets(
    connection: &mut sqlx::PgConnection,
    project_id: i64,
    parent_orchestration_id: Option<i64>,
    retry_of_orchestration_id: Option<i64>,
    next_cost: Option<f64>,
) -> Result<(), ProjectTokenBudgetError> {
    let observed_token_limit = orchestration_observed_token_budget_from_env()
        .map_err(ProjectTokenBudgetError::InvalidConfiguration)?;
    let cost_limit = orchestration_estimated_cost_budget_from_env()
        .map_err(ProjectTokenBudgetError::InvalidConfiguration)?;
    if cost_limit.is_none() && observed_token_limit.is_none() {
        return Ok(());
    }
    let next = if cost_limit.is_some() {
        Some(
            next_cost
                .filter(|cost| cost.is_finite() && *cost >= 0.0)
                .ok_or_else(|| {
                    ProjectTokenBudgetError::InvalidConfiguration(
                        "编排成本预算已启用，但模型缺少有效成本估算".into(),
                    )
                })?,
        )
    } else {
        None
    };
    let budget_parent = parent_orchestration_id.or(retry_of_orchestration_id);
    let root_id = if let Some(parent_id) = budget_parent {
        let root_id = sqlx::query_scalar::<_, i64>(
            r#"
            WITH RECURSIVE ancestors AS (
                SELECT id, parent_orchestration_id, retry_of_orchestration_id
                FROM agent_orchestrations
                WHERE id = $1 AND project_id = $2
                UNION
                SELECT parent.id, parent.parent_orchestration_id, parent.retry_of_orchestration_id
                FROM agent_orchestrations parent
                JOIN ancestors child
                  ON child.parent_orchestration_id = parent.id
                  OR child.retry_of_orchestration_id = parent.id
                WHERE parent.project_id = $2
            )
            SELECT id FROM ancestors
            WHERE parent_orchestration_id IS NULL AND retry_of_orchestration_id IS NULL
            LIMIT 1
            "#,
        )
        .bind(parent_id)
        .bind(project_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(ProjectTokenBudgetError::Database)?
        .ok_or_else(|| ProjectTokenBudgetError::Database(sqlx::Error::RowNotFound))?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('fuxi_orchestration_budget:' || $1::TEXT, 0))",
        )
        .bind(root_id)
        .execute(&mut *connection)
        .await
        .map_err(ProjectTokenBudgetError::Database)?;
        Some(root_id)
    } else {
        None
    };
    if let (Some(root_id), Some(token_limit)) = (root_id, observed_token_limit) {
        let (used_tokens, invalid_runs) = sqlx::query_as::<_, (i64, i64)>(
            r#"
            WITH RECURSIVE tree(id) AS (
                SELECT id FROM agent_orchestrations WHERE id = $1
                UNION
                SELECT child.id
                FROM agent_orchestrations child
                JOIN tree parent
                  ON child.parent_orchestration_id = parent.id
                  OR child.retry_of_orchestration_id = parent.id
            )
            SELECT COALESCE(SUM(COALESCE(NULLIF(run.usage_json->>'total_tokens', '')::BIGINT, 0)), 0)::BIGINT,
                   COUNT(*) FILTER (
                       WHERE run.usage_json IS NULL
                          OR (run.status <> 'running'
                              AND NULLIF(run.usage_json->>'total_tokens', '') IS NULL)
                          OR COALESCE(NULLIF(run.usage_json->>'total_tokens', '')::BIGINT, 0) < 0
                   )::BIGINT
            FROM tree
            JOIN agent_runs run ON run.orchestration_id = tree.id
            "#,
        )
        .bind(root_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(ProjectTokenBudgetError::Database)?;
        if invalid_runs > 0 {
            return Err(ProjectTokenBudgetError::InvalidConfiguration(format!(
                "编排树内有 {invalid_runs} 条运行缺少有效 Token 用量，无法可靠检查累计用量"
            )));
        }
        if used_tokens >= token_limit {
            return Err(ProjectTokenBudgetError::OrchestrationTokensExceeded {
                used: used_tokens,
                limit: token_limit,
            });
        }
    }
    let Some(limit) = cost_limit else {
        return Ok(());
    };
    let next = next.expect("cost reservation was validated when the cost limit is enabled");
    let used = if let Some(root_id) = root_id {
        let (used, unpriced_runs, negative_cost_runs) = sqlx::query_as::<_, (f64, i64, i64)>(
            r#"
            WITH RECURSIVE tree(id) AS (
                SELECT id FROM agent_orchestrations WHERE id = $1
                UNION
                SELECT child.id FROM agent_orchestrations child
                JOIN tree parent
                  ON child.parent_orchestration_id = parent.id
                  OR child.retry_of_orchestration_id = parent.id
            )
            SELECT COALESCE(SUM(NULLIF(run.usage_json->>'estimated_cost_usd', '')::FLOAT8), 0)::FLOAT8,
                   COUNT(*) FILTER (
                       WHERE run.usage_json IS NULL
                          OR (
                              run.status <> 'running'
                              OR run.usage_json->>'budget_reservation' = 'true'
                          )
                          AND NULLIF(run.usage_json->>'estimated_cost_usd', '') IS NULL
                   )::BIGINT,
                   COUNT(*) FILTER (
                       WHERE NULLIF(run.usage_json->>'estimated_cost_usd', '')::FLOAT8 < 0
                   )::BIGINT
            FROM tree
            JOIN agent_runs run ON run.orchestration_id = tree.id
            "#,
        )
        .bind(root_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(ProjectTokenBudgetError::Database)?;
        if unpriced_runs > 0 || negative_cost_runs > 0 || !used.is_finite() || used < 0.0 {
            return Err(ProjectTokenBudgetError::InvalidConfiguration(format!(
                "编排树内有 {unpriced_runs} 条缺少成本估算、{negative_cost_runs} 条负成本或无效汇总记录，无法可靠检查总成本预算"
            )));
        }
        used
    } else {
        0.0
    };
    if estimated_cost_budget_exceeded(used, next, limit) {
        return Err(ProjectTokenBudgetError::EstimatedCostExceeded { used, next, limit });
    }
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
#[derive(Debug, Clone, PartialEq)]
enum OptionalTesterBudget {
    Allowed,
    Exceeded(String),
}

#[cfg(not(target_family = "wasm"))]
async fn optional_tester_budget(
    db: &sqlx::PgPool,
    project_id: i64,
    orchestration_id: Option<i64>,
) -> Result<OptionalTesterBudget, String> {
    let mut transaction = db.begin().await.map_err(|error| error.to_string())?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('fuxi_project_daily_budget:' || $1::TEXT, 0))",
    )
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;
    let result = async {
        check_project_daily_budgets_on_connection(&mut transaction, project_id, Some(0.0)).await?;
        if let Some(orchestration_id) = orchestration_id {
            check_orchestration_budgets(
                &mut transaction,
                project_id,
                Some(orchestration_id),
                None,
                Some(0.0),
            )
            .await?;
        }
        Ok::<_, ProjectTokenBudgetError>(())
    }
    .await;
    match result {
        Ok(()) => {
            transaction
                .commit()
                .await
                .map_err(|error| error.to_string())?;
            Ok(OptionalTesterBudget::Allowed)
        }
        Err(ProjectTokenBudgetError::Exceeded { used, limit }) => {
            Ok(OptionalTesterBudget::Exceeded(format!(
                "项目今日 Token 预算已用尽（已用 {used}，上限 {limit}）"
            )))
        }
        Err(ProjectTokenBudgetError::OrchestrationTokensExceeded { used, limit }) => {
            Ok(OptionalTesterBudget::Exceeded(format!(
                "本编排树已达到累计 Token 门槛（已观测 {used}，门槛 {limit}）"
            )))
        }
        Err(ProjectTokenBudgetError::EstimatedCostExceeded {
            used,
            next: _,
            limit,
        }) => Ok(OptionalTesterBudget::Exceeded(format!(
            "项目今日估算成本预算不足（已用 ${used:.6}，上限 ${limit:.6}）"
        ))),
        Err(error) => Err(optional_tester_budget_error(&error)),
    }
}

#[cfg(not(target_family = "wasm"))]
fn optional_tester_budget_error(error: &ProjectTokenBudgetError) -> String {
    match error {
        ProjectTokenBudgetError::InvalidConfiguration(message) => message.clone(),
        ProjectTokenBudgetError::Database(error) => error.to_string(),
        ProjectTokenBudgetError::RetryAlreadyActive => "编排存在活跃重试".into(),
        ProjectTokenBudgetError::Exceeded { .. }
        | ProjectTokenBudgetError::OrchestrationTokensExceeded { .. }
        | ProjectTokenBudgetError::EstimatedCostExceeded { .. } => {
            "预算门控拒绝执行可选 Tester".into()
        }
    }
}

#[cfg(not(target_family = "wasm"))]
async fn record_running_dsl_usage_snapshot(
    db: &sqlx::PgPool,
    run: AgentExecutionRun,
    agent: &LlmAgent,
) -> Result<(), String> {
    let Some(run_id) = run.run_id else {
        return Ok(());
    };
    let usage = agent.usage_snapshot();
    let mut transaction = db.begin().await.map_err(|error| error.to_string())?;
    sqlx::query("UPDATE agent_runs SET usage_json = $1 WHERE id = $2 AND status = 'running'")
        .bind(&usage)
        .bind(run_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query(
        "UPDATE agent_steps SET usage_json = $1 WHERE agent_run_id = $2 AND status = 'running'",
    )
    .bind(usage)
    .bind(run_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn project_daily_token_budget_from_env() -> Result<Option<i64>, String> {
    project_daily_token_budget_from_raw(
        std::env::var("OPENAI_MAX_PROJECT_DAILY_TOKENS")
            .ok()
            .as_deref(),
    )
}

#[cfg(not(target_family = "wasm"))]
fn project_daily_token_budget_from_raw(raw: Option<&str>) -> Result<Option<i64>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let limit = raw
        .trim()
        .parse::<i64>()
        .map_err(|_| "OPENAI_MAX_PROJECT_DAILY_TOKENS 必须是非负整数".to_string())?;
    if limit < 0 {
        return Err("OPENAI_MAX_PROJECT_DAILY_TOKENS 必须是非负整数".to_string());
    }
    Ok(Some(limit))
}

#[cfg(not(target_family = "wasm"))]
async fn check_project_daily_token_budget_on_connection(
    connection: &mut sqlx::PgConnection,
    project_id: i64,
    limit: i64,
) -> Result<(), ProjectTokenBudgetError> {
    let (used, negative_runs) = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT COALESCE(SUM(COALESCE(NULLIF(usage_json->>'total_tokens', '')::BIGINT, 0)), 0)::BIGINT,
               COUNT(*) FILTER (
                   WHERE COALESCE(NULLIF(usage_json->>'total_tokens', '')::BIGINT, 0) < 0
               )::BIGINT
        FROM agent_runs
        WHERE project_id = $1
          AND (
              (status = 'running'
               AND started_at >= CURRENT_DATE
               AND usage_json->>'budget_reservation' IS DISTINCT FROM 'true')
              OR (status <> 'running' AND finished_at >= CURRENT_DATE)
          )
          AND usage_json IS NOT NULL
        "#,
    )
    .bind(project_id)
    .fetch_one(connection)
    .await
    .map_err(ProjectTokenBudgetError::Database)?;
    if negative_runs > 0 {
        return Err(ProjectTokenBudgetError::InvalidConfiguration(format!(
            "项目今日有 {negative_runs} 条运行记录包含负 token 用量，无法可靠检查累计 token 预算"
        )));
    }
    if used >= limit {
        return Err(ProjectTokenBudgetError::Exceeded { used, limit });
    }
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
async fn check_project_daily_estimated_cost_budget_on_connection(
    connection: &mut sqlx::PgConnection,
    project_id: i64,
    limit: f64,
    next: f64,
) -> Result<(), ProjectTokenBudgetError> {
    let (used, unpriced_runs, negative_cost_runs) = sqlx::query_as::<_, (f64, i64, i64)>(
        r#"
        SELECT COALESCE(SUM(NULLIF(usage_json->>'estimated_cost_usd', '')::FLOAT8), 0)::FLOAT8,
               COUNT(*) FILTER (
                   WHERE status <> 'running'
                     AND COALESCE((usage_json->>'request_count')::BIGINT, 0) > 0
                     AND NULLIF(usage_json->>'estimated_cost_usd', '') IS NULL
               )::BIGINT,
               COUNT(*) FILTER (
                   WHERE NULLIF(usage_json->>'estimated_cost_usd', '')::FLOAT8 < 0
               )::BIGINT
        FROM agent_runs
        WHERE project_id = $1
          -- Charge completed responses on their completion day; carry live reservations across midnight.
          AND (
              (status = 'running' AND started_at > NOW() - INTERVAL '2 hours')
              OR (status <> 'running' AND finished_at >= CURRENT_DATE)
          )
          AND usage_json IS NOT NULL
        "#,
    )
    .bind(project_id)
    .fetch_one(connection)
    .await
    .map_err(ProjectTokenBudgetError::Database)?;
    if negative_cost_runs > 0 {
        return Err(ProjectTokenBudgetError::InvalidConfiguration(format!(
            "项目今日有 {negative_cost_runs} 条运行记录包含负估算成本，无法可靠检查累计成本预算"
        )));
    }
    if unpriced_runs > 0 {
        return Err(ProjectTokenBudgetError::InvalidConfiguration(format!(
            "项目今日有 {unpriced_runs} 条已完成运行缺少成本估算，无法可靠检查累计成本预算"
        )));
    }
    if !used.is_finite() || used < 0.0 {
        return Err(ProjectTokenBudgetError::InvalidConfiguration(
            "项目今日已记录的估算成本不是有效的非负有限数字".into(),
        ));
    }
    if estimated_cost_budget_exceeded(used, next, limit) {
        return Err(ProjectTokenBudgetError::EstimatedCostExceeded { used, next, limit });
    }
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
pub(crate) async fn begin_project_daily_budget_transaction(
    db: &sqlx::PgPool,
    project_id: i64,
    estimated_response_cost: Option<f64>,
) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, ProjectTokenBudgetError> {
    let mut transaction = db
        .begin()
        .await
        .map_err(ProjectTokenBudgetError::Database)?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('fuxi_project_daily_budget:' || $1::TEXT, 0))",
    )
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(ProjectTokenBudgetError::Database)?;
    check_project_daily_budgets_on_connection(
        &mut transaction,
        project_id,
        estimated_response_cost,
    )
    .await?;
    Ok(transaction)
}

#[cfg(not(target_family = "wasm"))]
async fn check_project_daily_budgets_on_connection(
    connection: &mut sqlx::PgConnection,
    project_id: i64,
    estimated_response_cost: Option<f64>,
) -> Result<(), ProjectTokenBudgetError> {
    if let Some(limit) = project_daily_token_budget_from_env()
        .map_err(ProjectTokenBudgetError::InvalidConfiguration)?
    {
        check_project_daily_token_budget_on_connection(connection, project_id, limit).await?;
    }
    if let Some(limit) = project_daily_estimated_cost_budget_from_env()
        .map_err(ProjectTokenBudgetError::InvalidConfiguration)?
    {
        let Some(next) = estimated_response_cost.filter(|cost| cost.is_finite() && *cost >= 0.0)
        else {
            return Err(ProjectTokenBudgetError::InvalidConfiguration(
                "项目每日估算成本预算已启用，但主模型或备用模型缺少有效成本估算".into(),
            ));
        };
        check_project_daily_estimated_cost_budget_on_connection(
            connection, project_id, limit, next,
        )
        .await?;
    }
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn project_budget_reservation(estimated_response_cost: Option<f64>) -> Value {
    json!({
        "request_count": 0,
        "prompt_tokens": 0,
        "completion_tokens": 0,
        "total_tokens": 0,
        "estimated_cost_usd": estimated_response_cost,
        "budget_reservation": true,
    })
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn estimated_run_cost_reservation(
    estimated_cost_per_response: Option<f64>,
    response_count: usize,
) -> Option<f64> {
    estimated_cost_per_response.map(|cost| cost * response_count.max(1) as f64)
}

#[cfg(not(target_family = "wasm"))]
fn estimated_cost_budget_exceeded(used: f64, next: f64, limit: f64) -> bool {
    if !used.is_finite()
        || !next.is_finite()
        || !limit.is_finite()
        || used < 0.0
        || next < 0.0
        || limit < 0.0
        || used > limit
    {
        return true;
    }

    let remaining = limit - used;
    let rounding_tolerance = f64::EPSILON * next * 1.5;
    next > remaining + rounding_tolerance
}

#[cfg(not(target_family = "wasm"))]
fn project_daily_estimated_cost_budget_from_env() -> Result<Option<f64>, String> {
    project_daily_estimated_cost_budget_from_raw(
        std::env::var("OPENAI_MAX_PROJECT_DAILY_ESTIMATED_COST_USD")
            .ok()
            .as_deref(),
    )
}

#[cfg(not(target_family = "wasm"))]
fn project_daily_estimated_cost_budget_from_raw(raw: Option<&str>) -> Result<Option<f64>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let limit = raw.trim().parse::<f64>().map_err(|_| {
        "OPENAI_MAX_PROJECT_DAILY_ESTIMATED_COST_USD 必须是非负有限数字".to_string()
    })?;
    if !limit.is_finite() || limit < 0.0 {
        return Err("OPENAI_MAX_PROJECT_DAILY_ESTIMATED_COST_USD 必须是非负有限数字".into());
    }
    Ok(Some(limit))
}

#[cfg(not(target_family = "wasm"))]
async fn project_allows_model_experiment(db: &sqlx::PgPool, project_id: i64) -> bool {
    match sqlx::query_scalar::<_, String>("SELECT risk_level FROM decision_projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(db)
        .await
    {
        Ok(Some(risk_level)) => risk_level != "high",
        Ok(None) => false,
        Err(error) => {
            eprintln!("读取项目风险等级失败，已禁用模型实验：{error}");
            false
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn project_token_budget_response(error: ProjectTokenBudgetError) -> Response {
    match error {
        ProjectTokenBudgetError::Exceeded { used, limit } => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "ok": false,
                "error": format!("项目今日模型 token 预算已用尽（已用 {used}，上限 {limit}）")
            })),
        )
            .into_response(),
        ProjectTokenBudgetError::OrchestrationTokensExceeded { used, limit } => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "ok": false,
                "error": format!("本编排树已达到累计观测 Token 门槛（已观测 {used}，门槛 {limit}）；在途模型调用可能使实际用量超出门槛")
            })),
        )
            .into_response(),
        ProjectTokenBudgetError::EstimatedCostExceeded { used, next, limit } => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "ok": false,
                "error": format!("项目今日模型估算成本预算不足（已用 ${used:.6}，本次至少 ${next:.6}，上限 ${limit:.6}）")
            })),
        )
            .into_response(),
        ProjectTokenBudgetError::RetryAlreadyActive => (
            StatusCode::CONFLICT,
            Json(json!({
                "ok": false,
                "code": "ORCHESTRATION_RETRY_CONFLICT",
                "error": "该编排已有活跃重试或重试条件不满足"
            })),
        )
            .into_response(),
        ProjectTokenBudgetError::Database(sqlx::Error::RowNotFound) => (
            StatusCode::CONFLICT,
            Json(json!({
                "ok": false,
                "code": "WORKFLOW_ORCHESTRATION_UNAVAILABLE",
                "error": "需求或方案的上游编排已不可用，请刷新工作流并重新确认方案后重试"
            })),
        )
            .into_response(),
        ProjectTokenBudgetError::InvalidConfiguration(error) => {
            eprintln!("项目 token 预算配置无效，拒绝模型调用：{error}");
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"ok": false, "error": "项目预算配置无效，暂时无法调用模型"})),
            )
                .into_response()
        }
        ProjectTokenBudgetError::Database(error) => {
            eprintln!("项目预算或模型运行数据库操作失败: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"ok": false, "error": "检查模型预算失败"})),
            )
                .into_response()
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn fallback_models_from_raw(raw: Option<&str>) -> Vec<String> {
    let mut models = Vec::new();
    for model in raw.unwrap_or_default().split(',').map(str::trim).take(3) {
        if !model.is_empty() && !models.iter().any(|existing| existing == model) {
            models.push(model.to_string());
        }
    }
    models
}

#[cfg(not(target_family = "wasm"))]
fn model_selection_policy_from_env(prefix: &str) -> ModelSelectionPolicy {
    let task_variable = format!("OPENAI_{prefix}_MODEL_SELECTION_POLICY");
    let task_policy = std::env::var(task_variable).ok();
    let global_policy = std::env::var("OPENAI_MODEL_SELECTION_POLICY").ok();
    ModelSelectionPolicy::from_env_value(task_policy.as_deref().or(global_policy.as_deref()))
}

#[cfg(all(not(target_family = "wasm"), test))]
fn model_selection_policy_from_values(
    task_policy: Option<&str>,
    global_policy: Option<&str>,
) -> ModelSelectionPolicy {
    ModelSelectionPolicy::from_env_value(task_policy.or(global_policy))
}

#[cfg(not(target_family = "wasm"))]
fn model_candidates_from_raw(default_model: &str, raw: Option<&str>) -> Vec<ModelCandidate> {
    let mut candidates = vec![ModelCandidate {
        name: default_model.trim().to_string(),
        estimated_cost_usd: None,
        success_rate: None,
    }];
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return candidates;
    };
    let Ok(configured) = serde_json::from_str::<Vec<ModelCandidateConfig>>(raw) else {
        eprintln!("OPENAI_MODEL_CANDIDATES_JSON 必须是模型配置数组，已忽略该配置");
        return candidates;
    };
    for model in configured {
        let name = model.name.trim();
        if name.is_empty() {
            continue;
        }
        if let Some(existing) = candidates
            .iter_mut()
            .find(|candidate| candidate.name == name)
        {
            existing.estimated_cost_usd = model.estimated_cost_usd;
            existing.success_rate = model.success_rate;
            continue;
        }
        candidates.push(ModelCandidate {
            name: name.to_string(),
            estimated_cost_usd: model.estimated_cost_usd,
            success_rate: model.success_rate,
        });
    }
    candidates
}

#[cfg(not(target_family = "wasm"))]
#[derive(Deserialize)]
struct ModelCandidateConfig {
    name: String,
    estimated_cost_usd: Option<f64>,
    success_rate: Option<f64>,
}

// 拼接绝对 URL；供本模块内部请求复用（wasm 上 reqwest 不支持相对路径）
#[cfg(target_family = "wasm")]
pub(crate) fn api_url(path: &str) -> Option<String> {
    let origin = web_sys::window()?.location().origin().ok()?;
    Some(format!("{origin}{path}"))
}

/// 客户端请求 `Accept: text/event-stream` 时走 SSE 流式响应，否则返回普通 JSON
#[cfg(not(target_family = "wasm"))]
fn wants_stream(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/event-stream"))
        .unwrap_or(false)
}

/// 宽容 JSON 解析：不校验 Content-Type，只要 body 是合法 JSON 即可。
/// 解析失败返回 422（与 axum Json 提取器错误码一致）
#[cfg(not(target_family = "wasm"))]
async fn parse_json_body<T: serde::de::DeserializeOwned>(
    body: Bytes,
) -> Result<T, (StatusCode, Json<Value>)> {
    serde_json::from_slice(&body).map_err(|e| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "ok": false, "error": format!("请求体 JSON 解析失败: {}", e) })),
        )
    })
}

#[cfg(not(target_family = "wasm"))]
async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "dmn-nl_dsl",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// 登录：成功返回 JWT token，失败时保留服务端返回的诊断信息。
#[cfg(target_family = "wasm")]
pub async fn login(
    username: &str,
    password: &str,
    captcha_id: &str,
    captcha_code: &str,
    captcha_work_nonce: &str,
) -> Result<String, String> {
    let url = api_url("/api/login").ok_or_else(|| "无法确定登录接口地址".to_string())?;
    let res = reqwest::Client::new()
        .post(url)
        .json(&LoginPayload {
            username: username.to_string(),
            password: password.to_string(),
            captcha_id: captcha_id.to_string(),
            captcha_code: captcha_code.to_string(),
            captcha_work_nonce: captcha_work_nonce.to_string(),
        })
        .send()
        .await
        .map_err(|error| format!("登录请求失败：{error}"))?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.json::<serde_json::Value>().await.ok();
        return Err(body
            .and_then(|body| {
                body.get("error")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("登录失败（HTTP {}）", status.as_u16())));
    }
    let data: LoginResponse = res
        .json()
        .await
        .map_err(|error| format!("登录响应解析失败：{error}"))?;
    Ok(data.token)
}

#[cfg(target_family = "wasm")]
pub async fn captcha() -> Result<CaptchaResponse, String> {
    let url = api_url("/api/captcha").ok_or_else(|| "无法确定验证码接口地址".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|error| format!("验证码请求失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("验证码接口返回 HTTP {}", status.as_u16()));
    }
    response
        .json()
        .await
        .map_err(|error| format!("验证码响应解析失败：{error}"))
}

// 获取当前登录用户：成功返回用户名，token 失效/网络错误返回 None
#[cfg(target_family = "wasm")]
pub async fn me(token: &str) -> Option<(String, String)> {
    let url = api_url("/api/me")?;
    let res = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .ok()?;
    let status = res.status();
    handle_auth_status(status);
    if !status.is_success() {
        return None;
    }
    let data: serde_json::Value = res.json().await.ok()?;
    Some((
        data["username"].as_str()?.to_string(),
        data["role"].as_str().unwrap_or("user").to_string(),
    ))
}

#[cfg(target_family = "wasm")]
pub async fn list_admin_users(token: &str) -> Result<Vec<AdminUser>, String> {
    let url = api_url("/api/admin/users").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取用户列表失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取用户列表").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("用户列表响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_prompt_assets(
    token: &str,
) -> Result<Vec<crate::models::PromptAssetAdminView>, String> {
    let url = api_url("/api/admin/prompts").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取提示词资产失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取提示词资产").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词资产响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_prompt_asset_events(
    token: &str,
) -> Result<Vec<crate::models::PromptAssetEventAdminView>, String> {
    let url =
        api_url("/api/admin/prompts/events").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取提示词发布历史失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取提示词发布历史").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词发布历史响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn create_prompt_draft(
    token: &str,
    payload: crate::models::CreatePromptDraftRequest,
) -> Result<crate::models::PromptAssetAdminView, String> {
    let url = api_url("/api/admin/prompts").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("创建提示词草稿失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "创建提示词草稿").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词草稿响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn evaluate_prompt_asset(
    token: &str,
    asset_id: i64,
    payload: crate::models::PromptEvaluationRequest,
) -> Result<crate::models::PromptAssetAdminView, String> {
    let url = api_url(&format!("/api/admin/prompts/{asset_id}/evaluation"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .put(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("登记提示词评测失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "登记提示词评测").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词评测响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn run_prompt_asset_evaluation(
    token: &str,
    asset_id: i64,
) -> Result<crate::models::PromptEvaluationJobView, String> {
    let url = api_url(&format!("/api/admin/prompts/{asset_id}/run-evaluation"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("运行提示词评测失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "运行提示词评测").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词评测作业响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_prompt_evaluation_job(
    token: &str,
    job_id: i64,
) -> Result<crate::models::PromptEvaluationJobView, String> {
    let url = api_url(&format!("/api/admin/prompts/evaluation-jobs/{job_id}"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取提示词评测作业失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取提示词评测作业").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词评测作业响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_prompt_evaluation_jobs(
    token: &str,
) -> Result<Vec<crate::models::PromptEvaluationJobView>, String> {
    let url = api_url("/api/admin/prompts/evaluation-jobs")
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取提示词评测作业失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取提示词评测作业").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词评测作业列表响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn cancel_prompt_evaluation_job(
    token: &str,
    job_id: i64,
) -> Result<crate::models::PromptEvaluationJobView, String> {
    let url = api_url(&format!(
        "/api/admin/prompts/evaluation-jobs/{job_id}/cancel"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("取消提示词评测失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "取消提示词评测").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词评测作业响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn activate_prompt_asset(
    token: &str,
    asset_id: i64,
) -> Result<crate::models::PromptAssetAdminView, String> {
    let url = api_url(&format!("/api/admin/prompts/{asset_id}/activate"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("激活提示词版本失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "激活提示词版本").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词激活响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn rollback_prompt_asset(
    token: &str,
    prompt_key: &str,
) -> Result<crate::models::PromptAssetAdminView, String> {
    let url = api_url(&format!("/api/admin/prompts/{prompt_key}/rollback"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("回滚提示词版本失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "回滚提示词版本").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("提示词回滚响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_model_performance_metrics(
    token: &str,
) -> Result<Vec<ModelPerformanceMetricView>, String> {
    let url = api_url("/api/admin/model-performance")
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取模型性能指标失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取模型性能指标").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("模型性能指标响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_decision_explanation_admin_metrics(
    token: &str,
) -> Result<Vec<DecisionExplanationAdminMetricView>, String> {
    let url = api_url("/api/admin/decision-explanation-metrics")
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取决策解释指标失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取决策解释指标").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("决策解释指标响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_business_quality_metrics(
    token: &str,
) -> Result<Vec<AdminBusinessQualityMetricView>, String> {
    let url =
        api_url("/api/admin/business-quality").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取业务效果指标失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取业务效果指标").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("业务效果指标响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_model_experiment_metrics(
    token: &str,
) -> Result<Vec<ModelExperimentMetricView>, String> {
    let url = api_url("/api/admin/model-experiments")
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取模型实验指标失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取模型实验指标").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("模型实验指标响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_model_usage_metrics(token: &str) -> Result<Vec<ModelUsageMetricView>, String> {
    let url =
        api_url("/api/admin/model-usage").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取模型 token 用量失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取模型 token 用量").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("模型 token 用量响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn create_admin_user(token: &str, payload: CreateUser) -> Result<AdminUser, String> {
    let url = api_url("/api/admin/users").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("添加用户失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "添加用户").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("用户响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn update_admin_user(
    token: &str,
    username: &str,
    payload: UpdateUser,
) -> Result<AdminUser, String> {
    let encoded = js_sys::encode_uri_component(username)
        .as_string()
        .unwrap_or_else(|| username.to_string());
    let url = api_url(&format!("/api/admin/users/{encoded}"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .put(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("更新用户失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "更新用户").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("用户响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn delete_admin_user(token: &str, username: &str) -> Result<(), String> {
    let encoded = js_sys::encode_uri_component(username)
        .as_string()
        .unwrap_or_else(|| username.to_string());
    let url = api_url(&format!("/api/admin/users/{encoded}"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .delete(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("删除用户失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "删除用户").await);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
async fn project_api_error(response: reqwest::Response, operation: &str) -> String {
    let status = response.status();
    handle_auth_status(status);
    let body = response.text().await.unwrap_or_default();
    format_project_error(&body, operation, status)
}

#[cfg(target_family = "wasm")]
fn format_project_error(body: &str, operation: &str, status: reqwest::StatusCode) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            let message = value["error"].as_str()?.to_owned();
            let code = value["code"].as_str().unwrap_or("UNKNOWN");
            Some(format!("[{code}] {message}"))
        })
        .unwrap_or_else(|| format!("{operation}失败 ({status})"))
}

#[cfg(target_family = "wasm")]
pub async fn list_projects(token: &str) -> Result<Vec<DecisionProject>, String> {
    let url = api_url("/api/projects").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取决策项目失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取决策项目").await);
    }
    response
        .json::<Vec<DecisionProject>>()
        .await
        .map_err(|error| format!("决策项目响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_project_orchestrations(
    token: &str,
    project_id: i64,
    before_id: Option<i64>,
    limit: u16,
) -> Result<Vec<AgentOrchestrationHistoryItem>, String> {
    let mut url = api_url(&format!("/api/projects/{project_id}/orchestrations"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    url.push_str(&format!("?limit={}", limit.clamp(1, 100)));
    if let Some(before_id) = before_id {
        url.push_str(&format!("&beforeId={before_id}"));
    }
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取 Agent 编排历史失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取 Agent 编排历史").await);
    }
    response
        .json()
        .await
        .map_err(|error| format!("Agent 编排历史响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn create_project(
    token: &str,
    payload: CreateDecisionProject,
) -> Result<DecisionProject, String> {
    let url = api_url("/api/projects").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("创建决策项目失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "创建决策项目").await);
    }
    response
        .json::<DecisionProject>()
        .await
        .map_err(|error| format!("决策项目响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn update_project(
    token: &str,
    project_id: i64,
    payload: CreateDecisionProject,
) -> Result<DecisionProject, String> {
    let url = api_url(&format!("/api/projects/{project_id}"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .put(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("更新决策项目失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "更新决策项目").await);
    }
    response
        .json::<DecisionProject>()
        .await
        .map_err(|error| format!("决策项目响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn delete_project(token: &str, project_id: i64) -> Result<(), String> {
    let url = api_url(&format!("/api/projects/{project_id}"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .delete(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("删除决策项目失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "删除决策项目").await);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
pub async fn cancel_agent_orchestration(
    token: &str,
    project_id: i64,
    orchestration_id: i64,
) -> Result<(), String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/orchestrations/{orchestration_id}/cancel"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("取消 Agent 编排失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "取消 Agent 编排").await);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
pub async fn list_model_library(token: &str) -> Result<Vec<ModelLibraryItem>, String> {
    let url = api_url("/api/model-library").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取模型库失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取模型库").await);
    }
    response
        .json::<Vec<ModelLibraryItem>>()
        .await
        .map_err(|error| format!("模型库响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn recommend_decision_patterns(
    token: &str,
    query: &str,
    project_id: Option<i64>,
) -> Result<Vec<PatternRecommendation>, String> {
    let base = api_url("/api/model-library/recommendations")
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let mut params = vec![("q", query.to_string())];
    if let Some(project_id) = project_id {
        params.push(("projectId", project_id.to_string()));
    }
    let response = reqwest::Client::new()
        .get(base)
        .query(&params)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("检索相似模式失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "检索相似模式").await);
    }
    response
        .json::<Vec<PatternRecommendation>>()
        .await
        .map_err(|error| format!("相似模式响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn submit_pattern_recommendation_feedback(
    token: &str,
    recommendation_id: i64,
    payload: &PatternRecommendationFeedback,
) -> Result<(), String> {
    let url = api_url(&format!(
        "/api/model-library/recommendations/{recommendation_id}/feedback"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(payload)
        .send()
        .await
        .map_err(|error| format!("提交模式反馈失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "提交模式反馈").await);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
pub async fn list_decision_pattern_lineage(
    token: &str,
    version_id: i64,
) -> Result<Vec<PatternLineageItem>, String> {
    let url = api_url(&format!("/api/model-library/history/{version_id}/lineage"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取模式演进链失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取模式演进链").await);
    }
    response
        .json::<Vec<PatternLineageItem>>()
        .await
        .map_err(|error| format!("模式演进链响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_pattern_library_analytics(token: &str) -> Result<PatternLibraryAnalytics, String> {
    let url = api_url("/api/model-library/analytics")
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取模式统计失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取模式统计").await);
    }
    response
        .json::<PatternLibraryAnalytics>()
        .await
        .map_err(|error| format!("模式统计响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn debug_project_knowledge_search(
    token: &str,
    project_id: i64,
    query: &str,
) -> Result<KnowledgeSearchDiagnostics, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/knowledge-search-debug"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .query(&[("q", query)])
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("知识检索调试失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "知识检索调试").await);
    }
    response
        .json::<KnowledgeSearchDiagnostics>()
        .await
        .map_err(|error| format!("知识检索调试响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn debug_pattern_search(
    token: &str,
    project_id: i64,
    query: &str,
) -> Result<PatternSearchDiagnostics, String> {
    let url = api_url("/api/model-library/search-debug")
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .query(&[("q", query), ("projectId", &project_id.to_string())])
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("模式检索调试失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "模式检索调试").await);
    }
    response
        .json::<PatternSearchDiagnostics>()
        .await
        .map_err(|error| format!("模式检索调试响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_knowledge_sources(token: &str) -> Result<Vec<KnowledgeSource>, String> {
    knowledge_request::<Vec<KnowledgeSource>>(token, "/api/knowledge-sources", "GET", None).await
}

#[cfg(target_family = "wasm")]
pub async fn get_knowledge_index_status(token: &str) -> Result<KnowledgeIndexStatus, String> {
    knowledge_request(token, "/api/knowledge-sources/index-status", "GET", None).await
}

#[cfg(target_family = "wasm")]
pub async fn reindex_knowledge_source(token: &str, source_id: i64) -> Result<(), String> {
    knowledge_empty_request(
        token,
        &format!("/api/knowledge-sources/{source_id}/reindex"),
        "POST",
    )
    .await
}

#[cfg(target_family = "wasm")]
pub async fn list_project_knowledge_sources(
    token: &str,
    project_id: i64,
) -> Result<Vec<KnowledgeSource>, String> {
    knowledge_request::<Vec<KnowledgeSource>>(
        token,
        &format!("/api/projects/{project_id}/knowledge-sources"),
        "GET",
        None,
    )
    .await
}

#[cfg(target_family = "wasm")]
pub async fn create_knowledge_source(
    token: &str,
    payload: &SaveKnowledgeSource,
) -> Result<KnowledgeSource, String> {
    knowledge_request(token, "/api/knowledge-sources", "POST", Some(payload)).await
}

#[cfg(target_family = "wasm")]
pub async fn update_knowledge_source(
    token: &str,
    source_id: i64,
    payload: &SaveKnowledgeSource,
) -> Result<KnowledgeSource, String> {
    knowledge_request(
        token,
        &format!("/api/knowledge-sources/{source_id}"),
        "PUT",
        Some(payload),
    )
    .await
}

#[cfg(target_family = "wasm")]
pub async fn archive_knowledge_source(token: &str, source_id: i64) -> Result<(), String> {
    knowledge_empty_request(
        token,
        &format!("/api/knowledge-sources/{source_id}"),
        "DELETE",
    )
    .await
}

#[cfg(target_family = "wasm")]
pub async fn set_project_knowledge_source(
    token: &str,
    project_id: i64,
    source_id: i64,
    attached: bool,
) -> Result<(), String> {
    knowledge_empty_request(
        token,
        &format!("/api/projects/{project_id}/knowledge-sources/{source_id}"),
        if attached { "POST" } else { "DELETE" },
    )
    .await
}

#[cfg(target_family = "wasm")]
async fn knowledge_request<T: serde::de::DeserializeOwned>(
    token: &str,
    path: &str,
    method: &str,
    payload: Option<&SaveKnowledgeSource>,
) -> Result<T, String> {
    let url = api_url(path).ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let client = reqwest::Client::new();
    let request = match method {
        "POST" => client.post(url),
        "PUT" => client.put(url),
        _ => client.get(url),
    }
    .header("Authorization", format!("Bearer {token}"));
    let request = if let Some(value) = payload {
        request.json(value)
    } else {
        request
    };
    let response = request
        .send()
        .await
        .map_err(|error| format!("知识库请求失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "知识库操作").await);
    }
    response
        .json::<T>()
        .await
        .map_err(|error| format!("知识库响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
async fn knowledge_empty_request(token: &str, path: &str, method: &str) -> Result<(), String> {
    let url = api_url(path).ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let client = reqwest::Client::new();
    let response = if method == "POST" {
        client.post(url)
    } else {
        client.delete(url)
    }
    .header("Authorization", format!("Bearer {token}"))
    .send()
    .await
    .map_err(|error| format!("知识库请求失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "知识库操作").await);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
pub async fn instantiate_model_library_item(
    token: &str,
    source_kind: &str,
    source_id: &str,
    project_name: String,
    description: String,
) -> Result<DecisionWorkspace, String> {
    let url = api_url(&format!(
        "/api/model-library/{source_kind}/{source_id}/instantiate"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&InstantiateModelLibraryItem {
            project_name,
            description,
            instantiate_confirmed: true,
        })
        .send()
        .await
        .map_err(|error| format!("复用模型失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "复用模型").await);
    }
    response
        .json::<DecisionWorkspace>()
        .await
        .map_err(|error| format!("复用模型响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn save_decision_template(
    token: &str,
    project_id: i64,
    version_id: i64,
    payload: SaveDecisionTemplate,
) -> Result<ModelLibraryItem, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/templates"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("保存模型模板失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "保存模型模板").await);
    }
    response
        .json::<ModelLibraryItem>()
        .await
        .map_err(|error| format!("模型模板响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn delete_decision_template(token: &str, template_id: &str) -> Result<(), String> {
    let url = api_url(&format!("/api/model-library/templates/{template_id}"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .delete(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("删除模型模板失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "删除模型模板").await);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
pub async fn get_project_workspace(
    token: &str,
    project_id: i64,
) -> Result<DecisionWorkspace, String> {
    let url = api_url(&format!("/api/projects/{project_id}"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取项目工作区失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取项目工作区").await);
    }
    response
        .json::<DecisionWorkspace>()
        .await
        .map_err(|error| format!("项目工作区响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_project_members(
    token: &str,
    project_id: i64,
) -> Result<Vec<ProjectMember>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/members?refresh={}",
        js_sys::Date::now()
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取项目成员失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取项目成员").await);
    }
    response
        .json::<Vec<ProjectMember>>()
        .await
        .map_err(|error| format!("项目成员响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn save_project_member(
    token: &str,
    project_id: i64,
    username: &str,
    role: String,
) -> Result<ProjectMember, String> {
    let encoded = js_sys::encode_uri_component(username)
        .as_string()
        .unwrap_or_else(|| username.to_string());
    let url = api_url(&format!("/api/projects/{project_id}/members/{encoded}"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .put(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&SaveProjectMember { role })
        .send()
        .await
        .map_err(|error| format!("保存项目成员失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "保存项目成员").await);
    }
    response
        .json::<ProjectMember>()
        .await
        .map_err(|error| format!("项目成员响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn delete_project_member(
    token: &str,
    project_id: i64,
    username: &str,
    role: Option<&str>,
) -> Result<(), String> {
    let encoded = js_sys::encode_uri_component(username)
        .as_string()
        .unwrap_or_else(|| username.to_string());
    let url = api_url(&format!(
        "/api/projects/{project_id}/members/{encoded}{}",
        role.map(|value| format!("?role={value}"))
            .unwrap_or_default()
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .delete(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("移除项目成员失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "移除项目成员").await);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
pub async fn create_project_version(
    token: &str,
    project_id: i64,
    payload: CreateDecisionVersion,
) -> Result<DecisionVersion, String> {
    let url = api_url(&format!("/api/projects/{project_id}/versions"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("保存决策版本失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "保存决策版本").await);
    }
    response
        .json::<DecisionVersion>()
        .await
        .map_err(|error| format!("决策版本响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn preview_model_change(
    token: &str,
    project_id: i64,
    intent: ModelChangeIntent,
) -> Result<ModelChangePreview, String> {
    let url = api_url(&format!("/api/projects/{project_id}/model-changes/preview"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&intent)
        .send()
        .await
        .map_err(|error| format!("生成模型变更候选失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "生成模型变更候选").await);
    }
    response
        .json::<ModelChangePreview>()
        .await
        .map_err(|error| format!("模型变更候选响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn commit_model_change(
    token: &str,
    project_id: i64,
    candidate_id: i64,
) -> Result<DecisionVersion, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/model-changes/{candidate_id}/commit"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("提交模型变更失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "提交模型变更").await);
    }
    response
        .json::<DecisionVersion>()
        .await
        .map_err(|error| format!("模型变更提交响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn generate_change_intent(
    token: &str,
    project_id: i64,
    text: String,
    base_revision: i64,
    workflow_updated_at: chrono::DateTime<chrono::Utc>,
    repair_error: Option<String>,
    repair_attempt: i32,
    previous_intent: Option<ModelChangeIntent>,
    retry_of_orchestration_id: Option<i64>,
) -> Result<ModelChangeIntent, String> {
    let url =
        api_url("/api/generate-change-intent").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "text": text,
            "projectId": project_id,
            "baseRevision": base_revision,
            "workflowUpdatedAt": workflow_updated_at,
            "repairError": repair_error,
            "repairAttempt": repair_attempt,
            "previousIntent": previous_intent,
            "retryOfOrchestrationId": retry_of_orchestration_id,
        }))
        .send()
        .await
        .map_err(|error| format!("生成变更意图失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "生成变更意图").await);
    }
    let payload = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("变更意图响应无效: {error}"))?;
    serde_json::from_value(payload["changeIntent"].clone())
        .map_err(|error| format!("变更意图格式无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn generate_change_intent_stream(
    token: &str,
    project_id: i64,
    text: String,
    base_revision: i64,
    workflow_updated_at: chrono::DateTime<chrono::Utc>,
    repair_error: Option<String>,
    repair_attempt: i32,
    previous_intent: Option<ModelChangeIntent>,
    retry_of_orchestration_id: Option<i64>,
    controller: web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let url =
        api_url("/api/generate-change-intent").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let body = serde_json::to_string(&serde_json::json!({
        "text": text,
        "projectId": project_id,
        "baseRevision": base_revision,
        "workflowUpdatedAt": workflow_updated_at,
        "repairError": repair_error,
        "repairAttempt": repair_attempt,
        "previousIntent": previous_intent,
        "retryOfOrchestrationId": retry_of_orchestration_id,
    }))
    .map_err(|error| format!("变更意图请求无效: {error}"))?;
    let headers = web_sys::Headers::new().map_err(js_error)?;
    headers
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(js_error)?;
    headers
        .set("Content-Type", "application/json")
        .map_err(js_error)?;
    headers
        .set("Accept", "text/event-stream")
        .map_err(js_error)?;
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_headers_headers(&headers);
    init.set_body(&JsValue::from_str(&body));
    init.set_signal(Some(&controller.signal()));
    let request = web_sys::Request::new_with_str_and_init(&url, &init).map_err(js_error)?;
    let window = web_sys::window().ok_or_else(|| "浏览器窗口不可用".to_string())?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::Response>()
        .map_err(js_error)?;
    if !response.ok() {
        let status = response.status();
        if status == 401 {
            notify_auth_expired();
        }
        let body = JsFuture::from(response.text().map_err(js_error)?)
            .await
            .map_err(js_error)?
            .as_string()
            .unwrap_or_default();
        return Err(serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value["error"].as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("生成变更意图失败 ({status})")));
    }
    let mut terminal_error = None;
    read_sse_response_with_retry(response, token, project_id, &controller, |event| {
        if let SseEvent::Error(message) = &event {
            terminal_error = Some(message.clone());
        }
        on_event(event);
    })
    .await?;
    terminal_error.map_or(Ok(()), Err)
}

#[cfg(target_family = "wasm")]
pub async fn list_project_versions(
    token: &str,
    project_id: i64,
) -> Result<Vec<DecisionVersion>, String> {
    let url = api_url(&format!("/api/projects/{project_id}/versions"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取版本列表失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取版本列表").await);
    }
    response
        .json::<Vec<DecisionVersion>>()
        .await
        .map_err(|error| format!("版本列表响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn rollback_project_version(
    token: &str,
    project_id: i64,
    version_id: i64,
    reason: String,
) -> Result<DecisionWorkspace, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/rollback"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&RollbackDecisionVersion {
            reason,
            rollback_confirmed: true,
        })
        .send()
        .await
        .map_err(|error| format!("创建回滚草稿失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "创建回滚草稿").await);
    }
    response
        .json::<DecisionWorkspace>()
        .await
        .map_err(|error| format!("回滚草稿响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_project_audit_events(
    token: &str,
    project_id: i64,
) -> Result<Vec<AuditEvent>, String> {
    let url = api_url(&format!("/api/projects/{project_id}/audit-events"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取项目审计记录失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取项目审计记录").await);
    }
    response
        .json::<Vec<AuditEvent>>()
        .await
        .map_err(|error| format!("项目审计记录响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_version_review(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<VersionChangeReview, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/review"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取版本审查失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取版本审查").await);
    }
    response
        .json::<VersionChangeReview>()
        .await
        .map_err(|error| format!("版本审查响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_version_behavior_comparison(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<crate::models::BehaviorComparisonReport, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/behavior-comparison"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取版本行为对比失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取版本行为对比").await);
    }
    response
        .json::<crate::models::BehaviorComparisonReport>()
        .await
        .map_err(|error| format!("版本行为对比响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn run_version_regression_tests(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<crate::models::RegressionTestReport, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/regression-tests/run"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("运行历史回归测试失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "运行历史回归测试").await);
    }
    response
        .json::<crate::models::RegressionTestReport>()
        .await
        .map_err(|error| format!("历史回归测试响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_version_verification(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<Option<VersionVerificationReport>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/verification"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取验收报告失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取验收报告").await);
    }
    response
        .json::<Option<VersionVerificationReport>>()
        .await
        .map_err(|error| format!("验收报告响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn review_project_version(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<DecisionVersion, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/review-approval"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("独立复核版本失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "独立复核版本").await);
    }
    response
        .json::<DecisionVersion>()
        .await
        .map_err(|error| format!("独立复核响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn verify_project_version(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<VersionVerificationReport, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/verification"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("执行自动验收失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "执行自动验收").await);
    }
    response
        .json::<VersionVerificationReport>()
        .await
        .map_err(|error| format!("自动验收响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn publish_project_version(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<DecisionVersion, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/publish"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&PublishDecisionVersion {
            publish_confirmed: true,
        })
        .send()
        .await
        .map_err(|error| format!("发布决策版本失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "发布决策版本").await);
    }
    response
        .json::<DecisionVersion>()
        .await
        .map_err(|error| format!("发布版本响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_decision_test_cases(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<Vec<DecisionTestCase>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/test-cases"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取业务验收案例失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取业务验收案例").await);
    }
    response
        .json::<Vec<DecisionTestCase>>()
        .await
        .map_err(|error| format!("业务验收案例响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn create_decision_test_case(
    token: &str,
    project_id: i64,
    version_id: i64,
    payload: CreateDecisionTestCase,
) -> Result<DecisionTestCase, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/test-cases"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("创建业务验收案例失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "创建业务验收案例").await);
    }
    response
        .json::<DecisionTestCase>()
        .await
        .map_err(|error| format!("业务验收案例响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn convert_tester_proposal(
    token: &str,
    project_id: i64,
    version_id: i64,
    proposal_id: &str,
    payload: CreateDecisionTestCase,
) -> Result<DecisionTestCase, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/tester-proposals/{proposal_id}/convert"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("转换测试提案失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "转换测试提案").await);
    }
    response
        .json::<DecisionTestCase>()
        .await
        .map_err(|error| format!("测试提案转换响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn judge_tester_proposal(
    token: &str,
    project_id: i64,
    version_id: i64,
    proposal_id: &str,
    payload: CreateDecisionTestCase,
) -> Result<crate::workflow::TesterJudgeReview, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/tester-proposals/{proposal_id}/judge"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("复核测试提案失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "复核测试提案").await);
    }
    response
        .json::<crate::workflow::TesterJudgeReview>()
        .await
        .map_err(|error| format!("测试提案复核响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn generate_decision_test_cases(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<GenerateDecisionTestCasesResult, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/test-cases/generate"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("生成业务验收案例失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "生成业务验收案例").await);
    }
    response
        .json::<GenerateDecisionTestCasesResult>()
        .await
        .map_err(|error| format!("生成业务案例响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn run_decision_test_cases(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<Vec<DecisionTestCase>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/test-cases/run"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("运行业务验收案例失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "运行业务验收案例").await);
    }
    response
        .json::<Vec<DecisionTestCase>>()
        .await
        .map_err(|error| format!("业务验收执行响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn run_tester_proposal_cases(
    token: &str,
    project_id: i64,
    version_id: i64,
) -> Result<Vec<DecisionTestCase>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/tester-proposals/run"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("运行 Tester 提案用例失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "运行 Tester 提案用例").await);
    }
    response
        .json::<Vec<DecisionTestCase>>()
        .await
        .map_err(|error| format!("Tester 执行结果无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn approve_decision_test_case(
    token: &str,
    project_id: i64,
    version_id: i64,
    case_id: i64,
    approved: bool,
) -> Result<DecisionTestCase, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/test-cases/{case_id}/approval"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .put(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&ApproveDecisionTestCase { approved })
        .send()
        .await
        .map_err(|error| format!("更新验收批准状态失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "更新验收批准状态").await);
    }
    response
        .json::<DecisionTestCase>()
        .await
        .map_err(|error| format!("验收批准响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn delete_decision_test_case(
    token: &str,
    project_id: i64,
    version_id: i64,
    case_id: i64,
) -> Result<(), String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/versions/{version_id}/test-cases/{case_id}"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .delete(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("删除业务验收案例失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "删除业务验收案例").await);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
pub async fn list_published_decision_services(
    token: &str,
    project_id: i64,
) -> Result<Vec<PublishedDecisionService>, String> {
    let url = api_url(&format!("/api/projects/{project_id}/decision-services"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取已发布决策服务失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取已发布决策服务").await);
    }
    response
        .json::<Vec<PublishedDecisionService>>()
        .await
        .map_err(|error| format!("决策服务目录响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_decision_service_api_keys(
    token: &str,
    project_id: i64,
) -> Result<Vec<DecisionServiceApiKey>, String> {
    let url = api_url(&format!("/api/projects/{project_id}/decision-service-keys"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取服务调用凭证失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取服务调用凭证").await);
    }
    response
        .json::<Vec<DecisionServiceApiKey>>()
        .await
        .map_err(|error| format!("服务调用凭证响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn create_decision_service_api_key(
    token: &str,
    project_id: i64,
    service_id: String,
    name: String,
) -> Result<CreatedDecisionServiceApiKey, String> {
    let url = api_url(&format!("/api/projects/{project_id}/decision-service-keys"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&crate::models::CreateDecisionServiceApiKey {
            service_id,
            name,
            expires_at: None,
        })
        .send()
        .await
        .map_err(|error| format!("创建服务调用凭证失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "创建服务调用凭证").await);
    }
    response
        .json::<CreatedDecisionServiceApiKey>()
        .await
        .map_err(|error| format!("服务调用凭证响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn revoke_decision_service_api_key(
    token: &str,
    project_id: i64,
    key_id: i64,
) -> Result<(), String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/decision-service-keys/{key_id}"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .delete(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("撤销服务调用凭证失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "撤销服务调用凭证").await);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
pub async fn execute_published_decision_service(
    token: &str,
    project_id: i64,
    service_id: &str,
    business_reference: String,
    inputs: serde_json::Value,
) -> Result<DecisionServiceRun, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/decision-services/execute"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&ExecuteDecisionService {
            service_id: service_id.to_string(),
            inputs,
            business_reference,
        })
        .send()
        .await
        .map_err(|error| format!("调用决策服务失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "调用决策服务").await);
    }
    response
        .json::<DecisionServiceRun>()
        .await
        .map_err(|error| format!("决策服务运行响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn save_decision_run_outcome(
    token: &str,
    project_id: i64,
    run_id: i64,
    payload: SaveDecisionRunOutcome,
) -> Result<DecisionServiceRun, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/decision-runs/{run_id}/outcome"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .put(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("保存实际业务结果失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "保存实际业务结果").await);
    }
    response
        .json::<DecisionServiceRun>()
        .await
        .map_err(|error| format!("实际业务结果响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_decision_service_runs(
    token: &str,
    project_id: i64,
) -> Result<Vec<DecisionServiceRun>, String> {
    let url = api_url(&format!("/api/projects/{project_id}/decision-runs"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取决策服务运行记录失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取决策服务运行记录").await);
    }
    response
        .json::<Vec<DecisionServiceRun>>()
        .await
        .map_err(|error| format!("决策服务运行记录响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_decision_run_explanation(
    token: &str,
    project_id: i64,
    run_id: i64,
) -> Result<DecisionRunExplanationView, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/decision-runs/{run_id}/explanation"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取决策解释失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取决策解释").await);
    }
    response
        .json::<DecisionRunExplanationView>()
        .await
        .map_err(|error| format!("决策解释响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn generate_decision_run_explanation(
    token: &str,
    project_id: i64,
    run_id: i64,
) -> Result<DecisionRunExplanationView, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/decision-runs/{run_id}/explanation"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("生成决策解释失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "生成决策解释").await);
    }
    response
        .json::<DecisionRunExplanationView>()
        .await
        .map_err(|error| format!("决策解释响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_decision_run_explanation_reviews(
    token: &str,
    project_id: i64,
    run_id: i64,
    explanation_revision: i64,
) -> Result<Vec<DecisionRunExplanationReviewView>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/decision-runs/{run_id}/explanation/reviews?explanationRevision={explanation_revision}"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取解释审阅记录失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取解释审阅记录").await);
    }
    response
        .json::<Vec<DecisionRunExplanationReviewView>>()
        .await
        .map_err(|error| format!("解释审阅记录响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn review_decision_run_explanation(
    token: &str,
    project_id: i64,
    run_id: i64,
    explanation_revision: i64,
    review_label: &str,
) -> Result<DecisionRunExplanationReviewView, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/decision-runs/{run_id}/explanation/reviews"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&DecisionRunExplanationReviewRequest {
            review_label: review_label.to_string(),
            explanation_revision,
        })
        .send()
        .await
        .map_err(|error| format!("提交解释审阅失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "提交解释审阅").await);
    }
    response
        .json::<DecisionRunExplanationReviewView>()
        .await
        .map_err(|error| format!("解释审阅响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_decision_analytics(
    token: &str,
    project_id: i64,
) -> Result<DecisionAnalyticsReport, String> {
    let url = api_url(&format!("/api/projects/{project_id}/decision-analytics"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取运行分析失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取运行分析").await);
    }
    response
        .json::<DecisionAnalyticsReport>()
        .await
        .map_err(|error| format!("运行分析响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_decision_analytics_thresholds(
    token: &str,
    project_id: i64,
) -> Result<DecisionAnalyticsThresholds, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/decision-analytics/thresholds"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取分析阈值失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取分析阈值").await);
    }
    response
        .json::<DecisionAnalyticsThresholds>()
        .await
        .map_err(|error| format!("分析阈值响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn update_decision_analytics_thresholds(
    token: &str,
    project_id: i64,
    thresholds: &DecisionAnalyticsThresholds,
) -> Result<DecisionAnalyticsThresholds, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/decision-analytics/thresholds"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .put(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(thresholds)
        .send()
        .await
        .map_err(|error| format!("保存分析阈值失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "保存分析阈值").await);
    }
    response
        .json::<DecisionAnalyticsThresholds>()
        .await
        .map_err(|error| format!("分析阈值响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn query_decision_copilot(
    token: &str,
    project_id: i64,
    question: &str,
) -> Result<DecisionCopilotQueryResponse, String> {
    let url = api_url(&format!("/api/projects/{project_id}/copilot/query"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&DecisionCopilotQueryRequest {
            question: question.to_string(),
        })
        .send()
        .await
        .map_err(|error| format!("查询运营 Copilot 失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "查询运营 Copilot").await);
    }
    response
        .json::<DecisionCopilotQueryResponse>()
        .await
        .map_err(|error| format!("运营 Copilot 响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_drift_events(
    token: &str,
    project_id: i64,
) -> Result<Vec<DecisionDriftEvent>, String> {
    let url = api_url(&format!("/api/projects/{project_id}/drift-events"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取漂移事件失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取漂移事件").await);
    }
    response
        .json::<Vec<DecisionDriftEvent>>()
        .await
        .map_err(|error| format!("漂移事件响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_drift_scan_windows(
    token: &str,
    project_id: i64,
) -> Result<Vec<DecisionDriftScanWindow>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/drift-scan-windows?afterId=0&limit=100&newest=true"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取漂移扫描窗口失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取漂移扫描窗口").await);
    }
    response
        .json::<Vec<DecisionDriftScanWindow>>()
        .await
        .map_err(|error| format!("漂移扫描窗口响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_deviation_analyses(
    token: &str,
    project_id: i64,
) -> Result<Vec<DeviationAnalysis>, String> {
    let url = api_url(&format!("/api/projects/{project_id}/deviation-analyses"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取偏差归因抽检记录失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取偏差归因抽检记录").await);
    }
    response
        .json::<Vec<DeviationAnalysis>>()
        .await
        .map_err(|error| format!("偏差归因抽检响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn review_deviation_analysis(
    token: &str,
    project_id: i64,
    analysis_id: i64,
    review: &UpdateDeviationAnalysisReview,
) -> Result<DeviationAnalysisReviewResult, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/deviation-analyses/{analysis_id}/review"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(review)
        .send()
        .await
        .map_err(|error| format!("保存偏差归因人工判定失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "保存偏差归因人工判定").await);
    }
    response
        .json::<DeviationAnalysisReviewResult>()
        .await
        .map_err(|error| format!("偏差归因判定响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn review_drift_scan_window(
    token: &str,
    project_id: i64,
    window_id: i64,
    review: &UpdateDecisionDriftScanWindowReview,
) -> Result<DecisionDriftScanWindowReviewResult, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/drift-scan-windows/{window_id}/review"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(review)
        .send()
        .await
        .map_err(|error| format!("保存漂移窗口人工判定失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "保存漂移窗口人工判定").await);
    }
    response
        .json::<DecisionDriftScanWindowReviewResult>()
        .await
        .map_err(|error| format!("漂移窗口判定响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_drift_event_impact(
    token: &str,
    project_id: i64,
    event_id: i64,
) -> Result<DecisionDriftImpactReport, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/drift-events/{event_id}/impact"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取漂移影响报告失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取漂移影响报告").await);
    }
    response
        .json::<DecisionDriftImpactReport>()
        .await
        .map_err(|error| format!("漂移影响报告响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_drift_automation_rule(
    token: &str,
    project_id: i64,
) -> Result<Option<crate::models::DecisionDriftAutomationRule>, String> {
    let url = api_url(&format!("/api/projects/{project_id}/drift-automation"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取漂移规则失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取漂移规则").await);
    }
    response
        .json::<Option<crate::models::DecisionDriftAutomationRule>>()
        .await
        .map_err(|error| format!("漂移规则响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn update_drift_automation_rule(
    token: &str,
    project_id: i64,
    rule: &UpdateDecisionDriftAutomation,
) -> Result<crate::models::DecisionDriftAutomationRule, String> {
    let url = api_url(&format!("/api/projects/{project_id}/drift-automation"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .put(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(rule)
        .send()
        .await
        .map_err(|error| format!("保存漂移规则失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "保存漂移规则").await);
    }
    response
        .json::<crate::models::DecisionDriftAutomationRule>()
        .await
        .map_err(|error| format!("漂移规则响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn prepare_drift_event_proposals(
    token: &str,
    project_id: i64,
    event_id: i64,
) -> Result<DecisionDriftEventActionResult, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/drift-events/{event_id}/prepare-proposals"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("准备漂移提案失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "准备漂移提案").await);
    }
    response
        .json::<DecisionDriftEventActionResult>()
        .await
        .map_err(|error| format!("漂移提案动作响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn dismiss_drift_event(
    token: &str,
    project_id: i64,
    event_id: i64,
) -> Result<DecisionDriftEventActionResult, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/drift-events/{event_id}/dismiss"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("忽略漂移事件失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "忽略漂移事件").await);
    }
    response
        .json::<DecisionDriftEventActionResult>()
        .await
        .map_err(|error| format!("漂移事件响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn review_drift_event(
    token: &str,
    project_id: i64,
    event_id: i64,
    review: &UpdateDecisionDriftEventReview,
) -> Result<DecisionDriftEventReviewResult, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/drift-events/{event_id}/review"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(review)
        .send()
        .await
        .map_err(|error| format!("保存漂移事件人工判定失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "保存漂移事件人工判定").await);
    }
    response
        .json::<DecisionDriftEventReviewResult>()
        .await
        .map_err(|error| format!("漂移事件判定响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn retry_drift_webhook_delivery(
    token: &str,
    project_id: i64,
    event_id: i64,
) -> Result<DecisionDriftEventActionResult, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/drift-events/{event_id}/webhook-retry"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("重试漂移通知失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "重试漂移通知").await);
    }
    response
        .json::<DecisionDriftEventActionResult>()
        .await
        .map_err(|error| format!("漂移通知重试响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_optimization_proposals(
    token: &str,
    project_id: i64,
) -> Result<Vec<DecisionOptimizationProposal>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/optimization-proposals"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取优化建议失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取优化建议").await);
    }
    response
        .json::<Vec<DecisionOptimizationProposal>>()
        .await
        .map_err(|error| format!("优化建议响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn list_optimization_shadow_evaluations(
    token: &str,
    project_id: i64,
    after_id: i64,
    limit: usize,
) -> Result<Vec<OptimizationAutodraftShadowEvaluation>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/optimization-shadow-evaluations?afterId={after_id}&limit={limit}"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取 shadow 评估记录失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取 shadow 评估记录").await);
    }
    response
        .json::<Vec<OptimizationAutodraftShadowEvaluation>>()
        .await
        .map_err(|error| format!("shadow 评估记录响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn generate_optimization_proposals(
    token: &str,
    project_id: i64,
) -> Result<Vec<DecisionOptimizationProposal>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/optimization-proposals/generate"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("生成优化建议失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "生成优化建议").await);
    }
    response
        .json::<Vec<DecisionOptimizationProposal>>()
        .await
        .map_err(|error| format!("优化建议响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn simulate_optimization_proposal(
    token: &str,
    project_id: i64,
    proposal_id: i64,
    candidate_version_id: i64,
) -> Result<crate::models::OptimizationSimulationReport, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/optimization-proposals/{proposal_id}/simulate/{candidate_version_id}"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("模拟优化效果失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "模拟优化效果").await);
    }
    response
        .json::<crate::models::OptimizationSimulationReport>()
        .await
        .map_err(|error| format!("优化模拟响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn confirm_optimization_proposal(
    token: &str,
    project_id: i64,
    proposal_id: i64,
) -> Result<DecisionWorkspace, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/optimization-proposals/{proposal_id}/confirm"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("确认优化建议失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "确认优化建议").await);
    }
    response
        .json::<DecisionWorkspace>()
        .await
        .map_err(|error| format!("优化草稿响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn dismiss_optimization_proposal(
    token: &str,
    project_id: i64,
    proposal_id: i64,
) -> Result<DecisionOptimizationProposal, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/optimization-proposals/{proposal_id}/dismiss"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("驳回优化建议失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "驳回优化建议").await);
    }
    response
        .json::<DecisionOptimizationProposal>()
        .await
        .map_err(|error| format!("优化建议响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_decision_workflow(
    token: &str,
    project_id: i64,
) -> Result<Option<DecisionWorkflowState>, String> {
    let url = api_url(&format!("/api/projects/{project_id}/workflow"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取需求工作流失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取需求工作流").await);
    }
    response
        .json::<Option<DecisionWorkflowState>>()
        .await
        .map_err(|error| format!("需求工作流响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn get_pending_requirement_review(
    token: &str,
    project_id: i64,
) -> Result<Option<PendingRequirementReview>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/workflow/pending-review"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取人工复核候选失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取人工复核候选").await);
    }
    response
        .json::<Option<PendingRequirementReview>>()
        .await
        .map_err(|error| format!("人工复核候选响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn confirm_requirement_review(
    token: &str,
    project_id: i64,
    run_id: i64,
    model: String,
) -> Result<DecisionWorkflowState, String> {
    let url = api_url(&format!("/api/projects/{project_id}/workflow/review"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&ConfirmRequirementReviewRequest { run_id, model })
        .send()
        .await
        .map_err(|error| format!("提交人工复核失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "提交人工复核").await);
    }
    response
        .json::<DecisionWorkflowState>()
        .await
        .map_err(|error| format!("人工复核响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn replay_agent_orchestration_events(
    token: &str,
    project_id: i64,
    orchestration_id: i64,
    after_event_id: i64,
) -> Result<Vec<serde_json::Value>, String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/orchestrations/{orchestration_id}/events?afterEventId={after_event_id}"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|error| format!("读取编排事件失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, "读取编排事件").await);
    }
    response
        .json::<Vec<serde_json::Value>>()
        .await
        .map_err(|error| format!("编排事件响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn analyze_requirement(
    token: &str,
    project_id: i64,
    text: String,
) -> Result<DecisionWorkflowState, String> {
    workflow_post(
        token,
        project_id,
        "analyze",
        &AnalyzeRequirementRequest {
            text,
            retry_of_orchestration_id: None,
        },
        "需求分析",
    )
    .await
}

#[cfg(target_family = "wasm")]
pub async fn continue_workflow_clarifications(
    token: &str,
    project_id: i64,
    answers: Vec<ClarificationAnswer>,
    expected_updated_at: chrono::DateTime<chrono::Utc>,
    retry_of_orchestration_id: Option<i64>,
    controller: web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/workflow/orchestrate-stream"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let body = serde_json::to_string(&ContinueClarificationRequest {
        answers,
        expected_updated_at,
        retry_of_orchestration_id,
    })
    .map_err(|error| format!("澄清主持请求无效: {error}"))?;
    let headers = web_sys::Headers::new().map_err(js_error)?;
    headers
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(js_error)?;
    headers
        .set("Content-Type", "application/json")
        .map_err(js_error)?;
    headers
        .set("Accept", "text/event-stream")
        .map_err(js_error)?;
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_headers_headers(&headers);
    init.set_body(&JsValue::from_str(&body));
    init.set_signal(Some(&controller.signal()));
    let request = web_sys::Request::new_with_str_and_init(&url, &init).map_err(js_error)?;
    let window = web_sys::window().ok_or_else(|| "浏览器窗口不可用".to_string())?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::Response>()
        .map_err(js_error)?;
    if !response.ok() {
        let status = response.status();
        handle_auth_status(reqwest::StatusCode::from_u16(status).unwrap_or_default());
        let body = JsFuture::from(response.text().map_err(js_error)?)
            .await
            .map_err(js_error)?
            .as_string()
            .unwrap_or_default();
        let payload = serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
        let message = payload["error"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("澄清主持请求失败 ({status})"));
        if let Some(retryable) = payload["retryable"].as_bool() {
            on_event(SseEvent::RetryableFailure { message, retryable });
        } else {
            on_event(SseEvent::Error(message));
        }
        return Ok(());
    }
    read_sse_response_with_retry(response, token, project_id, &controller, &mut on_event).await
}

#[cfg(target_family = "wasm")]
pub async fn analyze_requirement_stream(
    token: &str,
    project_id: i64,
    text: String,
    retry_of_orchestration_id: Option<i64>,
    controller: web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/workflow/orchestrate-stream"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let body = serde_json::to_string(&AnalyzeRequirementRequest {
        text,
        retry_of_orchestration_id,
    })
    .map_err(|error| format!("需求分析请求无效: {error}"))?;
    let headers = web_sys::Headers::new().map_err(js_error)?;
    headers
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(js_error)?;
    headers
        .set("Content-Type", "application/json")
        .map_err(js_error)?;
    headers
        .set("Accept", "text/event-stream")
        .map_err(js_error)?;
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_headers_headers(&headers);
    init.set_body(&JsValue::from_str(&body));
    init.set_signal(Some(&controller.signal()));
    let request = web_sys::Request::new_with_str_and_init(&url, &init).map_err(js_error)?;
    let window = web_sys::window().ok_or_else(|| "浏览器窗口不可用".to_string())?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::Response>()
        .map_err(js_error)?;
    if !response.ok() {
        let status = response.status();
        if status == 401 {
            notify_auth_expired();
        }
        let body = JsFuture::from(response.text().map_err(js_error)?)
            .await
            .map_err(js_error)?
            .as_string()
            .unwrap_or_default();
        return Err(serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value["error"].as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("需求分析请求失败 ({status})")));
    }
    read_sse_response_with_retry(response, token, project_id, &controller, &mut on_event).await
}

#[cfg(target_family = "wasm")]
pub async fn create_decision_plan(
    token: &str,
    project_id: i64,
    answers: Vec<ClarificationAnswer>,
    expected_updated_at: chrono::DateTime<chrono::Utc>,
    retry_of_orchestration_id: Option<i64>,
    mut on_failure: impl FnMut(Option<i64>, bool),
) -> Result<DecisionWorkflowState, String> {
    let url = api_url(&format!("/api/projects/{project_id}/workflow/plan"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&CreateDecisionPlanRequest {
            answers,
            expected_updated_at,
            retry_of_orchestration_id,
        })
        .send()
        .await
        .map_err(|error| format!("决策方案规划请求失败: {error}"))?;
    if !response.status().is_success() {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            notify_auth_expired();
        }
        let body = response
            .json::<serde_json::Value>()
            .await
            .unwrap_or_default();
        on_failure(
            body["orchestrationId"].as_i64(),
            body["retryable"].as_bool().unwrap_or(false),
        );
        return Err(body["error"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| "决策方案规划请求失败".into()));
    }
    response
        .json::<DecisionWorkflowState>()
        .await
        .map_err(|error| format!("决策方案规划响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn create_decision_plan_stream(
    token: &str,
    project_id: i64,
    answers: Vec<ClarificationAnswer>,
    expected_updated_at: chrono::DateTime<chrono::Utc>,
    retry_of_orchestration_id: Option<i64>,
    controller: web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/workflow/orchestrate-stream"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let body = serde_json::to_string(&CreateDecisionPlanRequest {
        answers,
        expected_updated_at,
        retry_of_orchestration_id,
    })
    .map_err(|error| format!("决策方案请求无效: {error}"))?;
    let headers = web_sys::Headers::new().map_err(js_error)?;
    headers
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(js_error)?;
    headers
        .set("Content-Type", "application/json")
        .map_err(js_error)?;
    headers
        .set("Accept", "text/event-stream")
        .map_err(js_error)?;
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_headers_headers(&headers);
    init.set_body(&JsValue::from_str(&body));
    init.set_signal(Some(&controller.signal()));
    let request = web_sys::Request::new_with_str_and_init(&url, &init).map_err(js_error)?;
    let window = web_sys::window().ok_or_else(|| "浏览器窗口不可用".to_string())?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::Response>()
        .map_err(js_error)?;
    if !response.ok() {
        let status = response.status();
        handle_auth_status(reqwest::StatusCode::from_u16(status).unwrap_or_default());
        let body = JsFuture::from(response.text().map_err(js_error)?)
            .await
            .map_err(js_error)?
            .as_string()
            .unwrap_or_default();
        let payload = serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
        let message = payload["error"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("决策方案规划请求失败 ({status})"));
        if let Some(retryable) = payload["retryable"].as_bool() {
            on_event(SseEvent::RetryableFailure { message, retryable });
        } else {
            on_event(SseEvent::Error(message));
        }
        return Ok(());
    }
    read_sse_response_with_retry(response, token, project_id, &controller, &mut on_event).await
}

#[cfg(target_family = "wasm")]
pub async fn confirm_decision_plan(
    token: &str,
    project_id: i64,
    expected_updated_at: chrono::DateTime<chrono::Utc>,
) -> Result<DecisionWorkflowState, String> {
    workflow_post(
        token,
        project_id,
        "confirm",
        &ConfirmDecisionPlanRequest {
            expected_updated_at,
        },
        "确认决策方案",
    )
    .await
}

#[cfg(target_family = "wasm")]
async fn workflow_post<T: Serialize + ?Sized>(
    token: &str,
    project_id: i64,
    action: &str,
    payload: &T,
    operation: &str,
) -> Result<DecisionWorkflowState, String> {
    let url = api_url(&format!("/api/projects/{project_id}/workflow/{action}"))
        .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(payload)
        .send()
        .await
        .map_err(|error| format!("{operation}请求失败: {error}"))?;
    if !response.status().is_success() {
        return Err(project_api_error(response, operation).await);
    }
    response
        .json::<DecisionWorkflowState>()
        .await
        .map_err(|error| format!("{operation}响应无效: {error}"))
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub ok: bool,
    #[serde(alias = "nl_dsl")]
    pub dsl: Option<String>,
    pub xml: Option<String>,
    pub error: Option<String>,
    #[serde(default, rename = "changeSummary")]
    pub change_summary: Option<ChangeSummary>,
    #[serde(default, rename = "changeReview")]
    pub change_review: Option<VersionChangeReview>,
    #[serde(default, rename = "knowledgeCitations")]
    pub knowledge_citations: Vec<crate::models::KnowledgeCitation>,
    #[serde(default, rename = "testerStaticAnalysis")]
    pub tester_static_analysis: Option<serde_json::Value>,
    #[serde(default)]
    pub degradation: Option<serde_json::Value>,
    #[serde(default, rename = "simpleDirectPreview")]
    pub simple_direct_preview: bool,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSummary {
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub updated: Vec<String>,
    #[serde(default)]
    pub removed: Vec<String>,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone, Deserialize)]
pub struct EvaluateResult {
    pub ok: bool,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub decisions: Vec<EvaluatedDecision>,
    #[serde(default)]
    pub messages: Vec<EvaluationMessage>,
    #[serde(default)]
    pub trace: Option<ExecutionTrace>,
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone, Deserialize)]
pub struct EvaluatedDecision {
    pub id: String,
    pub name: String,
    pub status: String,
    pub value: String,
    #[serde(default, rename = "matchedRuleIds")]
    pub matched_rule_ids: Vec<String>,
    #[serde(default, rename = "evaluatedDependencies")]
    pub evaluated_dependencies: Vec<String>,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone, Deserialize)]
pub struct EvaluationMessage {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub decision: Option<String>,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone, Deserialize)]
pub struct ValidateDmnResult {
    pub ok: bool,
    pub summary: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone, Deserialize)]
pub struct AnalyzeInputsResult {
    pub ok: bool,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub summary: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(target_family = "wasm")]
pub async fn analyze_dmn_inputs(token: &str, xml: String) -> Result<AnalyzeInputsResult, String> {
    let url = api_url("/api/analyze-inputs").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "xml": xml }))
        .send()
        .await
        .map_err(|error| format!("入参分析请求失败: {error}"))?;
    let status = response.status();
    handle_auth_status(status);
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取入参分析结果失败: {error}"))?;
    let payload = serde_json::from_str::<serde_json::Value>(&body).map_err(|error| {
        let excerpt = body.trim().chars().take(240).collect::<String>();
        format!("入参分析服务返回无效响应 ({status}): {error}; 内容: {excerpt}")
    })?;
    if !status.is_success()
        || !payload
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    {
        return Err(payload
            .get("error")
            .and_then(serde_json::Value::as_str)
            .filter(|message| !message.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("入参分析失败 ({status})")));
    }
    serde_json::from_value::<AnalyzeInputsResult>(payload)
        .map_err(|error| format!("入参分析响应字段无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub async fn validate_dmn(token: &str, xml: String) -> Result<ValidateDmnResult, String> {
    let url = api_url("/api/validate-dmn").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "xml": xml }))
        .send()
        .await
        .map_err(|error| format!("核验请求失败: {error}"))?;
    let status = response.status();
    handle_auth_status(status);
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取核验结果失败: {error}"))?;
    let result = serde_json::from_str::<ValidateDmnResult>(&body)
        .map_err(|error| format!("核验结果解析失败: {error}"))?;
    if !status.is_success() || !result.ok {
        return Err(result
            .error
            .unwrap_or_else(|| format!("核验失败 ({status})")));
    }
    Ok(result)
}

#[cfg(target_family = "wasm")]
pub async fn compile_dsl(token: &str, dsl: String) -> Result<String, String> {
    let url = api_url("/api/compile").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "dsl": dsl }))
        .send()
        .await
        .map_err(|error| format!("DSL 编译请求失败: {error}"))?;
    let status = response.status();
    handle_auth_status(status);
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取编译结果失败: {error}"))?;
    let payload = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|error| format!("编译结果解析失败: {error}"))?;
    if !status.is_success()
        || !payload
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    {
        return Err(payload
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("DSL 编译失败")
            .to_string());
    }
    payload
        .get("xml")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "编译结果缺少 XML".into())
}

#[cfg(target_family = "wasm")]
pub async fn evaluate_dmn(
    token: &str,
    xml: String,
    inputs: String,
) -> Result<EvaluateResult, String> {
    let url = api_url("/api/evaluate").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let inputs = serde_json::from_str::<serde_json::Value>(&inputs)
        .map_err(|error| format!("测试输入 JSON 无效: {error}"))?;
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "xml": xml, "inputs": inputs }))
        .send()
        .await
        .map_err(|error| format!("测试请求失败: {error}"))?;
    let status = response.status();
    handle_auth_status(status);
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取测试结果失败: {error}"))?;
    let payload = serde_json::from_str::<serde_json::Value>(&body).map_err(|error| {
        let excerpt = body.trim().chars().take(240).collect::<String>();
        format!("测试服务返回无效响应 ({status}): {error}; 内容: {excerpt}")
    })?;
    if !status.is_success()
        || !payload
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    {
        return Err(payload
            .get("error")
            .and_then(serde_json::Value::as_str)
            .filter(|message| !message.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("测试失败 ({status})")));
    }
    serde_json::from_value::<EvaluateResult>(payload)
        .map_err(|error| format!("测试结果字段无效: {error}"))
}

#[cfg(target_family = "wasm")]
pub enum SseEvent {
    Orchestration(i64),
    Agent(serde_json::Value),
    Artifact(serde_json::Value),
    Handoff(serde_json::Value),
    Stage(serde_json::Value),
    Step(String),
    Delta(String),
    Preview(String),
    Result(RunResult),
    ChangeIntent(ModelChangeIntent),
    WorkflowResult(DecisionWorkflowState),
    Error(String),
    RetryableFailure { message: String, retryable: bool },
}

#[cfg(target_family = "wasm")]
fn js_error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("浏览器请求失败: {value:?}"))
}

fn is_terminal_sse_event_name(event_name: &str) -> bool {
    matches!(event_name, "result" | "workflow_result" | "error")
}

fn parse_sse_event_id(raw: &str) -> Option<i64> {
    raw.lines()
        .find_map(|line| line.strip_prefix("id:"))
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|id| *id > 0)
}

fn parse_sse_orchestration_id(raw: &str) -> Option<i64> {
    let event_name = raw
        .lines()
        .find_map(|line| line.strip_prefix("event:"))?
        .trim();
    if event_name != "orchestration" {
        return None;
    }
    let data = raw
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str::<serde_json::Value>(&data)
        .ok()?
        .get("id")?
        .as_i64()
}

fn finish_sse_stream(aborted: bool, terminal_received: bool) -> Result<(), String> {
    if aborted || terminal_received {
        Ok(())
    } else {
        Err("SSE 连接在收到终态结果前中断；可重试当前阶段".into())
    }
}

fn is_retryable_sse_disconnect(message: &str) -> bool {
    message == "SSE 连接在收到终态结果前中断；可重试当前阶段"
}

#[cfg(target_family = "wasm")]
fn dispatch_sse_event(raw: &str, on_event: &mut impl FnMut(SseEvent)) -> Result<bool, String> {
    let mut event_name = "message";
    let mut data = String::new();
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_name = value.trim();
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start());
        }
    }
    if data.is_empty() {
        return Ok(false);
    }
    let payload: serde_json::Value =
        serde_json::from_str(&data).map_err(|e| format!("SSE 事件格式无效: {e}"))?;
    let terminal = is_terminal_sse_event_name(event_name);
    match event_name {
        "orchestration" => {
            if let Some(id) = payload["id"].as_i64() {
                on_event(SseEvent::Orchestration(id));
            }
        }
        "agent" => on_event(SseEvent::Agent(payload)),
        "artifact" => on_event(SseEvent::Artifact(payload)),
        "handoff" => on_event(SseEvent::Handoff(payload)),
        "stage" => on_event(SseEvent::Stage(payload)),
        "step" => on_event(SseEvent::Step(
            payload["message"].as_str().unwrap_or_default().to_string(),
        )),
        "delta" => on_event(SseEvent::Delta(
            payload["content"].as_str().unwrap_or_default().to_string(),
        )),
        "preview" => on_event(SseEvent::Preview(
            payload["content"].as_str().unwrap_or_default().to_string(),
        )),
        "result" if payload.get("changeIntent").is_some() => on_event(SseEvent::ChangeIntent(
            serde_json::from_value(payload["changeIntent"].clone())
                .map_err(|e| format!("变更意图结果事件无效: {e}"))?,
        )),
        "result" => on_event(SseEvent::Result(
            serde_json::from_value(payload).map_err(|e| format!("结果事件无效: {e}"))?,
        )),
        "workflow_result" => on_event(SseEvent::WorkflowResult(
            serde_json::from_value(payload).map_err(|e| format!("需求分析结果事件无效: {e}"))?,
        )),
        "error" => {
            let message = payload["error"]
                .as_str()
                .unwrap_or("流水线失败")
                .to_string();
            if let Some(retryable) = payload["retryable"].as_bool() {
                on_event(SseEvent::RetryableFailure { message, retryable });
            } else {
                on_event(SseEvent::Error(message));
            }
        }
        _ => {}
    }
    Ok(terminal)
}

#[cfg(target_family = "wasm")]
async fn read_sse_response(
    response: web_sys::Response,
    controller: &web_sys::AbortController,
    last_event_id: &mut i64,
    orchestration_id: &mut Option<i64>,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let stream = response
        .body()
        .ok_or_else(|| "浏览器未提供响应流".to_string())?;
    let reader = web_sys::ReadableStreamDefaultReader::new(&stream).map_err(js_error)?;
    let decoder = web_sys::TextDecoder::new().map_err(js_error)?;
    let options = web_sys::TextDecodeOptions::new();
    options.set_stream(true);
    let mut buffer = String::new();
    let mut terminal_received = false;

    loop {
        if controller.signal().aborted() {
            return Ok(());
        }
        let result = JsFuture::from(reader.read()).await.map_err(js_error)?;
        if Reflect::get(&result, &JsValue::from_str("done"))
            .map_err(js_error)?
            .as_bool()
            .unwrap_or(false)
        {
            break;
        }
        let value = Reflect::get(&result, &JsValue::from_str("value")).map_err(js_error)?;
        if value.is_undefined() || value.is_null() {
            continue;
        }
        let bytes = Uint8Array::new(&value);
        buffer.push_str(
            &decoder
                .decode_with_js_u8_array_and_options(&bytes, &options)
                .map_err(js_error)?,
        );
        buffer = buffer.replace("\r\n", "\n");

        while let Some(end) = buffer.find("\n\n") {
            let event = buffer[..end].to_string();
            buffer.drain(..end + 2);
            if let Some(id) = parse_sse_event_id(&event) {
                *last_event_id = id;
            }
            if let Some(id) = parse_sse_orchestration_id(&event) {
                *orchestration_id = Some(id);
            }
            terminal_received |= dispatch_sse_event(&event, &mut on_event)?;
        }
    }
    buffer.push_str(&decoder.decode().map_err(js_error)?);
    if !buffer.trim().is_empty() {
        if let Some(id) = parse_sse_event_id(&buffer) {
            *last_event_id = id;
        }
        if let Some(id) = parse_sse_orchestration_id(&buffer) {
            *orchestration_id = Some(id);
        }
        terminal_received |= dispatch_sse_event(&buffer, &mut on_event)?;
    }
    finish_sse_stream(controller.signal().aborted(), terminal_received)
}

#[cfg(target_family = "wasm")]
async fn read_sse_response_with_retry(
    response: web_sys::Response,
    token: &str,
    project_id: i64,
    controller: &web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let mut last_event_id = 0;
    let mut orchestration_id = response
        .headers()
        .get("x-orchestration-id")
        .ok()
        .flatten()
        .and_then(|id| id.parse::<i64>().ok());
    match read_sse_response(
        response,
        controller,
        &mut last_event_id,
        &mut orchestration_id,
        &mut on_event,
    )
    .await
    {
        Err(message) if !controller.signal().aborted() && is_retryable_sse_disconnect(&message) => {
            if let Some(orchestration_id) = orchestration_id {
                match replay_sse_events(
                    token,
                    project_id,
                    orchestration_id,
                    &mut last_event_id,
                    controller,
                    &mut on_event,
                )
                .await
                {
                    Ok(true) => return Ok(()),
                    Ok(false) => {}
                    Err(error) => {
                        on_event(SseEvent::RetryableFailure {
                            message: format!("{message}；读取已保存进度失败：{error}"),
                            retryable: true,
                        });
                        return Ok(());
                    }
                }
            }
            on_event(SseEvent::RetryableFailure {
                message: if orchestration_id.is_some() {
                    format!("{message}；已恢复可读取的进度，当前阶段可重试")
                } else {
                    message
                },
                retryable: true,
            });
            Ok(())
        }
        result => result,
    }
}

#[cfg(target_family = "wasm")]
async fn replay_sse_events(
    token: &str,
    project_id: i64,
    orchestration_id: i64,
    last_event_id: &mut i64,
    controller: &web_sys::AbortController,
    on_event: &mut impl FnMut(SseEvent),
) -> Result<bool, String> {
    loop {
        if controller.signal().aborted() {
            return Ok(false);
        }
        let events =
            replay_agent_orchestration_events(token, project_id, orchestration_id, *last_event_id)
                .await?;
        if events.is_empty() {
            return Ok(false);
        }
        let batch_len = events.len();
        for event in events {
            let Some(id) = event["id"].as_i64() else {
                continue;
            };
            let Some(name) = event["eventName"].as_str() else {
                continue;
            };
            if id <= *last_event_id {
                continue;
            }
            let raw = format!("id: {id}\nevent: {name}\ndata: {}\n\n", event["payload"]);
            *last_event_id = id;
            if dispatch_sse_event(&raw, on_event)? {
                return Ok(true);
            }
        }
        if batch_len < 500 {
            return Ok(false);
        }
    }
}

/// Run the authenticated pipeline and forward each SSE event as soon as it arrives.
#[cfg(target_family = "wasm")]
pub async fn run_pipeline(
    token: &str,
    project_id: i64,
    _text: String,
    _existing_dsl: Option<String>,
    _base_version: Option<i32>,
    workflow_updated_at: chrono::DateTime<chrono::Utc>,
    retry_of_orchestration_id: Option<i64>,
    controller: web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/workflow/orchestrate-stream"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let body = serde_json::to_string(&serde_json::json!({
        "expectedUpdatedAt": workflow_updated_at,
        "retryOfOrchestrationId": retry_of_orchestration_id,
    }))
    .map_err(|e| e.to_string())?;
    let headers = web_sys::Headers::new().map_err(js_error)?;
    headers
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(js_error)?;
    headers
        .set("Content-Type", "application/json")
        .map_err(js_error)?;
    headers
        .set("Accept", "text/event-stream")
        .map_err(js_error)?;

    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_headers_headers(&headers);
    init.set_body(&JsValue::from_str(&body));
    init.set_signal(Some(&controller.signal()));
    let request = web_sys::Request::new_with_str_and_init(&url, &init).map_err(js_error)?;
    let window = web_sys::window().ok_or_else(|| "浏览器窗口不可用".to_string())?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::Response>()
        .map_err(js_error)?;

    if !response.ok() {
        let status = response.status();
        if status == 401 {
            notify_auth_expired();
        }
        let body = JsFuture::from(response.text().map_err(js_error)?)
            .await
            .map_err(js_error)?
            .as_string()
            .unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("请求失败 ({status})"));
        on_event(SseEvent::Error(message));
        return Ok(());
    }
    read_sse_response_with_retry(response, token, project_id, &controller, on_event).await
}

#[cfg(not(target_family = "wasm"))]
async fn validate_dsl(body: Bytes) -> (StatusCode, Json<Value>) {
    let req: DslRequest = match parse_json_body(body).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    match nl_dsl::parse_definitions(&req.dsl) {
        Ok(defs) => match nl_dsl::validate(&defs) {
            Ok(_) => (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "summary": {
                        "name": defs.name,
                        "decisions": defs.decisions.len(),
                        "inputs": defs.inputs.len(),
                        "bkms": defs.bkms.len(),
                        "itemDefinitions": defs.item_definitions.len()
                    }
                })),
            ),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": e.to_string() })),
            ),
        },
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

#[cfg(not(target_family = "wasm"))]
async fn compile_dsl(body: Bytes) -> (StatusCode, Json<Value>) {
    let req: DslRequest = match parse_json_body(body).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let defs = match nl_dsl::parse_definitions(&req.dsl) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": e.to_string() })),
            );
        }
    };
    if let Err(e) = nl_dsl::validate(&defs) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e.to_string() })),
        );
    }
    match dmn::generate_dmn_xml(&defs) {
        Ok(xml) => (StatusCode::OK, Json(json!({ "ok": true, "xml": xml }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

#[cfg(not(target_family = "wasm"))]
async fn evaluate_dmn(body: Bytes) -> (StatusCode, Json<Value>) {
    let req: EvaluateRequest = match parse_json_body(body).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    if req.xml.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "DMN XML 不能为空" })),
        );
    }
    let model = match executable_dmn::parse_dmn(&req.xml) {
        Ok(model) => model,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": format!("DMN XML 解析失败: {error}") })),
            );
        }
    };
    if let Err(error) = executable_dmn::compile_dmn(&model) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": format!("DMN 模型核验失败: {error}") })),
        );
    }
    // Convert against the parsed model's input declarations. In particular,
    // temporal JSON strings need FEEL temporal values, while booleans must
    // remain FEEL booleans instead of being treated as strings.
    let inputs = match typed_json_inputs_to_feel(&model, &req.inputs) {
        Ok(inputs) => inputs,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": format!("测试输入无效: {error}") })),
            );
        }
    };
    let result = executable_dmn::DmnExecutor::new(&model).evaluate_all(inputs);
    let trace = build_execution_trace(&model, None, None, &req.inputs, &result);
    let decisions = result
        .decision_results
        .iter()
        .map(|decision| {
            json!({
                "id": decision.decision_id,
                "name": decision.decision_name,
                "status": format!("{:?}", decision.status),
                "value": decision.value.to_feel_string().unwrap_or_else(|_| "<不可序列化>".to_string()),
                "matchedRuleIds": decision.matched_rule_ids,
                "evaluatedDependencies": decision.evaluated_dependencies,
            })
        })
        .collect::<Vec<_>>();
    let messages = result
        .messages
        .iter()
        .map(|message| {
            json!({
                "severity": format!("{:?}", message.severity),
                "code": message.code,
                "message": message.message,
                "decision": message.source.decision_name,
            })
        })
        .collect::<Vec<_>>();
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "status": format!("{:?}", result.status),
            "decisions": decisions,
            "messages": messages,
            "trace": trace,
        })),
    )
}

#[cfg(not(target_family = "wasm"))]
async fn validate_dmn(body: Bytes) -> (StatusCode, Json<Value>) {
    let req: XmlRequest = match parse_json_body(body).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    if req.xml.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "DMN XML 不能为空" })),
        );
    }
    let model = match executable_dmn::parse_dmn(&req.xml) {
        Ok(model) => model,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": format!("DMN XML 解析失败: {error}") })),
            );
        }
    };
    if let Err(error) = executable_dmn::compile_dmn(&model) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": format!("DMN 模型核验失败: {error}") })),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "summary": {
                "name": model.name,
                "decisions": model.decisions.len(),
                "inputs": model.input_data.len(),
                "businessKnowledgeModels": model.business_knowledge_models.len(),
                "itemDefinitions": model.item_definitions.len(),
                "decisionServices": model.decision_services.len(),
            }
        })),
    )
}

#[cfg(not(target_family = "wasm"))]
async fn analyze_dmn_inputs(body: Bytes) -> (StatusCode, Json<Value>) {
    let req: XmlRequest = match parse_json_body(body).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    if req.xml.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "template": "{}",
                "error": "DMN XML 不能为空"
            })),
        );
    }
    let model = match executable_dmn::parse_dmn(&req.xml) {
        Ok(model) => model,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(
                    json!({ "ok": false, "template": "{}", "error": format!("DMN XML 解析失败: {error}") }),
                ),
            );
        }
    };
    if let Err(error) = executable_dmn::compile_dmn(&model) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "ok": false, "template": "{}", "error": format!("DMN 模型核验失败: {error}") }),
            ),
        );
    }
    match analyze_model_inputs(&model) {
        Ok(analysis) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "template": serde_json::to_string_pretty(&Value::Object(analysis.template)).unwrap_or_else(|_| "{}".to_string()),
                "summary": {
                    "name": model.name,
                    "decisions": model.decisions.len(),
                    "inputs": model.input_data.len(),
                    "businessKnowledgeModels": model.business_knowledge_models.len(),
                    "itemDefinitions": model.item_definitions.len(),
                    "decisionServices": model.decision_services.len(),
                },
            })),
        ),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "template": "{}", "error" : error })),
        ),
    }
}

#[cfg(not(target_family = "wasm"))]
fn analyze_model_inputs(model: &executable_dmn::DmnModel) -> Result<DmnInputsAnalysis, String> {
    let mut template = serde_json::Map::new();
    for input in &model.input_data {
        // DmnExecutor validates inputs against variable.name when present; this must be
        // the JSON key shown to users, not only the inputData display name.
        let key = input.variable.clone().unwrap_or_else(|| input.name.clone());
        if template.contains_key(&key) {
            return Err(format!("DMN 输入执行键重复: '{key}'"));
        }
        template.insert(
            key.clone(),
            dmn_input_value(model, input.type_ref.as_deref(), &mut Vec::new())?,
        );
    }
    Ok(DmnInputsAnalysis { template })
}

#[cfg(not(target_family = "wasm"))]
fn dmn_input_value(
    model: &executable_dmn::DmnModel,
    type_ref: Option<&str>,
    visiting: &mut Vec<String>,
) -> Result<Value, String> {
    let Some(type_ref) = type_ref.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Value::String(String::new()));
    };
    let builtin = type_ref
        .strip_prefix("feel:")
        .or_else(|| type_ref.strip_prefix("feel."))
        .unwrap_or(type_ref);
    match builtin {
        // Keep generated templates executable for common ratio expressions.
        // User-supplied zero values are still validated/evaluated normally;
        // this only changes the automatic placeholder shown by the UI.
        "number" => return Ok(json!(1)),
        "boolean" => return Ok(json!(false)),
        "string" | "date" | "time" | "date and time" | "dateTime" | "duration"
        | "yearMonthDuration" | "dayTimeDuration" | "Any" | "any" => {
            return Ok(Value::String(String::new()));
        }
        "list" => return Ok(Value::Array(Vec::new())),
        _ => {}
    }
    let item = model
        .item_definitions
        .iter()
        .find(|item| item.name == type_ref)
        .ok_or_else(|| format!("input typeRef '{type_ref}' 未在模型中定义"))?;
    if visiting.iter().any(|name| name == &item.name) {
        return Err(format!(
            "数据类型 '{}' 存在循环引用，无法生成输入模板",
            item.name
        ));
    }
    visiting.push(item.name.clone());
    let value = if item.is_collection {
        Value::Array(Vec::new())
    } else if item.item_components.is_empty() {
        dmn_input_value(model, item.type_ref.as_deref(), visiting)?
    } else {
        let fields = item
            .item_components
            .iter()
            .map(|field| Ok((field.name.clone(), dmn_item_value(model, field, visiting)?)))
            .collect::<Result<serde_json::Map<_, _>, String>>()?;
        Value::Object(fields)
    };
    visiting.pop();
    Ok(value)
}

#[cfg(not(target_family = "wasm"))]
fn dmn_item_value(
    model: &executable_dmn::DmnModel,
    item: &executable_dmn::ItemDefinition,
    visiting: &mut Vec<String>,
) -> Result<Value, String> {
    if item.is_collection {
        return Ok(Value::Array(Vec::new()));
    }
    if item.item_components.is_empty() {
        return dmn_input_value(model, item.type_ref.as_deref(), visiting);
    }
    let fields = item
        .item_components
        .iter()
        .map(|field| Ok((field.name.clone(), dmn_item_value(model, field, visiting)?)))
        .collect::<Result<serde_json::Map<_, _>, String>>()?;
    Ok(Value::Object(fields))
}

#[cfg(not(target_family = "wasm"))]
async fn ensure_generation_snapshot(
    db: &sqlx::PgPool,
    request: &GenerateRequest,
) -> Result<(), String> {
    let current = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM decision_workflows workflow JOIN decision_projects project ON project.id = workflow.project_id WHERE workflow.project_id = $1 AND workflow.updated_at = $2 AND workflow.base_revision IS NOT DISTINCT FROM project.current_draft_version_id AND ((NOT $3 AND workflow.status = 'confirmed' AND workflow.decision_plan_json IS NOT NULL) OR ($3 AND workflow.status = 'analyzed' AND workflow.decision_brief_json->>'riskLevel' = 'low' AND jsonb_array_length(workflow.clarifications_json) = 0 AND workflow.base_revision IS NULL AND project.current_draft_version_id IS NULL)))",
    )
    .bind(request.project_id)
    .bind(request.workflow_updated_at)
    .bind(request.simple_direct)
    .fetch_one(db)
    .await
    .map_err(|error| {
        eprintln!("核对生成任务工作流失败: {error}");
        "无法核对生成任务，请刷新后重试".to_string()
    })?;
    if !current {
        return Err(
            "生成期间需求、方案或模型基线已变化，本次结果已作废，请刷新并重新确认方案".into(),
        );
    }
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
async fn apply_confirmed_workflow(
    state: &AppState,
    claims: &Claims,
    request: &mut GenerateRequest,
    base_revision: Option<i64>,
) -> Result<(Option<i64>, crate::workflow::DecisionPlan), Response> {
    let project_id = request.project_id.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "生成模型必须选择项目并确认决策方案"})),
        )
            .into_response()
    })?;
    let (prompt, parent_orchestration_id, plan) = if request.simple_direct {
        crate::workflow_api::simple_direct_generation_prompt(
            state,
            project_id,
            &claims.sub,
            base_revision,
            request.workflow_updated_at,
        )
        .await
    } else {
        crate::workflow_api::confirmed_generation_prompt(
            state,
            project_id,
            &claims.sub,
            base_revision,
            request.workflow_updated_at,
        )
        .await
    }
    .map_err(IntoResponse::into_response)?;
    request.text = prompt;
    Ok((parent_orchestration_id, plan))
}

#[cfg(not(target_family = "wasm"))]
fn apply_requirement_traceability(
    dsl: &str,
    plan: &crate::workflow::DecisionPlan,
) -> Result<String, String> {
    let mut definitions = nl_dsl::parse_definitions(dsl)
        .map_err(|error| format!("生成 DSL 的需求溯源解析失败: {error}"))?;
    let planned = plan
        .decisions
        .iter()
        .map(|decision| (decision.name.trim().to_lowercase(), decision))
        .collect::<std::collections::HashMap<_, _>>();
    let mut matched = std::collections::HashSet::new();
    for decision in &mut definitions.decisions {
        if let Some(planned_decision) = planned.get(&decision.name.trim().to_lowercase()) {
            if planned_decision.requirement_ids.is_empty() {
                return Err(format!("方案决策 '{}' 没有关联需求", planned_decision.name));
            }
            decision.requirement_ids = planned_decision.requirement_ids.clone();
            apply_rule_requirement_ids(
                &mut decision.logic,
                &decision.requirement_ids,
                &decision.name,
            )?;
            matched.insert(planned_decision.name.trim().to_lowercase());
        }
    }
    let missing = plan
        .decisions
        .iter()
        .filter(|decision| !matched.contains(&decision.name.trim().to_lowercase()))
        .map(|decision| decision.name.as_str())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "生成 DSL 未包含已确认方案决策，无法建立需求溯源: {}",
            missing.join("、")
        ));
    }
    nl_dsl::validate(&definitions).map_err(|error| format!("需求溯源 DSL 校验失败: {error}"))?;
    serde_yaml::to_string(&definitions).map_err(|error| format!("需求溯源 DSL 序列化失败: {error}"))
}

#[cfg(not(target_family = "wasm"))]
fn apply_rule_requirement_ids(
    expression: &mut nl_dsl::BoxedExpression,
    requirement_ids: &[String],
    decision_name: &str,
) -> Result<(), String> {
    use nl_dsl::BoxedExpression;
    match expression {
        BoxedExpression::DecisionTable { rules, .. } => {
            for (index, rule) in rules.iter_mut().enumerate() {
                if rule.requirement_ids.is_empty() {
                    rule.requirement_ids = requirement_ids.to_vec();
                    continue;
                }
                let mut seen = std::collections::HashSet::new();
                for id in &rule.requirement_ids {
                    if !requirement_ids.contains(id) {
                        return Err(format!(
                            "决策 '{}' 的第 {} 条规则引用了未由方案确认的需求 ID '{}'",
                            decision_name,
                            index + 1,
                            id
                        ));
                    }
                    if !seen.insert(id) {
                        return Err(format!(
                            "决策 '{}' 的第 {} 条规则重复引用需求 ID '{}'",
                            decision_name,
                            index + 1,
                            id
                        ));
                    }
                }
            }
        }
        BoxedExpression::Context { entries, .. } => {
            for entry in entries {
                apply_rule_requirement_ids(&mut entry.value, requirement_ids, decision_name)?;
            }
        }
        BoxedExpression::FunctionDefinition { body, .. } => {
            apply_rule_requirement_ids(body, requirement_ids, decision_name)?;
        }
        BoxedExpression::Conditional { then, else_, .. } => {
            apply_rule_requirement_ids(then, requirement_ids, decision_name)?;
            apply_rule_requirement_ids(else_, requirement_ids, decision_name)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod requirement_traceability_tests {
    use super::apply_requirement_traceability;
    use crate::workflow::{DecisionPlan, PlannedDecision};

    fn plan() -> DecisionPlan {
        DecisionPlan {
            name: "eligibility-model".into(),
            summary: String::new(),
            decisions: vec![PlannedDecision {
                name: "Eligibility".into(),
                purpose: "Determine eligibility".into(),
                output_type: "string".into(),
                expression_type: "decisionTable".into(),
                dependencies: vec![],
                requirement_ids: vec!["req-goal-a1".into(), "req-rule-b2".into()],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn confirmed_plan_overwrites_model_trace_ids_and_applies_them_to_rules() {
        let dsl = r#"
name: eligibility-model
inputs:
  - name: age
    typeRef: number
decisions:
  - name: Eligibility
    typeRef: string
    requirementIds: [req-model-made-up]
    logic:
      type: decisionTable
      inputs:
        - expression: age
      outputs:
        - name: result
          typeRef: string
      rules:
        - inputEntries: [">= 18"]
          outputEntries: ['"eligible"']
"#;
        let traced = apply_requirement_traceability(dsl, &plan()).unwrap();
        let definitions = crate::nl_dsl::parse_definitions(&traced).unwrap();
        assert_eq!(
            definitions.decisions[0].requirement_ids,
            ["req-goal-a1", "req-rule-b2"]
        );
        let crate::nl_dsl::BoxedExpression::DecisionTable { rules, .. } =
            &definitions.decisions[0].logic
        else {
            panic!("expected decision table");
        };
        assert_eq!(rules[0].requirement_ids, ["req-goal-a1", "req-rule-b2"]);
    }

    #[test]
    fn rule_traceability_can_narrow_to_a_confirmed_requirement() {
        let dsl = r#"
name: eligibility-model
decisions:
  - name: Eligibility
    typeRef: string
    logic:
      type: decisionTable
      outputs:
        - name: result
          typeRef: string
      rules:
        - inputEntries: []
          outputEntries: ['"eligible"']
          requirementIds: [req-rule-b2]
"#;
        let traced = apply_requirement_traceability(dsl, &plan()).unwrap();
        let definitions = crate::nl_dsl::parse_definitions(&traced).unwrap();
        let crate::nl_dsl::BoxedExpression::DecisionTable { rules, .. } =
            &definitions.decisions[0].logic
        else {
            panic!("expected decision table");
        };
        assert_eq!(rules[0].requirement_ids, ["req-rule-b2"]);
    }

    #[test]
    fn rule_traceability_rejects_unconfirmed_requirement_ids() {
        let dsl = r#"
name: eligibility-model
decisions:
  - name: Eligibility
    typeRef: string
    logic:
      type: decisionTable
      outputs:
        - name: result
          typeRef: string
      rules:
        - inputEntries: []
          outputEntries: ['"eligible"']
          requirementIds: [req-unknown]
"#;
        let error = apply_requirement_traceability(dsl, &plan()).unwrap_err();
        assert!(error.contains("req-unknown"));
    }

    #[test]
    fn traceability_rejects_a_plan_decision_missing_from_generated_model() {
        let error =
            apply_requirement_traceability("name: empty\ndecisions: []\n", &plan()).unwrap_err();
        assert!(error.contains("Eligibility"));
    }
}

#[cfg(not(target_family = "wasm"))]
#[derive(Clone, Copy)]
struct AgentExecutionRun {
    run_id: Option<i64>,
    orchestration_id: Option<i64>,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Clone, serde::Serialize)]
struct DslVoteCandidate {
    signature: String,
    #[serde(skip_serializing)]
    yaml: String,
}

#[cfg(not(target_family = "wasm"))]
struct DslVoteResult {
    generated: Option<nl_dsl::agent::GeneratedDsl>,
    consensus: Consensus<DslVoteCandidate>,
}

#[cfg(not(target_family = "wasm"))]
async fn relay_dsl_model_steps(
    model: String,
    mut model_rx: mpsc::Receiver<StreamEvent>,
    progress_tx: mpsc::Sender<StreamEvent>,
) {
    while let Some(event) = model_rx.recv().await {
        if let StreamEvent::Step(message) = event {
            let _ = progress_tx
                .send(StreamEvent::Step(format!("模型 {model}：{message}")))
                .await;
        }
    }
}

#[cfg(not(target_family = "wasm"))]
async fn generate_dsl_vote_candidate(
    agent: &LlmAgent,
    model: &str,
    requirement: &str,
    plan: &crate::workflow::DecisionPlan,
    tx: Option<&mpsc::Sender<StreamEvent>>,
) -> Result<DslVoteCandidate, String> {
    if let Some(tx) = tx {
        let _ = tx
            .send(StreamEvent::Step(format!("模型 {model} 已开始生成 DSL")))
            .await;
    }

    let generation = async {
        let generated = if let Some(progress_tx) = tx {
            let (model_tx, model_rx) = mpsc::channel(32);
            let relay = tokio::spawn(relay_dsl_model_steps(
                model.to_string(),
                model_rx,
                progress_tx.clone(),
            ));
            let result = agent
                .generate_dsl_stream(requirement, None, &model_tx)
                .await;
            drop(model_tx);
            let _ = relay.await;
            result.map_err(|error| error.to_string())?
        } else {
            agent
                .generate_dsl(requirement, None)
                .await
                .map_err(|error| error.to_string())?
        };
        let yaml = apply_requirement_traceability(&generated.yaml, plan)?;
        canonical_dsl_vote_candidate(yaml)
    };
    tokio::pin!(generation);

    loop {
        tokio::select! {
            result = &mut generation => return result,
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                if let Some(tx) = tx {
                    let _ = tx
                        .send(StreamEvent::Step(format!("模型 {model} 仍在生成或校验 DSL")))
                        .await;
                }
            }
        }
    }
}

#[cfg(not(target_family = "wasm"))]
async fn confirmed_requirement_complexity(
    db: &sqlx::PgPool,
    request: &GenerateRequest,
) -> Result<RequirementComplexity, String> {
    if request.project_id.is_none() {
        return Ok(classify_requirement_complexity(&request.text));
    }
    let requirement = sqlx::query_scalar::<_, String>(
        "SELECT requirement_text FROM decision_workflows WHERE project_id = $1 AND updated_at = $2 AND ((NOT $3 AND status = 'confirmed') OR ($3 AND status = 'analyzed' AND decision_brief_json->>'riskLevel' = 'low' AND jsonb_array_length(clarifications_json) = 0 AND base_revision IS NULL))",
    )
    .bind(request.project_id)
    .bind(request.workflow_updated_at)
    .bind(request.simple_direct)
    .fetch_optional(db)
    .await
    .map_err(|error| {
        eprintln!("读取已确认需求复杂度失败: {error}");
        "无法读取已确认需求，请刷新后重试".to_string()
    })?
    .ok_or_else(|| "已确认方案已变化，请刷新工作流后重试".to_string())?;
    let complexity = classify_requirement_complexity(&requirement);
    if request.simple_direct && complexity != RequirementComplexity::Simple {
        return Err("仅低风险 simple 需求可以跳过方案确认直接生成候选模型".into());
    }
    Ok(complexity)
}

#[cfg(not(target_family = "wasm"))]
async fn generate_complex_dsl_consensus(
    agent: &LlmAgent,
    requirement: &str,
    plan: &crate::workflow::DecisionPlan,
    tx: Option<&mpsc::Sender<StreamEvent>>,
) -> anyhow::Result<DslVoteResult> {
    let model_agents = agent.parallel_variants(3);
    let candidates = model_agents
        .iter()
        .map(|candidate| VoteCandidate {
            model: candidate.model().to_string(),
            weight: 1.0,
        })
        .collect::<Vec<_>>();
    if candidates.len() < 2 {
        anyhow::bail!(
            "复杂需求的 DSL 生成需要至少两个不同模型；当前模型配置不足，请补充 fallback 模型或转人工处理"
        );
    }
    if let Some(tx) = tx {
        let _ = tx
            .send(StreamEvent::Step(format!(
                "复杂需求路由：正在并行调用 {} 个模型；达到严格多数后将提前返回",
                candidates.len()
            )))
            .await;
    }
    let requirement = requirement.to_string();
    let plan = plan.clone();
    let progress_tx = tx.cloned();
    let completed_candidates = Arc::new(std::sync::Mutex::new(Vec::new()));
    let candidate_sink = Arc::clone(&completed_candidates);
    let voting = parallel_vote_until_majority_by(
        candidates,
        move |model| {
            let agent = model_agents
                .iter()
                .find(|agent| agent.model() == model)
                .cloned();
            let requirement = requirement.clone();
            let plan = plan.clone();
            let progress_tx = progress_tx.clone();
            let candidate_sink = Arc::clone(&candidate_sink);
            async move {
                let result = match agent {
                    Some(agent) => {
                        generate_dsl_vote_candidate(
                            &agent,
                            &model,
                            &requirement,
                            &plan,
                            progress_tx.as_ref(),
                        )
                        .await
                    }
                    None => Err(format!("并行模型不存在：{model}")),
                };
                if let Ok(candidate) = &result
                    && let Ok(mut completed) = candidate_sink.lock()
                {
                    completed.push((model.clone(), candidate.clone()));
                }
                if let Some(tx) = progress_tx {
                    let status = match &result {
                        Ok(_) => "已完成 DSL / DMN 编译及需求追溯校验".to_string(),
                        Err(error) => format!(
                            "DSL 生成或校验失败：{}",
                            dsl_vote_diagnostic_excerpt(error, 1_000)
                        ),
                    };
                    let _ = tx
                        .send(StreamEvent::Step(format!("模型 {model} {status}")))
                        .await;
                }
                result
            }
        },
        |candidate: &DslVoteCandidate| Ok(candidate.signature.clone()),
    );
    let consensus = voting.await?;
    let plurality = if consensus.decision.is_none() {
        let candidates = completed_candidates
            .lock()
            .map(|candidates| candidates.clone())
            .unwrap_or_default();
        plurality_dsl_candidate(&consensus.outcomes, &candidates)
    } else {
        None
    };
    if let Some(tx) = tx {
        let message = if consensus.early_majority {
            format!(
                "已获得严格多数 DSL 共识（{:.0}%），停止等待未完成模型",
                consensus.confidence * 100.0
            )
        } else if consensus.decision.is_some() {
            "所有模型均已返回，DSL 共识已达成".to_string()
        } else if plurality.is_some() {
            "DSL 未形成严格多数；将选择最高票且通过编译的候选（平票按配置顺序），进入业务验收并重点复核模型分歧".to_string()
        } else {
            dsl_vote_failure_message(&consensus.outcomes)
        };
        let _ = tx.send(StreamEvent::Step(message)).await;
    }
    let generated = consensus
        .decision
        .as_ref()
        .map(|winner| nl_dsl::agent::GeneratedDsl {
            yaml: winner.yaml.clone(),
        })
        .or_else(|| {
            plurality.map(|candidate| nl_dsl::agent::GeneratedDsl {
                yaml: candidate.yaml,
            })
        });
    Ok(DslVoteResult {
        generated,
        consensus,
    })
}

#[cfg(not(target_family = "wasm"))]
fn dsl_vote_diagnostic_excerpt(message: &str, max_chars: usize) -> String {
    let mut characters = message.chars();
    let mut excerpt = characters.by_ref().take(max_chars).collect::<String>();
    if characters.next().is_some() {
        excerpt.push_str("…（诊断已截断）");
    }
    excerpt
}

#[cfg(not(target_family = "wasm"))]
fn dsl_vote_failure_message(outcomes: &[VoteOutcome]) -> String {
    let mut message = "没有可进入验收的有效 DSL 候选，请根据以下模型诊断修复或重试".to_string();
    for outcome in outcomes.iter().take(3) {
        if let Err(error) = &outcome.result {
            message.push_str(&format!(
                "\n模型 {}：{}",
                dsl_vote_diagnostic_excerpt(&outcome.model, 120),
                dsl_vote_diagnostic_excerpt(error, 1_000)
            ));
        }
    }
    message
}

#[cfg(not(target_family = "wasm"))]
fn require_generated_dsl(
    (generated, consensus): (
        Option<nl_dsl::agent::GeneratedDsl>,
        Option<Consensus<DslVoteCandidate>>,
    ),
) -> anyhow::Result<nl_dsl::agent::GeneratedDsl> {
    generated.ok_or_else(|| {
        anyhow::anyhow!(dsl_vote_failure_message(
            consensus.as_ref().map_or(&[], |vote| &vote.outcomes)
        ))
    })
}

#[cfg(not(target_family = "wasm"))]
fn canonical_dsl_vote_candidate(yaml: String) -> Result<DslVoteCandidate, String> {
    let (definitions, _) = compile_generated_dsl(&yaml)?;
    let normalized =
        serde_json::to_vec(&definitions).map_err(|error| format!("规范化 DSL 失败：{error}"))?;
    let signature = format!("{:x}", sha2::Sha256::digest(normalized));
    Ok(DslVoteCandidate { signature, yaml })
}

#[cfg(not(target_family = "wasm"))]
fn plurality_dsl_candidate(
    outcomes: &[VoteOutcome],
    candidates: &[(String, DslVoteCandidate)],
) -> Option<DslVoteCandidate> {
    let signature = plurality_dsl_signature(outcomes)?;
    outcomes.iter().find_map(|outcome| {
        let Ok(result) = &outcome.result else {
            return None;
        };
        (result.get("signature").and_then(serde_json::Value::as_str) == Some(signature.as_str()))
            .then(|| {
                candidates
                    .iter()
                    .find(|(model, candidate)| {
                        model == &outcome.model && candidate.signature == signature
                    })
                    .map(|(_, candidate)| candidate.clone())
            })
            .flatten()
    })
}

#[cfg(not(target_family = "wasm"))]
fn plurality_dsl_signature(outcomes: &[VoteOutcome]) -> Option<String> {
    let mut groups = BTreeMap::<String, (f64, usize)>::new();
    for (index, outcome) in outcomes.iter().enumerate() {
        let Ok(result) = &outcome.result else {
            continue;
        };
        let Some(signature) = result.get("signature").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let group = groups.entry(signature.to_string()).or_insert((0.0, index));
        group.0 += outcome.weight;
    }
    Some(
        groups
            .into_iter()
            .max_by(|left, right| {
                left.1
                    .0
                    .total_cmp(&right.1.0)
                    .then_with(|| right.1.1.cmp(&left.1.1))
            })?
            .0,
    )
}

#[cfg(not(target_family = "wasm"))]
fn dsl_vote_resolution(consensus: &Consensus<DslVoteCandidate>) -> Value {
    let selected_signature = consensus
        .decision
        .as_ref()
        .map(|candidate| candidate.signature.clone())
        .or_else(|| plurality_dsl_signature(&consensus.outcomes));
    let valid_count = consensus
        .outcomes
        .iter()
        .filter(|outcome| outcome.result.is_ok())
        .count();
    let selected_models = consensus
        .outcomes
        .iter()
        .filter(|outcome| {
            selected_signature.is_some()
                && outcome
                    .result
                    .as_ref()
                    .ok()
                    .and_then(|result| result["signature"].as_str())
                    == selected_signature.as_deref()
        })
        .map(|outcome| outcome.model.clone())
        .collect::<Vec<_>>();
    let selected_weight = consensus
        .outcomes
        .iter()
        .filter(|outcome| {
            selected_signature.is_some()
                && outcome
                    .result
                    .as_ref()
                    .ok()
                    .and_then(|result| result["signature"].as_str())
                    == selected_signature.as_deref()
        })
        .map(|outcome| outcome.weight)
        .sum::<f64>();
    let total_weight = consensus
        .outcomes
        .iter()
        .map(|outcome| outcome.weight)
        .sum::<f64>();
    let strict_majority = consensus.decision.is_some();
    json!({
        "strategy": if strict_majority { "strict_majority" }
            else if valid_count == 1 { "single_valid_candidate" }
            else if selected_signature.is_some() { "plurality_with_configuration_order_tiebreak" }
            else { "no_valid_candidate" },
        "strictMajority": strict_majority,
        "requiresBusinessConfirmation": !strict_majority,
        "totalModelCount": consensus.outcomes.len(),
        "validModelCount": valid_count,
        "selectedSignature": selected_signature,
        "selectedModels": selected_models,
        "selectedWeight": selected_weight,
        "selectedShare": if total_weight > 0.0 { selected_weight / total_weight } else { 0.0 },
    })
}

#[cfg(not(target_family = "wasm"))]
fn add_dsl_vote_review_finding(
    review: &mut crate::models::VersionChangeReview,
    resolution: Option<&Value>,
) {
    let Some(resolution) = resolution else {
        return;
    };
    if resolution["strictMajority"] == true || resolution["selectedSignature"].is_null() {
        return;
    }
    let valid = resolution["validModelCount"].as_u64().unwrap_or_default();
    let total = resolution["totalModelCount"].as_u64().unwrap_or_default();
    let support = resolution["selectedModels"].as_array().map_or(0, Vec::len);
    let single = valid == 1;
    let (level, code, message) = if single {
        (
            "high",
            "DSL_SINGLE_VALID_CANDIDATE",
            format!(
                "{total} 个模型中仅 1 个生成了通过校验的候选，且所选候选仅获 {support}/{total} 个模型支持，缺少独立模型交叉验证。保存前请重点检查业务边界、默认行为及高风险规则。"
            ),
        )
    } else {
        (
            "medium",
            "DSL_NO_STRICT_MAJORITY",
            format!(
                "{total} 个模型中有 {valid} 个返回有效候选；所选候选获 {support}/{total} 个模型支持，未形成严格多数。候选按票数选择，平票按配置顺序；保存前请确认业务规则及边界案例。"
            ),
        )
    };
    if single || review.overall_risk == "low" {
        review.overall_risk = level.into();
    }
    review.confirmation_required = true;
    review.findings.push(crate::models::VersionRiskFinding {
        level: level.into(),
        code: code.into(),
        message,
        element: String::new(),
    });
}

#[cfg(not(target_family = "wasm"))]
async fn generate_dsl_for_route(
    agent: &LlmAgent,
    requirement: &str,
    plan: &crate::workflow::DecisionPlan,
    complexity: RequirementComplexity,
    tx: Option<&mpsc::Sender<StreamEvent>>,
) -> anyhow::Result<(
    Option<nl_dsl::agent::GeneratedDsl>,
    Option<Consensus<DslVoteCandidate>>,
)> {
    if complexity == RequirementComplexity::Complex {
        let vote = generate_complex_dsl_consensus(agent, requirement, plan, tx).await?;
        return Ok((vote.generated, Some(vote.consensus)));
    }
    let generated = match tx {
        Some(tx) => agent.generate_dsl_stream(requirement, None, tx).await,
        None => agent.generate_dsl(requirement, None).await,
    }
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok((Some(generated), None))
}

#[cfg(not(target_family = "wasm"))]
async fn generate_dsl(
    State(app_state): State<AppState>,
    Extension(state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut req: GenerateRequest = match parse_json_body(body).await {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    if let Err(error) = validate_generate_request(&req) {
        return error.into_response();
    }
    if req.existing_dsl.is_some() {
        return (
            StatusCode::GONE,
            Json(json!({"ok": false, "code": "CHANGE_INTENT_REQUIRED", "error": "已有模型的修改必须通过模型变更预览接口完成"})),
        ).into_response();
    }
    let complexity = match confirmed_requirement_complexity(&app_state.db, &req).await {
        Ok(complexity) => complexity,
        Err(error) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({"ok": false, "error": error})),
            )
                .into_response();
        }
    };
    let request_fingerprint = generation_request_fingerprint(&req, None);
    let (parent_orchestration_id, traceability_plan) =
        match apply_confirmed_workflow(&app_state, &claims, &mut req, None).await {
            Ok(context) => context,
            Err(error) => return error.into_response(),
        };
    let knowledge_citations = match apply_effective_knowledge(&app_state, &claims, &mut req).await {
        Ok(citations) => citations,
        Err(error) => return error.into_response(),
    };
    let project_id = req
        .project_id
        .expect("validated generation request must have project_id");
    let allow_experiment = project_allows_model_experiment(&app_state.db, project_id).await;
    let complexity_route = match complexity {
        RequirementComplexity::Simple => AgentComplexity::Simple,
        RequirementComplexity::Standard => AgentComplexity::Standard,
        RequirementComplexity::Complex => AgentComplexity::Complex,
    };
    let Some((dsl_agent, experiment)) =
        state.dsl_agent_for_project(project_id, allow_experiment, complexity_route)
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "error": "未配置 OPENAI_API_KEY，无法调用 LLM" })),
        )
            .into_response();
    };
    let dsl_agent = dsl_agent.fork_for_run();
    let run_id = match begin_dsl_generation_run(
        &app_state.db,
        &claims.sub,
        parent_orchestration_id,
        req.retry_of_orchestration_id,
        &request_fingerprint,
        &dsl_agent,
        &req,
        experiment,
        complexity,
    )
    .await
    {
        Ok(run_id) => run_id,
        Err(error) => return project_token_budget_response(error),
    };
    if wants_stream(&headers) {
        return generate_dsl_sse(
            dsl_agent,
            req,
            traceability_plan,
            knowledge_citations,
            app_state.db.clone(),
            run_id,
            complexity,
        );
    }
    let generated = crate::workflow_api::run_until_stream_disconnect(
        None,
        async {
            match run_id.orchestration_id {
                Some(orchestration_id) => {
                    crate::workflow_api::wait_for_orchestration_cancellation(
                        &app_state.db,
                        orchestration_id,
                    )
                    .await
                }
                None => std::future::pending().await,
            }
        },
        async {
            generate_dsl_for_route(&dsl_agent, &req.text, &traceability_plan, complexity, None)
                .await
        },
    )
    .await;
    if let Ok((_, Some(consensus))) = &generated {
        if let Some(run_id) = run_id.run_id {
            if let Err(error) = crate::workflow_api::record_model_vote_with_resolution(
                &app_state.db,
                run_id,
                "dsl_generation",
                consensus,
                Some(dsl_vote_resolution(consensus)),
            )
            .await
            {
                eprintln!("记录 DSL 多模型投票失败: {error}");
            }
        }
    }
    let model_selection = generated
        .as_ref()
        .ok()
        .and_then(|(_, consensus)| consensus.as_ref().map(dsl_vote_resolution));
    let generated = generated.and_then(require_generated_dsl);
    let outcome = match generated {
        Ok(generated) => {
            if let Err(message) = ensure_generation_snapshot(&app_state.db, &req).await {
                Err((StatusCode::CONFLICT, message))
            } else {
                match apply_requirement_traceability(&generated.yaml, &traceability_plan) {
                    Ok(yaml) => Ok(json!({
                        "ok": true,
                        "nl_dsl": yaml,
                        "knowledgeCitations": knowledge_citations,
                        "modelSelection": model_selection,
                    })),
                    Err(message) => Err((StatusCode::UNPROCESSABLE_ENTITY, message)),
                }
            }
        }
        Err(error) => Err((StatusCode::BAD_GATEWAY, error.to_string())),
    };
    let audit = outcome.clone().map_err(|(_, message)| message.clone());
    finish_dsl_generation_run(&app_state.db, run_id, &dsl_agent, audit).await;
    match outcome {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err((status, message)) => {
            let retryable = crate::workflow_api::is_retryable_agent_failure(&message);
            (
                status,
                Json(json!({"ok": false, "error": message, "retryable": retryable})),
            )
                .into_response()
        }
    }
}

#[cfg(not(target_family = "wasm"))]
async fn begin_dsl_generation_run(
    db: &sqlx::PgPool,
    username: &str,
    parent_orchestration_id: Option<i64>,
    retry_of_orchestration_id: Option<i64>,
    request_fingerprint: &str,
    agent: &LlmAgent,
    req: &GenerateRequest,
    experiment: Option<ExperimentAssignment<'_>>,
    complexity: RequirementComplexity,
) -> Result<AgentExecutionRun, ProjectTokenBudgetError> {
    let Some(project_id) = req.project_id else {
        return Ok(AgentExecutionRun {
            run_id: None,
            orchestration_id: None,
        });
    };
    let response_attempts = if complexity == RequirementComplexity::Complex {
        agent.maximum_parallel_dsl_response_attempts(3)
    } else {
        agent.maximum_dsl_response_attempts()
    };
    let estimated_cost =
        estimated_run_cost_reservation(agent.estimated_cost_per_response_usd(), response_attempts);
    let mut transaction =
        begin_project_daily_budget_transaction(db, project_id, estimated_cost).await?;
    check_orchestration_budgets(
        &mut transaction,
        project_id,
        parent_orchestration_id,
        retry_of_orchestration_id,
        estimated_cost,
    )
    .await?;
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO agent_orchestrations (
            project_id, username, trigger_kind, current_stage, parent_orchestration_id,
            retry_of_orchestration_id
        )
        SELECT $1, $2, 'dsl_generation', 'dsl_generation', $3, $4
        WHERE ($3 IS NULL OR EXISTS (
            SELECT 1 FROM agent_orchestrations
            WHERE id = $3 AND project_id = $1 AND status = 'succeeded'
        )) AND ($4 IS NULL OR EXISTS (
            SELECT 1
            FROM agent_orchestrations previous
            JOIN agent_runs previous_run ON previous_run.orchestration_id = previous.id
            JOIN agent_steps previous_step ON previous_step.agent_run_id = previous_run.id
            WHERE previous.id = $4
              AND previous.project_id = $1
              AND previous.trigger_kind = 'dsl_generation'
              AND previous.status = 'failed'
              AND previous.retry_of_orchestration_id IS NULL
              AND previous.parent_orchestration_id IS NOT DISTINCT FROM $3
              AND previous_step.error_json->>'retryable' = 'true'
              AND previous_run.input_json->>'requestFingerprint' = $5
              AND (SELECT COUNT(*) FROM agent_orchestrations retry
                   WHERE retry.retry_of_orchestration_id = previous.id) < 2
              AND NOT EXISTS (
                  SELECT 1 FROM agent_orchestrations retry
                  WHERE retry.retry_of_orchestration_id = previous.id
                    AND retry.status IN ('pending', 'running', 'succeeded')
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
    .bind(parent_orchestration_id)
    .bind(retry_of_orchestration_id)
    .bind(request_fingerprint)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ProjectTokenBudgetError::Database)?
    .ok_or_else(|| {
        if retry_of_orchestration_id.is_some() {
            ProjectTokenBudgetError::RetryAlreadyActive
        } else {
            ProjectTokenBudgetError::Database(sqlx::Error::RowNotFound)
        }
    })?;
    let run_id = sqlx::query_scalar(
        "INSERT INTO agent_runs (project_id, stage, status, model, prompt_version, prompt_digest, input_json, usage_json, experiment_name, experiment_variant, experiment_treatment_percent, experiment_bucket, orchestration_id) VALUES ($1, 'dsl_generation', 'running', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id",
    )
    .bind(project_id)
    .bind(agent.model())
    .bind(agent.prompt_asset("dsl_generation").version.as_str())
    .bind(format!("{:x}", sha2::Sha256::digest(agent.prompt_asset("dsl_generation").content.as_bytes())))
    .bind(json!({
        "requestBytes": req.text.len(),
        "requestFingerprint": request_fingerprint,
        "complexity": complexity.as_str(),
        "voteModelCount": if complexity == RequirementComplexity::Complex {
            agent.parallel_model_names(3).len()
        } else {
            1
        },
    }))
    .bind(project_budget_reservation(estimated_cost))
    .bind(experiment.map(|assignment| assignment.experiment_name))
    .bind(experiment.map(|assignment| assignment.variant))
    .bind(experiment.map(|assignment| i16::from(assignment.treatment_percent)))
    .bind(experiment.map(|assignment| i16::from(assignment.bucket)))
    .bind(orchestration_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(ProjectTokenBudgetError::Database)?;
    sqlx::query(
        "INSERT INTO agent_steps (orchestration_id, agent_run_id, step_no, agent_role, task_kind, model, prompt_version, prompt_digest, artifact_kind) VALUES ($1, $2, 1, 'engineer', 'generate_dmn_dsl', $3, $4, $5, 'dmn_model')",
    )
    .bind(orchestration_id)
    .bind(run_id)
    .bind(agent.model())
    .bind(agent.prompt_asset("dsl_generation").version.as_str())
    .bind(format!("{:x}", sha2::Sha256::digest(agent.prompt_asset("dsl_generation").content.as_bytes())))
    .execute(&mut *transaction)
    .await
    .map_err(ProjectTokenBudgetError::Database)?;
    transaction
        .commit()
        .await
        .map_err(ProjectTokenBudgetError::Database)?;
    Ok(AgentExecutionRun {
        run_id: Some(run_id),
        orchestration_id: Some(orchestration_id),
    })
}

#[cfg(not(target_family = "wasm"))]
fn sanitize_jsonb_value(mut value: Value) -> Value {
    fn sanitize(value: &mut Value) {
        match value {
            Value::String(text) => *text = text.replace('\0', "\u{FFFD}"),
            Value::Array(items) => items.iter_mut().for_each(sanitize),
            Value::Object(fields) => fields.values_mut().for_each(sanitize),
            _ => {}
        }
    }

    sanitize(&mut value);
    value
}

#[cfg(not(target_family = "wasm"))]
async fn finish_dsl_generation_run(
    db: &sqlx::PgPool,
    run: AgentExecutionRun,
    agent: &LlmAgent,
    output: Result<Value, String>,
) {
    let Some(run_id) = run.run_id else {
        return;
    };
    let (status, output_json, error_json) = match output {
        Ok(output) => ("succeeded", Some(output), None),
        Err(error) => {
            let status = if error.contains("编排已由用户取消") {
                "cancelled"
            } else {
                "failed"
            };
            let retryable = crate::workflow_api::is_retryable_agent_failure(&error);
            (
                status,
                None,
                Some(json!({
                    "message": error,
                    "retryable": retryable,
                    "failureKind": if retryable { "transient_provider" } else { "non_retryable_or_unknown" },
                })),
            )
        }
    };
    let output_json = output_json.map(sanitize_jsonb_value);
    let error_json = error_json.map(sanitize_jsonb_value);
    let usage = sanitize_jsonb_value(agent.usage_snapshot());
    let Some(orchestration_id) = run.orchestration_id else {
        return;
    };
    let digest = output_json
        .as_ref()
        .and_then(|value| serde_json::to_vec(value).ok())
        .map(|bytes| format!("{:x}", sha2::Sha256::digest(bytes)));
    let mut transaction = match db.begin().await {
        Ok(transaction) => transaction,
        Err(error) => {
            eprintln!("开始完成 DSL 编排失败: {error}");
            return;
        }
    };
    let run_result = sqlx::query(
        "UPDATE agent_runs SET status=CASE WHEN status='running' THEN $1 ELSE status END, output_json=CASE WHEN status='running' THEN $2 ELSE output_json END, error_json=CASE WHEN status='running' THEN $3 ELSE COALESCE(error_json, $3) END, usage_json=$4, finished_at=COALESCE(finished_at, NOW()) WHERE id=$5 AND status IN ('running', 'cancelled')",
    )
    .bind(status)
    .bind(&output_json)
    .bind(&error_json)
    .bind(&usage)
    .bind(run_id)
    .execute(&mut *transaction)
    .await;
    if let Err(error) = run_result {
        eprintln!("更新 DSL 生成智能运行失败: {error}");
        return;
    }
    let step_result = sqlx::query(
        "UPDATE agent_steps SET status=CASE WHEN status='running' THEN $1 ELSE status END, artifact_digest=COALESCE($2, artifact_digest), usage_json=$3, error_json=COALESCE(error_json, $4), finished_at=COALESCE(finished_at, NOW()) WHERE agent_run_id=$5 AND status IN ('running', 'cancelled')",
    )
    .bind(status)
    .bind(digest)
    .bind(&usage)
    .bind(&error_json)
    .bind(run_id)
    .execute(&mut *transaction)
    .await;
    if let Err(error) = step_result {
        eprintln!("更新 DSL 编排步骤失败: {error}");
        return;
    }
    let orchestration_result = sqlx::query(
        "UPDATE agent_orchestrations SET status=CASE WHEN status='running' THEN $1 ELSE status END, finished_at=COALESCE(finished_at, NOW()) WHERE id=$2 AND status IN ('running', 'cancelled')",
    )
    .bind(status)
    .bind(orchestration_id)
    .execute(&mut *transaction)
    .await;
    if let Err(error) = orchestration_result {
        eprintln!("完成 DSL 编排失败: {error}");
        return;
    }
    if let Err(error) = transaction.commit().await {
        eprintln!("更新 DSL 生成智能运行失败: {error}");
    }
}

#[cfg(not(target_family = "wasm"))]
async fn begin_change_intent_run(
    db: &sqlx::PgPool,
    project_id: i64,
    username: &str,
    parent_orchestration_id: Option<i64>,
    retry_of_orchestration_id: Option<i64>,
    request_fingerprint: &str,
    agent: &LlmAgent,
    req: &GenerateRequest,
    base_revision: i64,
    experiment: Option<ExperimentAssignment<'_>>,
) -> Result<AgentExecutionRun, ProjectTokenBudgetError> {
    let estimated_cost = estimated_run_cost_reservation(
        agent.estimated_cost_per_response_usd(),
        agent.maximum_model_response_attempts(),
    );
    let mut transaction =
        begin_project_daily_budget_transaction(db, project_id, estimated_cost).await?;
    check_orchestration_budgets(
        &mut transaction,
        project_id,
        parent_orchestration_id,
        retry_of_orchestration_id,
        estimated_cost,
    )
    .await?;
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO agent_orchestrations (
            project_id, username, trigger_kind, current_stage, parent_orchestration_id,
            retry_of_orchestration_id
        )
        SELECT $1, $2, 'change_intent', 'generate_change_intent', $3, $4
        WHERE ($3 IS NULL OR EXISTS (
            SELECT 1 FROM agent_orchestrations
            WHERE id = $3 AND project_id = $1 AND status = 'succeeded'
        )) AND ($4 IS NULL OR EXISTS (
            SELECT 1
            FROM agent_orchestrations previous
            JOIN agent_runs previous_run ON previous_run.orchestration_id = previous.id
            JOIN agent_steps previous_step ON previous_step.agent_run_id = previous_run.id
            WHERE previous.id = $4
              AND previous.project_id = $1
              AND previous.trigger_kind = 'change_intent'
              AND previous.status = 'failed'
              AND previous.retry_of_orchestration_id IS NULL
              AND previous.parent_orchestration_id IS NOT DISTINCT FROM $3
              AND previous_step.error_json->>'retryable' = 'true'
              AND previous_run.input_json->>'requestFingerprint' = $5
              AND (SELECT COUNT(*) FROM agent_orchestrations retry
                   WHERE retry.retry_of_orchestration_id = previous.id) < 2
              AND NOT EXISTS (
                  SELECT 1 FROM agent_orchestrations retry
                  WHERE retry.retry_of_orchestration_id = previous.id
                    AND retry.status IN ('pending', 'running', 'succeeded')
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
    .bind(parent_orchestration_id)
    .bind(retry_of_orchestration_id)
    .bind(request_fingerprint)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ProjectTokenBudgetError::Database)?
    .ok_or_else(|| {
        if retry_of_orchestration_id.is_some() {
            ProjectTokenBudgetError::RetryAlreadyActive
        } else {
            ProjectTokenBudgetError::Database(sqlx::Error::RowNotFound)
        }
    })?;
    let input = json!({
        "baseRevision": base_revision,
        "requestBytes": req.text.len(),
        "requestFingerprint": request_fingerprint,
        "hasCompilerFeedback": req.repair_error.is_some(),
        "previousChangeCount": req.previous_intent.as_ref().map_or(0, |intent| intent.changes.len()),
    });
    let run_id = sqlx::query_scalar(
        r#"
        INSERT INTO agent_runs (
            project_id, stage, status, model, prompt_version, prompt_digest, input_json, usage_json, retry_count,
            experiment_name, experiment_variant,
            experiment_treatment_percent, experiment_bucket, orchestration_id
        )
        VALUES ($1, 'change_intent', 'running', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        RETURNING id
        "#,
    )
    .bind(project_id)
    .bind(agent.model())
    .bind(agent.prompt_asset("change_intent").version.as_str())
    .bind(format!("{:x}", sha2::Sha256::digest(agent.prompt_asset("change_intent").content.as_bytes())))
    .bind(input)
    .bind(project_budget_reservation(estimated_cost))
    .bind(req.repair_attempt.unwrap_or(0))
    .bind(experiment.map(|assignment| assignment.experiment_name))
    .bind(experiment.map(|assignment| assignment.variant))
    .bind(experiment.map(|assignment| i16::from(assignment.treatment_percent)))
    .bind(experiment.map(|assignment| i16::from(assignment.bucket)))
    .bind(orchestration_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(ProjectTokenBudgetError::Database)?;
    sqlx::query(
        "INSERT INTO agent_steps (orchestration_id, agent_run_id, step_no, agent_role, task_kind, model, prompt_version, prompt_digest, artifact_kind) VALUES ($1, $2, 1, 'engineer', 'generate_change_intent', $3, $4, $5, 'model_patch')",
    )
    .bind(orchestration_id)
    .bind(run_id)
    .bind(agent.model())
    .bind(agent.prompt_asset("change_intent").version.as_str())
    .bind(format!("{:x}", sha2::Sha256::digest(agent.prompt_asset("change_intent").content.as_bytes())))
    .execute(&mut *transaction)
    .await
    .map_err(ProjectTokenBudgetError::Database)?;
    transaction
        .commit()
        .await
        .map_err(ProjectTokenBudgetError::Database)?;
    Ok(AgentExecutionRun {
        run_id: Some(run_id),
        orchestration_id: Some(orchestration_id),
    })
}

#[cfg(not(target_family = "wasm"))]
async fn finish_change_intent_run(
    db: &sqlx::PgPool,
    run: AgentExecutionRun,
    agent: &LlmAgent,
    output: Result<Value, String>,
) {
    let Some(run_id) = run.run_id else {
        return;
    };
    let (status, output, error) = match output {
        Ok(output) => ("succeeded", Some(output), None),
        Err(error) => {
            let status = if error.contains("编排已由用户取消") {
                "cancelled"
            } else {
                "failed"
            };
            let retryable = crate::workflow_api::is_retryable_agent_failure(&error);
            (
                status,
                None,
                Some(json!({
                    "message": error,
                    "retryable": retryable,
                    "failureKind": if retryable { "transient_provider" } else { "non_retryable_or_unknown" },
                })),
            )
        }
    };
    let output = output.map(sanitize_jsonb_value);
    let error = error.map(sanitize_jsonb_value);
    let Some(orchestration_id) = run.orchestration_id else {
        return;
    };
    let usage = sanitize_jsonb_value(agent.usage_snapshot());
    let digest = output
        .as_ref()
        .and_then(|value| serde_json::to_vec(value).ok())
        .map(|bytes| format!("{:x}", sha2::Sha256::digest(bytes)));
    let mut transaction = match db.begin().await {
        Ok(transaction) => transaction,
        Err(error) => {
            eprintln!("开始完成模型变更编排失败: {error}");
            return;
        }
    };
    let result = sqlx::query(
        "UPDATE agent_runs SET status=CASE WHEN status='running' THEN $1 ELSE status END, output_json=CASE WHEN status='running' THEN $2 ELSE output_json END, error_json=CASE WHEN status='running' THEN $3 ELSE COALESCE(error_json, $3) END, usage_json=$4, finished_at=COALESCE(finished_at, NOW()) WHERE id=$5 AND status IN ('running', 'cancelled')",
    )
    .bind(status)
    .bind(output)
    .bind(&error)
    .bind(&usage)
    .bind(run_id)
    .execute(&mut *transaction)
    .await;
    if let Err(error) = result {
        eprintln!("更新模型变更智能运行失败: {error}");
        return;
    }
    let result = sqlx::query(
        "UPDATE agent_steps SET status=CASE WHEN status='running' THEN $1 ELSE status END, artifact_digest=COALESCE($2, artifact_digest), usage_json=$3, error_json=COALESCE(error_json, $4), finished_at=COALESCE(finished_at, NOW()) WHERE agent_run_id=$5 AND status IN ('running', 'cancelled')",
    )
    .bind(status)
    .bind(digest)
    .bind(&usage)
    .bind(&error)
    .bind(run_id)
    .execute(&mut *transaction)
    .await;
    if let Err(error) = result {
        eprintln!("更新模型变更编排步骤失败: {error}");
        return;
    }
    let result = sqlx::query(
        "UPDATE agent_orchestrations SET status=CASE WHEN status='running' THEN $1 ELSE status END, finished_at=COALESCE(finished_at, NOW()) WHERE id=$2 AND status IN ('running', 'cancelled')",
    )
    .bind(status)
    .bind(orchestration_id)
    .execute(&mut *transaction)
    .await;
    if let Err(error) = result {
        eprintln!("完成模型变更编排失败: {error}");
        return;
    }
    if let Err(error) = transaction.commit().await {
        eprintln!("提交模型变更编排失败: {error}");
    }
}

#[cfg(not(target_family = "wasm"))]
async fn generate_change_intent(
    State(app_state): State<AppState>,
    Extension(state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut req: GenerateRequest = match parse_json_body(body).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = validate_change_intent_request(&req) {
        return error.into_response();
    }
    let Some(project_id) = req.project_id else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "code": "BAD_REQUEST", "error": "生成变更意图必须提供 projectId"})),
        ).into_response();
    };
    let Some(base_revision) = req
        .base_revision
        .or_else(|| req.base_version.map(i64::from))
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "code": "BAD_REQUEST", "error": "生成变更意图必须提供 baseRevision"})),
        ).into_response();
    };
    let allowed = match crate::project_access::has_permission(
        &app_state.db,
        project_id,
        &claims.sub,
        crate::project_access::ProjectPermission::Model,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            eprintln!("校验模型变更权限失败: {error}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"ok": false, "code": "DATABASE_ERROR", "error": "校验模型变更权限失败"}),
                ),
            )
                .into_response();
        }
    };
    if !allowed {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"ok": false, "code": "NOT_FOUND", "error": "决策项目不存在"})),
        )
            .into_response();
    }
    let baseline = match sqlx::query_as::<_, (i64, String)>(
        "SELECT v.id, a.dsl_yaml FROM decision_projects p JOIN decision_versions v ON v.id=p.current_draft_version_id JOIN decision_artifacts a ON a.version_id=v.id WHERE p.id=$1",
    )
    .bind(project_id)
    .fetch_optional(&app_state.db)
    .await
    {
        Ok(Some(value)) => value,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"ok": false, "code": "BASE_CONFLICT", "error": "项目没有可修改的草稿版本"})),
            ).into_response();
        }
        Err(error) => {
            eprintln!("读取模型变更基线失败: {error}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"ok": false, "code": "DATABASE_ERROR", "error": "读取模型变更基线失败"})),
            ).into_response();
        }
    };
    let (current_revision, existing) = baseline;
    if base_revision != current_revision {
        return (
            StatusCode::CONFLICT,
            Json(json!({"ok": false, "code": "BASE_CONFLICT", "error": "模型基线版本已变化，请刷新后重试"})),
        ).into_response();
    }
    let request_fingerprint = generation_request_fingerprint(&req, Some(base_revision));
    let (parent_orchestration_id, _) =
        match apply_confirmed_workflow(&app_state, &claims, &mut req, Some(base_revision)).await {
            Ok(context) => context,
            Err(error) => return error.into_response(),
        };
    let allow_experiment = project_allows_model_experiment(&app_state.db, project_id).await;
    let Some((agent, experiment)) =
        state.change_intent_agent_for_project(project_id, allow_experiment)
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"ok": false, "code": "LLM_UNAVAILABLE", "error": "未配置 OPENAI_API_KEY，无法调用 LLM"})),
        ).into_response();
    };
    let agent = agent.fork_for_run();
    let run_id = match begin_change_intent_run(
        &app_state.db,
        project_id,
        &claims.sub,
        parent_orchestration_id,
        req.retry_of_orchestration_id,
        &request_fingerprint,
        &agent,
        &req,
        base_revision,
        experiment,
    )
    .await
    {
        Ok(run_id) => run_id,
        Err(error) => return project_token_budget_response(error),
    };
    if wants_stream(&headers) {
        return generate_change_intent_sse(
            agent,
            req,
            existing,
            base_revision,
            app_state.db.clone(),
            run_id,
        );
    }
    let generated = crate::workflow_api::run_until_stream_disconnect(
        None,
        async {
            match run_id.orchestration_id {
                Some(orchestration_id) => {
                    crate::workflow_api::wait_for_orchestration_cancellation(
                        &app_state.db,
                        orchestration_id,
                    )
                    .await
                }
                None => std::future::pending().await,
            }
        },
        async {
            agent
                .generate_change_intent(
                    &req.text,
                    &existing,
                    base_revision,
                    req.repair_error.as_deref(),
                    req.previous_intent.as_ref(),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))
        },
    )
    .await;
    match generated {
        Ok(intent) => {
            if let Err(message) = ensure_generation_snapshot(&app_state.db, &req).await {
                finish_change_intent_run(&app_state.db, run_id, &agent, Err(message.clone())).await;
                return (
                    StatusCode::CONFLICT,
                    Json(json!({"ok": false, "error": message})),
                )
                    .into_response();
            }
            if intent.base_revision != current_revision {
                finish_change_intent_run(
                    &app_state.db,
                    run_id,
                    &agent,
                    Err("模型基线版本已变化，请刷新后重试".to_string()),
                )
                .await;
                return (
                    StatusCode::CONFLICT,
                    Json(json!({"ok": false, "code": "BASE_CONFLICT", "error": "模型基线版本已变化，请刷新后重试"})),
                ).into_response();
            }
            finish_change_intent_run(
                &app_state.db,
                run_id,
                &agent,
                Ok(serde_json::to_value(&intent).unwrap_or_else(|_| json!({}))),
            )
            .await;
            (
                StatusCode::OK,
                Json(json!({"ok": true, "changeIntent": intent})),
            )
                .into_response()
        }
        Err(error) => {
            let message = error.to_string();
            finish_change_intent_run(&app_state.db, run_id, &agent, Err(message.clone())).await;
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "ok": false,
                    "code": "LLM_PROTOCOL",
                    "error": message,
                    "retryable": crate::workflow_api::is_retryable_agent_failure(&message),
                })),
            )
                .into_response()
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn generate_change_intent_sse(
    agent: LlmAgent,
    req: GenerateRequest,
    existing: String,
    base_revision: i64,
    db: sqlx::PgPool,
    run: AgentExecutionRun,
) -> Response {
    let (tx, rx) = mpsc::channel::<StreamEvent>(32);
    let event_db = db.clone();
    if let Some(orchestration_id) = run.orchestration_id {
        let _ = tx.try_send(StreamEvent::Orchestration(orchestration_id));
    }
    tokio::spawn(async move {
        let _ = tx
            .send(StreamEvent::Step(
                "→ 正在生成受控模型变更意图...".to_string(),
            ))
            .await;
        let generated = crate::workflow_api::run_until_stream_disconnect(
            Some(&tx),
            async {
                match run.orchestration_id {
                    Some(orchestration_id) => {
                        crate::workflow_api::wait_for_orchestration_cancellation(
                            &db,
                            orchestration_id,
                        )
                        .await
                    }
                    None => std::future::pending().await,
                }
            },
            async {
                agent
                    .generate_change_intent_stream(
                        &req.text,
                        &existing,
                        base_revision,
                        req.repair_error.as_deref(),
                        req.previous_intent.as_ref(),
                        &tx,
                    )
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))
            },
        )
        .await;
        match generated {
            Ok(intent) => {
                if let Err(message) = ensure_generation_snapshot(&db, &req).await {
                    finish_change_intent_run(&db, run, &agent, Err(message.clone())).await;
                    let _ = tx.send(StreamEvent::Finished(Err(message))).await;
                    return;
                }
                let audit_output = serde_json::to_value(&intent).unwrap_or_else(|_| json!({}));
                let _ = tx
                    .send(StreamEvent::Step(
                        "✓ 模型变更意图已生成，正在准备 DSL 校验...".to_string(),
                    ))
                    .await;
                let output = json!({
                    "ok": true,
                    "changeIntent": intent,
                });
                finish_change_intent_run(&db, run, &agent, Ok(audit_output)).await;
                let _ = tx.send(StreamEvent::Finished(Ok(output))).await;
            }
            Err(error) => {
                let message = error.to_string();
                finish_change_intent_run(&db, run, &agent, Err(message.clone())).await;
                let _ = tx.send(StreamEvent::Finished(Err(message))).await;
            }
        }
    });
    sse_response(rx, run.orchestration_id, event_db)
}

/// SSE 流式生成：step/delta 事件实时推送 LLM 过程，结束推送 result/error 事件
#[cfg(not(target_family = "wasm"))]
fn generate_dsl_sse(
    dsl_agent: LlmAgent,
    req: GenerateRequest,
    traceability_plan: crate::workflow::DecisionPlan,
    knowledge_citations: Vec<crate::models::KnowledgeCitation>,
    db: sqlx::PgPool,
    run_id: AgentExecutionRun,
    complexity: RequirementComplexity,
) -> Response {
    let (tx, rx) = mpsc::channel::<StreamEvent>(64);
    let event_db = db.clone();
    if let Some(orchestration_id) = run_id.orchestration_id {
        let _ = tx.try_send(StreamEvent::Orchestration(orchestration_id));
    }
    let text = req.text.clone();
    let tx2 = tx.clone();
    tokio::spawn(async move {
        let generated = crate::workflow_api::run_until_stream_disconnect(
            Some(&tx2),
            async {
                match run_id.orchestration_id {
                    Some(orchestration_id) => {
                        crate::workflow_api::wait_for_orchestration_cancellation(
                            &db,
                            orchestration_id,
                        )
                        .await
                    }
                    None => std::future::pending().await,
                }
            },
            async {
                generate_dsl_for_route(
                    &dsl_agent,
                    &text,
                    &traceability_plan,
                    complexity,
                    Some(&tx2),
                )
                .await
            },
        )
        .await;
        if let Ok((_, Some(consensus))) = &generated {
            if let Some(run_id) = run_id.run_id {
                if let Err(error) = crate::workflow_api::record_model_vote_with_resolution(
                    &db,
                    run_id,
                    "dsl_generation",
                    consensus,
                    Some(dsl_vote_resolution(consensus)),
                )
                .await
                {
                    eprintln!("记录 DSL 多模型投票失败: {error}");
                }
            }
        }
        let model_selection = generated
            .as_ref()
            .ok()
            .and_then(|(_, consensus)| consensus.as_ref().map(dsl_vote_resolution));
        let generated = generated.and_then(require_generated_dsl);
        let outcome = match generated {
            Ok(generated) => match ensure_generation_snapshot(&db, &req).await {
                Ok(()) => match apply_requirement_traceability(&generated.yaml, &traceability_plan)
                {
                    Ok(yaml) => Ok(json!({
                        "ok": true,
                        "nl_dsl": yaml,
                        "knowledgeCitations": knowledge_citations,
                        "modelSelection": model_selection,
                    })),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            },
            Err(error) => Err(error.to_string()),
        };
        finish_dsl_generation_run(&db, run_id, &dsl_agent, outcome.clone()).await;
        let _ = tx2.send(StreamEvent::Finished(outcome)).await;
    });
    sse_response(rx, run_id.orchestration_id, event_db)
}

#[cfg(not(target_family = "wasm"))]
pub(crate) async fn run_pipeline(
    State(app_state): State<AppState>,
    Extension(state): Extension<Arc<LlmState>>,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut req: GenerateRequest = match parse_json_body(body).await {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    if let Err(error) = validate_generate_request(&req) {
        return error.into_response();
    }
    if req.existing_dsl.is_some() {
        return (
            StatusCode::GONE,
            Json(json!({"ok": false, "code": "CHANGE_INTENT_REQUIRED", "error": "已有模型的修改必须通过模型变更预览接口完成"})),
        ).into_response();
    }
    let complexity = match confirmed_requirement_complexity(&app_state.db, &req).await {
        Ok(complexity) => complexity,
        Err(error) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({"ok": false, "error": error})),
            )
                .into_response();
        }
    };
    let request_fingerprint = generation_request_fingerprint(&req, None);
    let (parent_orchestration_id, traceability_plan) =
        match apply_confirmed_workflow(&app_state, &claims, &mut req, None).await {
            Ok(context) => context,
            Err(error) => return error.into_response(),
        };
    let knowledge_citations = match apply_effective_knowledge(&app_state, &claims, &mut req).await {
        Ok(citations) => citations,
        Err(error) => return error.into_response(),
    };
    let project_id = req
        .project_id
        .expect("validated generation request must have project_id");
    let allow_experiment = project_allows_model_experiment(&app_state.db, project_id).await;
    let complexity_route = match complexity {
        RequirementComplexity::Simple => AgentComplexity::Simple,
        RequirementComplexity::Standard => AgentComplexity::Standard,
        RequirementComplexity::Complex => AgentComplexity::Complex,
    };
    let Some((dsl_agent, experiment)) =
        state.dsl_agent_for_project(project_id, allow_experiment, complexity_route)
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "error": "未配置 OPENAI_API_KEY，无法调用 LLM" })),
        )
            .into_response();
    };
    let dsl_agent = dsl_agent.fork_for_run();
    let run_id = match begin_dsl_generation_run(
        &app_state.db,
        &claims.sub,
        parent_orchestration_id,
        req.retry_of_orchestration_id,
        &request_fingerprint,
        &dsl_agent,
        &req,
        experiment,
        complexity,
    )
    .await
    {
        Ok(run_id) => run_id,
        Err(error) => return project_token_budget_response(error),
    };
    if wants_stream(&headers) {
        return run_pipeline_sse(
            dsl_agent,
            req,
            traceability_plan,
            knowledge_citations,
            app_state.db.clone(),
            run_id,
            complexity,
        );
    }

    // 步骤 1: 自然语言 → DSL
    let generated = crate::workflow_api::run_until_stream_disconnect(
        None,
        async {
            match run_id.orchestration_id {
                Some(orchestration_id) => {
                    crate::workflow_api::wait_for_orchestration_cancellation(
                        &app_state.db,
                        orchestration_id,
                    )
                    .await
                }
                None => std::future::pending().await,
            }
        },
        async {
            generate_dsl_for_route(&dsl_agent, &req.text, &traceability_plan, complexity, None)
                .await
        },
    )
    .await;
    if let Ok((_, Some(consensus))) = &generated {
        if let Some(run_id) = run_id.run_id {
            if let Err(error) = crate::workflow_api::record_model_vote_with_resolution(
                &app_state.db,
                run_id,
                "dsl_generation",
                consensus,
                Some(dsl_vote_resolution(consensus)),
            )
            .await
            {
                eprintln!("记录 DSL 多模型投票失败: {error}");
            }
        }
    }
    let model_selection = generated
        .as_ref()
        .ok()
        .and_then(|(_, consensus)| consensus.as_ref().map(dsl_vote_resolution));
    let generated = generated.and_then(require_generated_dsl);
    let generated = match generated {
        Ok(generated) => generated,
        Err(e) => {
            finish_dsl_generation_run(&app_state.db, run_id, &dsl_agent, Err(e.to_string())).await;
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "ok": false, "error": format!("LLM 生成失败: {}", e) })),
            )
                .into_response();
        }
    };
    let dsl_text = match apply_requirement_traceability(&generated.yaml, &traceability_plan) {
        Ok(yaml) => yaml,
        Err(message) => {
            finish_dsl_generation_run(&app_state.db, run_id, &dsl_agent, Err(message.clone()))
                .await;
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"ok": false, "error": message})),
            )
                .into_response();
        }
    };
    if let Err(message) = ensure_generation_snapshot(&app_state.db, &req).await {
        finish_dsl_generation_run(&app_state.db, run_id, &dsl_agent, Err(message.clone())).await;
        return (
            StatusCode::CONFLICT,
            Json(json!({"ok": false, "error": message})),
        )
            .into_response();
    }

    // 步骤 2-4: DSL 校验、DMN 生成，以及执行引擎重新解析/编译
    let (defs, xml) = match compile_generated_dsl(&dsl_text) {
        Ok(result) => result,
        Err(message) => {
            finish_dsl_generation_run(&app_state.db, run_id, &dsl_agent, Err(message.clone()))
                .await;
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "ok": false, "error": message })),
            )
                .into_response();
        }
    };

    let change_summary = change_summary(req.existing_dsl.as_deref(), &defs);
    let mut change_review =
        pending_change_review(req.existing_dsl.as_deref(), &defs, req.base_version);
    add_dsl_vote_review_finding(&mut change_review, model_selection.as_ref());

    let output = json!({"ok": true, "dsl": dsl_text, "xml": xml, "changeSummary": change_summary, "changeReview": change_review, "knowledgeCitations": knowledge_citations, "modelSelection": model_selection});
    finish_dsl_generation_run(&app_state.db, run_id, &dsl_agent, Ok(output.clone())).await;
    (StatusCode::OK, Json(output)).into_response()
}

/// Validate generated DSL all the way through the executable DMN compiler.
#[cfg(not(target_family = "wasm"))]
fn compile_generated_dsl(dsl_text: &str) -> Result<(nl_dsl::Definitions, String), String> {
    let definitions = nl_dsl::parse_definitions(dsl_text)
        .map_err(|error| format!("LLM 生成的 DSL 解析失败: {error}"))?;
    nl_dsl::validate(&definitions).map_err(|error| format!("LLM 生成的 DSL 校验失败: {error}"))?;
    let xml = dmn::generate_dmn_xml(&definitions)
        .map_err(|error| format!("LLM 生成的 DMN XML 失败: {error}"))?;
    let model = executable_dmn::parse_dmn(&xml)
        .map_err(|error| format!("生成的 DMN XML 解析失败: {error}"))?;
    executable_dmn::compile_dmn(&model)
        .map_err(|error| format!("生成的 DMN 模型编译失败: {error}"))?;
    Ok((definitions, xml))
}

#[cfg(not(target_family = "wasm"))]
fn final_dmn_tester_report(xml: &str) -> Result<Value, String> {
    let model =
        executable_dmn::parse_dmn(xml).map_err(|error| format!("DMN XML 解析失败：{error}"))?;
    executable_dmn::compile_dmn(&model).map_err(|error| format!("DMN 模型编译失败：{error}"))?;
    let findings = crate::version_verification::analyze_static_rules(xml)?;
    let suggested_inputs =
        crate::projects::suggested_inputs_for_static_findings(&model, 0, &findings);
    let execution_probes = findings
        .iter()
        .filter_map(|finding| {
            let source = crate::workflow_api::static_analysis_source_reference(0, finding);
            let input = suggested_inputs.get(&source)?.clone();
            let probe = execute_final_dmn_tester_probe(&model, finding, input);
            Some(probe)
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schemaVersion": 1,
        "findingCount": findings.len(),
        "findings": findings,
        "executionProbeCount": execution_probes.len(),
        "executionProbes": execution_probes,
        "expectedOutputsConfirmed": false,
    }))
}

#[cfg(not(target_family = "wasm"))]
fn execute_final_dmn_tester_probe(
    model: &executable_dmn::DmnModel,
    finding: &crate::models::VerificationFinding,
    input: Value,
) -> Value {
    let input_for_report = input.clone();
    let result = typed_json_inputs_to_feel(model, &input)
        .map(|inputs| executable_dmn::DmnExecutor::new(model).evaluate_all(inputs));
    match result {
        Ok(result) => {
            let decisions = result
                .decision_results
                .iter()
                .map(|decision| {
                    json!({
                        "decisionId": decision.decision_id,
                        "decisionName": decision.decision_name,
                        "status": match decision.status {
                            executable_dmn::DecisionStatus::Succeeded => "succeeded",
                            executable_dmn::DecisionStatus::Failed => "failed",
                            executable_dmn::DecisionStatus::Skipped => "skipped",
                        },
                        "observedOutput": feel_value_to_json(&decision.value).ok(),
                        "matchedRuleIds": decision.matched_rule_ids,
                    })
                })
                .collect::<Vec<_>>();
            let messages = result
                .messages
                .iter()
                .map(|message| json!({ "code": message.code, "message": message.message }))
                .collect::<Vec<_>>();
            json!({
                "findingCode": finding.code,
                "element": finding.element,
                "ruleIds": finding.rule_ids,
                "input": input_for_report,
                "executionStatus": match result.status {
                    executable_dmn::DmnResultStatus::Succeeded => "succeeded",
                    executable_dmn::DmnResultStatus::SucceededWithWarnings => "succeeded_with_warnings",
                    executable_dmn::DmnResultStatus::Failed => "failed",
                },
                "observedDecisions": decisions,
                "messages": messages,
                "reviewRequired": true,
            })
        }
        Err(error) => json!({
            "findingCode": finding.code,
            "element": finding.element,
            "ruleIds": finding.rule_ids,
            "input": input_for_report,
            "executionStatus": "input_conversion_failed",
            "error": error,
            "reviewRequired": true,
        }),
    }
}

#[cfg(not(target_family = "wasm"))]
fn tester_static_analysis_degradation(report: &Value) -> Option<Value> {
    match report["status"].as_str()? {
        "budget_skip" => Some(json!({
            "kind": "budget_skip",
            "skippedAgent": "tester",
            "manualReviewRequired": true,
            "reason": report["reason"],
        })),
        "unavailable" if report["manualReviewRequired"] == true => Some(json!({
            "kind": report["degradationKind"],
            "skippedAgent": "tester",
            "manualReviewRequired": true,
            "reason": report["reason"],
        })),
        _ => None,
    }
}

/// SSE 流式完整流水线：step 事件展示生成/校验/编译各阶段，LLM 内容增量实时推送
#[cfg(not(target_family = "wasm"))]
fn run_pipeline_sse(
    dsl_agent: LlmAgent,
    req: GenerateRequest,
    traceability_plan: crate::workflow::DecisionPlan,
    knowledge_citations: Vec<crate::models::KnowledgeCitation>,
    db: sqlx::PgPool,
    run_id: AgentExecutionRun,
    complexity: RequirementComplexity,
) -> Response {
    let (tx, rx) = mpsc::channel::<StreamEvent>(64);
    let event_db = db.clone();
    if let Some(orchestration_id) = run_id.orchestration_id {
        let _ = tx.try_send(StreamEvent::Orchestration(orchestration_id));
    }
    let text = req.text.clone();
    let simple_direct = req.simple_direct;
    let tx2 = tx.clone();
    tokio::spawn(async move {
        let _ = tx2.send(StreamEvent::Stage("engineer".into())).await;
        if simple_direct {
            let _ = tx2
                .send(StreamEvent::Step(
                    "低风险 simple 直达：使用未确认的需求卡结构生成候选；保存草稿前仍需人工检查"
                        .into(),
                ))
                .await;
        }
        let plan_value = serde_json::to_value(&traceability_plan).unwrap_or(Value::Null);
        let plan_digest = format!("{:x}", sha2::Sha256::digest(plan_value.to_string()));
        let _ = tx2
            .send(StreamEvent::Handoff {
                from_role: if simple_direct {
                    "analyst".into()
                } else {
                    "architect".into()
                },
                to_role: "engineer".into(),
                artifact_kind: if simple_direct {
                    "simple_requirement_basis".into()
                } else {
                    "confirmed_decision_plan".into()
                },
                digest: plan_digest,
            })
            .await;
        let _ = tx2
            .send(StreamEvent::Agent {
                role: "engineer".into(),
                status: "running".into(),
            })
            .await;
        // 步骤 1: 自然语言 → DSL（内部已推送 step/delta 事件）
        let generated = crate::workflow_api::run_until_stream_disconnect(
            Some(&tx2),
            async {
                match run_id.orchestration_id {
                    Some(orchestration_id) => {
                        crate::workflow_api::wait_for_orchestration_cancellation(
                            &db,
                            orchestration_id,
                        )
                        .await
                    }
                    None => std::future::pending().await,
                }
            },
            async {
                generate_dsl_for_route(
                    &dsl_agent,
                    &text,
                    &traceability_plan,
                    complexity,
                    Some(&tx2),
                )
                .await
            },
        )
        .await;
        if let Ok((_, Some(consensus))) = &generated {
            if let Some(run_id) = run_id.run_id {
                if let Err(error) = crate::workflow_api::record_model_vote_with_resolution(
                    &db,
                    run_id,
                    "dsl_generation",
                    consensus,
                    Some(dsl_vote_resolution(consensus)),
                )
                .await
                {
                    eprintln!("记录 DSL 多模型投票失败: {error}");
                }
            }
        }
        let model_selection = generated
            .as_ref()
            .ok()
            .and_then(|(_, consensus)| consensus.as_ref().map(dsl_vote_resolution));
        let generated = generated.and_then(require_generated_dsl);
        let generated = match generated {
            Ok(generated) => generated,
            Err(e) => {
                let message = format!("LLM 生成失败: {}", e);
                finish_dsl_generation_run(&db, run_id, &dsl_agent, Err(message.clone())).await;
                let _ = tx2
                    .send(StreamEvent::Agent {
                        role: "engineer".into(),
                        status: "failed".into(),
                    })
                    .await;
                let _ = tx2.send(StreamEvent::Finished(Err(message))).await;
                return;
            }
        };
        let dsl_text = match apply_requirement_traceability(&generated.yaml, &traceability_plan) {
            Ok(yaml) => yaml,
            Err(message) => {
                finish_dsl_generation_run(&db, run_id, &dsl_agent, Err(message.clone())).await;
                let _ = tx2
                    .send(StreamEvent::Agent {
                        role: "engineer".into(),
                        status: "failed".into(),
                    })
                    .await;
                let _ = tx2.send(StreamEvent::Finished(Err(message))).await;
                return;
            }
        };
        if let Err(message) = ensure_generation_snapshot(&db, &req).await {
            finish_dsl_generation_run(&db, run_id, &dsl_agent, Err(message.clone())).await;
            let _ = tx2
                .send(StreamEvent::Agent {
                    role: "engineer".into(),
                    status: "failed".into(),
                })
                .await;
            let _ = tx2.send(StreamEvent::Finished(Err(message))).await;
            return;
        }

        // 步骤 2-4: 校验 DSL、生成 DMN，并由执行引擎重新解析/编译
        let _ = tx2
            .send(StreamEvent::Step("✓ DSL 生成完成，正在校验...".to_string()))
            .await;
        let (defs, xml) = match compile_generated_dsl(&dsl_text) {
            Ok(result) => result,
            Err(message) => {
                finish_dsl_generation_run(&db, run_id, &dsl_agent, Err(message.clone())).await;
                let _ = tx2
                    .send(StreamEvent::Agent {
                        role: "engineer".into(),
                        status: "failed".into(),
                    })
                    .await;
                let _ = tx2.send(StreamEvent::Finished(Err(message))).await;
                return;
            }
        };
        let change_summary = change_summary(None, &defs);
        let mut change_review = pending_change_review(None, &defs, req.base_version);
        add_dsl_vote_review_finding(&mut change_review, model_selection.as_ref());
        let definition_value = serde_json::to_value(&defs).unwrap_or(Value::Null);
        let definition_digest = format!("{:x}", sha2::Sha256::digest(definition_value.to_string()));
        let _ = tx2
            .send(StreamEvent::Artifact {
                kind: "compiled_dmn_candidate".into(),
                digest: definition_digest.clone(),
            })
            .await;
        let _ = tx2
            .send(StreamEvent::Agent {
                role: "engineer".into(),
                status: "completed".into(),
            })
            .await;

        let _ = tx2.send(StreamEvent::Stage("tester".into())).await;
        let _ = tx2
            .send(StreamEvent::Handoff {
                from_role: "engineer".into(),
                to_role: "tester".into(),
                artifact_kind: "compiled_dmn_candidate".into(),
                digest: definition_digest.clone(),
            })
            .await;
        let tester_step_id = if let Some(agent_run_id) = run_id.run_id {
            let input = json!({
                "artifactType": "compiled_dmn_candidate",
                "schemaVersion": 1,
                "definitionDigest": definition_digest,
            });
            match crate::workflow_api::start_orchestration_step(
                &db,
                agent_run_id,
                2,
                "tester",
                "analyze_final_dmn_static_findings",
                None,
                "tester-static-analysis-v1",
                &format!("{:x}", sha2::Sha256::digest(b"tester-static-analysis-v1")),
                &input,
                None,
            )
            .await
            {
                Ok(step_id) => Some(step_id),
                Err(error) => {
                    eprintln!("记录最终 DMN Tester 步骤失败：{error}");
                    None
                }
            }
        } else {
            None
        };
        let tester_gate = match req.project_id {
            Some(project_id) => {
                match record_running_dsl_usage_snapshot(&db, run_id, &dsl_agent).await {
                    Ok(()) => {
                        optional_tester_budget(&db, project_id, run_id.orchestration_id).await
                    }
                    Err(error) => Err(format!("记录 Engineer 实际用量失败：{error}")),
                }
            }
            None => Err("请求缺少项目 ID，无法确认 Tester 预算".into()),
        };
        let tester_static_analysis = match tester_gate {
            Ok(OptionalTesterBudget::Allowed) => {
                let _ = tx2
                    .send(StreamEvent::Agent {
                        role: "tester".into(),
                        status: "running".into(),
                    })
                    .await;
                match final_dmn_tester_report(&xml) {
                    Ok(report) => {
                        if let Some(step_id) = tester_step_id {
                            if let Err(error) = crate::workflow_api::finish_orchestration_step(
                                &db,
                                step_id,
                                Ok(&report),
                            )
                            .await
                            {
                                eprintln!("保存最终 DMN Tester 产物摘要失败：{error}");
                            }
                        }
                        if let Ok(bytes) = serde_json::to_vec(&report) {
                            let digest = format!("{:x}", sha2::Sha256::digest(bytes));
                            let _ = tx2
                                .send(StreamEvent::Artifact {
                                    kind: "tester_static_analysis".into(),
                                    digest,
                                })
                                .await;
                        }
                        let _ = tx2
                            .send(StreamEvent::Agent {
                                role: "tester".into(),
                                status: "completed".into(),
                            })
                            .await;
                        json!({"status": "completed", "report": report})
                    }
                    Err(error) => {
                        if let Some(step_id) = tester_step_id {
                            let _ = crate::workflow_api::finish_orchestration_step(
                                &db,
                                step_id,
                                Err(&error),
                            )
                            .await;
                        }
                        eprintln!("最终 DMN 静态 Tester 检查失败：{error}");
                        let _ = tx2
                            .send(StreamEvent::Agent {
                                role: "tester".into(),
                                status: "failed".into(),
                            })
                            .await;
                        json!({
                            "status": "unavailable",
                            "degradationKind": "tester_unavailable",
                            "manualReviewRequired": true,
                            "reason": error,
                        })
                    }
                }
            }
            Ok(OptionalTesterBudget::Exceeded(reason)) => {
                let skipped = json!({
                    "kind": "budget_skip",
                    "skippedAgent": "tester",
                    "manualReviewRequired": true,
                    "reason": reason,
                });
                if let Some(step_id) = tester_step_id {
                    if let Err(error) =
                        crate::workflow_api::skip_orchestration_step(&db, step_id, &skipped).await
                    {
                        eprintln!("记录预算跳过的 Tester 步骤失败：{error}");
                    }
                }
                let _ = tx2
                    .send(StreamEvent::Agent {
                        role: "tester".into(),
                        status: "skipped".into(),
                    })
                    .await;
                let _ = tx2
                    .send(StreamEvent::Step(
                        "预算门控已跳过可选静态 Tester；请人工复核生成结果".into(),
                    ))
                    .await;
                json!({"status": "budget_skip", "manualReviewRequired": true, "reason": reason})
            }
            Err(error) => {
                let skipped = json!({
                    "kind": "budget_check_unavailable",
                    "skippedAgent": "tester",
                    "manualReviewRequired": true,
                    "reason": error,
                });
                if let Some(step_id) = tester_step_id {
                    if let Err(record_error) =
                        crate::workflow_api::skip_orchestration_step(&db, step_id, &skipped).await
                    {
                        eprintln!("记录待人工复核的 Tester 步骤失败：{record_error}");
                    }
                }
                let _ = tx2
                    .send(StreamEvent::Agent {
                        role: "tester".into(),
                        status: "skipped".into(),
                    })
                    .await;
                let _ = tx2
                    .send(StreamEvent::Step(
                        "预算状态无法确认，已跳过可选 Tester；请人工复核生成结果".into(),
                    ))
                    .await;
                json!({
                    "status": "unavailable",
                    "degradationKind": "budget_check_unavailable",
                    "manualReviewRequired": true,
                    "reason": error,
                })
            }
        };

        let _ = tx2
            .send(StreamEvent::Step(
                "✓ 校验及静态测试检查完成，正在呈现已验证的 DSL...".to_string(),
            ))
            .await;
        stream_dsl_preview(&tx2, &dsl_text).await;
        let degradation = tester_static_analysis_degradation(&tester_static_analysis);
        let output = json!({
            "ok": true,
            "dsl": dsl_text,
            "xml": xml,
            "changeSummary": change_summary,
            "changeReview": change_review,
            "knowledgeCitations": knowledge_citations,
            "testerStaticAnalysis": tester_static_analysis,
            "modelSelection": model_selection,
            "degradation": degradation,
            "simpleDirectPreview": simple_direct,
        });
        finish_dsl_generation_run(&db, run_id, &dsl_agent, Ok(output.clone())).await;
        let _ = tx2.send(StreamEvent::Finished(Ok(output))).await;
    });
    sse_response(rx, run_id.orchestration_id, event_db)
}

/// Tool-call arguments are often buffered by model gateways. Once the DSL has
/// passed validation, emit its normalized YAML line by line as a distinct SSE
/// preview so the UI always has a meaningful progressive rendering.
#[cfg(not(target_family = "wasm"))]
async fn stream_dsl_preview(tx: &mpsc::Sender<StreamEvent>, yaml: &str) {
    for line in yaml.split_inclusive('\n') {
        if tx
            .send(StreamEvent::Preview(line.to_string()))
            .await
            .is_err()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(12)).await;
    }
}

/// 为可被用户直接引用的 DMN 元素建立稳定快照。
/// 校验已保证这些元素在一个 definitions 中名称唯一，因此名称可作为对比键。
#[cfg(not(target_family = "wasm"))]
fn element_snapshots(definitions: &nl_dsl::Definitions) -> BTreeMap<String, String> {
    let mut snapshots = BTreeMap::new();
    macro_rules! snapshot_elements {
        ($elements:expr) => {
            for element in $elements {
                // 所有 DSL 模型类型均可序列化；失败时保留唯一占位值而非中断已完成的生成。
                let snapshot = serde_yaml::to_string(element)
                    .unwrap_or_else(|error| format!("<serialization failed: {error}>"));
                snapshots.insert(element.name.clone(), snapshot);
            }
        };
    }

    snapshot_elements!(&definitions.item_definitions);
    snapshot_elements!(&definitions.inputs);
    snapshot_elements!(&definitions.decisions);
    snapshot_elements!(&definitions.bkms);
    snapshot_elements!(&definitions.decision_services);
    snapshot_elements!(&definitions.knowledge_sources);
    snapshots
}

/// 对比规范化后的 DSL，供前端解释一次增量修改实际影响的元素。
#[cfg(not(target_family = "wasm"))]
fn change_summary(existing_dsl: Option<&str>, updated: &nl_dsl::Definitions) -> Option<Value> {
    let existing = existing_dsl?;
    let before = nl_dsl::parse_definitions(existing).ok()?;
    let before = element_snapshots(&before);
    let after = element_snapshots(updated);
    let before_names: BTreeSet<&String> = before.keys().collect();
    let after_names: BTreeSet<&String> = after.keys().collect();

    let added: Vec<&String> = after_names.difference(&before_names).copied().collect();
    let removed: Vec<&String> = before_names.difference(&after_names).copied().collect();
    let updated: Vec<&String> = before_names
        .intersection(&after_names)
        .filter(|name| before.get(**name) != after.get(**name))
        .copied()
        .collect();

    Some(json!({
        "added": added,
        "updated": updated,
        "removed": removed,
    }))
}

#[cfg(not(target_family = "wasm"))]
fn pending_change_review(
    existing_dsl: Option<&str>,
    updated: &nl_dsl::Definitions,
    base_version: Option<i32>,
) -> crate::models::VersionChangeReview {
    let base = existing_dsl.and_then(|dsl| nl_dsl::parse_definitions(dsl).ok());
    build_change_review(
        base.as_ref(),
        updated,
        None,
        base_version,
        base_version.map(|version| version + 1).or(Some(1)),
    )
}

/// 将 StreamEvent 通道转换为 SSE 响应流
#[cfg(not(target_family = "wasm"))]
fn sse_response(
    rx: mpsc::Receiver<StreamEvent>,
    orchestration_id: Option<i64>,
    db: sqlx::PgPool,
) -> Response {
    let mut cleanup_expired = true;
    let stream = ReceiverStream::new(rx).then(move |evt| {
        let cleanup_this_request = cleanup_expired;
        cleanup_expired = false;
        let db = db.clone();
        let (name, data) = match evt {
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
            StreamEvent::Step(msg) => ("step", json!({ "message": msg })),
            StreamEvent::Delta(content) => ("delta", json!({ "content": content })),
            StreamEvent::Preview(content) => ("preview", json!({ "content": content })),
            StreamEvent::Finished(Ok(payload)) => ("result", payload),
            StreamEvent::Finished(Err(e)) => (
                "error",
                json!({
                    "ok": false,
                    "error": e,
                    "retryable": crate::workflow_api::is_retryable_agent_failure(&e),
                }),
            ),
        };
        async move {
            let mut outgoing = Event::default().event(name).data(data.to_string());
            if let Some(orchestration_id) = orchestration_id {
                if cleanup_this_request {
                    let _ = sqlx::query(
                        "DELETE FROM agent_orchestration_events WHERE expires_at <= NOW()",
                    )
                    .execute(&db)
                    .await;
                }
                if let Ok(event_id) = sqlx::query_scalar::<_, i64>(
                    "INSERT INTO agent_orchestration_events (orchestration_id, event_name, payload_json) VALUES ($1, $2, $3) RETURNING id",
                )
                .bind(orchestration_id)
                .bind(name)
                .bind(&data)
                .fetch_one(&db)
                .await
                {
                    outgoing = outgoing.id(event_id.to_string());
                }
            }
            Ok::<Event, Infallible>(outgoing)
        }
    });
    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    headers.insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    if let Some(orchestration_id) = orchestration_id
        && let Ok(value) = HeaderValue::from_str(&orchestration_id.to_string())
    {
        headers.insert(HeaderName::from_static("x-orchestration-id"), value);
    }
    response
}

#[cfg(not(target_family = "wasm"))]
pub fn dms_routes(state: Arc<LlmState>) -> Router<AppState> {
    Router::new()
        .route("/validate", post(validate_dsl))
        .route("/compile", post(compile_dsl))
        .route("/validate-dmn", post(validate_dmn))
        .route("/analyze-inputs", post(analyze_dmn_inputs))
        .route("/evaluate", post(evaluate_dmn))
        .route("/generate", post(generate_dsl))
        .route("/generate-change-intent", post(generate_change_intent))
        .route("/run", post(run_pipeline))
        .layer(Extension(state))
}

#[cfg(not(target_family = "wasm"))]
pub fn public_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/health", get(health))
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::*;

    #[test]
    fn jsonb_persistence_replaces_embedded_nul_characters() {
        let value = json!({
            "output": "before\0after",
            "nested": ["\0", {"message": "valid"}],
        });

        let sanitized = sanitize_jsonb_value(value);

        assert_eq!(sanitized["output"], "before\u{FFFD}after");
        assert_eq!(sanitized["nested"][0], "\u{FFFD}");
        assert_eq!(sanitized["nested"][1]["message"], "valid");
    }

    #[tokio::test]
    async fn dsl_model_progress_relay_forwards_steps_without_mixing_output() {
        let (model_tx, model_rx) = mpsc::channel(4);
        let (progress_tx, mut progress_rx) = mpsc::channel(4);
        let relay = tokio::spawn(relay_dsl_model_steps(
            "model-a".into(),
            model_rx,
            progress_tx,
        ));

        model_tx
            .send(StreamEvent::Step("生成尝试 1/4".into()))
            .await
            .unwrap();
        model_tx
            .send(StreamEvent::Delta("model-specific DSL".into()))
            .await
            .unwrap();
        drop(model_tx);
        relay.await.unwrap();

        assert!(matches!(
            progress_rx.recv().await,
            Some(StreamEvent::Step(message)) if message == "模型 model-a：生成尝试 1/4"
        ));
        assert!(progress_rx.try_recv().is_err());
    }

    #[test]
    fn only_terminal_sse_events_complete_a_stream() {
        assert!(is_terminal_sse_event_name("result"));
        assert!(is_terminal_sse_event_name("workflow_result"));
        assert!(is_terminal_sse_event_name("error"));
        assert!(!is_terminal_sse_event_name("orchestration"));
        assert!(!is_terminal_sse_event_name("agent"));
        assert!(!is_terminal_sse_event_name("step"));
    }

    #[test]
    fn sse_replay_parses_event_cursors_and_initial_orchestration_id() {
        assert_eq!(
            parse_sse_event_id("id: 42\nevent: step\ndata: {}"),
            Some(42)
        );
        assert_eq!(parse_sse_event_id("id: invalid\nevent: step"), None);
        assert_eq!(parse_sse_event_id("id: 0\nevent: step"), None);
        assert_eq!(
            parse_sse_orchestration_id("id: 1\nevent: orchestration\ndata: {\"id\":91}"),
            Some(91)
        );
        assert_eq!(parse_sse_orchestration_id("event: step\ndata: {}"), None);
    }

    #[test]
    fn sse_end_requires_terminal_event_unless_user_aborted() {
        assert!(finish_sse_stream(false, true).is_ok());
        assert!(finish_sse_stream(true, false).is_ok());
        assert!(finish_sse_stream(true, true).is_ok());
        assert_eq!(
            finish_sse_stream(false, false).unwrap_err(),
            "SSE 连接在收到终态结果前中断；可重试当前阶段"
        );
        assert!(is_retryable_sse_disconnect(
            "SSE 连接在收到终态结果前中断；可重试当前阶段"
        ));
        assert!(!is_retryable_sse_disconnect("模型返回错误"));
    }

    #[tokio::test]
    async fn active_orchestration_retry_conflict_has_stable_http_response() {
        let response = project_token_budget_response(ProjectTokenBudgetError::RetryAlreadyActive);
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["code"], "ORCHESTRATION_RETRY_CONFLICT");
        assert_eq!(payload["ok"], false);
    }

    #[tokio::test]
    async fn missing_upstream_orchestration_is_not_reported_as_a_budget_failure() {
        let response = project_token_budget_response(ProjectTokenBudgetError::Database(
            sqlx::Error::RowNotFound,
        ));
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["code"], "WORKFLOW_ORCHESTRATION_UNAVAILABLE");
        assert!(payload["error"].as_str().unwrap().contains("重新确认方案"));
    }

    #[cfg(not(target_family = "wasm"))]
    fn experiment_test_agent(model: &str) -> LlmAgent {
        LlmAgent::new(
            "test-key".into(),
            "http://localhost/v1".into(),
            model.into(),
            Map::new(),
        )
    }

    #[cfg(target_family = "wasm")]
    #[test]
    fn project_error_preserves_stable_code() {
        let message = format_project_error(
            r#"{"ok":false,"code":"BASE_CONFLICT","error":"stale"}"#,
            "保存版本",
            reqwest::StatusCode::CONFLICT,
        );
        assert_eq!(message, "[BASE_CONFLICT] stale");
    }

    #[test]
    fn task_settings_fall_back_to_global_configuration() {
        let defaults = serde_json::from_value(json!({ "enable_thinking": false })).unwrap();
        let settings = llm_task_settings(
            "OPENAI_ANALYSIS_REQUEST_OPTIONS_JSON",
            "global-model",
            &defaults,
            None,
            None,
        );
        assert_eq!(settings.model, "global-model");
        assert_eq!(settings.request_options, defaults);
    }

    #[test]
    fn task_settings_override_only_the_selected_task() {
        let defaults = serde_json::from_value(json!({ "temperature": 0.1 })).unwrap();
        let settings = llm_task_settings(
            "OPENAI_CHANGE_INTENT_REQUEST_OPTIONS_JSON",
            "global-model",
            &defaults,
            Some("patch-model"),
            Some(r#"{"enable_thinking":false}"#),
        );
        assert_eq!(settings.model, "patch-model");
        assert_eq!(
            settings.request_options,
            serde_json::from_value(json!({ "enable_thinking": false })).unwrap()
        );
        assert_eq!(
            defaults,
            serde_json::from_value(json!({ "temperature": 0.1 })).unwrap()
        );
    }

    #[test]
    fn configured_candidates_are_selected_by_policy_and_task_overrides_win() {
        let defaults = Map::new();
        let candidates = model_candidates_from_raw(
            "global-model",
            Some(
                r#"[
                    {"name":"cheap","estimated_cost_usd":0.001,"success_rate":0.72},
                    {"name":"accurate","estimated_cost_usd":0.02,"success_rate":0.96}
                ]"#,
            ),
        );
        let cost_settings = llm_task_settings_with_candidates(
            "options",
            "global-model",
            &defaults,
            None,
            None,
            &candidates,
            ModelSelectionPolicy::CostOptimized,
            TaskType::Analyze,
            Vec::new(),
        );
        assert_eq!(cost_settings.model, "cheap");

        let quality_settings = llm_task_settings_with_candidates(
            "options",
            "global-model",
            &defaults,
            Some("task-model"),
            None,
            &candidates,
            ModelSelectionPolicy::QualityFirst,
            TaskType::Dsl,
            Vec::new(),
        );
        assert_eq!(quality_settings.model, "task-model");
    }

    #[test]
    fn estimated_call_cost_limit_filters_primary_and_fallback_models() {
        let defaults = Map::new();
        let candidates = model_candidates_from_raw(
            "global-model",
            Some(
                r#"[
                    {"name":"global-model","estimated_cost_usd":0.004,"success_rate":0.90},
                    {"name":"cheap","estimated_cost_usd":0.001,"success_rate":0.80},
                    {"name":"expensive","estimated_cost_usd":0.02,"success_rate":0.99}
                ]"#,
            ),
        );
        let settings = llm_task_settings_with_cost_limit(
            "options",
            "global-model",
            &defaults,
            None,
            None,
            &candidates,
            ModelSelectionPolicy::QualityFirst,
            TaskType::Analyze,
            vec!["cheap".into(), "expensive".into(), "unpriced".into()],
            Some(0.005),
        )
        .unwrap();

        assert_eq!(settings.model, "global-model");
        assert_eq!(settings.fallback_models, ["cheap"]);
        assert_eq!(settings.estimated_cost_per_response_usd, Some(0.004));
    }

    #[test]
    fn estimated_call_cost_limit_rejects_unpriced_explicit_model() {
        let error = llm_task_settings_with_cost_limit(
            "options",
            "global-model",
            &Map::new(),
            Some("manual-model"),
            None,
            &model_candidates_from_raw("global-model", None),
            ModelSelectionPolicy::BalancedMode,
            TaskType::Analyze,
            Vec::new(),
            Some(0.01),
        )
        .unwrap_err();

        assert!(error.contains("manual-model"));
    }

    #[test]
    fn daily_cost_estimate_requires_prices_for_primary_and_fallback_models() {
        let settings = llm_task_settings_with_candidates(
            "options",
            "primary",
            &Map::new(),
            None,
            None,
            &model_candidates_from_raw(
                "primary",
                Some(r#"[{"name":"primary","estimated_cost_usd":0.003},{"name":"backup"}]"#),
            ),
            ModelSelectionPolicy::BalancedMode,
            TaskType::Analyze,
            vec!["backup".into()],
        );

        assert_eq!(settings.estimated_cost_per_response_usd, None);
    }

    #[test]
    fn estimated_call_cost_limit_rejects_invalid_values() {
        assert!(estimated_call_cost_limit_from_raw(Some("NaN")).is_err());
        assert!(estimated_call_cost_limit_from_raw(Some("-0.01")).is_err());
        assert_eq!(estimated_call_cost_limit_from_raw(Some("0")), Ok(Some(0.0)));
        assert_eq!(estimated_call_cost_limit_from_raw(None), Ok(None));
    }

    #[test]
    fn project_daily_token_budget_requires_a_non_negative_integer() {
        assert_eq!(project_daily_token_budget_from_raw(None), Ok(None));
        assert_eq!(
            project_daily_token_budget_from_raw(Some(" 12000 ")),
            Ok(Some(12000))
        );
        assert!(project_daily_token_budget_from_raw(Some("-1")).is_err());
        assert!(project_daily_token_budget_from_raw(Some("12.5")).is_err());
    }

    #[test]
    fn completion_token_limit_requires_a_bounded_positive_integer() {
        assert_eq!(max_completion_tokens_from_raw(None), Ok(None));
        assert_eq!(
            max_completion_tokens_from_raw(Some(" 4096 ")),
            Ok(Some(4096))
        );
        assert!(max_completion_tokens_from_raw(Some("0")).is_err());
        assert!(max_completion_tokens_from_raw(Some("1000001")).is_err());
        assert!(max_completion_tokens_from_raw(Some("NaN")).is_err());
    }

    #[test]
    fn project_daily_estimated_cost_budget_requires_a_non_negative_finite_number() {
        assert_eq!(project_daily_estimated_cost_budget_from_raw(None), Ok(None));
        assert_eq!(
            project_daily_estimated_cost_budget_from_raw(Some(" 1.25 ")),
            Ok(Some(1.25))
        );
        assert!(project_daily_estimated_cost_budget_from_raw(Some("NaN")).is_err());
        assert!(project_daily_estimated_cost_budget_from_raw(Some("-0.1")).is_err());
    }

    #[test]
    fn orchestration_estimated_cost_budget_requires_a_non_negative_finite_number() {
        assert_eq!(orchestration_estimated_cost_budget_from_raw(None), Ok(None));
        assert_eq!(
            orchestration_estimated_cost_budget_from_raw(Some(" 1.25 ")),
            Ok(Some(1.25))
        );
        assert!(orchestration_estimated_cost_budget_from_raw(Some("NaN")).is_err());
        assert!(orchestration_estimated_cost_budget_from_raw(Some("-0.1")).is_err());
    }

    #[test]
    fn orchestration_observed_token_budget_requires_a_positive_integer() {
        assert_eq!(orchestration_observed_token_budget_from_raw(None), Ok(None));
        assert_eq!(
            orchestration_observed_token_budget_from_raw(Some(" 250000 ")),
            Ok(Some(250_000))
        );
        assert!(orchestration_observed_token_budget_from_raw(Some("0")).is_err());
        assert!(orchestration_observed_token_budget_from_raw(Some("-1")).is_err());
        assert!(orchestration_observed_token_budget_from_raw(Some("1.5")).is_err());
    }

    #[test]
    fn estimated_cost_budget_math_fails_closed_and_accounts_for_active_reservations() {
        assert!(!estimated_cost_budget_exceeded(0.0, 0.5, 1.0));
        assert!(!estimated_cost_budget_exceeded(1.0, 0.0, 1.0));
        assert!(!estimated_cost_budget_exceeded(0.1, 0.2, 0.3));
        assert!(estimated_cost_budget_exceeded(0.1, 0.200_001, 0.3));
        assert!(estimated_cost_budget_exceeded(0.0, 1.0e16 + 8.0, 1.0e16));
        assert!(estimated_cost_budget_exceeded(1.0e16, 1.0e-16, 1.0e16));
        assert!(estimated_cost_budget_exceeded(0.6, 0.5, 1.0));
        assert!(estimated_cost_budget_exceeded(1.0, 0.01, 1.0));
        assert!(estimated_cost_budget_exceeded(f64::NAN, 0.0, 1.0));
        assert!(estimated_cost_budget_exceeded(f64::INFINITY, 0.0, 1.0));
    }

    #[test]
    fn running_budget_reservation_counts_zero_usage_and_one_response_cost() {
        let reservation = project_budget_reservation(Some(0.125));
        assert_eq!(reservation["request_count"], 0);
        assert_eq!(reservation["total_tokens"], 0);
        assert_eq!(reservation["estimated_cost_usd"], 0.125);
        assert_eq!(reservation["budget_reservation"], true);
        assert_eq!(estimated_run_cost_reservation(Some(0.125), 3), Some(0.375));
        assert_eq!(estimated_run_cost_reservation(None, 3), None);
    }

    #[test]
    fn analysis_experiment_requires_name_and_treatment_model() {
        assert_eq!(
            analysis_experiment_config_from_values(None, None, None),
            Ok(None)
        );
        assert!(analysis_experiment_config_from_values(Some("trial"), None, None).is_err());
        assert_eq!(
            analysis_experiment_config_from_values(Some(" trial "), Some(" model-b "), None),
            Ok(Some(("trial".into(), "model-b".into(), 50)))
        );
        assert_eq!(
            analysis_experiment_config_from_values(Some("trial"), Some("model-b"), Some("100")),
            Ok(Some(("trial".into(), "model-b".into(), 100)))
        );
        assert!(
            analysis_experiment_config_from_values(Some("trial"), Some("model-b"), Some("101"))
                .is_err()
        );
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn analysis_routes_by_complexity_and_uses_project_stable_treatment_models() {
        let state = LlmState {
            analysis_agent: Some(experiment_test_agent("analysis-standard")),
            analysis_treatment_agent: Some(experiment_test_agent("analysis-treatment")),
            analysis_simple_agent: Some(experiment_test_agent("analysis-simple")),
            analysis_complex_agent: Some(experiment_test_agent("analysis-complex")),
            analysis_treatment_simple_agent: Some(experiment_test_agent("treatment-simple")),
            analysis_treatment_complex_agent: Some(experiment_test_agent("treatment-complex")),
            analysis_experiment_name: Some("analysis-trial".into()),
            analysis_treatment_percent: 100,
            plan_agent: None,
            plan_simple_agent: None,
            plan_treatment_agent: None,
            plan_experiment_name: None,
            plan_treatment_percent: 0,
            dsl_agent: None,
            dsl_simple_agent: None,
            dsl_treatment_agent: None,
            dsl_experiment_name: None,
            dsl_treatment_percent: 0,
            change_intent_agent: None,
            change_intent_treatment_agent: None,
            change_intent_experiment_name: None,
            change_intent_treatment_percent: 0,
            optimization_proposal_agent: None,
        };

        let (agent, assignment) = state
            .analysis_agent_for_project(42, false, AgentComplexity::Simple)
            .unwrap();
        assert_eq!(agent.model(), "analysis-simple");
        assert_eq!(assignment, None);

        let (agent, assignment) = state
            .analysis_agent_for_project(42, false, AgentComplexity::Standard)
            .unwrap();
        assert_eq!(agent.model(), "analysis-standard");
        assert_eq!(assignment, None);

        let (agent, assignment) = state
            .analysis_agent_for_project(42, false, AgentComplexity::Complex)
            .unwrap();
        assert_eq!(agent.model(), "analysis-complex");
        assert_eq!(assignment, None);

        let (agent, assignment) = state
            .analysis_agent_for_project(42, true, AgentComplexity::Simple)
            .unwrap();
        assert_eq!(agent.model(), "treatment-simple");
        assert_eq!(assignment.unwrap().variant, "treatment");

        let (agent, assignment) = state
            .analysis_agent_for_project(42, true, AgentComplexity::Standard)
            .unwrap();
        assert_eq!(agent.model(), "analysis-treatment");
        assert_eq!(assignment.unwrap().variant, "treatment");

        let (agent, assignment) = state
            .analysis_agent_for_project(42, true, AgentComplexity::Complex)
            .unwrap();
        assert_eq!(agent.model(), "treatment-complex");
        assert_eq!(assignment.unwrap().variant, "treatment");
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn unavailable_analysis_treatment_disables_experiment_assignment() {
        let state = LlmState {
            analysis_agent: Some(experiment_test_agent("analysis-standard")),
            analysis_treatment_agent: None,
            analysis_simple_agent: None,
            analysis_complex_agent: None,
            analysis_treatment_simple_agent: None,
            analysis_treatment_complex_agent: None,
            analysis_experiment_name: Some("analysis-trial".into()),
            analysis_treatment_percent: 100,
            plan_agent: None,
            plan_simple_agent: None,
            plan_treatment_agent: None,
            plan_experiment_name: None,
            plan_treatment_percent: 0,
            dsl_agent: None,
            dsl_simple_agent: None,
            dsl_treatment_agent: None,
            dsl_experiment_name: None,
            dsl_treatment_percent: 0,
            change_intent_agent: None,
            change_intent_treatment_agent: None,
            change_intent_experiment_name: None,
            change_intent_treatment_percent: 0,
            optimization_proposal_agent: None,
        };

        let (agent, assignment) = state
            .analysis_agent_for_project(42, true, AgentComplexity::Standard)
            .unwrap();

        assert_eq!(agent.model(), "analysis-standard");
        assert_eq!(assignment, None);
    }

    #[test]
    fn plan_experiment_requires_name_and_treatment_model() {
        assert_eq!(
            plan_experiment_config_from_values(None, None, None),
            Ok(None)
        );
        assert!(plan_experiment_config_from_values(Some("trial"), None, None).is_err());
        assert_eq!(
            plan_experiment_config_from_values(Some(" plan-trial "), Some(" model-b "), Some("25")),
            Ok(Some(("plan-trial".into(), "model-b".into(), 25)))
        );
        assert!(
            plan_experiment_config_from_values(Some("trial"), Some("model-b"), Some("101"))
                .is_err()
        );
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn plan_experiment_routes_to_treatment_and_returns_experiment_metadata() {
        let state = LlmState {
            analysis_agent: None,
            analysis_treatment_agent: None,
            analysis_simple_agent: None,
            analysis_complex_agent: None,
            analysis_treatment_simple_agent: None,
            analysis_treatment_complex_agent: None,
            analysis_experiment_name: None,
            analysis_treatment_percent: 0,
            plan_agent: Some(experiment_test_agent("plan-control")),
            plan_simple_agent: Some(experiment_test_agent("plan-simple")),
            plan_treatment_agent: Some(experiment_test_agent("plan-treatment")),
            plan_experiment_name: Some("plan-trial".into()),
            plan_treatment_percent: 100,
            dsl_agent: None,
            dsl_simple_agent: None,
            dsl_treatment_agent: None,
            dsl_experiment_name: None,
            dsl_treatment_percent: 0,
            change_intent_agent: None,
            change_intent_treatment_agent: None,
            change_intent_experiment_name: None,
            change_intent_treatment_percent: 0,
            optimization_proposal_agent: None,
        };

        let (simple_agent, simple_experiment) = state
            .plan_agent_for_project(42, false, AgentComplexity::Simple)
            .unwrap();
        assert_eq!(simple_agent.model(), "plan-simple");
        assert_eq!(simple_experiment, None);

        let (agent, experiment) = state
            .plan_agent_for_project(42, true, AgentComplexity::Standard)
            .unwrap();

        assert_eq!(agent.model(), "plan-treatment");
        assert_eq!(
            experiment.map(|assignment| (
                assignment.experiment_name,
                assignment.variant,
                assignment.treatment_percent
            )),
            Some(("plan-trial", "treatment", 100))
        );
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn plan_experiment_can_be_disabled_for_high_risk_projects() {
        let state = LlmState {
            analysis_agent: None,
            analysis_treatment_agent: None,
            analysis_simple_agent: None,
            analysis_complex_agent: None,
            analysis_treatment_simple_agent: None,
            analysis_treatment_complex_agent: None,
            analysis_experiment_name: None,
            analysis_treatment_percent: 0,
            plan_agent: Some(experiment_test_agent("plan-control")),
            plan_simple_agent: None,
            plan_treatment_agent: Some(experiment_test_agent("plan-treatment")),
            plan_experiment_name: Some("plan-trial".into()),
            plan_treatment_percent: 100,
            dsl_agent: None,
            dsl_simple_agent: None,
            dsl_treatment_agent: None,
            dsl_experiment_name: None,
            dsl_treatment_percent: 0,
            change_intent_agent: None,
            change_intent_treatment_agent: None,
            change_intent_experiment_name: None,
            change_intent_treatment_percent: 0,
            optimization_proposal_agent: None,
        };

        let (agent, experiment) = state
            .plan_agent_for_project(42, false, AgentComplexity::Standard)
            .unwrap();

        assert_eq!(agent.model(), "plan-control");
        assert_eq!(experiment, None);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn dsl_and_change_intent_experiments_use_project_stable_routing() {
        let dsl_control = experiment_test_agent("dsl-control");
        let dsl_treatment = experiment_test_agent("dsl-treatment");
        let change_control = experiment_test_agent("change-control");
        let change_treatment = experiment_test_agent("change-treatment");

        let (agent, experiment) = experiment_agent_for_project(
            Some(&dsl_control),
            Some(&dsl_treatment),
            Some("dsl-trial"),
            100,
            42,
            true,
        )
        .unwrap();
        assert_eq!(agent.model(), "dsl-treatment");
        assert_eq!(
            experiment.map(|assignment| (
                assignment.experiment_name,
                assignment.variant,
                assignment.treatment_percent
            )),
            Some(("dsl-trial", "treatment", 100))
        );

        let (agent, experiment) = experiment_agent_for_project(
            Some(&change_control),
            Some(&change_treatment),
            Some("change-trial"),
            100,
            42,
            false,
        )
        .unwrap();
        assert_eq!(agent.model(), "change-control");
        assert_eq!(experiment, None);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn simple_dsl_uses_optional_lightweight_model_without_changing_complex_voting_route() {
        let state = LlmState {
            analysis_agent: None,
            analysis_treatment_agent: None,
            analysis_simple_agent: None,
            analysis_complex_agent: None,
            analysis_treatment_simple_agent: None,
            analysis_treatment_complex_agent: None,
            analysis_experiment_name: None,
            analysis_treatment_percent: 0,
            plan_agent: None,
            plan_simple_agent: None,
            plan_treatment_agent: None,
            plan_experiment_name: None,
            plan_treatment_percent: 0,
            dsl_agent: Some(experiment_test_agent("dsl-standard")),
            dsl_simple_agent: Some(experiment_test_agent("dsl-simple")),
            dsl_treatment_agent: Some(experiment_test_agent("dsl-treatment")),
            dsl_experiment_name: Some("dsl-trial".into()),
            dsl_treatment_percent: 100,
            change_intent_agent: None,
            change_intent_treatment_agent: None,
            change_intent_experiment_name: None,
            change_intent_treatment_percent: 0,
            optimization_proposal_agent: None,
        };

        let (agent, experiment) = state
            .dsl_agent_for_project(42, false, AgentComplexity::Simple)
            .unwrap();
        assert_eq!(agent.model(), "dsl-simple");
        assert_eq!(experiment, None);

        let (agent, experiment) = state
            .dsl_agent_for_project(42, false, AgentComplexity::Complex)
            .unwrap();
        assert_eq!(agent.model(), "dsl-standard");
        assert_eq!(experiment, None);

        let (agent, experiment) = state
            .dsl_agent_for_project(42, true, AgentComplexity::Simple)
            .unwrap();
        assert_eq!(agent.model(), "dsl-treatment");
        assert_eq!(experiment.unwrap().variant, "treatment");
    }

    #[test]
    fn invalid_candidate_configuration_keeps_default_model() {
        let candidates = model_candidates_from_raw("global-model", Some("not-json"));
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "global-model");
    }

    #[test]
    fn fallback_model_list_is_trimmed_and_deduplicated() {
        assert_eq!(
            fallback_models_from_raw(Some(" backup-a, backup-b,backup-c,backup-d,backup-a ,, ")),
            vec!["backup-a", "backup-b", "backup-c"]
        );
        assert!(fallback_models_from_raw(None).is_empty());
    }

    #[test]
    fn task_model_selection_policy_overrides_global_policy() {
        assert_eq!(
            model_selection_policy_from_values(Some("quality_first"), Some("cost_optimized")),
            ModelSelectionPolicy::QualityFirst
        );
        assert_eq!(
            model_selection_policy_from_values(None, Some("cost_optimized")),
            ModelSelectionPolicy::CostOptimized
        );
        assert_eq!(
            model_selection_policy_from_values(None, None),
            ModelSelectionPolicy::BalancedMode
        );
    }

    #[test]
    fn invalid_task_options_fall_back_to_global_options() {
        let defaults = serde_json::from_value(json!({ "enable_thinking": false })).unwrap();
        let settings = llm_task_settings(
            "OPENAI_DSL_REQUEST_OPTIONS_JSON",
            "global-model",
            &defaults,
            Some("dsl-model"),
            Some("[]"),
        );
        assert_eq!(settings.model, "dsl-model");
        assert_eq!(settings.request_options, defaults);
    }

    #[test]
    fn patch_reasoning_model_detection_is_scoped() {
        assert!(is_deepseek_reasoning_model("deepseek-v4-flash"));
        assert!(is_deepseek_reasoning_model("deepseek-chat"));
        assert!(is_deepseek_reasoning_model("provider/deepseek-v3.2"));
        assert!(is_deepseek_reasoning_model("deepseek-reasoner"));
        assert!(!is_deepseek_reasoning_model("gpt-4o-mini"));
        assert!(!has_thinking_option(&Map::new()));
        assert!(has_thinking_option(
            &serde_json::from_value(json!({"thinking": {"type": "enabled"}})).unwrap()
        ));
    }

    #[test]
    fn patch_requests_always_disable_thinking() {
        let mut options = serde_json::from_value(json!({
            "thinking": {"type": "enabled"},
            "enable_thinking": true,
            "temperature": 0.1
        }))
        .unwrap();
        disable_thinking(&mut options);
        assert_eq!(options.get("enable_thinking"), Some(&Value::Bool(false)));
        assert!(!options.contains_key("thinking"));
        assert_eq!(options.get("temperature"), Some(&json!(0.1)));
    }

    fn request(text: String, existing_dsl: Option<String>) -> GenerateRequest {
        GenerateRequest {
            text,
            workflow_updated_at: Some(chrono::Utc::now()),
            project_id: None,
            existing_dsl,
            base_version: None,
            base_revision: None,
            repair_error: None,
            repair_attempt: None,
            previous_intent: None,
            retry_of_orchestration_id: None,
            simple_direct: false,
        }
    }

    #[test]
    fn generation_requires_explicit_workflow_snapshot() {
        let mut payload = request("按确认方案生成模型".into(), None);
        payload.workflow_updated_at = None;
        let error = validate_generate_request(&payload).unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(
            error.1.0["error"]
                .as_str()
                .unwrap()
                .contains("workflowUpdatedAt")
        );
    }

    #[test]
    fn generate_request_rejects_blank_prompt() {
        let error = validate_generate_request(&request("  \n\t".to_string(), None))
            .expect_err("blank prompts must be rejected");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(error.1.0["error"], "请求描述不能为空");
    }

    #[test]
    fn generate_request_rejects_oversized_payloads() {
        let prompt_error = validate_generate_request(&request("a".repeat(20_001), None))
            .expect_err("oversized prompts must be rejected");
        assert_eq!(prompt_error.0, StatusCode::PAYLOAD_TOO_LARGE);

        let dsl_error = validate_generate_request(&request(
            "create a model".to_string(),
            Some("a".repeat(200_001)),
        ))
        .expect_err("oversized existing DSL must be rejected");
        assert_eq!(dsl_error.0, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn generate_request_accepts_boundary_sizes() {
        assert!(
            validate_generate_request(&request("a".repeat(20_000), Some("b".repeat(200_000)),))
                .is_ok()
        );
    }

    #[test]
    fn change_intent_repair_requires_matching_error_and_attempt() {
        let mut missing_attempt = request("update model".to_string(), None);
        missing_attempt.repair_error = Some("[DMN_COMPILE] invalid binding".to_string());
        assert_eq!(
            validate_change_intent_request(&missing_attempt)
                .expect_err("compiler feedback without an attempt must be rejected")
                .0,
            StatusCode::BAD_REQUEST
        );

        let mut missing_error = request("update model".to_string(), None);
        missing_error.repair_attempt = Some(1);
        assert_eq!(
            validate_change_intent_request(&missing_error)
                .expect_err("repair attempt without compiler feedback must be rejected")
                .0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn change_intent_repair_accepts_two_bounded_attempts() {
        for attempt in 1..=2 {
            let mut req = request("update model".to_string(), None);
            req.repair_error = Some("[DMN_COMPILE] invalid binding".to_string());
            req.repair_attempt = Some(attempt);
            assert!(validate_change_intent_request(&req).is_ok());
        }

        let mut excessive = request("update model".to_string(), None);
        excessive.repair_error = Some("[DMN_COMPILE] invalid binding".to_string());
        excessive.repair_attempt = Some(3);
        assert_eq!(
            validate_change_intent_request(&excessive)
                .expect_err("repair attempts above the client limit must be rejected")
                .0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn engineer_retry_fingerprint_tracks_original_request_context() {
        let mut request = GenerateRequest {
            text: "生成审批决策".into(),
            workflow_updated_at: Some(chrono::Utc::now()),
            project_id: Some(7),
            existing_dsl: None,
            base_version: None,
            base_revision: Some(13),
            repair_error: None,
            repair_attempt: None,
            previous_intent: None,
            retry_of_orchestration_id: None,
            simple_direct: false,
        };
        let original = generation_request_fingerprint(&request, None);
        request.retry_of_orchestration_id = Some(22);
        assert_eq!(original, generation_request_fingerprint(&request, None));
        request.text.push('。');
        assert_ne!(original, generation_request_fingerprint(&request, None));
    }

    #[test]
    fn engineer_retry_rejects_non_positive_orchestration_ids() {
        let request = GenerateRequest {
            text: "生成审批决策".into(),
            workflow_updated_at: Some(chrono::Utc::now()),
            project_id: Some(7),
            existing_dsl: None,
            base_version: None,
            base_revision: None,
            repair_error: None,
            repair_attempt: None,
            previous_intent: None,
            retry_of_orchestration_id: Some(0),
            simple_direct: false,
        };
        assert_eq!(
            validate_generate_request(&request).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn change_summary_reports_added_updated_and_removed_elements() {
        let existing = r#"
name: LoanModel
inputs:
  - name: Age
    typeRef: number
  - name: LegacyScore
    typeRef: number
decisions:
  - name: Approval
    typeRef: string
    logic:
      type: literalExpression
      expression: '"Reject"'
"#;
        let updated = nl_dsl::parse_definitions(
            r#"
name: LoanModel
inputs:
  - name: Age
    typeRef: number
  - name: Income
    typeRef: number
decisions:
  - name: Approval
    typeRef: string
    logic:
      type: literalExpression
      expression: '"Approve"'
"#,
        )
        .expect("updated DSL should parse");

        let summary = change_summary(Some(existing), &updated)
            .expect("incremental changes should produce a summary");

        assert_eq!(summary["added"], json!(["Income"]));
        assert_eq!(summary["updated"], json!(["Approval"]));
        assert_eq!(summary["removed"], json!(["LegacyScore"]));
    }

    #[test]
    fn input_analysis_uses_dmn_variable_name_as_the_json_key() {
        let model = executable_dmn::parse_dmn(
            r#"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/">
                <itemDefinition name="Applicant"><itemComponent name="age"><typeRef>number</typeRef></itemComponent></itemDefinition>
                <inputData id="applicant-input" name="Applicant data"><variable name="applicant" typeRef="Applicant"/></inputData>
                <inputData id="active-input" name="Active"><variable name="Active" typeRef="boolean"/></inputData>
            </definitions>"#,
        )
        .expect("DMN should parse");
        executable_dmn::compile_dmn(&model).expect("DMN should compile");

        let analysis = analyze_model_inputs(&model).expect("inputs should be analyzable");

        assert_eq!(analysis.template["applicant"], json!({ "age": 1 }));
        assert_eq!(analysis.template["Active"], json!(false));
        assert!(analysis.template.get("Applicant data").is_none());
    }

    #[test]
    fn input_analysis_initializes_collection_templates() {
        let model = executable_dmn::parse_dmn(
            r#"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/">
                <itemDefinition name="Risk" isCollection="true"><typeRef>string</typeRef><allowedValues><text>"Low", "High"</text></allowedValues></itemDefinition>
                <inputData id="risk-input" name="Risks"><variable name="risks" typeRef="Risk"/></inputData>
            </definitions>"#,
        )
        .expect("DMN should parse");
        executable_dmn::compile_dmn(&model).expect("DMN should compile");

        let analysis = analyze_model_inputs(&model).expect("inputs should be analyzable");

        assert_eq!(analysis.template["risks"], json!([]));
    }

    #[test]
    fn evaluate_input_conversion_preserves_declared_boolean_type() {
        let model = executable_dmn::parse_dmn(
            r##"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="m" name="Deployment">
                <inputData id="production-input" name="Production"><variable name="IsProductionEnvironment" typeRef="feel:boolean"/></inputData>
                <decision id="mode" name="Mode"><informationRequirement><requiredInput href="#production-input"/></informationRequirement>
                    <decisionTable hitPolicy="FIRST"><input><inputExpression typeRef="feel:boolean"><text>IsProductionEnvironment</text></inputExpression></input>
                        <output name="mode" typeRef="feel:string"/><rule id="prod"><inputEntry><text>true</text></inputEntry><outputEntry><text>"production"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
        )
        .expect("DMN should parse");
        executable_dmn::compile_dmn(&model).expect("DMN should compile");
        let inputs = typed_json_inputs_to_feel(&model, &json!({"IsProductionEnvironment": true}))
            .expect("declared boolean input should convert");
        assert!(matches!(
            inputs.get("IsProductionEnvironment"),
            Some(crate::feel::FeelValue::Bool(true))
        ));
        let result = executable_dmn::DmnExecutor::new(&model).evaluate_all(inputs);
        assert!(!result.has_errors(), "{:?}", result.messages);
    }

    #[tokio::test]
    async fn evaluate_endpoint_preserves_declared_boolean_type() {
        let xml = r##"<definitions xmlns="https://www.omg.org/spec/DMN/20230324/MODEL/" id="m" name="Deployment">
            <inputData id="production-input" name="Production"><variable name="IsProductionEnvironment" typeRef="feel:boolean"/></inputData>
            <decision id="mode" name="Mode"><informationRequirement><requiredInput href="#production-input"/></informationRequirement>
                <decisionTable hitPolicy="FIRST"><input><inputExpression typeRef="feel:boolean"><text>IsProductionEnvironment</text></inputExpression></input>
                    <output name="mode" typeRef="feel:string"/><rule id="prod"><inputEntry><text>true</text></inputEntry><outputEntry><text>"production"</text></outputEntry></rule>
                </decisionTable>
            </decision>
        </definitions>"##;
        let request = json!({
            "xml": xml,
            "inputs": { "IsProductionEnvironment": true }
        });

        let (status, Json(response)) = evaluate_dmn(Bytes::from(
            serde_json::to_vec(&request).expect("request should serialize"),
        ))
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["ok"], json!(true));
        assert_eq!(response["status"], json!("Succeeded"));
        assert_eq!(response["decisions"][0]["value"], json!("\"production\""));
    }

    #[test]
    fn analyze_inputs_result_accepts_error_payload_without_input_fields() {
        let result: Value = serde_json::from_value(json!({
            "ok": false,
            "error": "DMN XML 不能为空"
        }))
        .expect("error responses should remain deserializable");
        assert_eq!(result["ok"], json!(false));
        assert!(result.get("inputs").is_none());
        assert_eq!(result["error"], json!("DMN XML 不能为空"));
    }

    #[test]
    fn compile_generated_dsl_round_trips_through_execution_engine() {
        let yaml = include_str!("loan.yaml");
        let (definitions, xml) = compile_generated_dsl(yaml).expect("generated DSL should verify");

        assert!(!definitions.decisions.is_empty());
        let model = executable_dmn::parse_dmn(&xml).expect("generated XML should parse");
        executable_dmn::compile_dmn(&model).expect("generated XML should compile");
    }

    #[test]
    fn final_dmn_tester_returns_static_findings_for_the_compiled_candidate() {
        let (_, xml) =
            compile_generated_dsl(include_str!("loan.yaml")).expect("fixture DSL should compile");

        let report = final_dmn_tester_report(&xml).expect("compiled candidate should be testable");

        assert_eq!(report["schemaVersion"], json!(1));
        assert_eq!(
            report["findingCount"].as_u64(),
            report["findings"]
                .as_array()
                .map(|items| items.len() as u64)
        );
        assert!(final_dmn_tester_report("<invalid").is_err());
    }

    #[test]
    fn final_dmn_tester_executes_verified_witnesses_without_inventing_expected_outputs() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="score-input" name="Score"><variable name="Score" typeRef="number"/></inputData>
                <decision id="result" name="Result">
                    <informationRequirement><requiredInput href="#score-input"/></informationRequirement>
                    <decisionTable hitPolicy="UNIQUE">
                        <input><inputExpression typeRef="number"><text>Score</text></inputExpression></input>
                        <input><inputExpression typeRef="number"><text>Score</text></inputExpression></input>
                        <output name="Band" typeRef="string"/>
                        <rule id="r1"><inputEntry><text>&gt; 10</text></inputEntry><inputEntry><text>&lt; 20</text></inputEntry><outputEntry><text>"A"</text></outputEntry></rule>
                        <rule id="r2"><inputEntry><text>&gt; 15</text></inputEntry><inputEntry><text>&lt; 25</text></inputEntry><outputEntry><text>"B"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );

        let report = final_dmn_tester_report(&xml).unwrap();
        let probes = report["executionProbes"].as_array().unwrap();

        assert_eq!(report["expectedOutputsConfirmed"], false);
        assert!(report["executionProbeCount"].as_u64().unwrap() >= 2);
        assert!(probes.iter().any(|probe| {
            probe["findingCode"] == "UNIQUE_RULE_CONFLICT"
                && probe["executionStatus"] == "failed"
                && probe["input"].is_object()
        }));
        assert!(probes.iter().any(|probe| {
            probe["findingCode"] == "MISSING_COVERAGE"
                && probe["executionStatus"] == "succeeded"
                && probe["observedDecisions"][0]["matchedRuleIds"] == json!([])
                && probe["observedDecisions"][0]["observedOutput"].is_null()
        }));
        assert!(probes.iter().all(|probe| {
            probe["reviewRequired"] == true && probe.get("expectedOutput").is_none()
        }));
    }

    #[test]
    fn budget_skipped_tester_is_explicitly_degraded_and_requires_review() {
        let report = json!({
            "status": "budget_skip",
            "manualReviewRequired": true,
            "reason": "本编排树已达到累计 Token 门槛",
        });

        let degradation = tester_static_analysis_degradation(&report).unwrap();

        assert_eq!(degradation["kind"], "budget_skip");
        assert_eq!(degradation["skippedAgent"], "tester");
        assert_eq!(degradation["manualReviewRequired"], true);
        assert_eq!(degradation["reason"], report["reason"]);
    }

    #[test]
    fn unverified_budget_gate_skips_tester_without_claiming_budget_exceeded() {
        let report = json!({
            "status": "unavailable",
            "degradationKind": "budget_check_unavailable",
            "manualReviewRequired": true,
            "reason": "数据库不可用",
        });

        let degradation = tester_static_analysis_degradation(&report).unwrap();

        assert_eq!(degradation["kind"], "budget_check_unavailable");
        assert_eq!(degradation["manualReviewRequired"], true);
    }

    #[test]
    fn failed_tester_is_not_mislabeled_as_a_budget_skip() {
        let report = json!({
            "status": "unavailable",
            "degradationKind": "tester_unavailable",
            "manualReviewRequired": true,
            "reason": "静态规则分析失败",
        });

        let degradation = tester_static_analysis_degradation(&report).unwrap();

        assert_eq!(degradation["kind"], "tester_unavailable");
    }

    #[test]
    fn successful_tester_has_no_degradation_marker() {
        assert!(
            tester_static_analysis_degradation(&json!({
                "status": "completed",
                "report": {"findingCount": 0},
            }))
            .is_none()
        );
    }

    #[tokio::test]
    async fn complex_dsl_vote_groups_equivalent_yaml_by_compiled_definitions() {
        let yaml = include_str!("loan.yaml").to_string();
        let formatted_yaml = format!("{}\n# alternate serialization\n", yaml.trim_end());
        let first = canonical_dsl_vote_candidate(yaml).expect("first candidate should compile");
        let second = canonical_dsl_vote_candidate(formatted_yaml)
            .expect("formatted candidate should compile");
        assert_eq!(first.signature, second.signature);

        let candidates = vec![
            VoteCandidate {
                model: "model-a".into(),
                weight: 1.0,
            },
            VoteCandidate {
                model: "model-b".into(),
                weight: 1.0,
            },
        ];
        let consensus = parallel_vote_until_majority_by(
            candidates,
            move |model| {
                let candidate = if model == "model-a" {
                    first.clone()
                } else {
                    second.clone()
                };
                async move { Ok::<_, String>(candidate) }
            },
            |candidate: &DslVoteCandidate| Ok(candidate.signature.clone()),
        )
        .await
        .expect("semantically equivalent DSL responses should be grouped");

        assert!(consensus.decision.is_some());
        assert_eq!(consensus.confidence, 1.0);
        let resolution = dsl_vote_resolution(&consensus);
        assert_eq!(resolution["strategy"], "strict_majority");
        assert_eq!(resolution["selectedModels"], json!(["model-a", "model-b"]));
        assert_eq!(resolution["selectedWeight"], 2.0);
        assert_eq!(resolution["selectedShare"], 1.0);
        let mut review = crate::models::VersionChangeReview::default();
        let unchanged_review = review.clone();
        add_dsl_vote_review_finding(&mut review, Some(&resolution));
        assert_eq!(review, unchanged_review);
    }

    #[test]
    fn complex_dsl_plurality_fallback_selects_compiled_candidate_deterministically() {
        let candidates = vec![
            (
                "model-a".into(),
                DslVoteCandidate {
                    signature: "a".into(),
                    yaml: "candidate a".into(),
                },
            ),
            (
                "model-b".into(),
                DslVoteCandidate {
                    signature: "b".into(),
                    yaml: "candidate b".into(),
                },
            ),
            (
                "model-c".into(),
                DslVoteCandidate {
                    signature: "a".into(),
                    yaml: "candidate a second copy".into(),
                },
            ),
        ];
        let outcomes = vec![
            VoteOutcome {
                model: "model-a".into(),
                weight: 1.0,
                result: Ok(serde_json::json!({"signature": "a"})),
            },
            VoteOutcome {
                model: "model-b".into(),
                weight: 1.0,
                result: Ok(serde_json::json!({"signature": "b"})),
            },
            VoteOutcome {
                model: "model-c".into(),
                weight: 1.0,
                result: Ok(serde_json::json!({"signature": "a"})),
            },
        ];

        let selected = plurality_dsl_candidate(&outcomes, &candidates).unwrap();
        assert_eq!(selected.signature, "a");
        assert_eq!(selected.yaml, "candidate a");
    }

    #[tokio::test]
    async fn complex_dsl_preserves_last_valid_candidate_for_acceptance_after_two_failures() {
        let candidate = canonical_dsl_vote_candidate(include_str!("loan.yaml").to_string())
            .expect("candidate must pass compilation");
        let expected_yaml = candidate.yaml.clone();
        let completed = vec![("model-c".to_string(), candidate.clone())];
        let models = ["model-a", "model-b", "model-c"]
            .into_iter()
            .map(|model| VoteCandidate {
                model: model.into(),
                weight: 1.0,
            })
            .collect();
        let consensus = parallel_vote_until_majority_by(
            models,
            move |model| {
                let candidate = candidate.clone();
                async move {
                    if model == "model-c" {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        Ok(candidate)
                    } else {
                        Err("FEEL compilation failed".into())
                    }
                }
            },
            |candidate: &DslVoteCandidate| Ok(candidate.signature.clone()),
        )
        .await
        .unwrap();

        assert!(consensus.requires_human_review);
        assert!(consensus.decision.is_none());
        let resolution = dsl_vote_resolution(&consensus);
        assert_eq!(resolution["strategy"], "single_valid_candidate");
        assert_eq!(resolution["strictMajority"], false);
        assert_eq!(resolution["requiresBusinessConfirmation"], true);
        assert_eq!(resolution["validModelCount"], 1);
        assert_eq!(resolution["totalModelCount"], 3);
        assert_eq!(resolution["selectedModels"], json!(["model-c"]));
        assert_eq!(resolution["selectedWeight"], 1.0);
        assert_eq!(resolution["selectedShare"], 1.0 / 3.0);
        let mut review = crate::models::VersionChangeReview::default();
        add_dsl_vote_review_finding(&mut review, Some(&resolution));
        assert_eq!(review.overall_risk, "high");
        assert!(review.confirmation_required);
        assert_eq!(review.findings[0].code, "DSL_SINGLE_VALID_CANDIDATE");
        assert!(review.findings[0].message.contains("仅获 1/3 个模型支持"));
        let selected = plurality_dsl_candidate(&consensus.outcomes, &completed)
            .expect("the remaining compiled candidate must reach business acceptance");
        assert_eq!(resolution["selectedSignature"], selected.signature);
        let generated = require_generated_dsl((
            Some(nl_dsl::agent::GeneratedDsl {
                yaml: selected.yaml,
            }),
            Some(consensus),
        ))
        .unwrap();
        assert_eq!(generated.yaml, expected_yaml);
    }

    #[tokio::test]
    async fn complex_dsl_resolution_and_fallback_agree_when_votes_tie() {
        let candidates = vec![
            (
                "model-a".to_string(),
                DslVoteCandidate {
                    signature: "z".into(),
                    yaml: "first".into(),
                },
            ),
            (
                "model-b".to_string(),
                DslVoteCandidate {
                    signature: "a".into(),
                    yaml: "second".into(),
                },
            ),
            (
                "model-c".to_string(),
                DslVoteCandidate {
                    signature: "b".into(),
                    yaml: "third".into(),
                },
            ),
        ];
        let models = candidates
            .iter()
            .map(|(model, _)| VoteCandidate {
                model: model.clone(),
                weight: 1.0,
            })
            .collect();
        let responses = candidates.clone();
        let consensus = parallel_vote_until_majority_by(
            models,
            move |model| {
                let candidate = responses
                    .iter()
                    .find(|(name, _)| name == &model)
                    .unwrap()
                    .1
                    .clone();
                async move { Ok(candidate) }
            },
            |candidate: &DslVoteCandidate| Ok(candidate.signature.clone()),
        )
        .await
        .unwrap();
        let mut completion_order = candidates;
        completion_order.reverse();
        let selected = plurality_dsl_candidate(&consensus.outcomes, &completion_order).unwrap();
        let resolution = dsl_vote_resolution(&consensus);
        assert_eq!(selected.yaml, "first");
        assert_eq!(resolution["selectedSignature"], selected.signature);
        assert_eq!(resolution["selectedModels"], json!(["model-a"]));
        assert_eq!(resolution["selectedWeight"], 1.0);
        assert!((resolution["selectedShare"].as_f64().unwrap() - (1.0 / 3.0)).abs() < 1e-9);
        assert_eq!(
            resolution["strategy"],
            "plurality_with_configuration_order_tiebreak"
        );
        assert_eq!(resolution["strictMajority"], false);
        assert!(consensus.decision.is_none());
        assert_eq!(consensus.confidence, 0.0);

        let mut review = crate::models::VersionChangeReview {
            overall_risk: "high".into(),
            ..Default::default()
        };
        add_dsl_vote_review_finding(&mut review, Some(&resolution));
        assert_eq!(
            review.overall_risk, "high",
            "an existing higher risk must be preserved"
        );
        assert!(review.confirmation_required);
        assert_eq!(review.findings[0].code, "DSL_NO_STRICT_MAJORITY");
    }

    #[test]
    fn complex_dsl_failure_preserves_bounded_model_diagnostics_in_final_error() {
        let outcomes = [
            (
                "model-a",
                "FEEL evaluation failed: Expected RParen, got Gt".to_string(),
            ),
            ("model-b", "模型调用已超时".to_string()),
            ("model-c", "诊断".repeat(2_000)),
        ]
        .into_iter()
        .map(|(model, error)| VoteOutcome {
            model: model.into(),
            weight: 1.0,
            result: Err(error),
        })
        .collect();
        let consensus = Consensus {
            decision: None,
            confidence: 0.0,
            requires_human_review: true,
            early_majority: false,
            stopped_early: false,
            stop_reason: None,
            outcomes,
        };
        let resolution = dsl_vote_resolution(&consensus);
        assert_eq!(resolution["strategy"], "no_valid_candidate");
        assert!(resolution["selectedSignature"].is_null());
        assert_eq!(resolution["selectedModels"], json!([]));
        let mut review = crate::models::VersionChangeReview::default();
        add_dsl_vote_review_finding(&mut review, Some(&resolution));
        assert!(
            review.findings.is_empty(),
            "all failed candidates must remain a generation failure"
        );
        let expected_message = dsl_vote_failure_message(&consensus.outcomes);
        let error = require_generated_dsl((None, Some(consensus)))
            .err()
            .unwrap()
            .to_string();

        assert_eq!(error, expected_message);
        assert!(error.contains("模型 model-a：FEEL evaluation failed: Expected RParen, got Gt"));
        assert!(error.contains("模型 model-b：模型调用已超时"));
        assert!(error.contains("模型 model-c："));
        assert!(error.ends_with("…（诊断已截断）"));
        assert!(error.chars().count() < 1_500);
    }

    #[test]
    fn compile_generated_dsl_rejects_invalid_output_before_pipeline_success() {
        let error = compile_generated_dsl("definitions: {}\n").unwrap_err();

        assert!(error.contains("DSL") || error.contains("DMN"));
    }

    #[test]
    fn loan_dsl_with_explicit_in_unary_test_compiles_and_evaluates() {
        let yaml = include_str!("loan.yaml");
        let definitions = nl_dsl::parse_definitions(yaml).expect("loan DSL should parse");
        nl_dsl::validate(&definitions).expect("loan DSL should validate");
        let xml = dmn::generate_dmn_xml(&definitions).expect("DMN XML should generate");
        let model = executable_dmn::parse_dmn(&xml).expect("generated DMN should parse");
        executable_dmn::compile_dmn(&model)
            .expect("explicit in (...) decision-table tests should compile");

        let result = executable_dmn::DmnExecutor::new(&model).evaluate_all(
            std::collections::HashMap::from([
                (
                    "PolicyStatus".into(),
                    crate::feel::FeelValue::String("Active".into()),
                ),
                (
                    "AccidentType".into(),
                    crate::feel::FeelValue::String("Collision".into()),
                ),
                (
                    "LossAmount".into(),
                    crate::feel::FeelValue::Number(20_000.0),
                ),
            ]),
        );
        assert!(!result.has_errors(), "{:?}", result.messages);
        assert_eq!(
            result.decision_by_name("ClaimEligibility").unwrap().value,
            crate::feel::FeelValue::String("Approve".into())
        );

        let fallback = executable_dmn::DmnExecutor::new(&model).evaluate_all(
            std::collections::HashMap::from([
                (
                    "PolicyStatus".into(),
                    crate::feel::FeelValue::String("Expired".into()),
                ),
                (
                    "AccidentType".into(),
                    crate::feel::FeelValue::String("Collision".into()),
                ),
                (
                    "LossAmount".into(),
                    crate::feel::FeelValue::Number(20_000.0),
                ),
            ]),
        );
        assert!(!fallback.has_errors(), "{:?}", fallback.messages);
        assert_eq!(
            fallback.decision_by_name("ClaimEligibility").unwrap().value,
            crate::feel::FeelValue::String("Reject".into())
        );
    }
}
