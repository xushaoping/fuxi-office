#[cfg(target_family = "wasm")]
use crate::models::{
    AdminUser, ApproveDecisionTestCase, AuditEvent, CreateDecisionProject, CreateDecisionTestCase,
    CreateDecisionVersion, CreateUser, CreatedDecisionServiceApiKey, DecisionAnalyticsReport,
    DecisionOptimizationProposal, DecisionProject, DecisionServiceApiKey, DecisionServiceRun,
    DecisionTestCase, DecisionVersion, DecisionWorkspace, ExecuteDecisionService, ExecutionTrace,
    GenerateDecisionTestCasesResult, InstantiateModelLibraryItem, KnowledgeSource,
    ModelChangeIntent, ModelChangePreview, ModelLibraryItem, ProjectMember, PublishDecisionVersion,
    PublishedDecisionService, RollbackDecisionVersion, SaveDecisionRunOutcome,
    SaveDecisionTemplate, SaveKnowledgeSource, SaveProjectMember, UpdateUser, VersionChangeReview,
    VersionVerificationReport,
};
#[cfg(target_family = "wasm")]
use crate::workflow::{
    AnalyzeRequirementRequest, ClarificationAnswer, CreateDecisionPlanRequest,
    DecisionWorkflowState,
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
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_family = "wasm"))]
use std::convert::Infallible;
#[cfg(not(target_family = "wasm"))]
use std::sync::Arc;

#[cfg(not(target_family = "wasm"))]
use crate::dmn as executable_dmn;
#[cfg(not(target_family = "wasm"))]
use crate::execution_trace::build_execution_trace;
#[cfg(not(target_family = "wasm"))]
use crate::knowledge::{citations, effective_project_sources, knowledge_context};
#[cfg(not(target_family = "wasm"))]
use crate::nl_dsl;
#[cfg(not(target_family = "wasm"))]
use crate::nl_dsl::agent::{LlmAgent, StreamEvent};
#[cfg(not(target_family = "wasm"))]
use crate::nl_dsl::dmn;
#[cfg(not(target_family = "wasm"))]
use crate::types::{AppState, Claims};
#[cfg(not(target_family = "wasm"))]
use crate::version_review::build_change_review;
#[cfg(not(target_family = "wasm"))]
use crate::version_verification::json_inputs_to_feel as typed_json_inputs_to_feel;
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
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
fn validate_change_intent_request(req: &GenerateRequest) -> Result<(), (StatusCode, Json<Value>)> {
    validate_generate_request(req)?;
    if req
        .repair_attempt
        .is_some_and(|attempt| !(0..=2).contains(&attempt))
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
    plan_agent: Option<LlmAgent>,
    dsl_agent: Option<LlmAgent>,
    change_intent_agent: Option<LlmAgent>,
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
    request_options: Map<String, Value>,
}

#[cfg(not(target_family = "wasm"))]
impl LlmState {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        let api_key = std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty());
        let api_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let default_model =
            std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
        println!("==== OPENAI_MODEL: {default_model}");
        let default_options = llm_request_options_from_env();
        let agent_for = |task| {
            let mut settings = llm_task_settings_from_env(task, &default_model, &default_options);
            if task == LlmTask::ChangeIntent {
                disable_thinking(&mut settings.request_options);
            }
            api_key.as_ref().map(|key| {
                LlmAgent::new(
                    key.clone(),
                    api_url.clone(),
                    settings.model,
                    settings.request_options,
                )
            })
        };
        Self {
            analysis_agent: agent_for(LlmTask::RequirementAnalysis),
            plan_agent: agent_for(LlmTask::DecisionPlanning),
            dsl_agent: agent_for(LlmTask::DslGeneration),
            change_intent_agent: agent_for(LlmTask::ChangeIntent),
        }
    }

    pub(crate) fn agent_for(&self, task: LlmTask) -> Option<&LlmAgent> {
        match task {
            LlmTask::RequirementAnalysis => self.analysis_agent.as_ref(),
            LlmTask::DecisionPlanning => self.plan_agent.as_ref(),
            LlmTask::DslGeneration => self.dsl_agent.as_ref(),
            LlmTask::ChangeIntent => self.change_intent_agent.as_ref(),
        }
    }
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
fn llm_task_settings_from_env(
    task: LlmTask,
    default_model: &str,
    default_options: &Map<String, Value>,
) -> LlmTaskSettings {
    let prefix = task.config_prefix();
    let model_variable = format!("OPENAI_{prefix}_MODEL");
    let options_variable = format!("OPENAI_{prefix}_REQUEST_OPTIONS_JSON");
    let stage_model = std::env::var(&model_variable).ok();
    let stage_options = std::env::var(&options_variable).ok();
    llm_task_settings(
        &options_variable,
        default_model,
        default_options,
        stage_model.as_deref(),
        stage_options.as_deref(),
    )
}

#[cfg(not(target_family = "wasm"))]
fn llm_task_settings(
    options_variable: &str,
    default_model: &str,
    default_options: &Map<String, Value>,
    stage_model: Option<&str>,
    stage_options: Option<&str>,
) -> LlmTaskSettings {
    let model = stage_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_model)
        .to_string();
    let request_options = request_options_from_raw(options_variable, stage_options)
        .unwrap_or_else(|| default_options.clone());
    LlmTaskSettings {
        model,
        request_options,
    }
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

// 登录：成功返回 JWT token，失败（密码错误/网络错误）返回 None
#[cfg(target_family = "wasm")]
pub async fn login(
    username: &str,
    password: &str,
    captcha_id: &str,
    captcha_code: &str,
) -> Option<String> {
    let url = api_url("/api/login")?;
    let res = reqwest::Client::new()
        .post(url)
        .json(&LoginPayload {
            username: username.to_string(),
            password: password.to_string(),
            captcha_id: captcha_id.to_string(),
            captcha_code: captcha_code.to_string(),
        })
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    let data: LoginResponse = res.json().await.ok()?;
    Some(data.token)
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
pub async fn list_knowledge_sources(token: &str) -> Result<Vec<KnowledgeSource>, String> {
    knowledge_request::<Vec<KnowledgeSource>>(token, "/api/knowledge-sources", "GET", None).await
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
    repair_error: Option<String>,
    repair_attempt: i32,
    previous_intent: Option<ModelChangeIntent>,
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
            "repairError": repair_error,
            "repairAttempt": repair_attempt,
            "previousIntent": previous_intent,
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
    repair_error: Option<String>,
    repair_attempt: i32,
    previous_intent: Option<ModelChangeIntent>,
    controller: web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let url =
        api_url("/api/generate-change-intent").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let body = serde_json::to_string(&serde_json::json!({
        "text": text,
        "projectId": project_id,
        "baseRevision": base_revision,
        "repairError": repair_error,
        "repairAttempt": repair_attempt,
        "previousIntent": previous_intent,
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
    read_sse_response(response, &controller, |event| {
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
pub async fn analyze_requirement(
    token: &str,
    project_id: i64,
    text: String,
) -> Result<DecisionWorkflowState, String> {
    workflow_post(
        token,
        project_id,
        "analyze",
        &AnalyzeRequirementRequest { text },
        "需求分析",
    )
    .await
}

#[cfg(target_family = "wasm")]
pub async fn analyze_requirement_stream(
    token: &str,
    project_id: i64,
    text: String,
    controller: web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let url = api_url(&format!(
        "/api/projects/{project_id}/workflow/analyze-stream"
    ))
    .ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let body = serde_json::to_string(&AnalyzeRequirementRequest { text })
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
    read_sse_response(response, &controller, &mut on_event).await
}

#[cfg(target_family = "wasm")]
pub async fn create_decision_plan(
    token: &str,
    project_id: i64,
    answers: Vec<ClarificationAnswer>,
) -> Result<DecisionWorkflowState, String> {
    workflow_post(
        token,
        project_id,
        "plan",
        &CreateDecisionPlanRequest { answers },
        "决策方案规划",
    )
    .await
}

#[cfg(target_family = "wasm")]
pub async fn confirm_decision_plan(
    token: &str,
    project_id: i64,
) -> Result<DecisionWorkflowState, String> {
    workflow_post(
        token,
        project_id,
        "confirm",
        &serde_json::json!({}),
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
    Step(String),
    Delta(String),
    Preview(String),
    Result(RunResult),
    ChangeIntent(ModelChangeIntent),
    WorkflowResult(DecisionWorkflowState),
    Error(String),
}

#[cfg(target_family = "wasm")]
fn js_error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("浏览器请求失败: {value:?}"))
}

#[cfg(target_family = "wasm")]
fn dispatch_sse_event(raw: &str, on_event: &mut impl FnMut(SseEvent)) -> Result<(), String> {
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
        return Ok(());
    }
    let payload: serde_json::Value =
        serde_json::from_str(&data).map_err(|e| format!("SSE 事件格式无效: {e}"))?;
    match event_name {
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
        "error" => on_event(SseEvent::Error(
            payload["error"]
                .as_str()
                .unwrap_or("流水线失败")
                .to_string(),
        )),
        _ => {}
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
async fn read_sse_response(
    response: web_sys::Response,
    controller: &web_sys::AbortController,
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
            dispatch_sse_event(&event, &mut on_event)?;
        }
    }
    buffer.push_str(&decoder.decode().map_err(js_error)?);
    if !buffer.trim().is_empty() {
        dispatch_sse_event(&buffer, &mut on_event)?;
    }
    Ok(())
}

/// Run the authenticated pipeline and forward each SSE event as soon as it arrives.
#[cfg(target_family = "wasm")]
pub async fn run_pipeline(
    token: &str,
    project_id: i64,
    text: String,
    existing_dsl: Option<String>,
    base_version: Option<i32>,
    controller: web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), String> {
    let url = api_url("/api/run").ok_or_else(|| "网络错误，请稍后重试".to_string())?;
    let body = serde_json::to_string(&serde_json::json!({
        "text": text,
        "projectId": project_id,
        "existingDsl": existing_dsl,
        "baseVersion": base_version,
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
    read_sse_response(response, &controller, on_event).await
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
    let knowledge_citations = match apply_effective_knowledge(&app_state, &claims, &mut req).await {
        Ok(citations) => citations,
        Err(error) => return error.into_response(),
    };
    let Some(dsl_agent) = state.agent_for(LlmTask::DslGeneration).cloned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "error": "未配置 OPENAI_API_KEY，无法调用 LLM" })),
        )
            .into_response();
    };
    if wants_stream(&headers) {
        return generate_dsl_sse(dsl_agent, req, knowledge_citations);
    }
    match dsl_agent.generate_dsl(&req.text, None).await {
        Ok(generated) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "nl_dsl": generated.yaml,
                "knowledgeCitations": knowledge_citations,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[cfg(not(target_family = "wasm"))]
async fn begin_change_intent_run(
    db: &sqlx::PgPool,
    project_id: i64,
    agent: &LlmAgent,
    req: &GenerateRequest,
    base_revision: i64,
) -> Option<i64> {
    let input = json!({
        "baseRevision": base_revision,
        "requestBytes": req.text.len(),
        "hasCompilerFeedback": req.repair_error.is_some(),
        "previousChangeCount": req.previous_intent.as_ref().map_or(0, |intent| intent.changes.len()),
    });
    match sqlx::query_scalar(
        r#"
        INSERT INTO agent_runs (
            project_id, stage, status, model, prompt_version, input_json, retry_count
        )
        VALUES ($1, 'change_intent', 'running', $2, 'change-intent-v3', $3, $4)
        RETURNING id
        "#,
    )
    .bind(project_id)
    .bind(agent.model())
    .bind(input)
    .bind(req.repair_attempt.unwrap_or(0))
    .fetch_one(db)
    .await
    {
        Ok(run_id) => Some(run_id),
        Err(error) => {
            eprintln!("记录模型变更智能运行失败: {error}");
            None
        }
    }
}

#[cfg(not(target_family = "wasm"))]
async fn finish_change_intent_run(
    db: &sqlx::PgPool,
    run_id: Option<i64>,
    output: Result<Value, String>,
) {
    let Some(run_id) = run_id else {
        return;
    };
    let result = match output {
        Ok(output) => {
            sqlx::query(
                "UPDATE agent_runs SET status='succeeded', output_json=$1, finished_at=NOW() WHERE id=$2",
            )
            .bind(output)
            .bind(run_id)
            .execute(db)
            .await
        }
        Err(error) => {
            let status = if error.contains("SSE 客户端已断开") {
                "cancelled"
            } else {
                "failed"
            };
            sqlx::query(
                "UPDATE agent_runs SET status=$1, error_json=$2, finished_at=NOW() WHERE id=$3",
            )
            .bind(status)
            .bind(json!({ "message": error }))
            .bind(run_id)
            .execute(db)
            .await
        }
    };
    if let Err(error) = result {
        eprintln!("更新模型变更智能运行失败: {error}");
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
    let req: GenerateRequest = match parse_json_body(body).await {
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
    let Some(agent) = state.agent_for(LlmTask::ChangeIntent).cloned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"ok": false, "code": "LLM_UNAVAILABLE", "error": "未配置 OPENAI_API_KEY，无法调用 LLM"})),
        ).into_response();
    };
    let run_id =
        begin_change_intent_run(&app_state.db, project_id, &agent, &req, base_revision).await;
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
    match agent
        .generate_change_intent(
            &req.text,
            &existing,
            base_revision,
            req.repair_error.as_deref(),
            req.previous_intent.as_ref(),
        )
        .await
    {
        Ok(intent) => {
            if intent.base_revision != current_revision {
                finish_change_intent_run(
                    &app_state.db,
                    run_id,
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
            finish_change_intent_run(&app_state.db, run_id, Err(message.clone())).await;
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"ok": false, "code": "LLM_PROTOCOL", "error": message})),
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
    run_id: Option<i64>,
) -> Response {
    let (tx, rx) = mpsc::channel::<StreamEvent>(32);
    tokio::spawn(async move {
        let _ = tx
            .send(StreamEvent::Step(
                "→ 正在生成受控模型变更意图...".to_string(),
            ))
            .await;
        match agent
            .generate_change_intent_stream(
                &req.text,
                &existing,
                base_revision,
                req.repair_error.as_deref(),
                req.previous_intent.as_ref(),
                &tx,
            )
            .await
        {
            Ok(intent) => {
                let audit_output = serde_json::to_value(&intent).unwrap_or_else(|_| json!({}));
                let _ = tx
                    .send(StreamEvent::Step(
                        "✓ 模型变更意图已生成，正在准备 DSL 校验...".to_string(),
                    ))
                    .await;
                let _ = tx
                    .send(StreamEvent::Finished(Ok(json!({
                        "ok": true,
                        "changeIntent": intent,
                    }))))
                    .await;
                finish_change_intent_run(&db, run_id, Ok(audit_output)).await;
            }
            Err(error) => {
                let message = error.to_string();
                let _ = tx.send(StreamEvent::Finished(Err(message.clone()))).await;
                finish_change_intent_run(&db, run_id, Err(message)).await;
            }
        }
    });
    sse_response(rx)
}

/// SSE 流式生成：step/delta 事件实时推送 LLM 过程，结束推送 result/error 事件
#[cfg(not(target_family = "wasm"))]
fn generate_dsl_sse(
    dsl_agent: LlmAgent,
    req: GenerateRequest,
    knowledge_citations: Vec<crate::models::KnowledgeCitation>,
) -> Response {
    let (tx, rx) = mpsc::channel::<StreamEvent>(64);
    let text = req.text.clone();
    let tx2 = tx.clone();
    tokio::spawn(async move {
        let result = dsl_agent.generate_dsl_stream(&text, None, &tx2).await;
        let _ = tx2
            .send(StreamEvent::Finished(
                result
                    .map(|generated| {
                        json!({
                        "ok": true,
                        "nl_dsl": generated.yaml,
                        "knowledgeCitations": knowledge_citations,
                        })
                    })
                    .map_err(|e| e.to_string()),
            ))
            .await;
    });
    sse_response(rx)
}

#[cfg(not(target_family = "wasm"))]
async fn run_pipeline(
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
    let knowledge_citations = match apply_effective_knowledge(&app_state, &claims, &mut req).await {
        Ok(citations) => citations,
        Err(error) => return error.into_response(),
    };
    let Some(dsl_agent) = state.agent_for(LlmTask::DslGeneration).cloned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "error": "未配置 OPENAI_API_KEY，无法调用 LLM" })),
        )
            .into_response();
    };
    if wants_stream(&headers) {
        return run_pipeline_sse(dsl_agent, req, knowledge_citations);
    }

    // 步骤 1: 自然语言 → DSL
    let generated = match dsl_agent.generate_dsl(&req.text, None).await {
        Ok(generated) => generated,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "ok": false, "error": format!("LLM 生成失败: {}", e) })),
            )
                .into_response();
        }
    };
    let dsl_text = generated.yaml;

    // 步骤 2: 校验 DSL
    let defs = match nl_dsl::parse_definitions(&dsl_text) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "ok": false, "error": format!("LLM 生成的 DSL 解析失败: {}", e) })),
            )
                .into_response();
        }
    };
    if let Err(e) = nl_dsl::validate(&defs) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "ok": false, "error": format!("LLM 生成的 DSL 校验失败: {}", e) })),
        )
            .into_response();
    }

    let change_summary = change_summary(req.existing_dsl.as_deref(), &defs);
    let change_review = pending_change_review(req.existing_dsl.as_deref(), &defs, req.base_version);

    // 步骤 3: DSL → DMN XML
    match dmn::generate_dmn_xml(&defs) {
        Ok(xml) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "dsl": dsl_text,
                "xml": xml,
                "changeSummary": change_summary,
                "changeReview": change_review,
                "knowledgeCitations": knowledge_citations,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// SSE 流式完整流水线：step 事件展示生成/校验/编译各阶段，LLM 内容增量实时推送
#[cfg(not(target_family = "wasm"))]
fn run_pipeline_sse(
    dsl_agent: LlmAgent,
    req: GenerateRequest,
    knowledge_citations: Vec<crate::models::KnowledgeCitation>,
) -> Response {
    let (tx, rx) = mpsc::channel::<StreamEvent>(64);
    let text = req.text.clone();
    let tx2 = tx.clone();
    tokio::spawn(async move {
        // 步骤 1: 自然语言 → DSL（内部已推送 step/delta 事件）
        let generated = match dsl_agent.generate_dsl_stream(&text, None, &tx2).await {
            Ok(generated) => generated,
            Err(e) => {
                let _ = tx2
                    .send(StreamEvent::Finished(Err(format!("LLM 生成失败: {}", e))))
                    .await;
                return;
            }
        };
        let dsl_text = generated.yaml;

        // 步骤 2: 校验 DSL
        let _ = tx2
            .send(StreamEvent::Step("✓ DSL 生成完成，正在校验...".to_string()))
            .await;
        let defs = match nl_dsl::parse_definitions(&dsl_text) {
            Ok(d) => d,
            Err(e) => {
                let _ = tx2
                    .send(StreamEvent::Finished(Err(format!(
                        "LLM 生成的 DSL 解析失败: {}",
                        e
                    ))))
                    .await;
                return;
            }
        };
        if let Err(e) = nl_dsl::validate(&defs) {
            let _ = tx2
                .send(StreamEvent::Finished(Err(format!(
                    "LLM 生成的 DSL 校验失败: {}",
                    e
                ))))
                .await;
            return;
        }
        let change_summary = change_summary(None, &defs);
        let change_review = pending_change_review(None, &defs, req.base_version);

        // 步骤 3: DSL → DMN XML
        let _ = tx2
            .send(StreamEvent::Step(
                "✓ 校验通过，正在编译 DMN XML...".to_string(),
            ))
            .await;
        match dmn::generate_dmn_xml(&defs) {
            Ok(xml) => {
                let _ = tx2
                    .send(StreamEvent::Step(
                        "✓ DMN XML 已生成，正在呈现已验证的 DSL...".to_string(),
                    ))
                    .await;
                stream_dsl_preview(&tx2, &dsl_text).await;
                let _ = tx2
                    .send(StreamEvent::Finished(Ok(json!({
                        "ok": true,
                        "dsl": dsl_text,
                        "xml": xml,
                        "changeSummary": change_summary,
                        "changeReview": change_review,
                        "knowledgeCitations": knowledge_citations,
                    }))))
                    .await;
            }
            Err(e) => {
                let _ = tx2.send(StreamEvent::Finished(Err(e.to_string()))).await;
            }
        }
    });
    sse_response(rx)
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
fn sse_response(rx: mpsc::Receiver<StreamEvent>) -> Response {
    let stream = ReceiverStream::new(rx).map(|evt| {
        let (name, data) = match evt {
            StreamEvent::Step(msg) => ("step", json!({ "message": msg })),
            StreamEvent::Delta(content) => ("delta", json!({ "content": content })),
            StreamEvent::Preview(content) => ("preview", json!({ "content": content })),
            StreamEvent::Finished(Ok(payload)) => ("result", payload),
            StreamEvent::Finished(Err(e)) => ("error", json!({ "ok": false, "error": e })),
        };
        Ok::<Event, Infallible>(Event::default().event(name).data(data.to_string()))
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
            project_id: None,
            existing_dsl,
            base_version: None,
            base_revision: None,
            repair_error: None,
            repair_attempt: None,
            previous_intent: None,
        }
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
