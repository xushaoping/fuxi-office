use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use hmac::{Hmac, Mac};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::Arc,
};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

mod decision_explanation;
mod deviation_inventory;

#[cfg(test)]
mod tester_judge_tests;

use crate::decision_analytics::{
    DecisionAnalyticsBuilder, DecisionAnalyticsRun, MAX_ANALYTICS_TRACE_BYTES,
    analytics_window_start, numeric_field_segments,
};
use crate::execution_trace::{
    build_decision_service_trace, build_execution_trace, build_rejected_execution_trace,
};
use crate::knowledge::{citations, effective_project_sources};
use crate::models::{
    ApproveDecisionTestCase, AuditEvent, BehaviorComparisonEntry, BehaviorComparisonReport,
    CreateDecisionProject, CreateDecisionServiceApiKey, CreateDecisionTestCase,
    CreateDecisionVersion, CreatedDecisionServiceApiKey, DecisionAnalyticsFinding,
    DecisionAnalyticsReport, DecisionAnalyticsSummary, DecisionAnalyticsThresholds,
    DecisionCopilotQueryRequest, DecisionCopilotQueryResponse, DecisionDriftAutomationRule,
    DecisionDriftElementImpact, DecisionDriftEvent, DecisionDriftEventReviewRecord,
    DecisionDriftEventReviewResult, DecisionDriftImpactReport, DecisionDriftScanWindow,
    DecisionDriftScanWindowReviewResult, DecisionOptimizationProposal, DecisionProject,
    DecisionRunOutcomeReceipt, DecisionServiceApiKey, DecisionServiceField, DecisionServiceRun,
    DecisionTestCase, DecisionVersion, DecisionWorkspace, DeviationAnalysis,
    DeviationAnalysisReviewRecord, DeviationAnalysisReviewResult, ExecuteDecisionService,
    ExecutionTrace, GenerateDecisionTestCasesResult, InstantiateModelLibraryItem,
    KnowledgeCitation, ModelChangeIntent, ModelChangePreview, ModelLibraryItem,
    ModelTemplateTestCase, OptimizationAutodraftShadowEvaluation, OptimizationSimulationReport,
    PatternLibraryAnalytics, PatternLineageItem, PatternRecommendation,
    PatternRecommendationFeedback, PatternUsageMetric, ProjectMember, PublicDecisionRunOutcome,
    PublicDecisionRunOutcomeBatch, PublicDecisionRunOutcomeBatchResult,
    PublicDecisionRunOutcomeReconciliationItem, PublicDecisionRunOutcomeReconciliationRequest,
    PublicDecisionRunOutcomeReconciliationResult, PublishDecisionVersion, PublishedDecisionService,
    RegressionTestCaseResult, RegressionTestReport, RollbackDecisionVersion,
    SaveDecisionRunOutcome, SaveDecisionTemplate, SaveProjectMember, UpdateDecisionDriftAutomation,
    UpdateDecisionDriftEventReview, UpdateDecisionDriftScanWindowReview,
    UpdateDeviationAnalysisReview, VerificationGate, VersionChangeReview,
    VersionVerificationReport,
};
use crate::workflow::{DecisionBrief, TesterJudgeReview, TesterReport};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatternRecommendationQuery {
    q: String,
    project_id: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PatternSearchDiagnostics {
    keyword_candidate_ids: Vec<i64>,
    vector_candidates: Vec<(i64, f64)>,
    rrf_candidate_ids: Vec<i64>,
}

#[derive(sqlx::FromRow)]
struct DecisionRunDailyAggregate {
    run_day: chrono::NaiveDate,
    total_runs: i64,
    succeeded_runs: i64,
    failed_runs: i64,
    average_duration_ms: f64,
}

fn validate_pattern_query(query: &str) -> Result<&str, &'static str> {
    let query = query.trim();
    if query.is_empty() {
        return Err("请提供需求关键词 q");
    }
    if query.chars().count() > 500 {
        return Err("需求关键词 q 不能超过 500 个字符");
    }
    Ok(query)
}

fn pattern_features(text: &str, category: &str, name: &str) -> (String, Vec<String>) {
    let definitions = [
        (
            "matrix",
            ["评分", "打分", "权重", "维度", "综合得分"].as_slice(),
        ),
        (
            "workflow",
            ["流程", "步骤", "首先", "然后", "审批"].as_slice(),
        ),
        (
            "threshold",
            ["阈值", "超过", "低于", "至少", "区间", "边界"].as_slice(),
        ),
        (
            "tree",
            ["如果", "若", "否则", "条件", "分级", "规则"].as_slice(),
        ),
    ];
    let mut keywords = Vec::new();
    let combined = format!("{name} {category} {text}").to_lowercase();
    let pattern_type = definitions
        .iter()
        .map(|(kind, terms)| {
            let matched = terms
                .iter()
                .filter(|term| combined.contains(**term))
                .map(|term| term.to_string())
                .collect::<Vec<_>>();
            (kind, matched)
        })
        .max_by_key(|(_, matched)| matched.len());
    if let Some((kind, matched)) = pattern_type.filter(|(_, terms)| !terms.is_empty()) {
        keywords.extend(matched);
        keywords.extend(
            [name.trim(), category.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(str::to_lowercase),
        );
        keywords.extend(semantic_terms(&combined));
        keywords.sort();
        keywords.dedup();
        keywords.truncate(80);
        ((*kind).to_string(), keywords)
    } else {
        let mut fallback = [name.trim(), category.trim()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        fallback.extend(semantic_terms(&combined));
        fallback.sort();
        fallback.dedup();
        fallback.truncate(80);
        ("general".to_string(), fallback)
    }
}

fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
    )
}

fn semantic_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut ascii_word = String::new();
    let mut cjk_run = String::new();
    let flush_ascii = |terms: &mut Vec<String>, word: &mut String| {
        if word.chars().count() >= 3 {
            terms.push(word.to_lowercase());
        }
        word.clear();
    };
    let flush_cjk = |terms: &mut Vec<String>, run: &mut String| {
        let characters = run.chars().collect::<Vec<_>>();
        if characters.len() >= 2 {
            terms.push(run.clone());
            for window in characters.windows(2) {
                terms.push(window.iter().collect());
            }
        }
        run.clear();
    };
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            flush_cjk(&mut terms, &mut cjk_run);
            ascii_word.push(character);
        } else if is_cjk(character) {
            flush_ascii(&mut terms, &mut ascii_word);
            cjk_run.push(character);
        } else {
            flush_ascii(&mut terms, &mut ascii_word);
            flush_cjk(&mut terms, &mut cjk_run);
        }
    }
    flush_ascii(&mut terms, &mut ascii_word);
    flush_cjk(&mut terms, &mut cjk_run);
    terms.sort();
    terms.dedup();
    terms.truncate(64);
    terms
}

#[cfg(test)]
fn pattern_match_score(
    query: &str,
    keywords: &[String],
    category: &str,
    pattern_type: &str,
) -> (f64, String) {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return (0.0, "请输入需求关键词以检索相似模式".into());
    }
    let hits = keywords
        .iter()
        .filter(|keyword| query.contains(keyword.as_str()) || keyword.contains(&query))
        .count();
    let category_hit = !category.is_empty() && query.contains(&category.to_lowercase());
    let type_hit = ["tree", "matrix", "workflow", "threshold"]
        .iter()
        .any(|term| query.contains(term) && *term == pattern_type);
    let matched = hits.min(3) as f64 * 20.0
        + if category_hit { 20.0 } else { 0.0 }
        + if type_hit { 20.0 } else { 0.0 };
    let reason = if hits > 0 {
        format!(
            "匹配到 {hits} 个业务关键词{}",
            if category_hit {
                "，且行业标签相符"
            } else {
                ""
            }
        )
    } else if category_hit {
        "行业标签相符；请人工确认业务规则适配性".into()
    } else {
        "关键词重叠较少，仅作为待评估参考".into()
    };
    (matched.min(100.0), reason)
}

fn semantic_pattern_match_score(
    query: &str,
    keywords: &[String],
    category: &str,
    pattern_type: &str,
    source_text: &str,
) -> (f64, String) {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return (0.0, "请输入需求关键词以检索相似模式".into());
    }
    let exact_hits = keywords
        .iter()
        .filter(|keyword| query.contains(keyword.as_str()) || keyword.contains(&query))
        .count();
    let query_terms = semantic_terms(&query);
    let mut source_terms = keywords
        .iter()
        .flat_map(|keyword| semantic_terms(keyword))
        .collect::<Vec<_>>();
    source_terms.extend(semantic_terms(&format!("{category} {source_text}")));
    source_terms.sort();
    source_terms.dedup();
    let matched_terms = query_terms
        .iter()
        .filter(|term| source_terms.binary_search(term).is_ok())
        .cloned()
        .collect::<Vec<_>>();
    let category_hit = !category.is_empty() && query.contains(&category.to_lowercase());
    let type_hit = ["tree", "matrix", "workflow", "threshold"]
        .iter()
        .any(|term| query.contains(term) && *term == pattern_type);
    let score = (exact_hits.min(3) as f64 * 16.0
        + matched_terms.len().min(5) as f64 * 8.0
        + if category_hit { 16.0 } else { 0.0 }
        + if type_hit { 12.0 } else { 0.0 })
    .min(100.0);
    let reason = if !matched_terms.is_empty() {
        let preview = matched_terms
            .into_iter()
            .take(3)
            .collect::<Vec<_>>()
            .join("、");
        format!(
            "匹配到业务语义片段：{preview}{}",
            if category_hit {
                "，且行业标签相符"
            } else {
                ""
            }
        )
    } else if exact_hits > 0 {
        format!("匹配到 {exact_hits} 个业务关键词，建议人工确认语义适配性")
    } else {
        "语义片段重叠较少，仅作为待评估参考".into()
    };
    (score, reason)
}
use crate::nl_dsl::{self, hash::definitions_hash};
use crate::project_access::{self, ProjectPermission};
use crate::types::{AppState, Claims};
use crate::version_review::behavior_comparison::ExecutionTraceComparator;
use crate::version_review::build_change_review;

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let sorted = object.iter().collect::<BTreeMap<_, _>>();
            let entries = sorted
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical_json(value)
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", entries.join(","))
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => value.to_string(),
    }
}

fn tester_proposal_matches_version(
    proposal: &crate::workflow::SemanticTestProposal,
    version_id: i64,
) -> bool {
    if proposal.category != "static_analysis" {
        return true;
    }
    proposal
        .source_reference
        .strip_prefix("draftVersion:")
        .and_then(|source| source.split_once('/'))
        .and_then(|(source_version, _)| source_version.parse::<i64>().ok())
        == Some(version_id)
}

fn match_trace_inputs(
    baseline_runs: &[(i64, ExecutionTrace)],
    candidate_runs: &[(i64, ExecutionTrace)],
) -> Vec<(usize, usize)> {
    let mut candidate_indices = HashMap::<String, VecDeque<usize>>::new();
    for (index, (_, trace)) in candidate_runs.iter().enumerate() {
        candidate_indices
            .entry(canonical_json(&trace.normalized_input))
            .or_default()
            .push_back(index);
    }
    baseline_runs
        .iter()
        .enumerate()
        .filter_map(|(baseline_index, (_, trace))| {
            candidate_indices
                .get_mut(&canonical_json(&trace.normalized_input))
                .and_then(VecDeque::pop_front)
                .map(|candidate_index| (baseline_index, candidate_index))
        })
        .collect()
}
use crate::version_review::optimization_simulation::{
    MIN_SIMULATION_SAMPLE_COUNT, calculate_metrics, impact_summary, passes_simulation,
    should_withdraw_proposal,
};
use crate::version_review::regression_testing::{
    HistoricalRegressionCase, RegressionTestGenerator, aggregate_risk, source_coverage_percent,
};
use crate::version_verification::{
    assign_input_path, decision_service_inputs_to_feel, json_inputs_to_feel, sample_input,
    verify_version,
};

#[derive(Debug)]
struct ProjectApiError {
    status: StatusCode,
    message: String,
    code: &'static str,
}

impl ProjectApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            code: "BAD_REQUEST",
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "决策项目不存在".to_string(),
            code: "NOT_FOUND",
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
            code: "FORBIDDEN",
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
            code: "BASE_CONFLICT",
        }
    }

    fn database(operation: &str, error: sqlx::Error) -> Self {
        eprintln!("数据库操作失败 [{operation}]: {error}");
        let environment = std::env::var("APP_ENV")
            .or_else(|_| std::env::var("RUST_ENV"))
            .or_else(|_| std::env::var("LEPTOS_ENV"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let message = if matches!(environment.as_str(), "production" | "prod") {
            format!("{operation}失败，请稍后重试")
        } else {
            format!("{operation}失败：{error}")
        };
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
            code: "DATABASE_ERROR",
        }
    }

    fn coded(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn patch(code: &'static str, message: impl Into<String>) -> Self {
        Self::coded(StatusCode::BAD_REQUEST, code, message)
    }

    fn patch_target(message: impl Into<String>) -> Self {
        Self::patch("PATCH_TARGET", message)
    }

    fn dsl(message: impl Into<String>) -> Self {
        Self::coded(StatusCode::BAD_REQUEST, "DSL_INVALID", message)
    }

    fn dmn(message: impl Into<String>) -> Self {
        Self::coded(StatusCode::BAD_REQUEST, "DMN_COMPILE", message)
    }
}

impl IntoResponse for ProjectApiError {
    fn into_response(self) -> Response {
        let mut payload = json!({ "ok": false, "error": self.message, "code": self.code });
        if self.code.starts_with("TESTER_JUDGE_BUDGET_") {
            payload["degradation"] = json!({
                "kind": "budget_skip",
                "skippedAgent": "tester_judge",
                "manualReviewRequired": true,
            });
        }
        (self.status, Json(payload)).into_response()
    }
}

async fn ensure_project_permission(
    state: &AppState,
    project_id: i64,
    username: &str,
    permission: ProjectPermission,
) -> Result<(), ProjectApiError> {
    let allowed = project_access::has_permission(&state.db, project_id, username, permission)
        .await
        .map_err(|error| ProjectApiError::database("校验项目权限", error))?;
    if allowed {
        Ok(())
    } else {
        Err(ProjectApiError::not_found())
    }
}

async fn ensure_review_permission(
    state: &AppState,
    project_id: i64,
    username: &str,
    action: &str,
) -> Result<(), ProjectApiError> {
    let allowed =
        project_access::has_permission(&state.db, project_id, username, ProjectPermission::Review)
            .await
            .map_err(|error| ProjectApiError::database("校验项目复核权限", error))?;
    if allowed {
        Ok(())
    } else {
        Err(ProjectApiError::forbidden(format!(
            "当前账号没有项目复核权限，请切换拥有“复核者”角色的账号后{action}"
        )))
    }
}

async fn ensure_publish_permission(
    state: &AppState,
    project_id: i64,
    username: &str,
) -> Result<(), ProjectApiError> {
    let allowed =
        project_access::has_permission(&state.db, project_id, username, ProjectPermission::Approve)
            .await
            .map_err(|error| ProjectApiError::database("校验项目发布权限", error))?;
    if allowed {
        Ok(())
    } else {
        Err(ProjectApiError::forbidden(
            "当前账号没有发布权限，请切换审批者账号后确认发布",
        ))
    }
}

fn validate_member_role(role: &str) -> Result<(), ProjectApiError> {
    if matches!(role, "modeler" | "reviewer" | "approver" | "viewer") {
        Ok(())
    } else {
        Err(ProjectApiError::bad_request(
            "项目角色必须是 modeler、reviewer、approver 或 viewer",
        ))
    }
}

fn member_roles_schema_error(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database_error)
            if database_error.code().is_some_and(|code| code == "42P10")
    )
}

fn validate_project(payload: &CreateDecisionProject) -> Result<(), ProjectApiError> {
    let name = payload.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(ProjectApiError::bad_request(
            "项目名称不能为空且不能超过 120 个字符",
        ));
    }
    if payload.description.len() > 4_000 {
        return Err(ProjectApiError::bad_request("项目描述不能超过 4000 字节"));
    }
    if !matches!(payload.risk_level.as_str(), "low" | "medium" | "high") {
        return Err(ProjectApiError::bad_request(
            "风险等级必须是 low、medium 或 high",
        ));
    }
    Ok(())
}

fn validate_version(payload: &CreateDecisionVersion) -> Result<(), ProjectApiError> {
    if !payload.change_confirmed {
        return Err(ProjectApiError::bad_request(
            "必须确认版本差异和风险摘要后才能保存版本",
        ));
    }
    if payload.dsl_yaml.trim().is_empty() || payload.dmn_xml.trim().is_empty() {
        return Err(ProjectApiError::bad_request("DSL YAML 和 DMN XML 不能为空"));
    }
    if payload.dsl_yaml.len() > 500_000 {
        return Err(ProjectApiError::bad_request(
            "DSL YAML 不能超过 500000 字节",
        ));
    }
    if payload.dmn_xml.len() > 2_000_000 {
        return Err(ProjectApiError::bad_request(
            "DMN XML 不能超过 2000000 字节",
        ));
    }
    let definitions = nl_dsl::parse_definitions(&payload.dsl_yaml)
        .map_err(|error| ProjectApiError::bad_request(format!("DSL YAML 无效：{error}")))?;
    nl_dsl::validate(&definitions)
        .map_err(|error| ProjectApiError::bad_request(format!("DSL 校验失败：{error}")))?;
    canonical_dmn_xml(&definitions)?;
    Ok(())
}

fn canonical_dmn_xml(definitions: &nl_dsl::Definitions) -> Result<String, ProjectApiError> {
    let xml = crate::nl_dsl::dmn::generate_dmn_xml(definitions)
        .map_err(|error| ProjectApiError::dmn(format!("由 DSL 生成 DMN XML 失败：{error}")))?;
    let model = crate::dmn::parse_dmn(&xml)
        .map_err(|error| ProjectApiError::dmn(format!("生成的 DMN XML 无效：{error}")))?;
    crate::dmn::compile_dmn(&model)
        .map_err(|error| ProjectApiError::dmn(format!("生成的 DMN XML 编译失败：{error}")))?;
    crate::dmn::validate_generated_feel_name_bindings(&model)
        .map_err(|error| ProjectApiError::dmn(format!("生成的 DMN FEEL 绑定校验失败：{error}")))?;
    Ok(xml)
}

fn validate_publish(payload: &PublishDecisionVersion) -> Result<(), ProjectApiError> {
    if !payload.publish_confirmed {
        return Err(ProjectApiError::bad_request(
            "必须确认验收报告和发布影响后才能发布版本",
        ));
    }
    Ok(())
}

fn historical_regression_gate_enabled() -> bool {
    historical_regression_gate_enabled_for(
        std::env::var("REQUIRE_HISTORICAL_REGRESSION_BEFORE_PUBLISH")
            .ok()
            .as_deref(),
    )
}

fn historical_regression_gate_enabled_for(setting: Option<&str>) -> bool {
    !matches!(
        setting
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "false" | "0" | "no" | "off"
    )
}

fn historical_regression_gate_error(total: i64, passed: i64) -> Option<&'static str> {
    if total == 0 {
        Some("发布前必须先运行至少一条历史回归用例")
    } else if passed != total {
        Some("历史回归测试存在失败、执行错误或未完成用例，不能发布")
    } else {
        None
    }
}

fn validate_rollback(payload: &RollbackDecisionVersion) -> Result<(), ProjectApiError> {
    if !payload.rollback_confirmed {
        return Err(ProjectApiError::bad_request(
            "必须确认回滚影响后才能创建回滚草稿",
        ));
    }
    let reason = payload.reason.trim();
    if reason.is_empty() || reason.chars().count() > 500 {
        return Err(ProjectApiError::bad_request(
            "回滚原因不能为空且不能超过 500 个字符",
        ));
    }
    Ok(())
}

fn validate_template(payload: &SaveDecisionTemplate) -> Result<(), ProjectApiError> {
    let name = payload.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(ProjectApiError::bad_request(
            "模板名称不能为空且不能超过 120 个字符",
        ));
    }
    if payload.description.len() > 4_000 {
        return Err(ProjectApiError::bad_request("模板描述不能超过 4000 字节"));
    }
    if payload.industry.chars().count() > 80 {
        return Err(ProjectApiError::bad_request("行业标签不能超过 80 个字符"));
    }
    Ok(())
}

fn validate_instantiation(payload: &InstantiateModelLibraryItem) -> Result<(), ProjectApiError> {
    if !payload.instantiate_confirmed {
        return Err(ProjectApiError::bad_request(
            "必须确认复用影响后才能创建项目",
        ));
    }
    let name = payload.project_name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(ProjectApiError::bad_request(
            "项目名称不能为空且不能超过 120 个字符",
        ));
    }
    if payload.description.len() > 4_000 {
        return Err(ProjectApiError::bad_request("项目描述不能超过 4000 字节"));
    }
    Ok(())
}

fn validate_run_outcome(payload: &SaveDecisionRunOutcome) -> Result<(), ProjectApiError> {
    if !payload.actual_outcome.is_object() {
        return Err(ProjectApiError::bad_request("实际业务结果必须是 JSON 对象"));
    }
    let size = serde_json::to_vec(&payload.actual_outcome)
        .map_err(|error| ProjectApiError::bad_request(format!("实际业务结果无法序列化：{error}")))?
        .len();
    if size > 1_000_000 {
        return Err(ProjectApiError::bad_request(
            "实际业务结果不能超过 1000000 字节",
        ));
    }
    if !matches!(
        payload.assessment.as_str(),
        "confirmed" | "mismatch" | "inconclusive"
    ) {
        return Err(ProjectApiError::bad_request(
            "结果判定必须是 confirmed、mismatch 或 inconclusive",
        ));
    }
    if payload.business_reference.trim().is_empty()
        || payload.business_reference.chars().count() > 255
    {
        return Err(ProjectApiError::bad_request(
            "业务关联号不能为空且不能超过 255 个字符",
        ));
    }
    if payload.note.len() > 4_000 {
        return Err(ProjectApiError::bad_request("反馈说明不能超过 4000 字节"));
    }
    if payload
        .occurred_at
        .is_some_and(|occurred_at| occurred_at > chrono::Utc::now() + chrono::Duration::minutes(5))
    {
        return Err(ProjectApiError::bad_request(
            "实际结果发生时间不能晚于当前时间",
        ));
    }
    Ok(())
}

fn validate_public_run_outcome(payload: &PublicDecisionRunOutcome) -> Result<(), ProjectApiError> {
    if !payload.actual_outcome.is_object() {
        return Err(ProjectApiError::bad_request("实际业务结果必须是 JSON 对象"));
    }
    let size = serde_json::to_vec(&payload.actual_outcome)
        .map_err(|error| ProjectApiError::bad_request(format!("实际业务结果无法序列化：{error}")))?
        .len();
    if size > 65_536 {
        return Err(ProjectApiError::bad_request(
            "实际业务结果不能超过 65536 字节",
        ));
    }
    if !matches!(
        payload.assessment.as_str(),
        "confirmed" | "mismatch" | "inconclusive"
    ) {
        return Err(ProjectApiError::bad_request(
            "结果判定必须是 confirmed、mismatch 或 inconclusive",
        ));
    }
    if payload.business_reference.chars().count() > 255 {
        return Err(ProjectApiError::bad_request(
            "业务关联号不能超过 255 个字符",
        ));
    }
    if payload.note.len() > 4_000 {
        return Err(ProjectApiError::bad_request("反馈说明不能超过 4000 字节"));
    }
    if payload
        .occurred_at
        .is_some_and(|occurred_at| occurred_at > chrono::Utc::now() + chrono::Duration::minutes(5))
    {
        return Err(ProjectApiError::bad_request(
            "实际结果发生时间不能晚于当前时间",
        ));
    }
    Ok(())
}

fn validate_idempotency_key(value: &str) -> Result<&str, ProjectApiError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        return Err(ProjectApiError::bad_request(
            "Idempotency-Key 需为 1-128 位字母、数字或 . _ : -",
        ));
    }
    Ok(value)
}

fn public_outcome_digest(payload: &PublicDecisionRunOutcome) -> Result<String, ProjectApiError> {
    let canonical = json!({
        "businessReference": payload.business_reference.trim(),
        "actualOutcome": payload.actual_outcome,
        "assessment": payload.assessment,
        "note": payload.note.trim(),
        "occurredAt": payload.occurred_at,
    });
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|error| ProjectApiError::bad_request(format!("业务结果摘要无效：{error}")))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn deviation_candidate(
    trace: &Value,
    actual_outcome: &Value,
) -> (String, Option<String>, f64, Value) {
    let mut mismatched_decisions = Vec::new();
    let mut candidate_rules = Vec::<Value>::new();
    if let (Some(actual), Some(decisions)) = (
        actual_outcome.as_object(),
        trace.get("decisions").and_then(Value::as_array),
    ) {
        for decision in decisions {
            let Some(name) = decision.get("decisionName").and_then(Value::as_str) else {
                continue;
            };
            let Some(actual_value) = actual.get(name) else {
                continue;
            };
            if decision.get("result") == Some(actual_value) {
                continue;
            }
            mismatched_decisions.push(json!({
                "decisionId": decision.get("decisionId"),
                "decisionName": name,
            }));
            if let Some(rule_ids) = decision.get("matchedRuleIds").and_then(Value::as_array) {
                for rule_id in rule_ids.iter().filter_map(Value::as_str) {
                    candidate_rules.push(json!({
                        "decisionId": decision.get("decisionId"),
                        "decisionName": name,
                        "ruleId": rule_id,
                    }));
                }
            }
        }
    }
    let unique_rules = candidate_rules
        .iter()
        .filter_map(|candidate| {
            candidate
                .get("ruleId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect::<HashSet<_>>();
    let attributed_rule = (unique_rules.len() == 1)
        .then(|| unique_rules.into_iter().next())
        .flatten();
    let status = if candidate_rules.is_empty() {
        "insufficient_evidence"
    } else {
        "candidate"
    };
    let confidence = if candidate_rules.len() == 1 && mismatched_decisions.len() == 1 {
        0.35
    } else if candidate_rules.is_empty() {
        0.0
    } else {
        0.15
    };
    let evidence = json!({
        "method": "matched_rules_on_comparable_mismatched_outputs",
        "mismatchedDecisions": mismatched_decisions,
        "candidateRules": candidate_rules,
        "limitations": "基于同名决策输出与回传结果的差异定位执行路径候选，不构成因果证明；需人工复核。",
    });
    (status.into(), attributed_rule, confidence, evidence)
}

#[derive(Clone)]
struct ModelSnapshot {
    item: ModelLibraryItem,
    requirement_text: Option<String>,
    decision_brief: Option<Value>,
    decision_plan: Option<Value>,
    dsl_yaml: String,
    dmn_xml: String,
    knowledge_citations: Vec<KnowledgeCitation>,
    test_cases: Vec<ModelTemplateTestCase>,
}

#[derive(sqlx::FromRow)]
struct SavedTemplateSnapshotRow {
    id: i64,
    name: String,
    description: String,
    industry: String,
    risk_level: String,
    source_project_id: Option<i64>,
    source_project_name: Option<String>,
    source_version_id: Option<i64>,
    source_version_number: Option<i32>,
    requirement_text: Option<String>,
    decision_brief_json: Option<Value>,
    decision_plan_json: Option<Value>,
    dsl_yaml: String,
    dmn_xml: String,
    knowledge_citations_json: Value,
    test_cases_json: Value,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow)]
struct HistorySnapshotRow {
    version_id: i64,
    version_number: i32,
    created_at: chrono::DateTime<chrono::Utc>,
    project_id: i64,
    project_name: String,
    project_description: String,
    risk_level: String,
    requirement_text: Option<String>,
    decision_brief_json: Option<Value>,
    decision_plan_json: Option<Value>,
    dsl_yaml: String,
    dmn_xml: String,
    knowledge_citations_json: Value,
}

struct BuiltinTemplate {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    industry: &'static str,
    risk_level: &'static str,
    dsl_yaml: &'static str,
    test_cases: fn() -> Vec<ModelTemplateTestCase>,
}

fn loan_template_cases() -> Vec<ModelTemplateTestCase> {
    vec![
        ModelTemplateTestCase {
            name: "优质申请人自动通过".into(),
            category: "business".into(),
            input_json: json!({"Age": 30, "AnnualIncome": 150000, "CreditScore": 760}),
            expected_output_json: json!({"LoanDecision": "Approve"}),
            source: "generated".into(),
        },
        ModelTemplateTestCase {
            name: "临界申请进入人工复核".into(),
            category: "business".into(),
            input_json: json!({"Age": 25, "AnnualIncome": 90000, "CreditScore": 690}),
            expected_output_json: json!({"LoanDecision": "ManualReview"}),
            source: "generated".into(),
        },
    ]
}

fn insurance_template_cases() -> Vec<ModelTemplateTestCase> {
    vec![
        ModelTemplateTestCase {
            name: "有效保单碰撞理赔通过".into(),
            category: "business".into(),
            input_json: json!({"PolicyStatus": "Active", "AccidentType": "Collision", "LossAmount": 30000}),
            expected_output_json: json!({"ClaimEligibility": "Approve"}),
            source: "generated".into(),
        },
        ModelTemplateTestCase {
            name: "无效保单理赔拒绝".into(),
            category: "business".into(),
            input_json: json!({"PolicyStatus": "Expired", "AccidentType": "Collision", "LossAmount": 10000}),
            expected_output_json: json!({"ClaimEligibility": "Reject"}),
            source: "generated".into(),
        },
    ]
}

fn builtin_templates() -> [BuiltinTemplate; 2] {
    [
        BuiltinTemplate {
            id: "loan-approval",
            name: "贷款审批",
            description: "根据年龄、年收入和信用分数给出通过、人工复核或拒绝结果。",
            industry: "金融信贷",
            risk_level: "high",
            dsl_yaml: include_str!("../resource/templates/loan-approval.yaml"),
            test_cases: loan_template_cases,
        },
        BuiltinTemplate {
            id: "insurance-claim",
            name: "保险理赔资格",
            description: "根据保单状态、事故类型和损失金额判断理赔资格。",
            industry: "保险",
            risk_level: "high",
            dsl_yaml: include_str!("loan.yaml"),
            test_cases: insurance_template_cases,
        },
    ]
}

fn builtin_snapshot(template: &BuiltinTemplate) -> Result<ModelSnapshot, ProjectApiError> {
    let definitions = nl_dsl::parse_definitions(template.dsl_yaml)
        .map_err(|error| ProjectApiError::bad_request(format!("内置模板 DSL 无效：{error}")))?;
    nl_dsl::validate(&definitions)
        .map_err(|error| ProjectApiError::bad_request(format!("内置模板校验失败：{error}")))?;
    let dmn_xml = crate::nl_dsl::dmn::generate_dmn_xml(&definitions)
        .map_err(|error| ProjectApiError::bad_request(format!("内置模板 DMN 生成失败：{error}")))?;
    let test_cases = (template.test_cases)();
    Ok(ModelSnapshot {
        item: ModelLibraryItem {
            source_kind: "builtin".into(),
            source_id: template.id.into(),
            name: template.name.into(),
            description: template.description.into(),
            industry: template.industry.into(),
            risk_level: template.risk_level.into(),
            source_project_id: None,
            source_project_name: None,
            source_version_id: None,
            source_version_number: None,
            test_case_count: test_cases.len() as i64,
            created_at: None,
        },
        requirement_text: None,
        decision_brief: None,
        decision_plan: None,
        dsl_yaml: template.dsl_yaml.into(),
        dmn_xml,
        knowledge_citations: Vec::new(),
        test_cases,
    })
}

fn validate_test_case(payload: &CreateDecisionTestCase) -> Result<(), ProjectApiError> {
    let name = payload.name.trim();
    if name.is_empty() || name.len() > 160 {
        return Err(ProjectApiError::bad_request(
            "验收案例名称不能为空且不能超过 160 字节",
        ));
    }
    if !payload.input_json.is_object() {
        return Err(ProjectApiError::bad_request("验收输入必须是 JSON 对象"));
    }
    if payload
        .expected_output_json
        .as_object()
        .is_none_or(|expected| expected.is_empty())
    {
        return Err(ProjectApiError::bad_request(
            "期望输出必须是至少包含一个决策断言的 JSON 对象",
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConvertTesterProposalRequest {
    input_json: Value,
    expected_output_json: Value,
}

fn tester_judge_context(
    requirement: &str,
    brief: &Value,
    answers: &Value,
    plan: &Option<Value>,
    proposal: &crate::workflow::SemanticTestProposal,
    candidate: &ConvertTesterProposalRequest,
) -> Value {
    json!({
        "requirement": requirement,
        "brief": brief,
        "confirmedAnswers": answers,
        "decisionPlan": plan,
        "proposal": {
            "category": proposal.category,
            "title": proposal.title,
            "rationale": proposal.rationale,
            "sourceReference": proposal.source_reference,
            "inputFields": proposal.input_fields,
        },
        "candidateCase": {
            "input": candidate.input_json,
            "expectedOutput": candidate.expected_output_json,
        },
    })
}

async fn tester_judge_sample_votes<Check, CheckFuture, Sample, SampleFuture, Cancel>(
    mut check_running: Check,
    mut sample: Sample,
    cancellation: Cancel,
) -> anyhow::Result<Vec<crate::workflow::TesterJudgeVote>>
where
    Check: FnMut() -> CheckFuture,
    CheckFuture: std::future::Future<Output = anyhow::Result<()>>,
    Sample: FnMut() -> SampleFuture,
    SampleFuture: std::future::Future<Output = anyhow::Result<crate::workflow::TesterJudgeVote>>,
    Cancel: std::future::Future<Output = anyhow::Result<()>>,
{
    let work = async {
        let mut votes = Vec::with_capacity(2);
        for _ in 0..2 {
            check_running().await?;
            let vote = sample().await?;
            if !matches!(
                vote.assessment.as_str(),
                "plausible" | "suspicious" | "insufficient_evidence"
            ) || vote.reason.trim().is_empty()
                || vote.reason.chars().count() > 600
            {
                return Err(anyhow::anyhow!("复核模型返回无效判定"));
            }
            votes.push(vote);
        }
        Ok(votes)
    };
    tokio::select! {
        biased;
        stopped = cancellation => {
            stopped?;
            Err(anyhow::anyhow!("编排已由用户取消"))
        }
        result = work => result,
    }
}

async fn ensure_tester_judge_running(
    connection: &mut sqlx::PgConnection,
    orchestration_id: i64,
) -> Result<(), ProjectApiError> {
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM agent_orchestrations WHERE id = $1 FOR UPDATE",
    )
    .bind(orchestration_id)
    .fetch_optional(connection)
    .await
    .map_err(|error| ProjectApiError::database("锁定测试期望值复核编排", error))?;
    if status.as_deref() != Some("running") {
        return Err(ProjectApiError::conflict(
            "复核编排已取消或结束，结果未保存",
        ));
    }
    Ok(())
}

fn tester_judge_review_from_votes(
    votes: Vec<crate::workflow::TesterJudgeVote>,
    input_digest: String,
    expected_output_digest: String,
) -> TesterJudgeReview {
    let status = if votes.len() != 2 {
        "inconclusive"
    } else if votes[0].assessment != votes[1].assessment {
        "inconclusive"
    } else {
        match votes[0].assessment.as_str() {
            "plausible" => "no_obvious_issue",
            "suspicious" => "suspicious",
            _ => "inconclusive",
        }
    };
    let summary = match status {
        "no_obvious_issue" => "两次独立判定均认为期望输出有已确认依据；这不代表用例已批准。",
        "suspicious" => "两次独立判定均发现期望输出可能缺少依据或与场景不符，请重点复核。",
        _ => "两次判定未能形成一致结论，请由业务人员直接复核。",
    };
    TesterJudgeReview {
        status: status.into(),
        summary: summary.into(),
        input_digest,
        expected_output_digest,
        prompt_version: String::new(),
        prompt_digest: String::new(),
        context_digest: String::new(),
        reviewed_at: chrono::Utc::now(),
        votes,
    }
}

fn tester_judge_budget_error(error: crate::api::ProjectTokenBudgetError) -> ProjectApiError {
    match error {
        crate::api::ProjectTokenBudgetError::Exceeded { used, limit } => ProjectApiError::coded(
            StatusCode::TOO_MANY_REQUESTS,
            "TESTER_JUDGE_BUDGET_PROJECT_TOKENS",
            format!(
                "预算不足，已跳过 AI 辅助复核；请业务人员人工复核后继续。项目今日 token 已用 {used}，上限 {limit}"
            ),
        ),
        crate::api::ProjectTokenBudgetError::OrchestrationTokensExceeded { used, limit } => {
            ProjectApiError::coded(
                StatusCode::TOO_MANY_REQUESTS,
                "TESTER_JUDGE_BUDGET_ORCHESTRATION_TOKENS",
                format!(
                    "预算不足，已跳过 AI 辅助复核；请业务人员人工复核后继续。当前编排 Token 已用 {used}，上限 {limit}"
                ),
            )
        }
        crate::api::ProjectTokenBudgetError::EstimatedCostExceeded { used, next, limit } => {
            ProjectApiError::coded(
                StatusCode::TOO_MANY_REQUESTS,
                "TESTER_JUDGE_BUDGET_COST",
                format!(
                    "预算不足，已跳过 AI 辅助复核；请业务人员人工复核后继续。成本已用 ${used:.6}，本次估算 ${next:.6}，上限 ${limit:.6}"
                ),
            )
        }
        crate::api::ProjectTokenBudgetError::InvalidConfiguration(message) => {
            ProjectApiError::coded(
                StatusCode::SERVICE_UNAVAILABLE,
                "MODEL_BUDGET_CONFIG_INVALID",
                message,
            )
        }
        crate::api::ProjectTokenBudgetError::Database(error) => {
            ProjectApiError::database("检查测试用例复核模型预算", error)
        }
        crate::api::ProjectTokenBudgetError::RetryAlreadyActive => {
            ProjectApiError::conflict("测试用例复核运行状态冲突")
        }
    }
}

async fn begin_tester_judge_agent_run(
    state: &AppState,
    project_id: i64,
    username: &str,
    agent: &crate::nl_dsl::agent::LlmAgent,
    input: &Value,
    parent_orchestration_id: Option<i64>,
) -> Result<(i64, i64), ProjectApiError> {
    let response_count = agent.maximum_model_response_attempts().saturating_mul(2);
    let estimated_cost = crate::api::estimated_run_cost_reservation(
        agent.estimated_cost_per_response_usd(),
        response_count,
    );
    let mut transaction =
        crate::api::begin_project_daily_budget_transaction(&state.db, project_id, estimated_cost)
            .await
            .map_err(tester_judge_budget_error)?;
    crate::api::check_orchestration_budgets(
        &mut transaction,
        project_id,
        parent_orchestration_id,
        None,
        estimated_cost,
    )
    .await
    .map_err(tester_judge_budget_error)?;
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO agent_orchestrations (project_id, username, trigger_kind, status, current_stage, parent_orchestration_id) VALUES ($1, $2, 'tester_judge', 'running', 'tester_judge', $3) RETURNING id",
    )
    .bind(project_id)
    .bind(username)
    .bind(parent_orchestration_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("创建期望输出复核编排", error))?;
    let prompt = agent.prompt_asset("tester_judge");
    let prompt_digest = format!("{:x}", Sha256::digest(prompt.content.as_bytes()));
    let run_id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO agent_runs (
            project_id, stage, status, model, prompt_version, prompt_digest,
            input_json, usage_json, orchestration_id
        ) VALUES ($1, 'tester_judge', 'running', $2, $3, $4, $5, $6, $7)
        RETURNING id"#,
    )
    .bind(project_id)
    .bind(agent.model())
    .bind(&prompt.version)
    .bind(&prompt_digest)
    .bind(input)
    .bind(crate::api::project_budget_reservation(estimated_cost))
    .bind(orchestration_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录期望输出复核 Agent 运行", error))?;
    sqlx::query(
        "INSERT INTO agent_steps (orchestration_id, agent_run_id, step_no, agent_role, task_kind, model, prompt_version, prompt_digest, artifact_kind) VALUES ($1, $2, 1, 'tester_judge', 'tester_expected_output_judge', $3, $4, $5, 'tester_judge_review')",
    )
    .bind(orchestration_id)
    .bind(run_id)
    .bind(agent.model())
    .bind(&prompt.version)
    .bind(prompt_digest)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录期望输出复核步骤", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交期望输出复核预算", error))?;
    Ok((run_id, orchestration_id))
}

async fn finish_tester_judge_agent_run(
    state: &AppState,
    run_id: i64,
    orchestration_id: i64,
    agent: &crate::nl_dsl::agent::LlmAgent,
    output: Result<Value, String>,
) -> Result<(), ProjectApiError> {
    let usage = agent.usage_snapshot();
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("开始完成期望输出复核运行", error))?;
    finish_tester_judge_agent_run_in_transaction(
        &mut transaction,
        run_id,
        orchestration_id,
        &usage,
        output,
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交期望输出复核结果", error))
}

async fn finish_tester_judge_agent_run_in_transaction(
    connection: &mut sqlx::PgConnection,
    run_id: i64,
    orchestration_id: i64,
    usage: &Value,
    output: Result<Value, String>,
) -> Result<(), ProjectApiError> {
    // Match the cancellation/recovery lock order: orchestration, run, step.
    let current_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM agent_orchestrations WHERE id = $1 FOR UPDATE",
    )
    .bind(orchestration_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| ProjectApiError::database("读取复核编排终态", error))?;
    if current_status != "running" {
        // Preserve terminal status/output, but retain usage observed before cancellation.
        sqlx::query(
            "UPDATE agent_runs SET usage_json = $1 WHERE id = $2 AND orchestration_id = $3",
        )
        .bind(usage)
        .bind(run_id)
        .bind(orchestration_id)
        .execute(&mut *connection)
        .await
        .map_err(|error| ProjectApiError::database("补记终止复核运行用量", error))?;
        sqlx::query("UPDATE agent_steps SET usage_json = $1 WHERE agent_run_id = $2 AND orchestration_id = $3")
            .bind(usage).bind(run_id).bind(orchestration_id)
            .execute(&mut *connection).await
            .map_err(|error| ProjectApiError::database("补记终止复核步骤用量", error))?;
        return Ok(());
    }
    let (status, output, error) = match output {
        Ok(output) => ("succeeded", Some(output), None),
        Err(message) => (
            "failed",
            None,
            Some(json!({"message": message, "retryable": false})),
        ),
    };
    let artifact_digest = output
        .as_ref()
        .map(|value| format!("{:x}", Sha256::digest(value.to_string().as_bytes())));
    sqlx::query("UPDATE agent_runs SET status = $1, output_json = $2, error_json = $3, usage_json = $4, finished_at = NOW() WHERE id = $5 AND status = 'running'")
        .bind(status)
        .bind(output)
        .bind(error.clone())
        .bind(&usage)
        .bind(run_id)
        .execute(&mut *connection)
        .await
        .map_err(|error| ProjectApiError::database("更新期望输出复核运行", error))?;
    sqlx::query("UPDATE agent_steps SET status = $1, artifact_digest = $2, usage_json = $3, error_json = $4, finished_at = NOW() WHERE agent_run_id = $5 AND status = 'running'")
        .bind(status)
        .bind(artifact_digest)
        .bind(usage)
        .bind(error)
        .bind(run_id)
        .execute(&mut *connection)
        .await
        .map_err(|error| ProjectApiError::database("更新期望输出复核步骤", error))?;
    sqlx::query("UPDATE agent_orchestrations SET status = $1, finished_at = NOW() WHERE id = $2 AND status = 'running'")
        .bind(status)
        .bind(orchestration_id)
        .execute(&mut *connection)
        .await
        .map_err(|error| ProjectApiError::database("更新期望输出复核编排", error))?;
    Ok(())
}

async fn judge_tester_proposal(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<crate::api::LlmState>>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id, proposal_id)): Path<(i64, i64, String)>,
    Json(payload): Json<ConvertTesterProposalRequest>,
) -> Result<Json<TesterJudgeReview>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    if !payload.input_json.is_object()
        || payload
            .expected_output_json
            .as_object()
            .is_none_or(|value| value.is_empty())
    {
        return Err(ProjectApiError::bad_request(
            "复核输入必须是 JSON 对象，且期望输出必须包含至少一个决策断言",
        ));
    }
    let input_bytes = serde_json::to_vec(&payload.input_json)
        .map_err(|error| ProjectApiError::bad_request(format!("测试输入无效：{error}")))?;
    let expected_bytes = serde_json::to_vec(&payload.expected_output_json)
        .map_err(|error| ProjectApiError::bad_request(format!("期望输出无效：{error}")))?;
    if input_bytes.len() > 32 * 1024 || expected_bytes.len() > 32 * 1024 {
        return Err(ProjectApiError::bad_request(
            "测试输入和期望输出各不能超过 32 KiB",
        ));
    }
    let input_digest = format!("{:x}", Sha256::digest(&input_bytes));
    let expected_output_digest = format!("{:x}", Sha256::digest(&expected_bytes));
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            Value,
            Value,
            Option<Value>,
            Option<Value>,
            Option<i64>,
            Option<i64>,
            String,
        ),
    >(
        r#"SELECT workflow.status, workflow.requirement_text, workflow.decision_brief_json,
                  workflow.answers_json, workflow.decision_plan_json, workflow.tester_report_json,
                  project.current_draft_version_id, workflow.plan_orchestration_id, version.status
           FROM decision_workflows workflow
           JOIN decision_projects project ON project.id = workflow.project_id
           JOIN decision_versions version ON version.id = project.current_draft_version_id
           WHERE workflow.project_id = $1 AND project.current_draft_version_id = $2"#,
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取测试提案复核上下文", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if !matches!(row.0.as_str(), "planned" | "confirmed")
        || matches!(row.8.as_str(), "published" | "retired")
    {
        return Err(ProjectApiError::bad_request(
            "只能复核当前可编辑版本中的已规划测试提案",
        ));
    }
    let mut report = serde_json::from_value::<TesterReport>(
        row.5
            .ok_or_else(|| ProjectApiError::bad_request("当前工作流没有测试工程提案"))?,
    )
    .map_err(|error| ProjectApiError::bad_request(format!("测试提案报告无效：{error}")))?;
    if report.schema_version != 1 {
        return Err(ProjectApiError::bad_request("不支持的测试提案报告版本"));
    }
    let proposal = report
        .proposals
        .iter_mut()
        .find(|proposal| proposal.id == proposal_id)
        .ok_or_else(ProjectApiError::not_found)?;
    if !tester_proposal_matches_version(proposal, version_id) {
        return Err(ProjectApiError::not_found());
    }
    if proposal.test_case_id.is_some() || !proposal.review_required {
        return Err(ProjectApiError::conflict(
            "该提案已转换或不再处于待复核状态",
        ));
    }
    let agent = llm_state
        .plan_agent_for_project(project_id, false, crate::api::AgentComplexity::Standard)
        .map(|(agent, _)| agent.fork_for_run())
        .ok_or_else(|| {
            ProjectApiError::coded(
                StatusCode::SERVICE_UNAVAILABLE,
                "MODEL_UNAVAILABLE",
                "当前未配置测试用例复核模型",
            )
        })?;
    let prompt = agent.prompt_asset("tester_judge");
    let prompt_digest = format!("{:x}", Sha256::digest(prompt.content.as_bytes()));
    let context = tester_judge_context(&row.1, &row.2, &row.3, &row.4, proposal, &payload);
    let context_digest = format!("{:x}", Sha256::digest(context.to_string().as_bytes()));
    if let Some(existing) = &proposal.judge_review
        && existing.input_digest == input_digest
        && existing.expected_output_digest == expected_output_digest
        && existing.prompt_digest == prompt_digest
        && existing.prompt_version == prompt.version
        && existing.context_digest == context_digest
    {
        return Ok(Json(existing.clone()));
    }
    let run_input = json!({
        "task": "tester_expected_output_judge",
        "proposalId": proposal_id,
        "inputDigest": input_digest,
        "expectedOutputDigest": expected_output_digest,
        "sampleCount": 2,
        "contextDigest": context_digest,
    });
    let (run_id, orchestration_id) =
        begin_tester_judge_agent_run(&state, project_id, &claims.sub, &agent, &run_input, row.7)
            .await?;
    let votes = tester_judge_sample_votes(
        || async {
            let running = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM agent_orchestrations WHERE id = $1 AND status = 'running')",
            ).bind(orchestration_id).fetch_one(&state.db).await?;
            anyhow::ensure!(running, "复核编排已取消或结束");
            Ok(())
        },
        || agent.judge_tester_expected_output(&context),
        crate::workflow_api::wait_for_orchestration_cancellation(&state.db, orchestration_id),
    ).await;
    let votes = match votes {
        Ok(votes) => votes,
        Err(error) => {
            let cancelled = error.to_string().contains("取消");
            eprintln!("测试期望值复核已停止：{error:#}");
            finish_tester_judge_agent_run(
                &state,
                run_id,
                orchestration_id,
                &agent,
                Err("辅助复核已停止，未保存结果".into()),
            )
            .await?;
            if cancelled {
                return Err(ProjectApiError::conflict(
                    "复核编排已取消或结束，结果未保存",
                ));
            }
            return Err(ProjectApiError::coded(
                StatusCode::BAD_GATEWAY,
                "TESTER_JUDGE_FAILED",
                "测试期望值辅助复核失败，请人工复核或稍后重试",
            ));
        }
    };
    let mut review = tester_judge_review_from_votes(votes, input_digest, expected_output_digest);
    review.prompt_version = prompt.version.clone();
    review.prompt_digest = prompt_digest;
    review.context_digest = context_digest;
    let output = serde_json::to_value(&review)
        .map_err(|error| ProjectApiError::bad_request(format!("编码复核结果失败：{error}")))?;
    let persist_result = async {
        let mut transaction = state
            .db
            .begin()
            .await
            .map_err(|error| ProjectApiError::database("开始保存测试期望值复核", error))?;
        ensure_tester_judge_running(&mut transaction, orchestration_id).await?;
        let current = sqlx::query_as::<_, (Option<Value>, Option<i64>, String, String, String, Value, Value, Option<Value>)>(
            r#"SELECT workflow.tester_report_json, project.current_draft_version_id, version.status,
                      workflow.status, workflow.requirement_text, workflow.decision_brief_json,
                      workflow.answers_json, workflow.decision_plan_json
               FROM decision_workflows workflow
               JOIN decision_projects project ON project.id = workflow.project_id
               JOIN decision_versions version ON version.id = project.current_draft_version_id
               WHERE workflow.project_id = $1 AND project.current_draft_version_id = $2
               FOR UPDATE OF workflow, project, version"#,
        )
        .bind(project_id)
        .bind(version_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("锁定测试提案复核上下文", error))?
        .ok_or_else(|| ProjectApiError::conflict("项目草稿已变化，请重新加载后复核"))?;
        if current.1 != Some(version_id) || matches!(current.2.as_str(), "published" | "retired")
            || !matches!(current.3.as_str(), "planned" | "confirmed")
        {
            return Err(ProjectApiError::conflict("项目草稿已变化，请重新加载后复核"));
        }
        let mut current_report = serde_json::from_value::<TesterReport>(
            current.0.ok_or_else(|| ProjectApiError::conflict("测试提案已被清除，请重新加载"))?,
        )
        .map_err(|error| ProjectApiError::bad_request(format!("测试提案报告无效：{error}")))?;
        let current_proposal = current_report
            .proposals
            .iter_mut()
            .find(|proposal| proposal.id == proposal_id)
            .ok_or_else(|| ProjectApiError::conflict("测试提案已变化，请重新加载"))?;
        if !tester_proposal_matches_version(current_proposal, version_id) {
            return Err(ProjectApiError::conflict(
                "静态发现提案已属于其他草稿版本，请重新加载",
            ));
        }
        if current_proposal.test_case_id.is_some() || !current_proposal.review_required {
            return Err(ProjectApiError::conflict("测试提案已被转换，请重新加载"));
        }
        if current_report.schema_version != 1 || tester_judge_context(
            &current.4, &current.5, &current.6, &current.7, current_proposal, &payload,
        ) != context {
            return Err(ProjectApiError::conflict("需求、方案或测试提案已变化，复核结果未保存"));
        }
        current_proposal.judge_review = Some(review.clone());
        sqlx::query("UPDATE decision_workflows SET tester_report_json = $1, updated_at = NOW() WHERE project_id = $2")
            .bind(serde_json::to_value(&current_report).unwrap_or(Value::Null))
            .bind(project_id)
            .execute(&mut *transaction)
            .await
            .map_err(|error| ProjectApiError::database("保存测试期望值复核结果", error))?;
        insert_audit_event(
            &mut *transaction,
            project_id,
            Some(version_id),
            "tester_proposal.judged",
            &claims.sub,
            "完成两次独立的测试期望值辅助复核",
            json!({
                "proposalId": proposal_id,
                "status": review.status,
                "inputDigest": review.input_digest,
                "expectedOutputDigest": review.expected_output_digest,
            }),
        )
        .await
        .map_err(|error| ProjectApiError::database("记录测试期望值复核审计", error))?;
        finish_tester_judge_agent_run_in_transaction(
            &mut transaction, run_id, orchestration_id, &agent.usage_snapshot(), Ok(output),
        ).await?;
        transaction
            .commit()
            .await
            .map_err(|error| ProjectApiError::database("提交测试期望值复核", error))
    }
    .await;
    if let Err(error) = persist_result {
        finish_tester_judge_agent_run(
            &state,
            run_id,
            orchestration_id,
            &agent,
            Err("复核结果因项目状态变化未写回提案".into()),
        )
        .await?;
        return Err(error);
    }
    Ok(Json(review))
}

async fn convert_tester_proposal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id, proposal_id)): Path<(i64, i64, String)>,
    Json(payload): Json<ConvertTesterProposalRequest>,
) -> Result<Json<DecisionTestCase>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("开始转换测试提案", error))?;
    let workflow = sqlx::query_as::<_, (String, Option<Value>)>(
        r#"
        SELECT workflow.status, workflow.tester_report_json
        FROM decision_workflows workflow
        JOIN decision_projects project ON project.id = workflow.project_id
        JOIN decision_versions version ON version.id = project.current_draft_version_id
        WHERE workflow.project_id = $1
          AND project.current_draft_version_id = $2
          AND version.status NOT IN ('published', 'retired')
        FOR UPDATE OF workflow, project, version
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取测试提案工作流", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if !matches!(workflow.0.as_str(), "planned" | "confirmed") {
        return Err(ProjectApiError::bad_request(
            "请先完成决策方案规划后再转换测试提案",
        ));
    }
    let report_json = workflow
        .1
        .ok_or_else(|| ProjectApiError::bad_request("当前工作流没有测试工程提案"))?;
    let mut report = serde_json::from_value::<TesterReport>(report_json)
        .map_err(|error| ProjectApiError::bad_request(format!("测试提案报告无效：{error}")))?;
    if report.schema_version != 1 {
        return Err(ProjectApiError::bad_request("不支持的测试提案报告版本"));
    }
    let proposal = report
        .proposals
        .iter_mut()
        .find(|proposal| proposal.id == proposal_id)
        .ok_or_else(ProjectApiError::not_found)?;
    if !tester_proposal_matches_version(proposal, version_id) {
        return Err(ProjectApiError::not_found());
    }
    let proposal_id = proposal.id.clone();
    if !proposal.review_required || proposal.expected_output.is_some() {
        return Err(ProjectApiError::bad_request("该提案不处于待业务复核状态"));
    }
    if let Some(test_case_id) = proposal.test_case_id {
        let existing = sqlx::query_as::<_, DecisionTestCase>(
            r#"
            SELECT id, version_id, name, category, input_json, expected_output_json,
                   actual_output_json, execution_trace_json, status, source, approved_by, approved_at,
                   created_at, updated_at
            FROM decision_test_cases
            WHERE id = $1 AND version_id = $2 AND tester_proposal_id = $3
            "#,
        )
        .bind(test_case_id)
        .bind(version_id)
        .bind(&proposal_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("读取已转换的测试用例", error))?
        .ok_or_else(|| ProjectApiError::conflict("提案关联的正式测试用例不存在，请刷新工作流"))?;
        transaction
            .commit()
            .await
            .map_err(|error| ProjectApiError::database("提交幂等测试提案转换", error))?;
        return Ok(Json(existing));
    }

    let name = proposal.title.trim();
    let name = if name.chars().count() > 50 {
        name.chars().take(50).collect::<String>()
    } else {
        name.to_string()
    };
    let case_payload = CreateDecisionTestCase {
        name,
        input_json: payload.input_json,
        expected_output_json: payload.expected_output_json,
    };
    validate_test_case(&case_payload)?;
    let test_case = sqlx::query_as::<_, DecisionTestCase>(
        r#"
        INSERT INTO decision_test_cases (
            version_id, name, category, input_json, expected_output_json,
            status, source, tester_proposal_id
        ) VALUES ($1, $2, 'business', $3, $4, 'pending', 'tester_proposal', $5)
        RETURNING id, version_id, name, category, input_json, expected_output_json,
                  actual_output_json, execution_trace_json, status, source, approved_by, approved_at,
                  created_at, updated_at
        "#,
    )
    .bind(version_id)
    .bind(case_payload.name)
    .bind(case_payload.input_json)
    .bind(case_payload.expected_output_json)
    .bind(&proposal_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存提案业务测试用例", error))?;
    proposal.test_case_id = Some(test_case.id);
    proposal.judge_review = None;
    sqlx::query("UPDATE decision_workflows SET tester_report_json = $1 WHERE project_id = $2")
        .bind(serde_json::to_value(&report).unwrap_or(Value::Null))
        .bind(project_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("记录提案转换状态", error))?;
    invalidate_verification(&mut transaction, version_id).await?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version_id),
        "tester_proposal.converted",
        &claims.sub,
        "将测试工程提案转换为待执行业务用例",
        json!({
            "proposalId": proposal_id,
            "testCaseId": test_case.id,
            "testCaseName": test_case.name,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录测试提案转换审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交测试提案转换", error))?;
    Ok(Json(test_case))
}

fn business_acceptance_gate(total: i64, passed: i64, approved: i64) -> VerificationGate {
    let (status, message) = if total == 0 {
        ("failed", "至少需要一个人工业务验收案例")
    } else if passed != total {
        ("failed", "存在尚未通过的业务验收案例")
    } else if approved != total {
        ("failed", "存在尚未人工批准的业务验收案例")
    } else {
        ("passed", "全部业务验收案例已通过并完成人工批准")
    };
    VerificationGate {
        code: "BUSINESS_ACCEPTANCE".into(),
        name: "业务验收".into(),
        status: status.into(),
        message: message.into(),
    }
}

async fn test_case_counts(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    version_id: i64,
) -> Result<(i64, i64, i64), sqlx::Error> {
    sqlx::query_as::<_, (i64, i64, i64)>(
        r#"
        SELECT COUNT(*),
               COUNT(*) FILTER (WHERE status = 'passed'),
               COUNT(*) FILTER (WHERE approved_by IS NOT NULL AND approved_at IS NOT NULL)
        FROM decision_test_cases
        WHERE version_id = $1 AND category = 'business'
        "#,
    )
    .bind(version_id)
    .fetch_one(executor)
    .await
}

async fn insert_audit_event(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    project_id: i64,
    version_id: Option<i64>,
    event_type: &str,
    actor: &str,
    summary: &str,
    details: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_events (
            project_id, version_id, event_type, actor_username, summary, details_json
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .bind(event_type)
    .bind(actor)
    .bind(summary)
    .bind(details)
    .execute(executor)
    .await
    .map(|_| ())
}

async fn invalidate_verification(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    version_id: i64,
) -> Result<(), ProjectApiError> {
    sqlx::query(
        "UPDATE decision_artifacts SET verification_report_json = NULL WHERE version_id = $1",
    )
    .bind(version_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| ProjectApiError::database("使旧验收报告失效", error))?;
    sqlx::query(
        "UPDATE decision_versions SET status = 'draft' WHERE id = $1 AND status = 'validated'",
    )
    .bind(version_id)
    .execute(&mut **transaction)
    .await
    .map_err(|error| ProjectApiError::database("重置版本验收状态", error))?;
    Ok(())
}

async fn validate_change_submission(
    base_version_id: Option<i64>,
    payload: &CreateDecisionVersion,
) -> Result<(), ProjectApiError> {
    if payload.candidate_id.is_some() {
        return Ok(());
    }
    if base_version_id.is_some() {
        return Err(ProjectApiError::patch(
            "PATCH_PROTOCOL",
            "已有版本的修改必须通过模型变更预览和提交接口完成",
        ));
    }
    Ok(())
}

async fn list_projects(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<DecisionProject>>, ProjectApiError> {
    let projects = sqlx::query_as::<_, DecisionProject>(
        r#"
        SELECT id, owner_username, name, description, risk_level,
               current_draft_version_id, published_version_id, created_at, updated_at
        FROM decision_projects
        WHERE EXISTS (
               SELECT 1 FROM users admin
               WHERE admin.username = $1 AND admin.role = 'admin'
           )
           OR owner_username = $1
           OR EXISTS (
               SELECT 1 FROM project_members member
               WHERE member.project_id = decision_projects.id AND member.username = $1
           )
        ORDER BY updated_at DESC, id DESC
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取决策项目", error))?;
    Ok(Json(projects))
}

async fn create_project(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<CreateDecisionProject>,
) -> Result<(StatusCode, Json<DecisionProject>), ProjectApiError> {
    validate_project(&payload)?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("创建决策项目", error))?;
    let project = sqlx::query_as::<_, DecisionProject>(
        r#"
        INSERT INTO decision_projects (owner_username, name, description, risk_level)
        VALUES ($1, $2, $3, $4)
        RETURNING id, owner_username, name, description, risk_level,
                  current_draft_version_id, published_version_id, created_at, updated_at
        "#,
    )
    .bind(&claims.sub)
    .bind(payload.name.trim())
    .bind(payload.description.trim())
    .bind(&payload.risk_level)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("创建决策项目", error))?;
    insert_audit_event(
        &mut *transaction,
        project.id,
        None,
        "project.created",
        &claims.sub,
        "创建决策项目",
        json!({
            "name": project.name,
            "riskLevel": project.risk_level,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录项目审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交决策项目", error))?;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn update_project(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<CreateDecisionProject>,
) -> Result<Json<DecisionProject>, ProjectApiError> {
    validate_project(&payload)?;
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Manage).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("更新决策项目", error))?;
    let project = sqlx::query_as::<_, DecisionProject>(
        r#"
        UPDATE decision_projects
        SET name=$1, description=$2, risk_level=$3, updated_at=NOW()
        WHERE id=$4
        RETURNING id, owner_username, name, description, risk_level,
                  current_draft_version_id, published_version_id, created_at, updated_at
        "#,
    )
    .bind(payload.name.trim())
    .bind(payload.description.trim())
    .bind(&payload.risk_level)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新决策项目", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    insert_audit_event(
        &mut *transaction,
        project.id,
        None,
        "project.updated",
        &claims.sub,
        "更新决策项目",
        json!({"name": project.name, "riskLevel": project.risk_level}),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录项目审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交决策项目更新", error))?;
    Ok(Json(project))
}

async fn delete_project(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<StatusCode, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Manage).await?;

    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("开始删除决策项目", error))?;

    // decision_runs 对 version 使用 RESTRICT 外键，必须在级联删除版本前清理。
    sqlx::query("DELETE FROM decision_runs WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("删除决策运行记录", error))?;

    let project_name = sqlx::query_scalar::<_, String>(
        "DELETE FROM decision_projects
         WHERE id = $1
           AND (owner_username = $2 OR EXISTS (
               SELECT 1 FROM users WHERE username = $2 AND role = 'admin'
           ))
         RETURNING name",
    )
    .bind(project_id)
    .bind(&claims.sub)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("删除决策项目", error))?
    .ok_or_else(ProjectApiError::not_found)?;

    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交删除决策项目", error))?;
    eprintln!(
        "决策项目已删除: id={project_id}, name={project_name}, owner={}",
        claims.sub
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn list_project_members(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<ProjectMember>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let members = sqlx::query_as::<_, ProjectMember>(
        r#"
        SELECT project_id, username, role, added_by, created_at, updated_at
        FROM project_members
        WHERE project_id = $1
        ORDER BY role, username
        "#,
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取项目成员", error))?;
    Ok(Json(members))
}

async fn save_project_member(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, username)): Path<(i64, String)>,
    Json(payload): Json<SaveProjectMember>,
) -> Result<Json<ProjectMember>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Manage).await?;
    let username = username.trim();
    if username.is_empty() || username.chars().count() > 255 {
        return Err(ProjectApiError::bad_request(
            "成员用户名不能为空且不能超过 255 个字符",
        ));
    }
    validate_member_role(payload.role.as_str())?;
    let owner = sqlx::query_scalar::<_, String>(
        "SELECT owner_username FROM decision_projects WHERE id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取项目负责人", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if owner == username {
        return Err(ProjectApiError::bad_request("项目负责人无需重复添加为成员"));
    }
    let user_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
            .bind(username)
            .fetch_one(&state.db)
            .await
            .map_err(|error| ProjectApiError::database("校验项目成员用户", error))?;
    if !user_exists {
        return Err(ProjectApiError::bad_request(
            "待添加的成员不存在，请先联系管理员添加用户",
        ));
    }
    let member = sqlx::query_as::<_, ProjectMember>(
        r#"
        INSERT INTO project_members (project_id, username, role, added_by)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (project_id, username, role) DO UPDATE
        SET role = EXCLUDED.role, added_by = EXCLUDED.added_by, updated_at = NOW()
        RETURNING project_id, username, role, added_by, created_at, updated_at
        "#,
    )
    .bind(project_id)
    .bind(username)
    .bind(payload.role.trim())
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(|error| {
        if member_roles_schema_error(&error) {
            ProjectApiError::coded(
                StatusCode::CONFLICT,
                "MEMBER_SCHEMA_UPGRADE_REQUIRED",
                "项目成员表尚未启用多角色支持，请先执行 script/017_project_member_roles.sql 后重试",
            )
        } else {
            ProjectApiError::database("保存项目成员", error)
        }
    })?;
    sqlx::query(
        r#"INSERT INTO audit_events (project_id, event_type, actor_username, summary, details_json)
           VALUES ($1, 'project.member_saved', $2, '更新项目成员角色', $3)"#,
    )
    .bind(project_id)
    .bind(&claims.sub)
    .bind(json!({"username": member.username, "role": member.role}))
    .execute(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("记录成员变更审计事件", error))?;
    Ok(Json(member))
}

#[derive(Debug, Deserialize)]
struct DeleteProjectMemberQuery {
    role: Option<String>,
}

async fn delete_project_member(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, username)): Path<(i64, String)>,
    Query(query): Query<DeleteProjectMemberQuery>,
) -> Result<StatusCode, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Manage).await?;
    if let Some(role) = query.role.as_deref() {
        validate_member_role(role)?;
    }
    let (statement, role_value) = match query.role.as_deref() {
        Some(role) => (
            "DELETE FROM project_members WHERE project_id = $1 AND username = $2 AND role = $3",
            Some(role),
        ),
        None => (
            "DELETE FROM project_members WHERE project_id = $1 AND username = $2",
            None,
        ),
    };
    let mut deletion = sqlx::query(statement)
        .bind(project_id)
        .bind(username.trim());
    if let Some(role) = role_value {
        deletion = deletion.bind(role);
    }
    let removed = deletion
        .execute(&state.db)
        .await
        .map_err(|error| ProjectApiError::database("移除项目成员角色", error))?;
    if removed.rows_affected() == 0 {
        return Err(ProjectApiError::not_found());
    }
    insert_audit_event(
        &state.db,
        project_id,
        None,
        "project.member_removed",
        &claims.sub,
        "移除项目成员角色",
        json!({"username": username, "role": query.role}),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录成员移除审计事件", error))?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_template_cases(value: Value) -> Result<Vec<ModelTemplateTestCase>, ProjectApiError> {
    serde_json::from_value(value)
        .map_err(|error| ProjectApiError::bad_request(format!("模板验收案例无效：{error}")))
}

async fn load_business_cases(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    version_id: i64,
) -> Result<Vec<ModelTemplateTestCase>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String, Value, Value, String)>(
        r#"
        SELECT name, category, input_json, expected_output_json, source
        FROM decision_test_cases
        WHERE version_id = $1 AND category = 'business'
        ORDER BY created_at, id
        "#,
    )
    .bind(version_id)
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(name, category, input_json, expected_output_json, source)| ModelTemplateTestCase {
                name,
                category,
                input_json,
                expected_output_json,
                source,
            },
        )
        .collect())
}

fn saved_snapshot(row: SavedTemplateSnapshotRow) -> Result<ModelSnapshot, ProjectApiError> {
    let test_cases = parse_template_cases(row.test_cases_json)?;
    Ok(ModelSnapshot {
        item: ModelLibraryItem {
            source_kind: "template".into(),
            source_id: row.id.to_string(),
            name: row.name,
            description: row.description,
            industry: row.industry,
            risk_level: row.risk_level,
            source_project_id: row.source_project_id,
            source_project_name: row.source_project_name,
            source_version_id: row.source_version_id,
            source_version_number: row.source_version_number,
            test_case_count: test_cases.len() as i64,
            created_at: Some(row.created_at),
        },
        requirement_text: row.requirement_text,
        decision_brief: row.decision_brief_json,
        decision_plan: row.decision_plan_json,
        dsl_yaml: row.dsl_yaml,
        dmn_xml: row.dmn_xml,
        knowledge_citations: serde_json::from_value(row.knowledge_citations_json)
            .map_err(|error| ProjectApiError::bad_request(format!("模板知识引用无效：{error}")))?,
        test_cases,
    })
}

async fn list_model_library(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<ModelLibraryItem>>, ProjectApiError> {
    let mut items = builtin_templates()
        .iter()
        .map(builtin_snapshot)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|snapshot| snapshot.item)
        .collect::<Vec<_>>();

    let saved = sqlx::query_as::<_, SavedTemplateSnapshotRow>(
        r#"
        SELECT template.id, template.name, template.description, template.industry,
               template.risk_level, template.source_project_id, project.name AS source_project_name,
               template.source_version_id, version.version_number AS source_version_number,
               template.requirement_text, template.decision_brief_json,
               template.decision_plan_json, template.dsl_yaml, template.dmn_xml,
               template.knowledge_citations_json, template.test_cases_json, template.created_at
        FROM decision_templates template
        LEFT JOIN decision_projects project ON project.id = template.source_project_id
        LEFT JOIN decision_versions version ON version.id = template.source_version_id
        WHERE template.owner_username = $1
        ORDER BY template.updated_at DESC, template.id DESC
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取已保存模板", error))?;
    items.extend(
        saved
            .into_iter()
            .map(saved_snapshot)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|snapshot| snapshot.item),
    );

    let history = sqlx::query_as::<
        _,
        (
            i64,
            i32,
            chrono::DateTime<chrono::Utc>,
            i64,
            String,
            String,
            String,
            i64,
        ),
    >(
        r#"
        SELECT version.id, version.version_number, version.created_at,
               project.id, project.name, project.description, project.risk_level,
               COUNT(test.id) FILTER (WHERE test.category = 'business')
        FROM decision_versions version
        JOIN decision_projects project ON project.id = version.project_id
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        LEFT JOIN decision_test_cases test ON test.version_id = version.id
        WHERE project.owner_username = $1 AND version.status IN ('published', 'retired')
        GROUP BY version.id, project.id
        ORDER BY version.created_at DESC, version.id DESC
        LIMIT 100
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取历史模型", error))?;
    items.extend(history.into_iter().map(
        |(
            version_id,
            version_number,
            created_at,
            project_id,
            project_name,
            description,
            risk_level,
            test_case_count,
        )| ModelLibraryItem {
            source_kind: "history".into(),
            source_id: version_id.to_string(),
            name: format!("{} v{}", project_name, version_number),
            description,
            industry: "历史模型".into(),
            risk_level,
            source_project_id: Some(project_id),
            source_project_name: Some(project_name),
            source_version_id: Some(version_id),
            source_version_number: Some(version_number),
            test_case_count,
            created_at: Some(created_at),
        },
    ));
    Ok(Json(items))
}

async fn save_decision_template(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
    Json(payload): Json<SaveDecisionTemplate>,
) -> Result<(StatusCode, Json<ModelLibraryItem>), ProjectApiError> {
    validate_template(&payload)?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存模型模板", error))?;
    let source = sqlx::query_as::<
        _,
        (
            String,
            String,
            i32,
            String,
            String,
            String,
            Option<String>,
            Option<Value>,
            Option<Value>,
            Value,
        ),
    >(
        r#"
        SELECT project.name, project.risk_level, version.version_number, version.status,
               artifact.dsl_yaml, artifact.dmn_xml, workflow.requirement_text,
               artifact.decision_brief_json, artifact.decision_plan_json,
               artifact.knowledge_citations_json
        FROM decision_versions version
        JOIN decision_projects project ON project.id = version.project_id
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        LEFT JOIN decision_workflows workflow ON workflow.project_id = project.id
        WHERE version.id = $1 AND version.project_id = $2 AND project.owner_username = $3
        "#,
    )
    .bind(version_id)
    .bind(project_id)
    .bind(&claims.sub)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取模板来源版本", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if !matches!(source.3.as_str(), "published" | "retired") {
        return Err(ProjectApiError::bad_request(
            "只能将已发布或已退役版本保存为模板",
        ));
    }
    let test_cases = load_business_cases(&mut *transaction, version_id)
        .await
        .map_err(|error| ProjectApiError::database("读取模板业务案例", error))?;
    let test_cases_json = serde_json::to_value(&test_cases)
        .map_err(|error| ProjectApiError::bad_request(format!("模板案例序列化失败：{error}")))?;
    let row = sqlx::query_as::<_, (i64, chrono::DateTime<chrono::Utc>)>(
        r#"
        INSERT INTO decision_templates (
            owner_username, name, description, industry, risk_level,
            source_project_id, source_version_id, requirement_text,
            decision_brief_json, decision_plan_json, dsl_yaml, dmn_xml,
            knowledge_citations_json, test_cases_json
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        RETURNING id, created_at
        "#,
    )
    .bind(&claims.sub)
    .bind(payload.name.trim())
    .bind(payload.description.trim())
    .bind(payload.industry.trim())
    .bind(&source.1)
    .bind(project_id)
    .bind(version_id)
    .bind(&source.6)
    .bind(&source.7)
    .bind(&source.8)
    .bind(&source.4)
    .bind(&source.5)
    .bind(&source.9)
    .bind(test_cases_json)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存模型模板", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version_id),
        "template.saved",
        &claims.sub,
        "将历史版本保存为模型模板",
        json!({"templateId": row.0, "templateName": payload.name.trim()}),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录模板审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交模型模板", error))?;
    Ok((
        StatusCode::CREATED,
        Json(ModelLibraryItem {
            source_kind: "template".into(),
            source_id: row.0.to_string(),
            name: payload.name.trim().into(),
            description: payload.description.trim().into(),
            industry: payload.industry.trim().into(),
            risk_level: source.1,
            source_project_id: Some(project_id),
            source_project_name: Some(source.0),
            source_version_id: Some(version_id),
            source_version_number: Some(source.2),
            test_case_count: test_cases.len() as i64,
            created_at: Some(row.1),
        }),
    ))
}

async fn load_model_snapshot(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    username: &str,
    source_kind: &str,
    source_id: &str,
) -> Result<ModelSnapshot, ProjectApiError> {
    match source_kind {
        "builtin" => {
            let template = builtin_templates()
                .into_iter()
                .find(|template| template.id == source_id)
                .ok_or_else(|| ProjectApiError::bad_request("内置模板不存在"))?;
            builtin_snapshot(&template)
        }
        "template" => {
            let template_id = source_id
                .parse::<i64>()
                .map_err(|_| ProjectApiError::bad_request("模板标识无效"))?;
            let row = sqlx::query_as::<_, SavedTemplateSnapshotRow>(
                r#"
                SELECT template.id, template.name, template.description, template.industry,
                       template.risk_level, template.source_project_id,
                       project.name AS source_project_name, template.source_version_id,
                       version.version_number AS source_version_number,
                       template.requirement_text, template.decision_brief_json,
                       template.decision_plan_json, template.dsl_yaml, template.dmn_xml,
                       template.knowledge_citations_json, template.test_cases_json, template.created_at
                FROM decision_templates template
                LEFT JOIN decision_projects project ON project.id = template.source_project_id
                LEFT JOIN decision_versions version ON version.id = template.source_version_id
                WHERE template.id = $1 AND template.owner_username = $2
                "#,
            )
            .bind(template_id)
            .bind(username)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(|error| ProjectApiError::database("读取模型模板", error))?
            .ok_or_else(|| ProjectApiError::bad_request("模型模板不存在"))?;
            saved_snapshot(row)
        }
        "history" => {
            let version_id = source_id
                .parse::<i64>()
                .map_err(|_| ProjectApiError::bad_request("历史版本标识无效"))?;
            let row = sqlx::query_as::<_, HistorySnapshotRow>(
                r#"
                SELECT version.id AS version_id, version.version_number, version.created_at,
                       project.id AS project_id, project.name AS project_name,
                       project.description AS project_description, project.risk_level,
                       workflow.requirement_text, artifact.decision_brief_json,
                       artifact.decision_plan_json, artifact.dsl_yaml, artifact.dmn_xml,
                       artifact.knowledge_citations_json
                FROM decision_versions version
                JOIN decision_projects project ON project.id = version.project_id
                JOIN decision_artifacts artifact ON artifact.version_id = version.id
                LEFT JOIN decision_workflows workflow ON workflow.project_id = project.id
                WHERE version.id = $1 AND project.owner_username = $2
                      AND version.status IN ('published', 'retired')
                "#,
            )
            .bind(version_id)
            .bind(username)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(|error| ProjectApiError::database("读取历史模型", error))?
            .ok_or_else(|| ProjectApiError::bad_request("可复用的历史模型不存在"))?;
            let test_cases = load_business_cases(&mut **transaction, row.version_id)
                .await
                .map_err(|error| ProjectApiError::database("读取历史模型案例", error))?;
            Ok(ModelSnapshot {
                item: ModelLibraryItem {
                    source_kind: "history".into(),
                    source_id: row.version_id.to_string(),
                    name: format!("{} v{}", row.project_name, row.version_number),
                    description: row.project_description,
                    industry: "历史模型".into(),
                    risk_level: row.risk_level,
                    source_project_id: Some(row.project_id),
                    source_project_name: Some(row.project_name),
                    source_version_id: Some(row.version_id),
                    source_version_number: Some(row.version_number),
                    test_case_count: test_cases.len() as i64,
                    created_at: Some(row.created_at),
                },
                requirement_text: row.requirement_text,
                decision_brief: row.decision_brief_json,
                decision_plan: row.decision_plan_json,
                dsl_yaml: row.dsl_yaml,
                dmn_xml: row.dmn_xml,
                knowledge_citations: serde_json::from_value(row.knowledge_citations_json).map_err(
                    |error| ProjectApiError::bad_request(format!("历史知识引用无效：{error}")),
                )?,
                test_cases,
            })
        }
        _ => Err(ProjectApiError::bad_request(
            "模型来源类型必须是 builtin、template 或 history",
        )),
    }
}

async fn instantiate_model_library_item(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((source_kind, source_id)): Path<(String, String)>,
    Json(payload): Json<InstantiateModelLibraryItem>,
) -> Result<(StatusCode, Json<DecisionWorkspace>), ProjectApiError> {
    validate_instantiation(&payload)?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("复用模型", error))?;
    let snapshot =
        load_model_snapshot(&mut transaction, &claims.sub, &source_kind, &source_id).await?;
    let definitions = nl_dsl::parse_definitions(&snapshot.dsl_yaml)
        .map_err(|error| ProjectApiError::bad_request(format!("来源模型 DSL 无效：{error}")))?;
    nl_dsl::validate(&definitions)
        .map_err(|error| ProjectApiError::bad_request(format!("来源模型校验失败：{error}")))?;
    let executable = crate::dmn::parse_dmn(&snapshot.dmn_xml)
        .map_err(|error| ProjectApiError::bad_request(format!("来源模型 DMN 无效：{error}")))?;
    crate::dmn::compile_dmn(&executable)
        .map_err(|error| ProjectApiError::bad_request(format!("来源模型 DMN 编译失败：{error}")))?;

    let project = sqlx::query_as::<_, DecisionProject>(
        r#"
        INSERT INTO decision_projects (owner_username, name, description, risk_level)
        VALUES ($1, $2, $3, $4)
        RETURNING id, owner_username, name, description, risk_level,
                  current_draft_version_id, published_version_id, created_at, updated_at
        "#,
    )
    .bind(&claims.sub)
    .bind(payload.project_name.trim())
    .bind(if payload.description.trim().is_empty() {
        snapshot.item.description.as_str()
    } else {
        payload.description.trim()
    })
    .bind(&snapshot.item.risk_level)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("创建复用项目", error))?;

    let change_review = build_change_review(None, &definitions, None, None, Some(1));
    let change_review_json = serde_json::to_value(&change_review).map_err(|error| {
        ProjectApiError::bad_request(format!("复用版本审查序列化失败：{error}"))
    })?;
    let change_summary = json!({
        "reused": true,
        "sourceKind": snapshot.item.source_kind,
        "sourceId": snapshot.item.source_id,
        "sourceProjectId": snapshot.item.source_project_id,
        "sourceVersionId": snapshot.item.source_version_id,
        "added": change_review.changes.iter().filter(|change| change.change_type == "add").map(|change| change.element.clone()).collect::<Vec<_>>(),
        "updated": [],
        "removed": [],
    });
    let version = sqlx::query_as::<_, DecisionVersion>(
        r#"
        INSERT INTO decision_versions (
            project_id, version_number, status, base_version_id, change_summary,
            created_by, reviewed_by, reviewed_at
        ) VALUES ($1, 1, 'draft', NULL, $2, $3, $3, NOW())
        RETURNING id, project_id, version_number, status, base_version_id,
                  rollback_source_version_id, rollback_reason, change_summary,
                  created_by, created_at, reviewed_by, reviewed_at
        "#,
    )
    .bind(project.id)
    .bind(change_summary)
    .bind(&claims.sub)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("创建复用草稿版本", error))?;
    sqlx::query(
        r#"
        INSERT INTO decision_artifacts (
            version_id, dsl_yaml, dmn_xml, decision_brief_json, decision_plan_json,
            model_patch_json, change_review_json, verification_report_json,
            knowledge_citations_json
        ) VALUES ($1, $2, $3, $4, $5, NULL, $6, NULL, $7)
        "#,
    )
    .bind(version.id)
    .bind(&snapshot.dsl_yaml)
    .bind(&snapshot.dmn_xml)
    .bind(&snapshot.decision_brief)
    .bind(&snapshot.decision_plan)
    .bind(change_review_json)
    .bind(serde_json::to_value(&snapshot.knowledge_citations).unwrap_or_else(|_| json!([])))
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存复用模型制品", error))?;
    for test_case in &snapshot.test_cases {
        let source = if matches!(test_case.source.as_str(), "manual" | "generated") {
            test_case.source.as_str()
        } else {
            "manual"
        };
        sqlx::query(
            r#"
            INSERT INTO decision_test_cases (
                version_id, name, category, input_json, expected_output_json, status, source
            ) VALUES ($1, $2, 'business', $3, $4, 'pending', $5)
            "#,
        )
        .bind(version.id)
        .bind(&test_case.name)
        .bind(&test_case.input_json)
        .bind(&test_case.expected_output_json)
        .bind(source)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("复制模板业务案例", error))?;
    }
    if let (Some(brief), Some(plan)) = (&snapshot.decision_brief, &snapshot.decision_plan) {
        sqlx::query(
            r#"
            INSERT INTO decision_workflows (
                project_id, status, requirement_text, decision_brief_json,
                clarifications_json, answers_json, decision_plan_json, confirmed_at
            ) VALUES ($1, 'confirmed', $2, $3, '[]'::JSONB, '[]'::JSONB, $4, NOW())
            "#,
        )
        .bind(project.id)
        .bind(
            snapshot
                .requirement_text
                .as_deref()
                .unwrap_or(&snapshot.item.description),
        )
        .bind(brief)
        .bind(plan)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("恢复模板需求与方案", error))?;
    }
    sqlx::query("UPDATE decision_projects SET current_draft_version_id = $1, updated_at = NOW() WHERE id = $2")
        .bind(version.id).bind(project.id).execute(&mut *transaction).await
        .map_err(|error| ProjectApiError::database("更新复用项目草稿", error))?;
    insert_audit_event(&mut *transaction, project.id, None, "project.created", &claims.sub,
        "从模型库创建决策项目", json!({"name": project.name, "riskLevel": project.risk_level, "sourceKind": snapshot.item.source_kind, "sourceId": snapshot.item.source_id}))
        .await.map_err(|error| ProjectApiError::database("记录复用项目审计事件", error))?;
    insert_audit_event(&mut *transaction, project.id, Some(version.id), "model.reused", &claims.sub,
        "复用模型库快照创建草稿版本", json!({
            "sourceKind": snapshot.item.source_kind, "sourceId": snapshot.item.source_id,
            "sourceProjectId": snapshot.item.source_project_id, "sourceVersionId": snapshot.item.source_version_id,
            "testCaseCount": snapshot.test_cases.len(),
        }))
        .await.map_err(|error| ProjectApiError::database("记录模型复用审计事件", error))?;
    if let Some(source_version_id) = snapshot.item.source_version_id {
        sqlx::query(
            r#"
            UPDATE decision_patterns
            SET usage_count = usage_count + 1, last_used = NOW()
            WHERE source_version_id = $1 AND owner_username = $2
            "#,
        )
        .bind(source_version_id)
        .bind(&claims.sub)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("更新模式复用次数", error))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交复用项目", error))?;
    let mut project = project;
    project.current_draft_version_id = Some(version.id);
    Ok((
        StatusCode::CREATED,
        Json(DecisionWorkspace {
            project,
            version: Some(version),
            dsl_yaml: Some(snapshot.dsl_yaml),
            dmn_xml: Some(snapshot.dmn_xml),
            knowledge_citations: snapshot.knowledge_citations,
        }),
    ))
}

async fn delete_decision_template(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(template_id): Path<i64>,
) -> Result<StatusCode, ProjectApiError> {
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("删除模型模板", error))?;
    let deleted = sqlx::query_as::<_, (String, Option<i64>, Option<i64>)>(
        r#"
        DELETE FROM decision_templates
        WHERE id = $1 AND owner_username = $2
        RETURNING name, source_project_id, source_version_id
        "#,
    )
    .bind(template_id)
    .bind(&claims.sub)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("删除模型模板", error))?
    .ok_or_else(|| ProjectApiError::bad_request("模型模板不存在"))?;
    if let Some(project_id) = deleted.1 {
        insert_audit_event(
            &mut *transaction,
            project_id,
            deleted.2,
            "template.deleted",
            &claims.sub,
            "删除模型模板",
            json!({"templateId": template_id, "templateName": deleted.0}),
        )
        .await
        .map_err(|error| ProjectApiError::database("记录模板删除审计事件", error))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交模板删除", error))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_workspace(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<DecisionWorkspace>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let project = sqlx::query_as::<_, DecisionProject>(
        r#"
        SELECT id, owner_username, name, description, risk_level,
               current_draft_version_id, published_version_id, created_at, updated_at
        FROM decision_projects
        WHERE id = $1
        "#,
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取决策项目", error))?
    .ok_or_else(ProjectApiError::not_found)?;

    let (version, dsl_yaml, dmn_xml, knowledge_citations) = if let Some(version_id) =
        project.current_draft_version_id
    {
        let version = sqlx::query_as::<_, DecisionVersion>(
            r#"
            SELECT id, project_id, version_number, status, base_version_id,
                   rollback_source_version_id, rollback_reason, change_summary,
                   created_by, created_at, reviewed_by, reviewed_at
            FROM decision_versions
            WHERE id = $1 AND project_id = $2
            "#,
        )
        .bind(version_id)
        .bind(project_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|error| ProjectApiError::database("读取决策版本", error))?;
        let artifact = sqlx::query_as::<_, (String, String, Value)>(
            "SELECT dsl_yaml, dmn_xml, knowledge_citations_json FROM decision_artifacts WHERE version_id = $1",
        )
        .bind(version_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|error| ProjectApiError::database("读取决策制品", error))?;
        match artifact {
            Some((dsl, xml, citations)) => (
                version,
                Some(dsl),
                Some(xml),
                serde_json::from_value(citations).map_err(|error| {
                    ProjectApiError::bad_request(format!("版本知识引用无效：{error}"))
                })?,
            ),
            None => (version, None, None, Vec::new()),
        }
    } else {
        (None, None, None, Vec::new())
    };

    Ok(Json(DecisionWorkspace {
        project,
        version,
        dsl_yaml,
        dmn_xml,
        knowledge_citations,
    }))
}

async fn list_versions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DecisionVersion>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;

    let versions = sqlx::query_as::<_, DecisionVersion>(
        r#"
        SELECT id, project_id, version_number, status, base_version_id,
               rollback_source_version_id, rollback_reason, change_summary,
               created_by, created_at, reviewed_by, reviewed_at
        FROM decision_versions
        WHERE project_id = $1
        ORDER BY version_number DESC
        "#,
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取决策版本", error))?;
    Ok(Json(versions))
}

async fn record_static_tester_stage(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: i64,
    version_id: i64,
    username: &str,
    parent_orchestration_id: Option<i64>,
    finding_count: usize,
    proposal_count: usize,
    report: &TesterReport,
) -> Result<i64, ProjectApiError> {
    const PROMPT_VERSION: &str = "tester-static-analysis-v1";
    let prompt_digest = format!("{:x}", Sha256::digest(PROMPT_VERSION.as_bytes()));
    let report_value = serde_json::to_value(report)
        .map_err(|error| ProjectApiError::bad_request(format!("编码 Tester 报告失败：{error}")))?;
    let report_bytes = serde_json::to_vec(&report_value)
        .map_err(|error| ProjectApiError::bad_request(format!("编码 Tester 报告失败：{error}")))?;
    let artifact_digest = format!("{:x}", Sha256::digest(report_bytes));
    let usage = json!({ "total_tokens": 0, "source": "deterministic" });
    let input = json!({
        "artifactType": "final_dmn_static_findings",
        "versionId": version_id,
        "findingCount": finding_count,
        "proposalCount": proposal_count,
    });
    let output = json!({
        "artifactType": "tester_report",
        "versionId": version_id,
        "findingCount": finding_count,
        "proposalCount": proposal_count,
        "artifactDigest": artifact_digest,
    });
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO agent_orchestrations (
            project_id, username, trigger_kind, status, current_stage,
            parent_orchestration_id, finished_at
        ) VALUES ($1, $2, 'tester_static_analysis', 'succeeded', 'tester', $3, NOW())
        RETURNING id
        "#,
    )
    .bind(project_id)
    .bind(username)
    .bind(parent_orchestration_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录最终模型 Tester 编排", error))?;
    let run_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO agent_runs (
            project_id, version_id, stage, status, prompt_version, prompt_digest,
            input_json, output_json, usage_json, finished_at, orchestration_id
        ) VALUES ($1, $2, 'tester_static_analysis', 'succeeded', $3, $4, $5, $6, $7, NOW(), $8)
        RETURNING id
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .bind(PROMPT_VERSION)
    .bind(&prompt_digest)
    .bind(input)
    .bind(output)
    .bind(&usage)
    .bind(orchestration_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录最终模型 Tester 运行", error))?;
    sqlx::query(
        r#"
        INSERT INTO agent_steps (
            orchestration_id, agent_run_id, step_no, agent_role, task_kind,
            status, prompt_version, prompt_digest, artifact_kind,
            artifact_digest, usage_json, finished_at
        ) VALUES ($1, $2, 1, 'tester', 'analyze_final_dmn_static_findings',
                  'succeeded', $3, $4, 'tester_report', $5, $6, NOW())
        "#,
    )
    .bind(orchestration_id)
    .bind(run_id)
    .bind(PROMPT_VERSION)
    .bind(prompt_digest)
    .bind(artifact_digest)
    .bind(usage)
    .execute(&mut **transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录最终模型 Tester 步骤", error))?;
    Ok(orchestration_id)
}

async fn create_version(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<CreateDecisionVersion>,
) -> Result<(StatusCode, Json<DecisionVersion>), ProjectApiError> {
    validate_version(&payload)?;
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let effective_knowledge = effective_project_sources(&state.db, project_id, &claims.sub)
        .await
        .map_err(|error| ProjectApiError::database("读取版本有效知识", error))?;
    let effective_citations = citations(&effective_knowledge);
    if payload.knowledge_citations != effective_citations {
        return Err(ProjectApiError::bad_request(
            "项目知识内容、绑定或有效期已变化，请重新执行智能生成后再保存版本",
        ));
    }
    let knowledge_citations_json = serde_json::to_value(effective_citations)
        .map_err(|error| ProjectApiError::bad_request(format!("知识引用序列化失败：{error}")))?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("创建决策版本", error))?;

    let (base_version_id, current_revision_id) =
        sqlx::query_as::<_, (Option<i64>, Option<String>)>(
            r#"
        SELECT p.current_draft_version_id,
               v.revision_id
        FROM decision_projects p
        LEFT JOIN decision_versions v ON v.id = p.current_draft_version_id
        WHERE p.id = $1
        FOR UPDATE OF p
        "#,
        )
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("读取决策项目", error))?
        .ok_or_else(ProjectApiError::not_found)?;

    if let Some(candidate_id) = payload.candidate_id {
        let (candidate_base, candidate_revision, candidate_dsl) = sqlx::query_as::<_, (i64, Option<String>, String)>(
            "SELECT base_version_id, base_revision_id, dsl_yaml FROM model_change_candidates WHERE id = $1 AND project_id = $2 AND status = 'committing'",
        )
        .bind(candidate_id)
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("校验模型变更候选", error))?
        .ok_or_else(|| ProjectApiError::conflict("模型变更候选不存在、已过期或未锁定"))?;
        if Some(candidate_base) != base_version_id {
            return Err(ProjectApiError::conflict(
                "模型变更候选基线已变化，请刷新后重新生成",
            ));
        }
        if candidate_revision.is_some() && candidate_revision != current_revision_id {
            return Err(ProjectApiError::conflict(
                "模型变更候选 revision 已变化，请刷新后重新生成",
            ));
        }
        if payload.dsl_yaml != candidate_dsl {
            return Err(ProjectApiError::conflict(
                "模型变更候选内容已变化，请重新预览后提交",
            ));
        }
    }

    let version_number = sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM decision_versions WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("计算决策版本号", error))?;

    validate_change_submission(base_version_id, &payload).await?;
    let updated_definitions = nl_dsl::parse_definitions(&payload.dsl_yaml)
        .map_err(|error| ProjectApiError::bad_request(format!("提交的 DSL 无效：{error}")))?;
    let content_hash = definitions_hash(&updated_definitions)
        .map_err(|error| ProjectApiError::bad_request(format!("计算版本内容哈希失败：{error}")))?;
    // The DSL is the authoritative model representation. Do not persist the
    // browser-supplied XML directly: a stale UI snapshot can otherwise leave
    // `dsl_yaml` and `dmn_xml` describing different models. Candidate cases
    // execute the stored XML, so always rebuild and compile the artifact from
    // the exact DSL that is about to be saved.
    let canonical_dmn_xml = canonical_dmn_xml(&updated_definitions)?;
    let base_review_model = if let Some(base_id) = base_version_id {
        sqlx::query_as::<_, (i32, String)>(
            r#"
            SELECT version.version_number, artifact.dsl_yaml
            FROM decision_versions version
            JOIN decision_artifacts artifact ON artifact.version_id = version.id
            WHERE version.id = $1 AND version.project_id = $2
            "#,
        )
        .bind(base_id)
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("读取版本审查基线", error))?
        .map(|(number, dsl)| {
            nl_dsl::parse_definitions(&dsl)
                .map(|definitions| (number, definitions))
                .map_err(|error| ProjectApiError::bad_request(format!("基线 DSL 无效：{error}")))
        })
        .transpose()?
    } else {
        None
    };
    let change_review = build_change_review(
        base_review_model.as_ref().map(|(_, model)| model),
        &updated_definitions,
        None,
        base_review_model.as_ref().map(|(number, _)| *number),
        Some(version_number),
    );
    let change_review_json = serde_json::to_value(&change_review)
        .map_err(|error| ProjectApiError::bad_request(format!("版本审查序列化失败：{error}")))?;
    let change_summary = json!({
        "added": change_review.changes.iter().filter(|change| change.change_type == "add").map(|change| change.element.clone()).collect::<Vec<_>>(),
        "updated": change_review.changes.iter().filter(|change| change.change_type == "update").map(|change| change.element.clone()).collect::<Vec<_>>(),
        "removed": change_review.changes.iter().filter(|change| change.change_type == "remove").map(|change| change.element.clone()).collect::<Vec<_>>(),
    });

    let version = sqlx::query_as::<_, DecisionVersion>(
        r#"
        INSERT INTO decision_versions (
            project_id, version_number, status, base_version_id, parent_revision_id,
            change_summary, created_by
        )
        VALUES (
            $1,
            $2,
            'draft',
            $3,
            (SELECT revision_id FROM decision_versions WHERE id = $3),
            $4,
            $5
        )
        RETURNING id, project_id, version_number, status, base_version_id,
                  rollback_source_version_id, rollback_reason, change_summary,
                  created_by, created_at, reviewed_by, reviewed_at
        "#,
    )
    .bind(project_id)
    .bind(version_number)
    .bind(base_version_id)
    .bind(change_summary)
    .bind(&claims.sub)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("创建决策版本", error))?;

    let confirmed_workflow = sqlx::query_as::<
        _,
        (
            serde_json::Value,
            serde_json::Value,
            Option<serde_json::Value>,
            Option<i64>,
        ),
    >(
        r#"
        SELECT decision_brief_json, decision_plan_json, tester_report_json,
               plan_orchestration_id
        FROM decision_workflows
        WHERE project_id = $1 AND status = 'confirmed' AND decision_plan_json IS NOT NULL
          AND base_revision IS NOT DISTINCT FROM $2
        "#,
    )
    .bind(project_id)
    .bind(base_version_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取已确认决策方案", error))?;
    let (decision_brief, decision_plan, tester_report_json, plan_orchestration_id) =
        confirmed_workflow
            .map(|(brief, plan, tester_report, orchestration_id)| {
                (Some(brief), Some(plan), tester_report, orchestration_id)
            })
            .unwrap_or((
                payload.decision_brief.clone(),
                payload.decision_plan.clone(),
                None,
                None,
            ));

    if let (Some(brief_json), Some(report_json)) = (&decision_brief, tester_report_json) {
        let report = serde_json::from_value::<TesterReport>(report_json);
        let brief = serde_json::from_value::<DecisionBrief>(brief_json.clone());
        match (report, brief) {
            (Ok(mut report), Ok(brief)) if report.schema_version == 1 => {
                match crate::version_verification::analyze_static_rules(&canonical_dmn_xml) {
                    Ok(findings) => {
                        let suggested_inputs = crate::dmn::parse_dmn(&canonical_dmn_xml)
                            .ok()
                            .map(|model| {
                                suggested_inputs_for_static_findings(&model, version.id, &findings)
                            })
                            .unwrap_or_default();
                        let added = crate::workflow_api::append_static_analysis_proposals(
                            &mut report,
                            &brief,
                            version.id,
                            &findings,
                            &suggested_inputs,
                        );
                        record_static_tester_stage(
                            &mut transaction,
                            project_id,
                            version.id,
                            &claims.sub,
                            plan_orchestration_id,
                            findings.len(),
                            added,
                            &report,
                        )
                        .await?;
                        if added > 0 {
                            sqlx::query(
                                "UPDATE decision_workflows SET tester_report_json = $1 WHERE project_id = $2 AND status = 'confirmed' AND base_revision IS NOT DISTINCT FROM $3",
                            )
                            .bind(serde_json::to_value(&report).unwrap_or(Value::Null))
                            .bind(project_id)
                            .bind(base_version_id)
                            .execute(&mut *transaction)
                            .await
                            .map_err(|error| {
                                ProjectApiError::database("保存静态分析测试提案", error)
                            })?;
                        }
                    }
                    Err(error) => eprintln!("跳过静态分析 Tester 提案：{error}"),
                }
            }
            (Err(error), _) => eprintln!("跳过无效 Tester 报告的静态分析增强：{error}"),
            (_, Err(error)) => eprintln!("跳过无效需求卡的静态分析增强：{error}"),
            (Ok(_), Ok(_)) => {}
        }
    }

    sqlx::query(
        r#"
        INSERT INTO decision_artifacts (
            version_id, dsl_yaml, dmn_xml, decision_brief_json, decision_plan_json,
            model_patch_json, change_review_json, knowledge_citations_json, content_hash
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(version.id)
    .bind(&payload.dsl_yaml)
    .bind(&canonical_dmn_xml)
    .bind(decision_brief)
    .bind(decision_plan)
    .bind(Option::<Value>::None)
    .bind(change_review_json)
    .bind(knowledge_citations_json)
    .bind(&content_hash)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存决策制品", error))?;

    sqlx::query(
        "UPDATE decision_projects SET current_draft_version_id = $1, updated_at = NOW() WHERE id = $2",
    )
    .bind(version.id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新项目草稿版本", error))?;

    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version.id),
        "version.created",
        &claims.sub,
        &format!("创建草稿版本 v{}", version.version_number),
        json!({
            "versionNumber": version.version_number,
            "baseVersionId": version.base_version_id,
            "changeSummary": version.change_summary,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录版本审计事件", error))?;

    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交决策版本", error))?;
    Ok((StatusCode::CREATED, Json(version)))
}

fn normalize_intent_field(path: &str, element_type: &str, element: &str) -> String {
    let mut value = path.trim().trim_start_matches('/').to_string();
    let collection = match element_type {
        "decision" => "decisions",
        "input" | "inputData" => "inputs",
        "bkm" => "bkms",
        "itemDefinition" => "itemDefinitions",
        "decisionService" => "decisionServices",
        "knowledgeSource" => "knowledgeSources",
        _ => "",
    };
    if !collection.is_empty() {
        let prefix = format!("{collection}[");
        if let Some(end) = value.find(']') {
            if value.starts_with(&prefix) && &value[prefix.len()..end] == element {
                value = value[end + 1..].trim_start_matches('.').to_string();
            }
        }
    }
    let element_prefix = format!("{element}.");
    if let Some(rest) = value.strip_prefix(&element_prefix) {
        value = rest.to_string();
    }
    let mut segments = value
        .split('.')
        .map(|segment| match segment {
            "type_ref" => "typeRef",
            "required_inputs" => "requiredInputs",
            "required_decisions" => "requiredDecisions",
            "required_knowledge" | "knowledgeRequirements" => "requiredKnowledge",
            "is_collection" => "isCollection",
            "output_decisions" => "outputDecisions",
            "encapsulated_decisions" => "encapsulatedDecisions",
            "input_data" => "inputData",
            "authority_targets" => "authorityTargets",
            "input_entries" => "inputEntries",
            "output_entries" => "outputEntries",
            "hit_policy" => "hitPolicy",
            other => other,
        })
        .collect::<Vec<_>>();
    // LLMs commonly name the tagged expression variant directly. Resolve it
    // to the actual DSL field before whitelist and object-path validation.
    if matches!(
        segments.first().copied(),
        Some("decisionTable" | "literalExpression")
    ) {
        segments[0] = "logic";
    }
    segments.join(".")
}

fn parse_intent_path(path: &str) -> Result<Vec<&str>, ProjectApiError> {
    let path = path.trim();
    if path.is_empty() || path.contains('[') || path.contains(']') {
        return Err(ProjectApiError::patch_target(
            "ChangeIntent 字段路径必须使用对象字段，不能使用数组下标",
        ));
    }
    let segments = path.split('.').map(str::trim).collect::<Vec<_>>();
    if segments.iter().any(|segment| {
        segment.is_empty() || segment.chars().all(|character| character.is_ascii_digit())
    }) {
        return Err(ProjectApiError::patch_target(format!(
            "ChangeIntent 字段路径无效：{path}；只允许对象字段，不允许数字路径段或数组下标"
        )));
    }
    Ok(segments)
}

fn is_allowed_intent_field(element_type: &str, root_field: &str) -> bool {
    match element_type {
        "model" | "definitions" => {
            matches!(root_field, "name" | "namespace" | "description" | "imports")
        }
        "decision" => matches!(
            root_field,
            "name"
                | "description"
                | "typeRef"
                | "requiredInputs"
                | "requiredDecisions"
                | "requiredKnowledge"
                | "logic"
        ),
        "input" | "inputData" => matches!(root_field, "name" | "description" | "typeRef"),
        "bkm" => matches!(
            root_field,
            "name" | "description" | "typeRef" | "parameters" | "logic"
        ),
        "itemDefinition" => matches!(root_field, "name" | "isCollection" | "type"),
        "decisionService" => matches!(
            root_field,
            "name" | "description" | "outputDecisions" | "encapsulatedDecisions" | "inputData"
        ),
        "knowledgeSource" => matches!(
            root_field,
            "name" | "description" | "url" | "authorityTargets"
        ),
        "textAnnotation" => matches!(root_field, "text" | "target"),
        _ => false,
    }
}

fn set_intent_path(root: &mut Value, path: &str, value: Value) -> Result<(), ProjectApiError> {
    let segments = parse_intent_path(path)?;
    let mut current = root;
    for segment in &segments[..segments.len() - 1] {
        current = current
            .get_mut(*segment)
            .ok_or_else(|| ProjectApiError::patch_target("ChangeIntent 字段路径不存在"))?;
    }
    let key = *segments.last().expect("non-empty path");
    let object = current
        .as_object_mut()
        .ok_or_else(|| ProjectApiError::patch_target("ChangeIntent 目标不是对象"))?;
    if !object.contains_key(key) {
        return Err(ProjectApiError::patch_target("ChangeIntent 目标字段不存在"));
    }
    object.insert(key.to_string(), value);
    Ok(())
}

/// Correct only unambiguous casing mistakes in generated BKM invocation
/// bindings. The surrounding model supplies the canonical formal-parameter
/// spelling, so this never guesses a renamed or semantically different field.
fn normalize_case_insensitive_invocation_bindings(definitions: &mut nl_dsl::Definitions) -> usize {
    let bkm_parameters = definitions
        .bkms
        .iter()
        .map(|bkm| {
            (
                bkm.name.clone(),
                bkm.parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut corrected = 0;
    for decision in &mut definitions.decisions {
        corrected += normalize_expression_invocation_bindings(&mut decision.logic, &bkm_parameters);
    }
    for bkm in &mut definitions.bkms {
        corrected += normalize_expression_invocation_bindings(&mut bkm.logic, &bkm_parameters);
    }
    corrected
}

fn normalize_expression_invocation_bindings(
    expression: &mut nl_dsl::BoxedExpression,
    bkm_parameters: &HashMap<String, Vec<String>>,
) -> usize {
    match expression {
        nl_dsl::BoxedExpression::Invocation { bkm, bindings } => {
            let Some(parameters) = bkm_parameters.get(bkm) else {
                return 0;
            };
            let mut corrected = 0;
            for binding in bindings {
                if parameters
                    .iter()
                    .any(|parameter| parameter == &binding.parameter)
                {
                    continue;
                }
                let mut candidates = parameters
                    .iter()
                    .filter(|parameter| parameter.eq_ignore_ascii_case(&binding.parameter));
                let Some(candidate) = candidates.next() else {
                    continue;
                };
                if candidates.next().is_none() {
                    binding.parameter = candidate.clone();
                    corrected += 1;
                }
            }
            corrected
        }
        nl_dsl::BoxedExpression::Context { entries, .. } => entries
            .iter_mut()
            .map(|entry| normalize_expression_invocation_bindings(&mut entry.value, bkm_parameters))
            .sum(),
        nl_dsl::BoxedExpression::FunctionDefinition { body, .. } => {
            normalize_expression_invocation_bindings(body, bkm_parameters)
        }
        nl_dsl::BoxedExpression::Conditional { then, else_, .. } => {
            normalize_expression_invocation_bindings(then, bkm_parameters)
                + normalize_expression_invocation_bindings(else_, bkm_parameters)
        }
        _ => 0,
    }
}

fn apply_change_intent(
    base: &nl_dsl::Definitions,
    intent: &ModelChangeIntent,
) -> Result<nl_dsl::Definitions, ProjectApiError> {
    if intent.changes.is_empty() || intent.changes.len() > 64 {
        return Err(ProjectApiError::bad_request(
            "changes 数量必须在 1 到 64 之间",
        ));
    }
    let mut document = serde_json::to_value(base)
        .map_err(|error| ProjectApiError::bad_request(format!("DSL 序列化失败：{error}")))?;
    for change in &intent.changes {
        let (element_type, element) = change.target.split_once(':').ok_or_else(|| {
            ProjectApiError::patch_target("target 必须使用 elementType:name 格式")
        })?;
        let element_type = element_type.trim();
        let element = element.trim();
        if element_type.is_empty() || element.is_empty() {
            return Err(ProjectApiError::patch_target("target 不能为空"));
        }
        let root_model = matches!(element_type, "model" | "definitions");
        let collection_name = match element_type {
            "decision" => Some("decisions"),
            "input" | "inputData" => Some("inputs"),
            "bkm" => Some("bkms"),
            "itemDefinition" => Some("itemDefinitions"),
            "decisionService" => Some("decisionServices"),
            "knowledgeSource" => Some("knowledgeSources"),
            "textAnnotation" => Some("textAnnotations"),
            _ if root_model => None,
            _ => return Err(ProjectApiError::patch_target("不支持的 target 类型")),
        };
        let action = change.action.trim().to_ascii_lowercase();
        if root_model && element != base.name {
            return Err(ProjectApiError::patch_target(
                "model target 必须匹配模型名称",
            ));
        }
        match action.as_str() {
            "update" => {
                let field = change
                    .field
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| ProjectApiError::patch_target("update 必须指定 field"))?;
                let field = normalize_intent_field(field, element_type, element);
                if field.contains(['[', ']']) {
                    return Err(ProjectApiError::patch_target(format!(
                        "不允许使用数组下标定位字段：target={}，field={field}。请按元素名称定位 target，使用对象字段更新；例如 field=logic.rules 或 logic.bindings，value 必须为调整后的完整数组，保留未修改项及顺序。请重新生成意图，不要仅删除下标后复用单个成员值",
                        change.target,
                    )));
                }
                let segments = parse_intent_path(&field)?;
                let root_field = segments[0];
                if !is_allowed_intent_field(element_type, root_field) {
                    return Err(ProjectApiError::patch_target("不允许修改该字段"));
                }
                let target = if root_model {
                    &mut document
                } else {
                    let collection = document
                        .get_mut(collection_name.expect("non-root collection"))
                        .and_then(Value::as_array_mut)
                        .ok_or_else(|| ProjectApiError::patch_target("DSL 元素集合不存在"))?;
                    let index = collection
                        .iter()
                        .position(|value| {
                            value.get("name").and_then(Value::as_str) == Some(element)
                        })
                        .ok_or_else(|| ProjectApiError::patch_target("目标元素不存在"))?;
                    collection
                        .get_mut(index)
                        .ok_or_else(|| ProjectApiError::patch_target("目标元素不存在"))?
                };
                let value = change
                    .value
                    .clone()
                    .ok_or_else(|| ProjectApiError::patch_target("update 必须提供 value"))?;
                set_intent_path(target, &field, value)?;
            }
            "remove" => {
                if root_model {
                    return Err(ProjectApiError::patch_target("model 只支持 update"));
                }
                let collection = document
                    .get_mut(collection_name.expect("non-root collection"))
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| ProjectApiError::patch_target("DSL 元素集合不存在"))?;
                let index = collection
                    .iter()
                    .position(|value| value.get("name").and_then(Value::as_str) == Some(element))
                    .ok_or_else(|| ProjectApiError::patch_target("目标元素不存在"))?;
                collection.remove(index);
            }
            "add" => {
                if root_model {
                    return Err(ProjectApiError::patch_target("model 只支持 update"));
                }
                let value = change
                    .value
                    .clone()
                    .ok_or_else(|| ProjectApiError::patch_target("add 必须提供 value"))?;
                if value.get("name").and_then(Value::as_str) != Some(element) {
                    return Err(ProjectApiError::patch_target(
                        "add.value.name 必须与 target 名称一致",
                    ));
                }
                let collection = document
                    .get_mut(collection_name.expect("non-root collection"))
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| ProjectApiError::patch_target("DSL 元素集合不存在"))?;
                if collection
                    .iter()
                    .any(|item| item.get("name").and_then(Value::as_str) == Some(element))
                {
                    return Err(ProjectApiError::patch_target("目标元素已存在"));
                }
                collection.push(value);
            }
            _ => {
                return Err(ProjectApiError::patch_target(
                    "action 必须是 update、add 或 remove",
                ));
            }
        }
    }
    let mut definitions = serde_json::from_value(document).map_err(|error| {
        ProjectApiError::patch_target(format!("变更结果无法解析为 DSL：{error}"))
    })?;
    nl_dsl::normalize_type_refs(&mut definitions);
    nl_dsl::normalize_dependency_references(&mut definitions);
    synchronize_change_intent_diagram(&mut definitions);
    Ok(definitions)
}

fn synchronize_change_intent_diagram(definitions: &mut nl_dsl::Definitions) {
    let Some(diagram) = definitions.diagram.as_mut() else {
        return;
    };

    let element_names = definitions
        .item_definitions
        .iter()
        .map(|element| element.name.as_str())
        .chain(
            definitions
                .inputs
                .iter()
                .map(|element| element.name.as_str()),
        )
        .chain(
            definitions
                .decisions
                .iter()
                .map(|element| element.name.as_str()),
        )
        .chain(definitions.bkms.iter().map(|element| element.name.as_str()))
        .chain(
            definitions
                .decision_services
                .iter()
                .map(|element| element.name.as_str()),
        )
        .chain(
            definitions
                .knowledge_sources
                .iter()
                .map(|element| element.name.as_str()),
        )
        .collect::<std::collections::HashSet<_>>();
    let relationships = definitions
        .decisions
        .iter()
        .flat_map(|decision| {
            decision
                .required_inputs
                .iter()
                .chain(decision.required_decisions.iter())
                .chain(decision.required_knowledge.iter())
                .map(move |source| (source.as_str(), decision.name.as_str()))
        })
        .collect::<std::collections::HashSet<_>>();

    diagram
        .nodes
        .retain(|node| element_names.contains(node.element.as_str()));
    diagram.edges.retain(|edge| {
        element_names.contains(edge.source.as_str())
            && element_names.contains(edge.target.as_str())
            && relationships.contains(&(edge.source.as_str(), edge.target.as_str()))
    });
}

fn validate_intent_base(
    intent: &ModelChangeIntent,
    current_revision: i64,
    current_hash: &str,
) -> Result<(), ProjectApiError> {
    if intent.base_revision != current_revision || intent.base_hash != current_hash {
        return Err(ProjectApiError::conflict(
            "模型基线已变化，请刷新后重新生成",
        ));
    }
    Ok(())
}

async fn preview_model_change(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(intent): Json<ModelChangeIntent>,
) -> Result<Json<ModelChangePreview>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let row = sqlx::query_as::<_, (i64, i32, String, String)>("SELECT v.id, v.version_number, v.revision_id, a.dsl_yaml FROM decision_projects p JOIN decision_versions v ON v.id=p.current_draft_version_id JOIN decision_artifacts a ON a.version_id=v.id WHERE p.id=$1")
        .bind(project_id).fetch_optional(&state.db).await.map_err(|e| ProjectApiError::database("读取模型基线", e))?.ok_or_else(|| ProjectApiError::bad_request("项目没有可修改的草稿版本"))?;
    if intent.base_revision != row.0 {
        return Err(ProjectApiError::conflict(
            "模型基线版本已变化，请刷新后重试",
        ));
    }
    let base = nl_dsl::parse_definitions(&row.3)
        .map_err(|e| ProjectApiError::dsl(format!("基线 DSL 无效：{e}")))?;
    let base_hash = definitions_hash(&base)
        .map_err(|e| ProjectApiError::bad_request(format!("计算基线哈希失败：{e}")))?;
    if intent.base_hash.trim().is_empty() {
        return Err(ProjectApiError::patch_target(
            "变更意图缺少 baseHash，请重新生成变更意图",
        ));
    }
    validate_intent_base(&intent, row.0, &base_hash)?;
    let mut updated = apply_change_intent(&base, &intent)?;
    normalize_case_insensitive_invocation_bindings(&mut updated);
    nl_dsl::validate(&updated).map_err(|e| ProjectApiError::dsl(format!("变更结果无效：{e}")))?;
    let dsl_yaml = serde_yaml::to_string(&updated)
        .map_err(|e| ProjectApiError::dsl(format!("DSL 序列化失败：{e}")))?;
    let dmn_xml = canonical_dmn_xml(&updated)?;
    let change_review =
        build_change_review(Some(&base), &updated, None, Some(row.1), Some(row.1 + 1));
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(1);
    let candidate_id = sqlx::query_scalar::<_, i64>("INSERT INTO model_change_candidates (project_id, base_version_id, base_revision_id, base_hash, dsl_yaml, dmn_xml, change_json, created_by, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id")
        .bind(project_id).bind(row.0).bind(&row.2).bind(&base_hash).bind(&dsl_yaml).bind(&dmn_xml).bind(serde_json::to_value(&intent).unwrap_or_else(|_| json!({}))).bind(&claims.sub).bind(expires_at).fetch_one(&state.db).await.map_err(|e| ProjectApiError::database("保存模型变更候选", e))?;
    Ok(Json(ModelChangePreview {
        candidate_id,
        project_id,
        base_revision: row.0,
        base_revision_id: Some(row.2),
        base_hash,
        dsl_yaml,
        dmn_xml,
        status: "pending".into(),
        expires_at,
        change_review: Some(change_review),
    }))
}

async fn commit_model_change(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, candidate_id)): Path<(i64, i64)>,
) -> Result<(StatusCode, Json<DecisionVersion>), ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let candidate = sqlx::query_as::<_, (i64, Option<String>, String, String)>("UPDATE model_change_candidates SET status='committing' WHERE id=$1 AND project_id=$2 AND status='pending' AND expires_at>NOW() RETURNING base_version_id, base_revision_id, dsl_yaml, dmn_xml")
        .bind(candidate_id).bind(project_id).fetch_optional(&state.db).await.map_err(|e| ProjectApiError::database("锁定模型变更候选", e))?;
    let candidate = match candidate {
        Some(value) => value,
        None => {
            let existing = sqlx::query_as::<_, (String, Option<i64>)>("SELECT status, resulting_version_id FROM model_change_candidates WHERE id=$1 AND project_id=$2")
                .bind(candidate_id).bind(project_id).fetch_optional(&state.db).await.map_err(|e| ProjectApiError::database("读取模型候选状态", e))?;
            if let Some((_status, Some(version_id))) =
                existing.filter(|(status, _)| status == "committed")
            {
                let version = sqlx::query_as::<_, DecisionVersion>("SELECT id, project_id, version_number, status, base_version_id, rollback_source_version_id, rollback_reason, change_summary, created_by, created_at, reviewed_by, reviewed_at FROM decision_versions WHERE id=$1 AND project_id=$2")
                    .bind(version_id).bind(project_id).fetch_optional(&state.db).await.map_err(|e| ProjectApiError::database("读取已提交版本", e))?.ok_or_else(ProjectApiError::not_found)?;
                return Ok((StatusCode::OK, Json(version)));
            }
            return Err(ProjectApiError::conflict("候选不存在、已过期或正在提交"));
        }
    };
    let effective_knowledge = effective_project_sources(&state.db, project_id, &claims.sub)
        .await
        .map_err(|e| ProjectApiError::database("读取候选提交知识引用", e))?;
    let current_revision_id = sqlx::query_scalar::<_, Option<String>>(
        "SELECT v.revision_id FROM decision_projects p JOIN decision_versions v ON v.id = p.current_draft_version_id WHERE p.id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ProjectApiError::database("读取当前模型 revision", e))?
    .flatten();
    if candidate.1.is_some() && candidate.1 != current_revision_id {
        let _ = sqlx::query("UPDATE model_change_candidates SET status='pending' WHERE id=$1 AND status='committing'")
            .bind(candidate_id)
            .execute(&state.db)
            .await;
        return Err(ProjectApiError::conflict(
            "模型变更候选 revision 已变化，请刷新后重新生成",
        ));
    }
    let payload = CreateDecisionVersion {
        dsl_yaml: candidate.2,
        dmn_xml: candidate.3,
        change_summary: None,
        decision_brief: None,
        decision_plan: None,
        candidate_id: Some(candidate_id),
        knowledge_citations: citations(&effective_knowledge),
        change_confirmed: true,
    };
    match create_version(
        State(state.clone()),
        Extension(claims),
        Path(project_id),
        Json(payload),
    )
    .await
    {
        Ok((status_code, Json(version))) => {
            sqlx::query("UPDATE model_change_candidates SET status='committed', resulting_version_id=$1 WHERE id=$2")
                .bind(version.id).bind(candidate_id).execute(&state.db).await
                .map_err(|e| ProjectApiError::database("更新候选状态", e))?;
            Ok((status_code, Json(version)))
        }
        Err(error) => {
            let _ = sqlx::query("UPDATE model_change_candidates SET status='pending' WHERE id=$1 AND status='committing'").bind(candidate_id).execute(&state.db).await;
            Err(error)
        }
    }
}

async fn rollback_project_version(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, source_version_id)): Path<(i64, i64)>,
    Json(payload): Json<RollbackDecisionVersion>,
) -> Result<(StatusCode, Json<DecisionWorkspace>), ProjectApiError> {
    validate_rollback(&payload)?;
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("创建回滚草稿", error))?;

    let mut project = sqlx::query_as::<_, DecisionProject>(
        r#"
        SELECT id, owner_username, name, description, risk_level,
               current_draft_version_id, published_version_id, created_at, updated_at
        FROM decision_projects
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取回滚项目", error))?
    .ok_or_else(ProjectApiError::not_found)?;

    let current_version_id = project
        .current_draft_version_id
        .ok_or_else(|| ProjectApiError::bad_request("项目尚无可作为回滚基线的当前版本"))?;
    if current_version_id == source_version_id {
        return Err(ProjectApiError::bad_request("所选版本已经是当前草稿版本"));
    }

    let source = sqlx::query_as::<
        _,
        (
            i32,
            String,
            Option<serde_json::Value>,
            Option<serde_json::Value>,
            String,
            String,
            Value,
        ),
    >(
        r#"
        SELECT version.version_number, version.status,
               artifact.decision_brief_json, artifact.decision_plan_json,
               artifact.dsl_yaml, artifact.dmn_xml, artifact.knowledge_citations_json
        FROM decision_versions version
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE version.id = $1 AND version.project_id = $2
        "#,
    )
    .bind(source_version_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取回滚来源版本", error))?
    .ok_or_else(|| ProjectApiError::bad_request("回滚来源版本不存在"))?;
    let (
        source_number,
        source_status,
        decision_brief,
        decision_plan,
        dsl_yaml,
        dmn_xml,
        knowledge_citations_json,
    ) = source;
    let knowledge_citations =
        serde_json::from_value::<Vec<KnowledgeCitation>>(knowledge_citations_json.clone())
            .map_err(|error| ProjectApiError::bad_request(format!("回滚知识引用无效：{error}")))?;
    if !matches!(source_status.as_str(), "published" | "retired") {
        return Err(ProjectApiError::bad_request(
            "只能从已发布或已退役的历史版本创建回滚草稿",
        ));
    }

    let (base_number, base_dsl) = sqlx::query_as::<_, (i32, String)>(
        r#"
        SELECT version.version_number, artifact.dsl_yaml
        FROM decision_versions version
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE version.id = $1 AND version.project_id = $2
        "#,
    )
    .bind(current_version_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取当前回滚基线", error))?
    .ok_or_else(|| ProjectApiError::bad_request("当前回滚基线版本不存在"))?;

    let base_definitions = nl_dsl::parse_definitions(&base_dsl)
        .map_err(|error| ProjectApiError::bad_request(format!("当前版本 DSL 无效：{error}")))?;
    let source_definitions = nl_dsl::parse_definitions(&dsl_yaml)
        .map_err(|error| ProjectApiError::bad_request(format!("回滚版本 DSL 无效：{error}")))?;
    nl_dsl::validate(&source_definitions)
        .map_err(|error| ProjectApiError::bad_request(format!("回滚版本校验失败：{error}")))?;

    let version_number = sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM decision_versions WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("计算回滚版本号", error))?;
    let change_review = build_change_review(
        Some(&base_definitions),
        &source_definitions,
        None,
        Some(base_number),
        Some(version_number),
    );
    let change_review_json = serde_json::to_value(&change_review)
        .map_err(|error| ProjectApiError::bad_request(format!("回滚审查序列化失败：{error}")))?;
    let reason = payload.reason.trim().to_string();
    let change_summary = json!({
        "rollback": true,
        "rollbackSourceVersionId": source_version_id,
        "rollbackSourceVersionNumber": source_number,
        "reason": reason,
        "added": change_review.changes.iter().filter(|change| change.change_type == "add").map(|change| change.element.clone()).collect::<Vec<_>>(),
        "updated": change_review.changes.iter().filter(|change| change.change_type == "update").map(|change| change.element.clone()).collect::<Vec<_>>(),
        "removed": change_review.changes.iter().filter(|change| change.change_type == "remove").map(|change| change.element.clone()).collect::<Vec<_>>(),
    });

    let version = sqlx::query_as::<_, DecisionVersion>(
        r#"
        INSERT INTO decision_versions (
            project_id, version_number, status, base_version_id, parent_revision_id,
            rollback_source_version_id, rollback_reason, change_summary, created_by
        )
        VALUES (
            $1,
            $2,
            'draft',
            $3,
            (SELECT revision_id FROM decision_versions WHERE id = $3),
            $4,
            $5,
            $6,
            $7
        )
        RETURNING id, project_id, version_number, status, base_version_id,
                  rollback_source_version_id, rollback_reason, change_summary,
                  created_by, created_at, reviewed_by, reviewed_at
        "#,
    )
    .bind(project_id)
    .bind(version_number)
    .bind(current_version_id)
    .bind(source_version_id)
    .bind(&reason)
    .bind(change_summary)
    .bind(&claims.sub)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("创建回滚版本", error))?;

    sqlx::query(
        r#"
        INSERT INTO decision_artifacts (
            version_id, dsl_yaml, dmn_xml, decision_brief_json, decision_plan_json,
            model_patch_json, change_review_json, knowledge_citations_json
        )
        VALUES ($1, $2, $3, $4, $5, NULL, $6, $7)
        "#,
    )
    .bind(version.id)
    .bind(&dsl_yaml)
    .bind(&dmn_xml)
    .bind(decision_brief)
    .bind(decision_plan)
    .bind(change_review_json)
    .bind(knowledge_citations_json)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("复制回滚版本制品", error))?;

    sqlx::query(
        r#"
        INSERT INTO decision_test_cases (
            version_id, name, category, input_json, expected_output_json, status, source
        )
        SELECT $1, name, category, input_json, expected_output_json, 'pending', source
        FROM decision_test_cases
        WHERE version_id = $2 AND category = 'business'
        "#,
    )
    .bind(version.id)
    .bind(source_version_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("复制回滚业务验收案例", error))?;

    sqlx::query(
        "UPDATE decision_projects SET current_draft_version_id = $1, updated_at = NOW() WHERE id = $2",
    )
    .bind(version.id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新回滚草稿版本", error))?;

    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version.id),
        "version.rollback_created",
        &claims.sub,
        &format!("从 v{source_number} 创建回滚草稿 v{version_number}"),
        json!({
            "sourceVersionId": source_version_id,
            "sourceVersionNumber": source_number,
            "baseVersionId": current_version_id,
            "baseVersionNumber": base_number,
            "reason": reason,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录回滚审计事件", error))?;

    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交回滚草稿", error))?;
    project.current_draft_version_id = Some(version.id);
    Ok((
        StatusCode::CREATED,
        Json(DecisionWorkspace {
            project,
            version: Some(version),
            dsl_yaml: Some(dsl_yaml),
            dmn_xml: Some(dmn_xml),
            knowledge_citations,
        }),
    ))
}

async fn list_audit_events(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<AuditEvent>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let events = sqlx::query_as::<_, AuditEvent>(
        r#"
        SELECT id, project_id, version_id, event_type, actor_username, summary,
               details_json AS details, created_at
        FROM audit_events
        WHERE project_id = $1
        ORDER BY created_at DESC, id DESC
        LIMIT 100
        "#,
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取项目审计记录", error))?;
    Ok(Json(events))
}

async fn get_version_review(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
) -> Result<Json<VersionChangeReview>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let row = sqlx::query_as::<
        _,
        (
            i32,
            Option<i64>,
            String,
            Option<serde_json::Value>,
            Option<serde_json::Value>,
        ),
    >(
        r#"
        SELECT version.version_number, version.base_version_id, artifact.dsl_yaml,
               artifact.model_patch_json, artifact.change_review_json
        FROM decision_versions version
        JOIN decision_projects project ON project.id = version.project_id
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE version.id = $1 AND version.project_id = $2
        "#,
    )
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取版本审查", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (version_number, base_version_id, dsl_yaml, _raw_patch, review_snapshot) = row;
    if let Some(snapshot) = review_snapshot {
        let mut review = serde_json::from_value::<VersionChangeReview>(snapshot)
            .map_err(|error| ProjectApiError::bad_request(format!("版本审查快照无效：{error}")))?;
        review.version_id = Some(version_id);
        return Ok(Json(review));
    }
    let updated = nl_dsl::parse_definitions(&dsl_yaml)
        .map_err(|error| ProjectApiError::bad_request(format!("版本 DSL 无效：{error}")))?;
    let base = if let Some(base_version_id) = base_version_id {
        sqlx::query_as::<_, (i32, String)>(
            r#"
            SELECT version.version_number, artifact.dsl_yaml
            FROM decision_versions version
            JOIN decision_artifacts artifact ON artifact.version_id = version.id
            WHERE version.id = $1 AND version.project_id = $2
            "#,
        )
        .bind(base_version_id)
        .bind(project_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|error| ProjectApiError::database("读取基线版本", error))?
        .map(|(number, dsl)| {
            nl_dsl::parse_definitions(&dsl)
                .map(|definitions| (number, definitions))
                .map_err(|error| ProjectApiError::bad_request(format!("基线 DSL 无效：{error}")))
        })
        .transpose()?
    } else {
        None
    };
    let review = build_change_review(
        base.as_ref().map(|(_, definitions)| definitions),
        &updated,
        Some(version_id),
        base.as_ref().map(|(number, _)| *number),
        Some(version_number),
    );
    Ok(Json(review))
}

async fn get_version_behavior_comparison(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, candidate_version_id)): Path<(i64, i64)>,
) -> Result<Json<BehaviorComparisonReport>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let baseline_version_id = sqlx::query_scalar::<_, Option<i64>>(
        r#"SELECT baseline.id
           FROM decision_versions candidate
           LEFT JOIN decision_versions baseline
             ON baseline.id = candidate.base_version_id
            AND baseline.project_id = candidate.project_id
           WHERE candidate.id = $1 AND candidate.project_id = $2"#,
    )
    .bind(candidate_version_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取行为对比基线", error))?
    .flatten()
    .ok_or_else(ProjectApiError::not_found)?;

    let db = state.db.clone();
    let load_runs = |version_id| {
        let db = db.clone();
        async move {
            sqlx::query_as::<_, (i64, Value, i64)>(
                "SELECT id, execution_trace_json, COUNT(*) OVER() FROM decision_runs WHERE project_id = $1 AND version_id = $2 ORDER BY executed_at DESC, id DESC LIMIT 1000",
            )
            .bind(project_id)
            .bind(version_id)
            .fetch_all(&db)
            .await
        }
    };
    let baseline_rows = load_runs(baseline_version_id)
        .await
        .map_err(|error| ProjectApiError::database("读取基线执行轨迹", error))?;
    let candidate_rows = load_runs(candidate_version_id)
        .await
        .map_err(|error| ProjectApiError::database("读取候选执行轨迹", error))?;
    let baseline_total_runs = baseline_rows
        .first()
        .map(|(_, _, total)| (*total).max(0) as usize)
        .unwrap_or_default();
    let candidate_total_runs = candidate_rows
        .first()
        .map(|(_, _, total)| (*total).max(0) as usize)
        .unwrap_or_default();
    let baseline_sampled_runs = baseline_rows.len();
    let candidate_sampled_runs = candidate_rows.len();
    let baseline_runs = baseline_rows
        .into_iter()
        .filter_map(|(id, value, _)| {
            serde_json::from_value::<ExecutionTrace>(value)
                .ok()
                .map(|trace| (id, trace))
        })
        .collect::<Vec<_>>();
    let candidate_runs = candidate_rows
        .into_iter()
        .filter_map(|(id, value, _)| {
            serde_json::from_value::<ExecutionTrace>(value)
                .ok()
                .map(|trace| (id, trace))
        })
        .collect::<Vec<_>>();
    let matched_pairs = match_trace_inputs(&baseline_runs, &candidate_runs);
    let matched_candidates = matched_pairs
        .iter()
        .map(|(_, candidate_index)| *candidate_index)
        .collect::<HashSet<_>>();
    let mut comparisons = Vec::new();
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存行为差异", error))?;
    for (baseline_index, candidate_index) in matched_pairs {
        let (baseline_run_id, baseline_trace) = &baseline_runs[baseline_index];
        let (candidate_run_id, candidate_trace) = &candidate_runs[candidate_index];
        let diff = ExecutionTraceComparator {
            baseline_trace,
            candidate_trace,
        }
        .compare();
        let diff_json = serde_json::to_value(&diff).unwrap_or(Value::Null);
        sqlx::query(
            r#"INSERT INTO version_behavior_diff (
                project_id, baseline_version_id, candidate_version_id,
                baseline_run_id, candidate_run_id, baseline_output,
                candidate_output, diff_json, risk_level
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (baseline_run_id, candidate_run_id) DO UPDATE SET
                diff_json = EXCLUDED.diff_json, risk_level = EXCLUDED.risk_level,
                baseline_output = EXCLUDED.baseline_output,
                candidate_output = EXCLUDED.candidate_output, created_at = NOW()"#,
        )
        .bind(project_id)
        .bind(baseline_version_id)
        .bind(candidate_version_id)
        .bind(baseline_run_id)
        .bind(candidate_run_id)
        .bind(&baseline_trace.output)
        .bind(&candidate_trace.output)
        .bind(diff_json)
        .bind(&diff.risk_level)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("保存行为差异", error))?;
        comparisons.push(BehaviorComparisonEntry {
            baseline_run_id: *baseline_run_id,
            candidate_run_id: *candidate_run_id,
            diff,
        });
    }
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交行为差异", error))?;

    Ok(Json(BehaviorComparisonReport {
        baseline_version_id,
        candidate_version_id,
        matched_cases: comparisons.len(),
        baseline_sampled_runs,
        baseline_total_runs,
        candidate_sampled_runs,
        candidate_total_runs,
        unmatched_baseline_runs: baseline_sampled_runs.saturating_sub(comparisons.len()),
        unmatched_candidate_runs: candidate_sampled_runs.saturating_sub(matched_candidates.len()),
        comparisons,
    }))
}

async fn get_version_verification(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
) -> Result<Json<Option<VersionVerificationReport>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let report = sqlx::query_scalar::<_, Option<serde_json::Value>>(
        r#"
        SELECT artifact.verification_report_json
        FROM decision_versions version
        JOIN decision_projects project ON project.id = version.project_id
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE version.id = $1 AND version.project_id = $2
        "#,
    )
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取版本验收报告", error))?
    .ok_or_else(ProjectApiError::not_found)?
    .map(serde_json::from_value::<VersionVerificationReport>)
    .transpose()
    .map_err(|error| ProjectApiError::bad_request(format!("版本验收报告无效：{error}")))?;
    Ok(Json(report))
}

const MAX_GENERATED_BUSINESS_CASES: usize = 32;

#[derive(Debug)]
struct GeneratedBusinessCandidate {
    name: String,
    input_json: Value,
    expected_output_json: Value,
    rule_key: String,
    // 仅在测试断言中读取（验证依赖决策兜底候选生成），非 test 构建允许未读
    #[cfg_attr(not(test), allow(dead_code))]
    fallback: bool,
}

#[derive(Debug)]
struct BusinessCandidateGeneration {
    candidates: Vec<GeneratedBusinessCandidate>,
    total_rule_count: usize,
    skipped_rule_count: usize,
    notes: Vec<String>,
}

#[derive(Debug, Default)]
struct CandidateSkipCounts {
    capacity: usize,
    unsupported_expression: usize,
    input_conversion: usize,
    target_missing: usize,
    target_failed: usize,
    target_not_matched: usize,
    trace_output_missing: usize,
    duplicate: usize,
}

impl CandidateSkipCounts {
    fn note(&self, notes: &mut Vec<String>) {
        let entries = [
            ("数量上限", self.capacity),
            ("输入条件无法反解", self.unsupported_expression),
            ("输入类型转换失败", self.input_conversion),
            ("目标决策未产生结果", self.target_missing),
            ("目标决策执行失败或被依赖跳过", self.target_failed),
            ("目标规则未命中", self.target_not_matched),
            ("执行结果缺少目标输出", self.trace_output_missing),
            ("生成内容重复", self.duplicate),
        ];
        let details = entries
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(label, count)| format!("{label} {count} 条"))
            .collect::<Vec<_>>();
        if !details.is_empty() {
            notes.push(format!("跳过明细：{}", details.join("、")));
        }
    }
}

fn numeric_delta(value: f64) -> f64 {
    (value.abs() * 0.000_001).max(0.000_001)
}

fn parse_feel_scalar(value: &str) -> Option<Value> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("true") {
        return Some(json!(true));
    }
    if value.eq_ignore_ascii_case("false") {
        return Some(json!(false));
    }
    if value.eq_ignore_ascii_case("null") {
        return Some(Value::Null);
    }
    if let Ok(number) = value.parse::<f64>() {
        return Some(json!(number));
    }
    // FEEL temporal literals are commonly represented as constructor calls in
    // decision-table entries. The executor validates the actual type later;
    // for case generation the ISO value is the correct JSON representation.
    for function in ["date", "time", "date and time", "duration"] {
        let prefix = format!("{function}(\"");
        if let Some(rest) = value.strip_prefix(&prefix)
            && let Some(inner) = rest.strip_suffix("\")")
            && !inner.is_empty()
        {
            return Some(json!(inner));
        }
    }
    serde_json::from_str::<Value>(value)
        .ok()
        .filter(|parsed| parsed.is_string())
}

fn split_feel_list(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quoted = false;
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match *byte {
            b'"' => quoted = !quoted,
            b'(' | b'[' | b'{' if !quoted => depth += 1,
            b')' | b']' | b'}' if !quoted && depth > 0 => depth -= 1,
            b',' if !quoted && depth == 0 => {
                parts.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

fn balanced_outer_call<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}(");
    let inner = value.strip_prefix(&prefix)?.strip_suffix(')')?;
    let mut depth = 0usize;
    let mut quoted = false;
    for (index, byte) in inner.as_bytes().iter().enumerate() {
        match *byte {
            b'"' => quoted = !quoted,
            b'(' if !quoted => depth += 1,
            b')' if !quoted => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            _ => {}
        }
        if depth == 0 && index + 1 < inner.len() {
            // A closing parenthesis before the end means this was not one
            // outer call, e.g. `not(< 10) and ...`.
            continue;
        }
    }
    (depth == 0 && !quoted).then_some(inner.trim())
}

fn split_top_level_operator<'a>(value: &'a str, operator: &str) -> Option<(&'a str, &'a str)> {
    let bytes = value.as_bytes();
    let operator_bytes = operator.as_bytes();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut index = 0usize;
    while index + operator_bytes.len() <= bytes.len() {
        match bytes[index] {
            b'"' => quoted = !quoted,
            b'(' | b'[' | b'{' if !quoted => depth += 1,
            b')' | b']' | b'}' if !quoted && depth > 0 => depth -= 1,
            _ => {}
        }
        if !quoted
            && depth == 0
            && value
                .get(index..)
                .is_some_and(|remaining| remaining.starts_with(operator))
        {
            return Some((
                value[..index].trim(),
                value[index + operator.len()..].trim(),
            ));
        }
        index += 1;
    }
    None
}

fn representative_unary_value(entry: &str) -> Option<Value> {
    let entry = entry.trim();
    if entry.is_empty() || entry == "-" {
        return None;
    }
    if let Some(inner) = balanced_outer_call(entry, "not") {
        let comparison = Regex::new(r"^(>=|>|<=|<|=|!=|<>)\s*(-?\d+(?:\.\d+)?)$").ok()?;
        if let Some(captures) = comparison.captures(inner) {
            let value = captures[2].parse::<f64>().ok()?;
            let delta = numeric_delta(value);
            let sample = match &captures[1] {
                "<" | "=" => value,
                "<=" | "!=" | "<>" => value + delta,
                ">" => value,
                ">=" => value - delta,
                _ => return None,
            };
            return Some(json!(sample));
        }
        // Negated string equality is common in generated tables. Pick a
        // stable value different from the excluded literal.
        if let Some(value) = inner
            .strip_prefix("=")
            .or_else(|| inner.strip_prefix("=="))
            .map(str::trim)
            .and_then(parse_feel_scalar)
        {
            return Some(match value {
                Value::String(value) if value == "sample" => json!("__other__"),
                Value::String(_) => json!("sample"),
                Value::Bool(value) => json!(!value),
                Value::Number(value) => value
                    .as_f64()
                    .map(|number| json!(number + numeric_delta(number)))?,
                _ => json!("sample"),
            });
        }
        // Keep unsupported negated literals conservative. A value that merely
        // differs in type could still make the generated case invalid.
        if entry.starts_with("not(") {
            return None;
        }
    }

    if let Some((left, right)) = split_top_level_operator(entry, " or ") {
        return representative_unary_value(left).or_else(|| representative_unary_value(right));
    }
    if let Some((left, right)) = split_top_level_operator(entry, " and ") {
        let left = representative_unary_value(left)?;
        let right = representative_unary_value(right)?;
        if let (Some(left_number), Some(right_number)) = (left.as_f64(), right.as_f64()) {
            return Some(json!(left_number.max(right_number)));
        }
        return Some(left);
    }

    static COMPARISON: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let comparison = COMPARISON.get_or_init(|| {
        Regex::new(r"^(>=|>|<=|<|=|!=|<>)\s*(-?\d+(?:\.\d+)?)$").expect("valid comparison regex")
    });
    if let Some(captures) = comparison.captures(entry) {
        let value = captures[2].parse::<f64>().ok()?;
        let sample = match &captures[1] {
            ">" => value + numeric_delta(value),
            "<" => value - numeric_delta(value),
            "!=" | "<>" => value + numeric_delta(value),
            _ => value,
        };
        return Some(json!(sample));
    }

    // Handle exact string and boolean unary tests in addition to numbers.
    if let Some(value) = entry
        .strip_prefix('=')
        .map(str::trim)
        .and_then(parse_feel_scalar)
    {
        return Some(value);
    }

    static RANGE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let range = RANGE.get_or_init(|| {
        Regex::new(r"^([\[(])\s*(-?\d+(?:\.\d+)?)\s*\.\.\s*(-?\d+(?:\.\d+)?)\s*([\])])$")
            .expect("valid range regex")
    });
    let range_entry = entry.strip_prefix("in ").unwrap_or(entry).trim();
    if let Some(captures) = range.captures(range_entry) {
        let lower = captures[2].parse::<f64>().ok()?;
        let upper = captures[3].parse::<f64>().ok()?;
        if lower > upper {
            return None;
        }
        let mut sample = if lower == upper {
            lower
        } else {
            lower + (upper - lower) / 2.0
        };
        if lower == upper && (&captures[1] != "[" || &captures[4] != "]") {
            return None;
        }
        if sample == lower && &captures[1] != "[" {
            sample += numeric_delta(lower);
        }
        if sample == upper && &captures[4] != "]" {
            sample -= numeric_delta(upper);
        }
        return Some(json!(sample));
    }

    let normalized = entry.strip_prefix('=').map(str::trim).unwrap_or(entry);
    if let Some(values) = normalized
        .strip_prefix("in (")
        .and_then(|value| value.strip_suffix(')'))
        .or_else(|| {
            normalized
                .strip_prefix("in [")
                .and_then(|value| value.strip_suffix(']'))
        })
    {
        if let Some(value) = split_feel_list(values)
            .into_iter()
            .next()
            .and_then(parse_feel_scalar)
        {
            return Some(value);
        }
    }
    // Some DMN exporters omit the `in` keyword and emit a parenthesized or
    // bracketed list directly as the unary test.
    let list_values = normalized
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .or_else(|| {
            normalized
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
        });
    if let Some(values) = list_values
        && let Some(value) = split_feel_list(values)
            .into_iter()
            .next()
            .and_then(parse_feel_scalar)
    {
        return Some(value);
    }
    if let Some(value) = normalized
        .strip_prefix("in ")
        .and_then(|value| representative_unary_value(value.trim()))
    {
        return Some(value);
    }
    split_feel_list(normalized)
        .into_iter()
        .next()
        .and_then(parse_feel_scalar)
}

fn bounded_case_name(decision_name: &str, rule_id: &str) -> String {
    let name = format!("自动覆盖：{decision_name} / {rule_id}");
    name.chars().take(160).collect()
}

fn assign_generated_input(
    model: &crate::dmn::DmnModel,
    input: &mut Value,
    expression: &str,
    value: Value,
) -> bool {
    if assign_input_path(input, expression, value.clone()) {
        return true;
    }
    model
        .input_data
        .iter()
        .find(|input_data| {
            input_data.name == expression || input_data.variable.as_deref() == Some(expression)
        })
        .map(|input_data| {
            let path = input_data.variable.as_deref().unwrap_or(&input_data.name);
            assign_input_path(input, path, value)
        })
        .unwrap_or(false)
}

fn normalized_type_name(type_ref: &str) -> &str {
    type_ref
        .strip_prefix("feel:")
        .or_else(|| type_ref.strip_prefix("feel."))
        .unwrap_or(type_ref)
        .trim()
}

fn representative_input_value(
    model: &crate::dmn::DmnModel,
    expression: &str,
    clause_type_ref: Option<&str>,
    input_values: Option<&str>,
) -> Option<Value> {
    // First check if the clause has inputValues constraint
    if let Some(input_values) = input_values
        && let Some(value) = representative_unary_value(input_values)
    {
        return Some(value);
    }

    // A generated decision-table column commonly uses `feel:Any` when the
    // DSL omitted a clause type. For an external input, the inputData
    // declaration is more specific and must drive candidate generation;
    // otherwise a boolean field falls back to the string sample value.
    let clause_is_generic = clause_type_ref
        .is_some_and(|type_ref| normalized_type_name(type_ref).eq_ignore_ascii_case("any"));
    let declared_input_type = model
        .input_data
        .iter()
        .find(|input| input.name == expression || input.variable.as_deref() == Some(expression))
        .and_then(|input| input.type_ref.as_deref());
    let input_type = if clause_is_generic {
        declared_input_type.or(clause_type_ref)
    } else {
        clause_type_ref.or(declared_input_type)
    }?;

    // Check if it's a custom type with allowedValues
    let normalized_type = normalized_type_name(input_type);
    if !matches!(
        normalized_type.to_ascii_lowercase().as_str(),
        "number"
            | "integer"
            | "long"
            | "double"
            | "boolean"
            | "string"
            | "any"
            | "date"
            | "time"
            | "date and time"
            | "datetime"
            | "duration"
            | "daytimeduration"
            | "yearmonthduration"
    ) {
        // It's a custom type, check for allowedValues
        if let Some(item) = model
            .item_definitions
            .iter()
            .find(|item| item.name.eq_ignore_ascii_case(normalized_type) || item.name == input_type)
        {
            if let Some(allowed_values) = &item.allowed_values {
                // Extract first allowed value from the constraint
                if let Some(value) = representative_unary_value(allowed_values) {
                    return Some(value);
                }
            }
        }
    }

    // Fall back to default values for built-in types
    match normalized_type.to_ascii_lowercase().as_str() {
        // Candidate inputs are executed immediately to derive expected
        // outputs. A non-zero default avoids making denominator-based
        // decisions fail before a user can run the generated case.
        "number" | "integer" | "long" | "double" => Some(json!(1)),
        "boolean" => Some(json!(false)),
        "date" => Some(json!("2024-01-01")),
        "time" => Some(json!("12:00:00")),
        "date and time" | "datetime" => Some(json!("2024-01-01T12:00:00")),
        "duration" | "daytimeduration" | "yearmonthduration" => Some(json!("P1D")),
        "string" | "any" => Some(json!("sample")),
        _ => None,
    }
}

fn baseline_number(model: &crate::dmn::DmnModel, baseline: &Value, name: &str) -> Option<f64> {
    let key = model
        .input_data
        .iter()
        .find(|input| input.name == name || input.variable.as_deref() == Some(name))
        .map(|input| input.variable.as_deref().unwrap_or(&input.name))
        .unwrap_or(name);
    baseline.get(key).and_then(Value::as_f64)
}

/// Derive a representative value for comparisons against another input, such
/// as `> AvgPurchaseAmount * 5`. These expressions cannot be inverted from a
/// unary test alone, but the baseline provides a deterministic reference.
fn representative_input_value_for_entry(
    model: &crate::dmn::DmnModel,
    baseline: &Value,
    expression: &str,
    entry: &str,
) -> Option<Value> {
    if let Some(value) = representative_unary_value(entry) {
        return Some(value);
    }
    static RELATIVE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let relative = RELATIVE.get_or_init(|| {
        Regex::new(
            r"^(>=|>|<=|<|=|!=|<>)\s*([A-Za-z_][A-Za-z0-9_.]*)\s*(?:\*\s*(-?\d+(?:\.\d+)?))?$",
        )
        .expect("valid relative comparison regex")
    });
    let captures = relative.captures(entry.trim())?;
    let reference = captures.get(2)?.as_str();
    if reference == expression {
        return None;
    }
    let factor = captures
        .get(3)
        .and_then(|value| value.as_str().parse::<f64>().ok())
        .unwrap_or(1.0);
    let threshold = baseline_number(model, baseline, reference)? * factor;
    let delta = numeric_delta(threshold);
    let sample = match captures.get(1)?.as_str() {
        ">" => threshold + delta,
        ">=" | "=" => threshold,
        "<" => threshold - delta,
        "<=" | "!=" | "<>" => threshold + delta,
        _ => return None,
    };
    Some(json!(sample))
}

// Keep enough combinations for reused inputs and derived-input paths while
// bounding evaluation cost for unusually wide models.
const MAX_CANDIDATE_INPUT_VARIANTS: usize = 16384;

fn candidate_input_variants(model: &crate::dmn::DmnModel, baseline: &Value) -> Vec<Value> {
    let threshold_regex =
        Regex::new(r"(>=|>|<=|<|=|!=|<>)\s*(-?\d+(?:\.\d+)?)").expect("valid threshold regex");
    let mut variants = vec![baseline.clone()];
    let mut expanded_external_inputs = HashSet::new();
    // Literal decisions often establish the boolean/derived inputs consumed
    // by downstream tables (for example `age >= 18`). Include threshold
    // representatives from those expressions so dependent tables can be
    // exercised with external inputs rather than only the baseline sample.
    for input in &model.input_data {
        let key = input.variable.as_deref().unwrap_or(&input.name);
        let mut values = Vec::new();
        for decision in &model.decisions {
            let crate::dmn::DecisionLogic::LiteralExpression(expression) = &decision.logic else {
                continue;
            };
            if !expression.contains(&input.name) && !expression.contains(key) {
                continue;
            }
            for capture in threshold_regex.captures_iter(expression) {
                let Ok(number) = capture[2].parse::<f64>() else {
                    continue;
                };
                let value = match &capture[1] {
                    ">" => number + numeric_delta(number),
                    "<" => number - numeric_delta(number),
                    "!=" | "<>" => number + numeric_delta(number),
                    _ => number,
                };
                let value = json!(value);
                if !values.iter().any(|existing| existing == &value) {
                    values.push(value);
                }
            }
        }
        if values.is_empty() {
            continue;
        }
        let mut expanded = Vec::new();
        for variant in &variants {
            for value in &values {
                let mut candidate = variant.clone();
                if assign_generated_input(model, &mut candidate, key, value.clone()) {
                    expanded.push(candidate);
                    if expanded.len() >= MAX_CANDIDATE_INPUT_VARIANTS {
                        break;
                    }
                }
            }
            if expanded.len() >= MAX_CANDIDATE_INPUT_VARIANTS {
                break;
            }
        }
        if !expanded.is_empty() {
            variants = expanded;
        }
    }
    // Some generators put the condition in the input expression itself, for
    // example `OrderFrequency > 20 or ShippingRatio > 0.3`, with a boolean
    // table entry. Extract thresholds for any external fields mentioned by
    // such expressions so dependent decisions can still be driven by inputs.
    let expression_thresholds = Regex::new(r"(?:>=|>|<=|<|=|!=|<>)\s*(-?\d+(?:\.\d+)?)")
        .expect("valid table expression threshold regex");
    for input in &model.input_data {
        let key = input.variable.as_deref().unwrap_or(&input.name);
        let mut values = Vec::new();
        for decision in &model.decisions {
            let crate::dmn::DecisionLogic::DecisionTable(table) = &decision.logic else {
                continue;
            };
            for clause in &table.inputs {
                if !clause.expression.contains(&input.name) && !clause.expression.contains(key) {
                    continue;
                }
                for capture in expression_thresholds.captures_iter(&clause.expression) {
                    let Ok(number) = capture[1].parse::<f64>() else {
                        continue;
                    };
                    for value in [number, number + numeric_delta(number)] {
                        let value = json!(value);
                        if !values.iter().any(|existing| existing == &value) {
                            values.push(value);
                        }
                    }
                }
            }
        }
        if values.is_empty() {
            continue;
        }
        if let Some(value) = baseline.get(key).cloned()
            && !values.iter().any(|existing| existing == &value)
        {
            values.push(value);
        }
        let mut expanded = Vec::new();
        for variant in &variants {
            for value in &values {
                let mut candidate = variant.clone();
                if assign_generated_input(model, &mut candidate, key, value.clone()) {
                    expanded.push(candidate);
                    if expanded.len() >= MAX_CANDIDATE_INPUT_VARIANTS {
                        break;
                    }
                }
            }
            if expanded.len() >= MAX_CANDIDATE_INPUT_VARIANTS {
                break;
            }
        }
        if !expanded.is_empty() {
            variants = expanded;
        }
    }
    for decision in &model.decisions {
        let crate::dmn::DecisionLogic::DecisionTable(table) = &decision.logic else {
            continue;
        };
        for (index, clause) in table.inputs.iter().enumerate() {
            let is_external = model.input_data.iter().any(|input| {
                input.name == clause.expression
                    || input.variable.as_deref() == Some(clause.expression.as_str())
            });
            if !is_external {
                continue;
            }
            let input_key = model
                .input_data
                .iter()
                .find(|input| {
                    input.name == clause.expression
                        || input.variable.as_deref() == Some(clause.expression.as_str())
                })
                .map(|input| input.variable.as_deref().unwrap_or(&input.name).to_owned());
            let repeated_input = input_key
                .as_ref()
                .is_some_and(|input_key| !expanded_external_inputs.insert(input_key.clone()));
            let mut values = Vec::new();
            for rule in &table.rules {
                let Some(entry) = rule
                    .input_entries
                    .get(index)
                    .map(String::as_str)
                    .map(str::trim)
                else {
                    continue;
                };
                let value = if entry.is_empty() || entry == "-" {
                    representative_input_value(
                        model,
                        &clause.expression,
                        clause.type_ref.as_deref(),
                        clause.input_values.as_deref(),
                    )
                } else {
                    representative_input_value_for_entry(model, baseline, &clause.expression, entry)
                };
                if let Some(value) = value
                    && !values.iter().any(|existing| existing == &value)
                {
                    values.push(value);
                }
                // Add a value on the other side of simple numeric bounds so
                // later FIRST rules can be reached (e.g. an adult value for
                // a preceding `< 14` rule).
                if !entry.contains(" or ")
                    && !entry.contains(" and ")
                    && let Some(value) = representative_unary_value(&format!("not({entry})"))
                    && !values.iter().any(|existing| existing == &value)
                {
                    values.push(value);
                }
            }
            if values.is_empty()
                && let Some(value) = representative_input_value(
                    model,
                    &clause.expression,
                    clause.type_ref.as_deref(),
                    clause.input_values.as_deref(),
                )
            {
                values.push(value);
            }
            if repeated_input {
                // Merge values discovered by another table for the same
                // input. Replacing the dimension would lose boundaries such
                // as both `< 7` and `< 1` when a model reuses an input.
                for variant in &variants {
                    if let Some(value) = variant
                        .get(input_key.as_deref().unwrap_or(&clause.expression))
                        .cloned()
                        && !values.iter().any(|existing| existing == &value)
                    {
                        values.push(value);
                    }
                }
            }
            // Keep the neutral baseline in every dimension. Without it, the
            // first constrained table column can discard all unconstrained
            // values needed to reach later FIRST rules (for example an
            // eligible adult with a normal account age).
            if let Some(baseline_value) = model
                .input_data
                .iter()
                .find(|input| {
                    input.name == clause.expression
                        || input.variable.as_deref() == Some(clause.expression.as_str())
                })
                .and_then(|input| baseline.get(input.variable.as_deref().unwrap_or(&input.name)))
                .cloned()
                && !values.iter().any(|existing| existing == &baseline_value)
            {
                values.push(baseline_value);
            }
            if model
                .input_data
                .iter()
                .find(|input| {
                    input.name == clause.expression
                        || input.variable.as_deref() == Some(clause.expression.as_str())
                })
                .and_then(|input| input.type_ref.as_deref())
                .is_some_and(|type_ref| {
                    normalized_type_name(type_ref).eq_ignore_ascii_case("number")
                })
                && !values.iter().any(|existing| existing == &json!(0))
            {
                // Zero is useful for numerator inputs such as shipping cost
                // and is filtered by execution when it would be an invalid
                // denominator.
                values.push(json!(0));
            }
            if values.is_empty() {
                continue;
            }
            let mut expanded = Vec::new();
            for variant in &variants {
                for value in &values {
                    let mut candidate = variant.clone();
                    if assign_generated_input(
                        model,
                        &mut candidate,
                        &clause.expression,
                        value.clone(),
                    ) {
                        expanded.push(candidate);
                        if expanded.len() >= MAX_CANDIDATE_INPUT_VARIANTS {
                            break;
                        }
                    }
                }
                if expanded.len() >= MAX_CANDIDATE_INPUT_VARIANTS {
                    break;
                }
            }
            if !expanded.is_empty() {
                variants = expanded;
            }
        }
    }
    // Include numeric input data that only participates in literal
    // expressions (for example the numerator of a derived ratio). Such
    // inputs do not have a decision-table column to contribute variants, but
    // changing them is necessary to reach dependent table rules.
    for input in &model.input_data {
        let type_ref = input.type_ref.as_deref().unwrap_or_default();
        if !normalized_type_name(type_ref).eq_ignore_ascii_case("number") {
            continue;
        }
        let key = input.variable.as_deref().unwrap_or(&input.name);
        if model.decisions.iter().any(|decision| {
            let crate::dmn::DecisionLogic::DecisionTable(table) = &decision.logic else {
                return false;
            };
            table
                .inputs
                .iter()
                .any(|clause| clause.expression == input.name || clause.expression == key)
        }) {
            continue;
        }
        let used_in_literal = model.decisions.iter().any(|decision| {
            matches!(&decision.logic, crate::dmn::DecisionLogic::LiteralExpression(expression)
                if expression.contains(&input.name) || expression.contains(key))
        });
        if !used_in_literal {
            continue;
        }
        let mut values = vec![json!(0)];
        if let Some(value) = baseline.get(key).cloned()
            && !values.iter().any(|existing| existing == &value)
        {
            values.push(value);
        }
        for variant in &variants {
            if let Some(value) = variant.get(key).cloned()
                && !values.iter().any(|existing| existing == &value)
            {
                values.push(value);
            }
        }
        let mut expanded = Vec::new();
        for variant in &variants {
            for value in &values {
                let mut candidate = variant.clone();
                if assign_generated_input(model, &mut candidate, key, value.clone()) {
                    expanded.push(candidate);
                    if expanded.len() >= MAX_CANDIDATE_INPUT_VARIANTS {
                        break;
                    }
                }
            }
            if expanded.len() >= MAX_CANDIDATE_INPUT_VARIANTS {
                break;
            }
        }
        if !expanded.is_empty() {
            variants = expanded;
        }
    }
    variants
}

fn evaluate_rule_candidate(
    model: &crate::dmn::DmnModel,
    decision_id: &str,
    rule_id: &str,
    input: &Value,
) -> Option<(Value, crate::dmn::DmnResult)> {
    let inputs = json_inputs_to_feel(model, input).ok()?;
    let result = crate::dmn::DmnExecutor::new(model).evaluate_all(inputs);
    let target = result.decision_by_id(decision_id)?;
    if target.status == crate::dmn::DecisionStatus::Succeeded
        && target
            .matched_rule_ids
            .iter()
            .any(|matched| matched == rule_id)
    {
        Some((input.clone(), result))
    } else {
        None
    }
}

pub(crate) fn suggested_inputs_for_static_findings(
    model: &crate::dmn::DmnModel,
    draft_version_id: i64,
    findings: &[crate::models::VerificationFinding],
) -> HashMap<String, Value> {
    let Ok(baseline) = sample_input(model) else {
        return HashMap::new();
    };
    let mut candidates = candidate_input_variants(model, &baseline);
    if !candidates.contains(&baseline) {
        candidates.push(baseline);
    }
    findings
        .iter()
        .filter_map(|finding| {
            let input = suggested_input_for_static_finding(model, finding, &candidates)?;
            Some((
                crate::workflow_api::static_analysis_source_reference(draft_version_id, finding),
                input,
            ))
        })
        .collect()
}

fn coverage_boundary_candidates(
    model: &crate::dmn::DmnModel,
    logic: &crate::dmn::DecisionLogic,
    candidates: &[Value],
) -> Vec<Value> {
    let crate::dmn::DecisionLogic::DecisionTable(table) = logic else {
        return Vec::new();
    };
    let threshold = Regex::new(r"(?:>=|>|<=|<|=|!=|<>)\s*(-?\d+(?:\.\d+)?)")
        .expect("valid coverage boundary regex");
    let mut boundaries = Vec::new();
    for (column_index, clause) in table.inputs.iter().enumerate() {
        let Some(input_data) = model.input_data.iter().find(|input| {
            input.name == clause.expression
                || input.variable.as_deref() == Some(clause.expression.as_str())
        }) else {
            continue;
        };
        let input_key = input_data.variable.as_deref().unwrap_or(&input_data.name);
        let mut values = Vec::new();
        for rule in &table.rules {
            let Some(entry) = rule.input_entries.get(column_index) else {
                continue;
            };
            for capture in threshold.captures_iter(entry) {
                let Ok(value) = capture[1].parse::<f64>() else {
                    continue;
                };
                for candidate in [
                    value - numeric_delta(value),
                    value,
                    value + numeric_delta(value),
                ] {
                    let candidate = json!(candidate);
                    if !values.contains(&candidate) {
                        values.push(candidate);
                    }
                }
            }
        }
        for base in candidates.iter().take(32) {
            for value in &values {
                let mut candidate = base.clone();
                if assign_generated_input(model, &mut candidate, input_key, value.clone())
                    && !boundaries.contains(&candidate)
                {
                    boundaries.push(candidate);
                    if boundaries.len() >= MAX_CANDIDATE_INPUT_VARIANTS {
                        return boundaries;
                    }
                }
            }
        }
    }
    boundaries
}

fn suggested_input_for_static_finding(
    model: &crate::dmn::DmnModel,
    finding: &crate::models::VerificationFinding,
    candidates: &[Value],
) -> Option<Value> {
    if matches!(
        finding.code.as_str(),
        "UNIQUE_RULE_CONFLICT" | "ANY_OUTPUT_CONFLICT"
    ) {
        let input = crate::version_verification::static_finding_witness_input(model, finding)?;
        let feel_input = json_inputs_to_feel(model, &input).ok()?;
        let result = crate::dmn::DmnExecutor::new(model).evaluate_all(feel_input);
        return result
            .first_error()
            .is_some_and(|error| error.code == "DMN-EVAL-HIT-POLICY")
            .then_some(input);
    }
    if finding.code == "UNREACHABLE_RULE" {
        return None;
    }
    let decision = model
        .decisions
        .iter()
        .find(|decision| decision.name == finding.element)?;
    let coverage_candidates = if finding.code == "MISSING_COVERAGE" {
        let mut expanded = candidates.to_vec();
        expanded.extend(coverage_boundary_candidates(
            model,
            &decision.logic,
            candidates,
        ));
        expanded
    } else {
        candidates.to_vec()
    };
    coverage_candidates
        .iter()
        .find(|input| {
            let Ok(inputs) = json_inputs_to_feel(model, input) else {
                return false;
            };
            let result = crate::dmn::DmnExecutor::new(model).evaluate_all(inputs);
            let Some(target) = result.decision_by_id(&decision.id) else {
                return false;
            };
            match finding.code.as_str() {
                "UNIQUE_RULE_CONFLICT" | "ANY_OUTPUT_CONFLICT" => {
                    finding.rule_ids.len() >= 2
                        && finding
                            .rule_ids
                            .iter()
                            .all(|rule_id| target.matched_rule_ids.contains(rule_id))
                }
                "MISSING_COVERAGE" => {
                    target.status == crate::dmn::DecisionStatus::Succeeded
                        && target.matched_rule_ids.is_empty()
                }
                _ => false,
            }
        })
        .cloned()
}

/// Evaluate a candidate input and return diagnostic information about why it failed.
fn evaluate_rule_candidate_with_diagnostic(
    model: &crate::dmn::DmnModel,
    decision_id: &str,
    rule_id: &str,
    input: &Value,
) -> Result<(Value, crate::dmn::DmnResult), String> {
    let inputs = json_inputs_to_feel(model, input).map_err(|e| format!("输入转换失败: {}", e))?;
    let result = crate::dmn::DmnExecutor::new(model).evaluate_all(inputs);

    // Check if any decision failed with dependency issues
    for decision_result in &result.decision_results {
        if decision_result.status == crate::dmn::DecisionStatus::Failed
            || decision_result.status == crate::dmn::DecisionStatus::Skipped
        {
            // Find error messages for this decision
            let errors: Vec<_> = result
                .messages
                .iter()
                .filter(|msg| {
                    msg.source.decision_id.as_deref() == Some(&decision_result.decision_id)
                })
                .map(|msg| format!("{}: {}", msg.code, msg.message))
                .collect();
            if !errors.is_empty() {
                return Err(format!(
                    "决策 '{}' 失败: {}",
                    decision_result.decision_name,
                    errors.join("; ")
                ));
            }
        }
    }

    let target = result
        .decision_by_id(decision_id)
        .ok_or_else(|| format!("找不到目标决策 {}", decision_id))?;

    if target.status != crate::dmn::DecisionStatus::Succeeded {
        return Err(format!("目标决策状态: {:?}", target.status));
    }

    if !target
        .matched_rule_ids
        .iter()
        .any(|matched| matched == rule_id)
    {
        return Err(format!(
            "规则未命中，实际命中: {:?}",
            target.matched_rule_ids
        ));
    }

    Ok((input.clone(), result))
}

fn generate_business_candidates(
    model: &crate::dmn::DmnModel,
    version_id: i64,
    version_number: i32,
) -> Result<BusinessCandidateGeneration, ProjectApiError> {
    let baseline = sample_input(model)
        .map_err(|error| ProjectApiError::bad_request(format!("无法构造基础业务输入：{error}")))?;
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut total_rule_count: usize = 0;
    let mut skipped_rule_count: usize = 0;
    let mut notes = Vec::new();
    let mut skip_counts = CandidateSkipCounts::default();
    let input_variants = candidate_input_variants(model, &baseline);

    for decision in &model.decisions {
        let crate::dmn::DecisionLogic::DecisionTable(table) = &decision.logic else {
            continue;
        };
        let decision_candidate_start = candidates.len();
        for (rule_index, rule) in table.rules.iter().enumerate() {
            total_rule_count += 1;
            if candidates.len() >= MAX_GENERATED_BUSINESS_CASES {
                skipped_rule_count += 1;
                skip_counts.capacity += 1;
                continue;
            }
            let rule_id = rule
                .id
                .clone()
                .unwrap_or_else(|| format!("rule-{}", rule_index + 1));
            let rule_key = format!("{}:{rule_id}", decision.id);
            let mut input = baseline.clone();
            let mut supported = rule.input_entries.len() == table.inputs.len();
            for (index, entry) in rule.input_entries.iter().enumerate() {
                let entry = entry.trim();
                let Some(clause) = table.inputs.get(index) else {
                    supported = false;
                    break;
                };
                if entry.is_empty() || entry == "-" {
                    // A wildcard still has to satisfy the column-level
                    // inputValues constraint. The baseline sample (which may
                    // be null or an arbitrary string) may be outside that
                    // domain and would otherwise make every wildcard rule
                    // look unreachable.
                    let generated_value = representative_input_value(
                        model,
                        &clause.expression,
                        clause.type_ref.as_deref(),
                        clause.input_values.as_deref(),
                    );
                    // Preserve a type-correct baseline for wildcard columns
                    // when the exporter omitted a clause type.
                    let value = if clause.input_values.is_none() {
                        let baseline_value = model
                            .input_data
                            .iter()
                            .find(|item| {
                                item.name == clause.expression
                                    || item.variable.as_deref() == Some(clause.expression.as_str())
                            })
                            .and_then(|item| {
                                input
                                    .get(item.variable.as_deref().unwrap_or(&item.name))
                                    .cloned()
                            });
                        baseline_value.or(generated_value)
                    } else {
                        generated_value
                    };
                    if let Some(value) = value {
                        if !assign_generated_input(model, &mut input, &clause.expression, value) {
                            // The expression may be a required decision rather
                            // than external input. Leave it untouched so the
                            // executor can populate it from its dependency.
                            let is_external_input = model.input_data.iter().any(|input_data| {
                                input_data.name == clause.expression
                                    || input_data.variable.as_deref()
                                        == Some(clause.expression.as_str())
                            });
                            if !is_external_input {
                                continue;
                            }
                            supported = false;
                            break;
                        }
                    }
                    continue;
                }
                let Some(value) = representative_input_value_for_entry(
                    model,
                    &baseline,
                    &clause.expression,
                    entry,
                ) else {
                    supported = false;
                    break;
                };
                if !assign_generated_input(model, &mut input, &clause.expression, value) {
                    // DMN input expressions are allowed to use either the
                    // inputData name or its variable name. `sample_input`
                    // uses the variable key, so resolve the alternate name
                    // before declaring the rule unsupported.
                    supported = false;
                    break;
                }
            }
            let mut fallback = false;
            let evaluated = if supported {
                evaluate_rule_candidate(model, &decision.id, &rule_id, &input)
            } else {
                None
            };
            let Some((input, result)) = evaluated.or_else(|| {
                fallback = true;
                input_variants.iter().find_map(|variant| {
                    evaluate_rule_candidate(model, &decision.id, &rule_id, variant)
                })
            }) else {
                skipped_rule_count += 1;
                if !supported {
                    skip_counts.unsupported_expression += 1;
                } else {
                    // Log diagnostic information for target_failed cases
                    if let Err(reason) = evaluate_rule_candidate_with_diagnostic(
                        model,
                        &decision.id,
                        &rule_id,
                        &input,
                    ) {
                        eprintln!("规则 {} 失败: {}", rule_key, reason);
                    }
                    skip_counts.target_failed += 1;
                }
                continue;
            };
            let trace = build_execution_trace(
                model,
                Some(version_id),
                Some(version_number),
                &input,
                &result,
            );
            let Some(expected) = trace.output.get(&decision.name).cloned() else {
                skipped_rule_count += 1;
                skip_counts.trace_output_missing += 1;
                continue;
            };
            let expected_output_json = json!({ decision.name.clone(): expected });
            let fingerprint = serde_json::to_string(&(input.clone(), expected_output_json.clone()))
                .map_err(|error| {
                    ProjectApiError::bad_request(format!("生成案例去重失败：{error}"))
                })?;
            if !seen.insert(fingerprint) {
                skipped_rule_count += 1;
                skip_counts.duplicate += 1;
                continue;
            }
            candidates.push(GeneratedBusinessCandidate {
                name: bounded_case_name(&decision.name, &rule_id),
                input_json: input,
                expected_output_json,
                rule_key,
                fallback,
            });
        }

        // A table can depend on another decision and therefore cannot be
        // inverted into external inputs rule by rule. Preserve one runnable
        // baseline for that table when the normal rule synthesis produced no
        // candidate, so the acceptance suite never silently appears empty.
        if candidates.len() == decision_candidate_start
            && candidates.len() < MAX_GENERATED_BUSINESS_CASES
        {
            let mut recovered_rules = 0;
            for rule in &table.rules {
                let rule_id = rule.id.clone().unwrap_or_else(|| {
                    format!(
                        "rule-{}",
                        table
                            .rules
                            .iter()
                            .position(|candidate| std::ptr::eq(candidate, rule))
                            .unwrap_or_default()
                            + 1
                    )
                });
                let Some((input, result)) = input_variants.iter().find_map(|input| {
                    let inputs = json_inputs_to_feel(model, input).ok()?;
                    let result = crate::dmn::DmnExecutor::new(model).evaluate_all(inputs);
                    let target = result.decision_by_id(&decision.id)?;
                    (target.status == crate::dmn::DecisionStatus::Succeeded
                        && target
                            .matched_rule_ids
                            .iter()
                            .any(|matched| matched == &rule_id))
                    .then_some((input, result))
                }) else {
                    continue;
                };
                let trace = build_execution_trace(
                    model,
                    Some(version_id),
                    Some(version_number),
                    input,
                    &result,
                );
                if let Some(expected) = trace.output.get(&decision.name).cloned() {
                    let expected_output_json = json!({ decision.name.clone(): expected });
                    let fingerprint =
                        serde_json::to_string(&((*input).clone(), expected_output_json.clone()))
                            .map_err(|error| {
                                ProjectApiError::bad_request(format!("生成案例去重失败：{error}"))
                            })?;
                    if seen.insert(fingerprint) {
                        candidates.push(GeneratedBusinessCandidate {
                            name: bounded_case_name(&decision.name, &rule_id),
                            input_json: (*input).clone(),
                            expected_output_json,
                            rule_key: format!("{}:{rule_id}", decision.id),
                            fallback: true,
                        });
                        recovered_rules += 1;
                    }
                }
            }
            if recovered_rules > 0 {
                skipped_rule_count = skipped_rule_count.saturating_sub(recovered_rules);
                skip_counts.unsupported_expression = skip_counts
                    .unsupported_expression
                    .saturating_sub(recovered_rules);
                notes.push(format!(
                    "决策 {} 的依赖规则通过外部输入组合补充了 {} 个可执行候选",
                    decision.name, recovered_rules
                ));
            } else if candidates.len() == decision_candidate_start
                && let Ok(inputs) = json_inputs_to_feel(model, &baseline)
            {
                let result = crate::dmn::DmnExecutor::new(model).evaluate_all(inputs);
                if let Some(target) = result.decision_by_id(&decision.id)
                    && target.status == crate::dmn::DecisionStatus::Succeeded
                {
                    let trace = build_execution_trace(
                        model,
                        Some(version_id),
                        Some(version_number),
                        &baseline,
                        &result,
                    );
                    if let Some(expected) = trace.output.get(&decision.name).cloned() {
                        let expected_output_json = json!({ decision.name.clone(): expected });
                        let fingerprint = serde_json::to_string(&(
                            baseline.clone(),
                            expected_output_json.clone(),
                        ))
                        .map_err(|error| {
                            ProjectApiError::bad_request(format!("生成案例去重失败：{error}"))
                        })?;
                        if seen.insert(fingerprint) {
                            candidates.push(GeneratedBusinessCandidate {
                                name: bounded_case_name(&decision.name, "baseline"),
                                input_json: baseline.clone(),
                                expected_output_json,
                                rule_key: format!("{}:baseline", decision.id),
                                fallback: true,
                            });
                            notes.push(format!(
                                "决策 {} 无法反解具体规则，已补充 1 个可执行基线候选",
                                decision.name
                            ));
                        }
                    }
                }
            }
        }
    }
    if total_rule_count == 0 {
        notes.push("模型没有可自动反解的决策表规则，可继续手工添加业务案例".into());
    }
    if skipped_rule_count > 0 {
        notes.push(format!(
            "{skipped_rule_count} 条规则因复杂 FEEL、依赖决策输入、未命中或数量上限而跳过"
        ));
        skip_counts.note(&mut notes);
    }
    Ok(BusinessCandidateGeneration {
        candidates,
        total_rule_count,
        skipped_rule_count,
        notes,
    })
}

async fn generate_test_cases(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
) -> Result<Json<GenerateDecisionTestCasesResult>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("生成业务验收案例", error))?;
    let row = sqlx::query_as::<_, (String, String, Option<i64>, i32)>(
        r#"
        SELECT artifact.dmn_xml, version.status, project.current_draft_version_id,
               version.version_number
        FROM decision_versions version
        JOIN decision_projects project ON project.id = version.project_id
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE version.id = $1 AND version.project_id = $2
        FOR UPDATE OF version, project
        "#,
    )
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取案例生成模型", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if row.2 != Some(version_id) || matches!(row.1.as_str(), "published" | "retired") {
        return Err(ProjectApiError::bad_request(
            "只能为项目的当前可编辑版本生成验收案例",
        ));
    }
    let model = crate::dmn::parse_dmn(&row.0)
        .map_err(|error| ProjectApiError::bad_request(format!("DMN XML 解析失败: {error}")))?;
    crate::dmn::compile_dmn(&model)
        .map_err(|error| ProjectApiError::bad_request(format!("DMN 模型编译失败: {error}")))?;
    let generated = generate_business_candidates(&model, version_id, row.3)?;
    let existing = sqlx::query_as::<_, (Value, Value)>(
        "SELECT input_json, expected_output_json FROM decision_test_cases WHERE version_id = $1 AND category = 'business'",
    )
    .bind(version_id).fetch_all(&mut *transaction).await
    .map_err(|error| ProjectApiError::database("读取已有业务案例", error))?;
    let existing = existing
        .into_iter()
        .filter_map(|pair| serde_json::to_string(&pair).ok())
        .collect::<HashSet<_>>();
    let mut duplicate_count = 0;
    let mut inserted_rule_keys = Vec::new();
    let mut covered_rule_keys = Vec::new();
    for candidate in generated.candidates {
        covered_rule_keys.push(candidate.rule_key.clone());
        let fingerprint = serde_json::to_string(&(
            candidate.input_json.clone(),
            candidate.expected_output_json.clone(),
        ))
        .map_err(|error| ProjectApiError::bad_request(format!("生成案例去重失败：{error}")))?;
        if existing.contains(&fingerprint) {
            duplicate_count += 1;
            continue;
        }
        sqlx::query(
            r#"
            INSERT INTO decision_test_cases (
                version_id, name, category, input_json, expected_output_json, status, source
            ) VALUES ($1, $2, 'business', $3, $4, 'pending', 'generated')
            "#,
        )
        .bind(version_id)
        .bind(candidate.name)
        .bind(candidate.input_json)
        .bind(candidate.expected_output_json)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("保存生成业务案例", error))?;
        inserted_rule_keys.push(candidate.rule_key);
    }
    if !inserted_rule_keys.is_empty() {
        invalidate_verification(&mut transaction, version_id).await?;
    }
    insert_audit_event(&mut *transaction, project_id, Some(version_id), "test_cases.generated", &claims.sub,
        "按决策规则生成业务验收候选案例", json!({
            "generatedCount": inserted_rule_keys.len(), "duplicateCount": duplicate_count,
            "coveredRuleCount": covered_rule_keys.len(), "totalRuleCount": generated.total_rule_count,
            "skippedRuleCount": generated.skipped_rule_count, "coveredRules": &covered_rule_keys,
        }))
        .await.map_err(|error| ProjectApiError::database("记录案例生成审计事件", error))?;
    let cases = sqlx::query_as::<_, DecisionTestCase>(
        r#"
        SELECT id, version_id, name, category, input_json, expected_output_json,
               actual_output_json, execution_trace_json, status, source, approved_by, approved_at,
               created_at, updated_at
        FROM decision_test_cases WHERE version_id = $1 ORDER BY created_at, id
        "#,
    )
    .bind(version_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取生成后的业务案例", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交生成业务案例", error))?;
    Ok(Json(GenerateDecisionTestCasesResult {
        cases,
        generated_count: inserted_rule_keys.len(),
        duplicate_count,
        covered_rule_count: covered_rule_keys.len(),
        total_rule_count: generated.total_rule_count,
        skipped_rule_count: generated.skipped_rule_count,
        notes: generated.notes,
    }))
}

async fn list_test_cases(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<DecisionTestCase>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let cases = sqlx::query_as::<_, DecisionTestCase>(
        r#"
        SELECT id, version_id, name, category, input_json, expected_output_json,
               actual_output_json, execution_trace_json, status, source, approved_by, approved_at,
               created_at, updated_at
        FROM decision_test_cases
        WHERE version_id = $1
        ORDER BY created_at, id
        "#,
    )
    .bind(version_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取业务验收案例", error))?;
    Ok(Json(cases))
}

async fn create_test_case(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
    Json(payload): Json<CreateDecisionTestCase>,
) -> Result<(StatusCode, Json<DecisionTestCase>), ProjectApiError> {
    validate_test_case(&payload)?;
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("创建业务验收案例", error))?;
    let version = sqlx::query_as::<_, (String, Option<i64>)>(
        r#"
        SELECT version.status, project.current_draft_version_id
        FROM decision_versions version
        JOIN decision_projects project ON project.id = version.project_id
        WHERE version.id = $1 AND version.project_id = $2
        FOR UPDATE
        "#,
    )
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取验收案例版本", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if version.1 != Some(version_id) || matches!(version.0.as_str(), "published" | "retired") {
        return Err(ProjectApiError::bad_request(
            "只能为项目的当前可编辑版本添加验收案例",
        ));
    }
    let test_case = sqlx::query_as::<_, DecisionTestCase>(
        r#"
        INSERT INTO decision_test_cases (
            version_id, name, category, input_json, expected_output_json, status, source
        )
        VALUES ($1, $2, 'business', $3, $4, 'pending', 'manual')
        RETURNING id, version_id, name, category, input_json, expected_output_json,
                  actual_output_json, execution_trace_json, status, source, approved_by, approved_at,
                  created_at, updated_at
        "#,
    )
    .bind(version_id)
    .bind(payload.name.trim())
    .bind(payload.input_json)
    .bind(payload.expected_output_json)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存业务验收案例", error))?;
    invalidate_verification(&mut transaction, version_id).await?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交业务验收案例", error))?;
    Ok((StatusCode::CREATED, Json(test_case)))
}

fn json_contains(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|actual| json_contains(actual, value))
        }),
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| json_contains(actual, expected))
        }
        _ => actual == expected,
    }
}

fn execute_test_case(
    model: &crate::dmn::DmnModel,
    version_number: i32,
    test_case: &DecisionTestCase,
) -> (String, Value, Value) {
    let inputs = match json_inputs_to_feel(model, &test_case.input_json) {
        Ok(inputs) => inputs,
        Err(error) => {
            let trace = build_rejected_execution_trace(
                model,
                Some(test_case.version_id),
                Some(version_number),
                &test_case.input_json,
                "INPUT_CONVERSION",
                &error,
            );
            return (
                "error".into(),
                json!({ "error": error }),
                serde_json::to_value(trace).expect("execution trace is serializable"),
            );
        }
    };
    let result = crate::dmn::DmnExecutor::new(model).evaluate_all(inputs);
    let trace = build_execution_trace(
        model,
        Some(test_case.version_id),
        Some(version_number),
        &test_case.input_json,
        &result,
    );
    let trace_json = serde_json::to_value(&trace).expect("execution trace is serializable");
    let relevant_failure = test_case
        .expected_output_json
        .as_object()
        .and_then(|expected| {
            expected.keys().find_map(|name| {
                result
                    .decision_by_name(name)
                    .or_else(|| result.decision_by_id(name))
                    .filter(|decision| decision.status != crate::dmn::DecisionStatus::Succeeded)
            })
        });
    if relevant_failure.is_some() {
        let error = result
            .first_error()
            .map(|message| message.message.clone())
            .unwrap_or_else(|| "决策执行失败".into());
        return ("error".into(), json!({ "error": error }), trace_json);
    }
    let actual = trace.output;
    let status = if json_contains(&actual, &test_case.expected_output_json) {
        "passed"
    } else {
        "failed"
    };
    (status.into(), actual, trace_json)
}

fn execute_historical_regression_case(
    model: &crate::dmn::DmnModel,
    candidate_version_id: i64,
    version_number: i32,
    test_case: HistoricalRegressionCase,
) -> RegressionTestCaseResult {
    let (status, actual_output, trace_json) = match json_inputs_to_feel(model, &test_case.input) {
        Ok(inputs) => {
            let result = crate::dmn::DmnExecutor::new(model).evaluate_all(inputs);
            let trace = build_execution_trace(
                model,
                Some(candidate_version_id),
                Some(version_number),
                &test_case.input,
                &result,
            );
            let trace_json = serde_json::to_value(&trace).unwrap_or(Value::Null);
            let relevant_failure = test_case.expected_output.as_object().and_then(|expected| {
                expected.keys().find_map(|name| {
                    result
                        .decision_by_name(name)
                        .or_else(|| result.decision_by_id(name))
                        .filter(|decision| decision.status != crate::dmn::DecisionStatus::Succeeded)
                })
            });
            if relevant_failure.is_some() {
                (
                    "error".to_string(),
                    json!({"error": "决策执行失败"}),
                    trace_json,
                )
            } else {
                let actual = trace.output;
                let status = if json_contains(&actual, &test_case.expected_output) {
                    "passed"
                } else {
                    "failed"
                };
                (status.into(), actual, trace_json)
            }
        }
        Err(error) => {
            let trace = build_rejected_execution_trace(
                model,
                Some(candidate_version_id),
                Some(version_number),
                &test_case.input,
                "INPUT_CONVERSION",
                &error,
            );
            (
                "error".into(),
                json!({"error": error}),
                serde_json::to_value(trace).unwrap_or(Value::Null),
            )
        }
    };
    RegressionTestCaseResult {
        source_run_id: test_case.source_run_id,
        input: test_case.input,
        expected_output: test_case.expected_output,
        actual_output,
        execution_trace: trace_json,
        risk_level: if status == "passed" { "low" } else { "high" }.into(),
        status,
    }
}

async fn run_historical_regression_tests(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, candidate_version_id)): Path<(i64, i64)>,
) -> Result<Json<RegressionTestReport>, ProjectApiError> {
    let started_at = std::time::Instant::now();
    ensure_project_permission(
        &state,
        project_id,
        &claims.sub,
        ProjectPermission::ExecuteAcceptance,
    )
    .await?;
    let version = sqlx::query_as::<_, (Option<i64>, String, i32)>(
        r#"SELECT baseline.id, artifact.dmn_xml, candidate.version_number
           FROM decision_versions candidate
           JOIN decision_versions baseline
             ON baseline.id = candidate.base_version_id
            AND baseline.project_id = candidate.project_id
           JOIN decision_projects project ON project.id = candidate.project_id
           JOIN decision_artifacts artifact ON artifact.version_id = candidate.id
           WHERE candidate.id = $1 AND candidate.project_id = $2"#,
    )
    .bind(candidate_version_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取回归测试候选版本", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let Some(baseline_version_id) = version.0 else {
        return Err(ProjectApiError::bad_request(
            "该版本没有基线版本，无法生成历史回归测试",
        ));
    };
    let model = crate::dmn::parse_dmn(&version.1)
        .map_err(|error| ProjectApiError::bad_request(format!("DMN XML 解析失败: {error}")))?;
    crate::dmn::compile_dmn(&model)
        .map_err(|error| ProjectApiError::bad_request(format!("DMN 模型编译失败: {error}")))?;
    let history = sqlx::query_as::<_, (i64, Value, Value, i64)>(
        r#"SELECT id, input_json, output_json, COUNT(*) OVER() AS available_source_count
           FROM decision_runs
           WHERE project_id = $1 AND version_id = $2 AND status = 'succeeded'
           ORDER BY executed_at DESC, id DESC
           LIMIT 1000"#,
    )
    .bind(project_id)
    .bind(baseline_version_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取历史决策执行记录", error))?;
    let available_source_count = history
        .first()
        .map(|(_, _, _, count)| (*count).max(0) as usize)
        .unwrap_or_default();
    let generated = RegressionTestGenerator::generate(history.into_iter().map(
        |(source_run_id, input, expected_output, _)| HistoricalRegressionCase {
            source_run_id,
            input,
            expected_output,
        },
    ));
    if generated.cases.is_empty() {
        return Err(ProjectApiError::bad_request(
            "基线版本没有成功的历史执行记录可用于回归测试",
        ));
    }

    let duplicate_count = generated.duplicate_count;
    let mut pending = VecDeque::from(generated.cases);
    let mut tasks = tokio::task::JoinSet::new();
    let mut results = Vec::with_capacity(pending.len());
    while !pending.is_empty() || !tasks.is_empty() {
        while tasks.len() < 32 {
            let Some(test_case) = pending.pop_front() else {
                break;
            };
            let model = model.clone();
            let version_number = version.2;
            tasks.spawn_blocking(move || {
                execute_historical_regression_case(
                    &model,
                    candidate_version_id,
                    version_number,
                    test_case,
                )
            });
        }
        if let Some(result) = tasks.join_next().await {
            results.push(result.map_err(|error| {
                ProjectApiError::coded(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "REGRESSION_TASK_FAILED",
                    format!("回归测试任务异常：{error}"),
                )
            })?);
        }
    }
    results.sort_by_key(|result| result.source_run_id);
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存回归测试结果", error))?;
    for batch in results.chunks(100) {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            r#"INSERT INTO regression_test_sources (
                candidate_version_id, source_version_id, source_run_id,
                test_input, expected_output, actual_output, execution_trace_json,
                status, risk_level, executed_at
            ) "#,
        );
        query.push_values(batch, |mut row, result| {
            row.push_bind(candidate_version_id)
                .push_bind(baseline_version_id)
                .push_bind(result.source_run_id)
                .push_bind(&result.input)
                .push_bind(&result.expected_output)
                .push_bind(&result.actual_output)
                .push_bind(&result.execution_trace)
                .push_bind(&result.status)
                .push_bind(&result.risk_level)
                .push("NOW()");
        });
        query.push(
            r#" ON CONFLICT (candidate_version_id, source_run_id) DO UPDATE SET
                test_input = EXCLUDED.test_input, expected_output = EXCLUDED.expected_output,
                actual_output = EXCLUDED.actual_output, execution_trace_json = EXCLUDED.execution_trace_json,
                status = EXCLUDED.status, risk_level = EXCLUDED.risk_level,
                executed_at = NOW(), updated_at = NOW()"#,
        );
        query
            .build()
            .execute(&mut *transaction)
            .await
            .map_err(|error| ProjectApiError::database("保存回归测试结果", error))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交回归测试结果", error))?;

    let passed = results
        .iter()
        .filter(|result| result.status == "passed")
        .count();
    let failed = results
        .iter()
        .filter(|result| result.status == "failed")
        .count();
    let errors = results
        .iter()
        .filter(|result| result.status == "error")
        .count();
    let risk_level = aggregate_risk(results.iter().map(|result| result.status.clone()));
    let selected_source_count = results.len() + duplicate_count;
    let source_coverage_percent =
        source_coverage_percent(selected_source_count, available_source_count);
    Ok(Json(RegressionTestReport {
        baseline_version_id,
        candidate_version_id,
        generated_count: results.len(),
        duplicate_count,
        available_source_count,
        source_coverage_percent,
        passed,
        failed,
        errors,
        risk_level,
        elapsed_ms: started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        cases: results,
    }))
}

async fn run_test_cases(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<DecisionTestCase>>, ProjectApiError> {
    ensure_project_permission(
        &state,
        project_id,
        &claims.sub,
        ProjectPermission::ExecuteAcceptance,
    )
    .await?;
    let cases = execute_version_test_cases(&state, project_id, version_id, false).await?;
    Ok(Json(cases))
}

async fn run_tester_proposal_cases(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<DecisionTestCase>>, ProjectApiError> {
    ensure_project_permission(
        &state,
        project_id,
        &claims.sub,
        ProjectPermission::ExecuteAcceptance,
    )
    .await?;
    execute_version_test_cases(&state, project_id, version_id, true)
        .await
        .map(Json)
}

async fn execute_version_test_cases(
    state: &AppState,
    project_id: i64,
    version_id: i64,
    tester_proposals_only: bool,
) -> Result<Vec<DecisionTestCase>, ProjectApiError> {
    let row = sqlx::query_as::<_, (String, String, Option<i64>, i32)>(
        r#"
        SELECT artifact.dmn_xml, version.status, project.current_draft_version_id,
               version.version_number
        FROM decision_versions version
        JOIN decision_projects project ON project.id = version.project_id
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE version.id = $1 AND version.project_id = $2
        "#,
    )
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取业务验收模型", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if row.2 != Some(version_id) || matches!(row.1.as_str(), "published" | "retired") {
        return Err(ProjectApiError::bad_request(
            "只能运行项目的当前可编辑版本验收案例",
        ));
    }
    let model = crate::dmn::parse_dmn(&row.0)
        .map_err(|error| ProjectApiError::bad_request(format!("DMN XML 解析失败: {error}")))?;
    crate::dmn::compile_dmn(&model)
        .map_err(|error| ProjectApiError::bad_request(format!("DMN 模型编译失败: {error}")))?;
    let cases = sqlx::query_as::<_, DecisionTestCase>(
        r#"
        SELECT id, version_id, name, category, input_json, expected_output_json,
               actual_output_json, execution_trace_json, status, source, approved_by, approved_at,
               created_at, updated_at
        FROM decision_test_cases
        WHERE version_id = $1 AND category = 'business'
          AND ($2 = FALSE OR source = 'tester_proposal')
        ORDER BY created_at, id
        "#,
    )
    .bind(version_id)
    .bind(tester_proposals_only)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取待运行验收案例", error))?;
    if cases.is_empty() {
        return Err(ProjectApiError::bad_request(if tester_proposals_only {
            "请先将至少一个 Tester 提案转为正式业务用例"
        } else {
            "请先添加至少一个业务验收案例"
        }));
    }
    let results = cases
        .iter()
        .map(|test_case| (test_case.id, execute_test_case(&model, row.3, test_case)))
        .collect::<Vec<_>>();
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存业务验收结果", error))?;
    let mut executed_cases = Vec::with_capacity(cases.len());
    let mut execution_status_by_case = HashMap::with_capacity(results.len());
    for (mut test_case, (status, actual, trace)) in cases
        .into_iter()
        .zip(results.into_iter().map(|(_, result)| result))
    {
        execution_status_by_case.insert(test_case.id, status.clone());
        let updated_at = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            r#"
            UPDATE decision_test_cases
            SET actual_output_json = $1, execution_trace_json = $2, status = $3,
                approved_by = NULL, approved_at = NULL, updated_at = NOW()
            WHERE id = $4 AND version_id = $5
            RETURNING updated_at
            "#,
        )
        .bind(&actual)
        .bind(&trace)
        .bind(&status)
        .bind(test_case.id)
        .bind(version_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("更新业务验收结果", error))?;
        test_case.status = status;
        test_case.actual_output_json = Some(actual);
        test_case.execution_trace_json = Some(trace);
        test_case.approved_by = None;
        test_case.approved_at = None;
        test_case.updated_at = updated_at;
        executed_cases.push(test_case);
    }
    let report_json = sqlx::query_scalar::<_, Option<Value>>(
        "SELECT tester_report_json FROM decision_workflows WHERE project_id = $1 FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取 Tester 执行报告", error))?
    .flatten();
    if let Some(report_json) = report_json {
        let mut report = serde_json::from_value::<TesterReport>(report_json)
            .map_err(|error| ProjectApiError::bad_request(format!("Tester 报告无效：{error}")))?;
        if report.schema_version != 1 {
            return Err(ProjectApiError::bad_request("不支持的 Tester 报告版本"));
        }
        apply_tester_execution_statuses(&mut report, &execution_status_by_case);
        sqlx::query("UPDATE decision_workflows SET tester_report_json = $1 WHERE project_id = $2")
            .bind(serde_json::to_value(&report).unwrap_or(Value::Null))
            .bind(project_id)
            .execute(&mut *transaction)
            .await
            .map_err(|error| ProjectApiError::database("保存 Tester 执行报告", error))?;
    }
    invalidate_verification(&mut transaction, version_id).await?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交业务验收结果", error))?;
    Ok(executed_cases)
}

fn apply_tester_execution_statuses(
    report: &mut TesterReport,
    execution_status_by_case: &HashMap<i64, String>,
) {
    for proposal in &mut report.proposals {
        if let Some(status) = proposal
            .test_case_id
            .and_then(|case_id| execution_status_by_case.get(&case_id))
        {
            proposal.execution_status = Some(status.clone());
        }
    }
}

async fn approve_test_case(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id, case_id)): Path<(i64, i64, i64)>,
    Json(payload): Json<ApproveDecisionTestCase>,
) -> Result<Json<DecisionTestCase>, ProjectApiError> {
    ensure_review_permission(&state, project_id, &claims.sub, "批准验收案例").await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("批准业务验收案例", error))?;
    let (current_status, created_by) = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT test.status, version.created_by
        FROM decision_test_cases test
        JOIN decision_versions version ON version.id = test.version_id
        JOIN decision_projects project ON project.id = version.project_id
        WHERE test.id = $1 AND test.version_id = $2 AND version.project_id = $3
              AND project.current_draft_version_id = version.id
              AND version.status NOT IN ('published', 'retired')
        FOR UPDATE
        "#,
    )
    .bind(case_id)
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取待批准验收案例", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if created_by == claims.sub {
        return Err(ProjectApiError::bad_request(
            "版本创建者不能批准自己的业务验收案例",
        ));
    }
    if payload.approved && current_status != "passed" {
        return Err(ProjectApiError::bad_request("只有执行通过的案例可以批准"));
    }
    let test_case = sqlx::query_as::<_, DecisionTestCase>(
        r#"
        UPDATE decision_test_cases
        SET approved_by = CASE WHEN $1 THEN $2 ELSE NULL END,
            approved_at = CASE WHEN $1 THEN NOW() ELSE NULL END,
            updated_at = NOW()
        WHERE id = $3 AND version_id = $4
        RETURNING id, version_id, name, category, input_json, expected_output_json,
                  actual_output_json, execution_trace_json, status, source, approved_by, approved_at,
                  created_at, updated_at
        "#,
    )
    .bind(payload.approved)
    .bind(&claims.sub)
    .bind(case_id)
    .bind(version_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存验收批准状态", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version_id),
        if payload.approved {
            "test_case.approved"
        } else {
            "test_case.approval_revoked"
        },
        &claims.sub,
        if payload.approved {
            "批准业务验收案例"
        } else {
            "撤销业务验收案例批准"
        },
        json!({
            "testCaseId": test_case.id,
            "testCaseName": test_case.name,
            "status": test_case.status,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录验收批准审计事件", error))?;
    invalidate_verification(&mut transaction, version_id).await?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交验收批准状态", error))?;
    Ok(Json(test_case))
}

async fn delete_test_case(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id, case_id)): Path<(i64, i64, i64)>,
) -> Result<StatusCode, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("删除业务验收案例", error))?;
    let deleted = sqlx::query(
        r#"
        DELETE FROM decision_test_cases test
        USING decision_versions version, decision_projects project
        WHERE test.id = $1 AND test.version_id = $2
              AND version.id = test.version_id AND version.project_id = $3
              AND project.id = version.project_id
              AND project.current_draft_version_id = version.id
              AND version.status NOT IN ('published', 'retired')
        "#,
    )
    .bind(case_id)
    .bind(version_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("删除业务验收案例", error))?;
    if deleted.rows_affected() == 0 {
        return Err(ProjectApiError::not_found());
    }
    invalidate_verification(&mut transaction, version_id).await?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交验收案例删除", error))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn verify_project_version(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
) -> Result<Json<VersionVerificationReport>, ProjectApiError> {
    // Automatic verification executes the current model and writes its gate
    // snapshot. Both modelers and reviewers may run acceptance; review
    // approval remains protected by the Review permission below.
    ensure_project_permission(
        &state,
        project_id,
        &claims.sub,
        ProjectPermission::ExecuteAcceptance,
    )
    .await?;
    let row = sqlx::query_as::<
        _,
        (
            String,
            Option<serde_json::Value>,
            Option<String>,
            Option<chrono::DateTime<chrono::Utc>>,
            String,
            Option<i64>,
        ),
    >(
        r#"
        SELECT artifact.dmn_xml, artifact.change_review_json, version.reviewed_by,
               version.reviewed_at, version.status, project.current_draft_version_id
        FROM decision_versions version
        JOIN decision_projects project ON project.id = version.project_id
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE version.id = $1 AND version.project_id = $2
        "#,
    )
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取待验收版本", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (dmn_xml, review_json, reviewed_by, reviewed_at, status, current_draft_version_id) = row;
    if current_draft_version_id != Some(version_id) {
        return Err(ProjectApiError::bad_request("只能验收项目的当前草稿版本"));
    }
    if matches!(status.as_str(), "published" | "retired") {
        return Err(ProjectApiError::bad_request(
            "已发布或已退役版本的验收快照不可修改",
        ));
    }
    let review = review_json
        .map(serde_json::from_value::<VersionChangeReview>)
        .transpose()
        .map_err(|error| ProjectApiError::bad_request(format!("版本审查快照无效：{error}")))?;
    let mut report = verify_version(
        &dmn_xml,
        version_id,
        review.as_ref(),
        reviewed_by.is_some() && reviewed_at.is_some(),
    )
    .map_err(ProjectApiError::bad_request)?;
    let (business_total, business_passed, business_approved) =
        test_case_counts(&state.db, version_id)
            .await
            .map_err(|error| ProjectApiError::database("汇总业务验收案例", error))?;
    let business_gate =
        business_acceptance_gate(business_total, business_passed, business_approved);
    if business_gate.status == "failed" {
        report.status = "failed".into();
    }
    report.gates.push(business_gate);
    let report_json = serde_json::to_value(&report)
        .map_err(|error| ProjectApiError::bad_request(format!("验收报告序列化失败：{error}")))?;
    let next_status = if report.status == "passed" {
        "validated"
    } else {
        "draft"
    };
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存版本验收结果", error))?;
    sqlx::query(
        "UPDATE decision_artifacts SET verification_report_json = $1 WHERE version_id = $2",
    )
    .bind(report_json)
    .bind(version_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存版本验收报告", error))?;
    sqlx::query(
        "UPDATE decision_versions SET status = $1 WHERE id = $2 AND status NOT IN ('published', 'retired')",
    )
    .bind(next_status)
    .bind(version_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新版本验收状态", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version_id),
        "version.verified",
        &claims.sub,
        if report.status == "passed" {
            "版本自动验收通过"
        } else {
            "版本自动验收未通过"
        },
        json!({
            "status": report.status,
            "gateCount": report.gates.len(),
            "failedGateCount": report.gates.iter().filter(|gate| gate.status == "failed").count(),
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录验收审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交版本验收结果", error))?;
    Ok(Json(report))
}

async fn review_project_version(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
) -> Result<Json<DecisionVersion>, ProjectApiError> {
    // Keep nonexistent projects indistinguishable from inaccessible projects,
    // while giving viewers a useful permission error for the review action.
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    ensure_review_permission(&state, project_id, &claims.sub, "确认高风险变更").await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("独立复核版本", error))?;
    let version = sqlx::query_as::<_, DecisionVersion>(
        r#"
        UPDATE decision_versions version
        SET reviewed_by = $1, reviewed_at = NOW()
        FROM decision_projects project
        WHERE version.id = $2 AND version.project_id = $3 AND project.id = version.project_id
          AND project.current_draft_version_id = version.id
          AND version.status NOT IN ('published', 'retired')
          AND version.created_by <> $1
        RETURNING version.id, version.project_id, version.version_number, version.status,
                  version.base_version_id, version.rollback_source_version_id,
                  version.rollback_reason, version.change_summary, version.created_by,
                  version.created_at, version.reviewed_by, version.reviewed_at
        "#,
    )
    .bind(&claims.sub)
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存独立复核", error))?
    .ok_or_else(|| {
        ProjectApiError::bad_request("只能复核当前未发布草稿，且复核人不能是版本创建者")
    })?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version_id),
        "version.reviewed",
        &claims.sub,
        &format!("独立复核决策版本 v{}", version.version_number),
        json!({"versionNumber": version.version_number, "createdBy": version.created_by}),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录独立复核审计事件", error))?;
    // The previous verification snapshot was calculated before this review.
    // Invalidate it so a refresh cannot keep showing the stale high-risk gate.
    invalidate_verification(&mut transaction, version.id).await?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交独立复核", error))?;
    Ok(Json(version))
}

async fn publish_project_version(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, version_id)): Path<(i64, i64)>,
    Json(payload): Json<PublishDecisionVersion>,
) -> Result<Json<DecisionVersion>, ProjectApiError> {
    validate_publish(&payload)?;
    ensure_publish_permission(&state, project_id, &claims.sub).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("发布决策版本", error))?;
    let row = sqlx::query_as::<
        _,
        (
            String,
            Option<serde_json::Value>,
            Option<String>,
            Option<chrono::DateTime<chrono::Utc>>,
            Option<i64>,
            Option<i64>,
            String,
        ),
    >(
        r#"
        SELECT version.status, artifact.verification_report_json, version.reviewed_by,
               version.reviewed_at, version.base_version_id,
               project.current_draft_version_id, version.created_by
        FROM decision_versions version
        JOIN decision_projects project ON project.id = version.project_id
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE version.id = $1 AND version.project_id = $2
        FOR UPDATE
        "#,
    )
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取待发布版本", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (
        status,
        report_json,
        reviewed_by,
        reviewed_at,
        base_version_id,
        current_draft_version_id,
        created_by,
    ) = row;
    if status == "retired" {
        return Err(ProjectApiError::bad_request("已退役版本不能重新发布"));
    }
    if current_draft_version_id != Some(version_id) {
        return Err(ProjectApiError::bad_request("只能发布项目的当前草稿版本"));
    }
    if !matches!(status.as_str(), "validated" | "published") {
        return Err(ProjectApiError::bad_request("当前版本尚未通过自动验收"));
    }
    if reviewed_by.is_none() || reviewed_at.is_none() {
        return Err(ProjectApiError::bad_request("版本尚未完成独立复核"));
    }
    if created_by == claims.sub {
        return Err(ProjectApiError::forbidden(
            "当前账号是版本创建者，不能兼任审批人；请使用其他拥有 approver 角色的账号发布",
        ));
    }
    if reviewed_by.as_deref() == Some(claims.sub.as_str()) {
        return Err(ProjectApiError::forbidden(
            "当前账号已执行独立复核，不能兼任审批人；请由其他拥有 approver 角色的账号发布",
        ));
    }
    let report = report_json
        .map(serde_json::from_value::<VersionVerificationReport>)
        .transpose()
        .map_err(|error| ProjectApiError::bad_request(format!("版本验收报告无效：{error}")))?
        .ok_or_else(|| ProjectApiError::bad_request("请先执行自动验收"))?;
    if report.version_id != version_id {
        return Err(ProjectApiError::bad_request("验收报告与待发布版本不匹配"));
    }
    if report.status != "passed" || report.gates.iter().any(|gate| gate.status == "failed") {
        return Err(ProjectApiError::bad_request(
            "发布门禁未通过，请先修复失败项并重新验收",
        ));
    }
    if historical_regression_gate_enabled()
        && let Some(baseline_version_id) = base_version_id
    {
        let (total, passed) = sqlx::query_as::<_, (i64, i64)>(
            r#"SELECT COUNT(*)::BIGINT,
                      COUNT(*) FILTER (WHERE status = 'passed')::BIGINT
               FROM regression_test_sources
               WHERE candidate_version_id = $1 AND source_version_id = $2"#,
        )
        .bind(version_id)
        .bind(baseline_version_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("检查历史回归发布门禁", error))?;
        if let Some(message) = historical_regression_gate_error(total, passed) {
            return Err(ProjectApiError::bad_request(message));
        }
    }
    let (business_total, business_passed, business_approved) =
        test_case_counts(&mut *transaction, version_id)
            .await
            .map_err(|error| ProjectApiError::database("复核业务验收案例", error))?;
    if business_acceptance_gate(business_total, business_passed, business_approved).status
        == "failed"
    {
        return Err(ProjectApiError::bad_request(
            "业务验收案例未全部通过并完成人工批准",
        ));
    }

    let previous_published = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT published_version_id FROM decision_projects WHERE id = $1 FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取当前发布版本", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if let Some(previous_id) = previous_published.filter(|id| *id != version_id) {
        let previous_number = sqlx::query_scalar::<_, i32>(
            "UPDATE decision_versions SET status = 'retired' WHERE id = $1 RETURNING version_number",
        )
            .bind(previous_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|error| ProjectApiError::database("退役旧发布版本", error))?;
        insert_audit_event(
            &mut *transaction,
            project_id,
            Some(previous_id),
            "version.retired",
            &claims.sub,
            &format!("退役决策版本 v{previous_number}"),
            json!({
                "versionNumber": previous_number,
                "replacedByVersionId": version_id,
            }),
        )
        .await
        .map_err(|error| ProjectApiError::database("记录版本退役审计事件", error))?;
    }
    let version = sqlx::query_as::<_, DecisionVersion>(
        r#"
        UPDATE decision_versions
        SET status = 'published'
        WHERE id = $1 AND project_id = $2
        RETURNING id, project_id, version_number, status, base_version_id,
                  rollback_source_version_id, rollback_reason, change_summary,
                  created_by, created_at, reviewed_by, reviewed_at
        "#,
    )
    .bind(version_id)
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("固化发布版本", error))?;
    sqlx::query(
        "UPDATE decision_projects SET published_version_id = $1, updated_at = NOW() WHERE id = $2",
    )
    .bind(version_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新项目发布版本", error))?;
    let withdrawn_proposals = sqlx::query_as::<_, (i64, i64)>(
        r#"
        UPDATE decision_optimization_proposals
        SET status = 'withdrawn', resolved_at = NOW(),
            resolution_note = '自动撤回：项目发布版本已变化'
        WHERE project_id = $1 AND source_version_id <> $2 AND status = 'open'
        RETURNING id, source_version_id
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("撤回旧发布版本的优化建议", error))?;
    if !withdrawn_proposals.is_empty() {
        insert_audit_event(
            &mut *transaction,
            project_id,
            Some(version_id),
            "optimization_proposal.withdrawn",
            &claims.sub,
            "新版本发布后撤回基于旧版本的待处理优化建议",
            json!({
                "newSourceVersionId": version_id,
                "withdrawnProposalCount": withdrawn_proposals.len(),
                "withdrawnProposalIds": withdrawn_proposals.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            }),
        )
        .await
        .map_err(|error| ProjectApiError::database("记录旧优化建议撤回审计事件", error))?;
    }
    let (owner, project_name, category, description, dsl_yaml) =
        sqlx::query_as::<_, (String, String, String, String, String)>(
            r#"
        SELECT project.owner_username, project.name, ''::text,
               project.description, artifact.dsl_yaml
        FROM decision_projects project
        JOIN decision_artifacts artifact ON artifact.version_id = $1
        WHERE project.id = $2
        "#,
        )
        .bind(version_id)
        .bind(project_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("提取已发布决策模式", error))?;
    let (pattern_type, keywords) = pattern_features(
        &format!("{description}\n{dsl_yaml}"),
        &category,
        &project_name,
    );
    let dsl_snippet = dsl_yaml.chars().take(12_000).collect::<String>();
    let supersedes = sqlx::query_as::<_, (i64, i32)>(
        r#"
        SELECT id, evolution_number
        FROM decision_patterns
        WHERE source_project_id = $1 AND source_version_id <> $2
        ORDER BY evolution_number DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取模式演进链", error))?;
    let (supersedes_pattern_id, evolution_number) = supersedes
        .map(|(pattern_id, number)| (Some(pattern_id), number.saturating_add(1)))
        .unwrap_or((None, 1));
    sqlx::query(
        r#"
        INSERT INTO decision_patterns (
            owner_username, source_project_id, source_version_id, pattern_type,
            category, name, description, keywords, dsl_snippet,
            supersedes_pattern_id, evolution_number
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (source_version_id) DO UPDATE SET
            pattern_type = EXCLUDED.pattern_type,
            category = EXCLUDED.category,
            name = EXCLUDED.name,
            description = EXCLUDED.description,
            keywords = EXCLUDED.keywords,
            dsl_snippet = EXCLUDED.dsl_snippet,
            supersedes_pattern_id = EXCLUDED.supersedes_pattern_id,
            evolution_number = EXCLUDED.evolution_number
        "#,
    )
    .bind(owner)
    .bind(project_id)
    .bind(version_id)
    .bind(pattern_type)
    .bind(category)
    .bind(project_name)
    .bind(description)
    .bind(keywords)
    .bind(dsl_snippet)
    .bind(supersedes_pattern_id)
    .bind(evolution_number)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存已发布决策模式", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version_id),
        "version.published",
        &claims.sub,
        &format!("发布决策版本 v{}", version.version_number),
        json!({
            "versionNumber": version.version_number,
            "previousPublishedVersionId": previous_published,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录发布审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交版本发布", error))?;
    Ok(Json(version))
}

async fn recommend_decision_patterns(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<PatternRecommendationQuery>,
) -> Result<Json<Vec<PatternRecommendation>>, ProjectApiError> {
    let query_text = validate_pattern_query(&query.q).map_err(ProjectApiError::bad_request)?;
    if let Some(project_id) = query.project_id {
        ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    }
    let query_terms = semantic_terms(query_text);
    let vector_candidates = crate::rag::patterns::semantic_candidates(
        &state.db,
        &claims.sub,
        query_text,
        &crate::rag::config::EmbeddingConfig::from_env(),
    )
    .await;
    let vector_scores = vector_candidates.iter().copied().collect::<HashMap<_, _>>();
    let rows = sqlx::query_as::<_, (i64, i64, String, i64, i32, String, String, String, String, Vec<String>, Option<f64>, i32, i32, String)>(
        r#"
        WITH keyword_candidates AS (
            SELECT pattern.id, pattern.created_at, TRUE AS keyword_overlap
            FROM decision_patterns pattern
            JOIN decision_versions version ON version.id = pattern.source_version_id
            WHERE pattern.owner_username = $1
              AND version.status IN ('published', 'retired')
              AND pattern.keywords && $2::TEXT[]
            ORDER BY pattern.created_at DESC, pattern.id DESC
            LIMIT 200
        ), recent_fallback AS (
            SELECT pattern.id, pattern.created_at, FALSE AS keyword_overlap
            FROM decision_patterns pattern
            JOIN decision_versions version ON version.id = pattern.source_version_id
            WHERE pattern.owner_username = $1
              AND version.status IN ('published', 'retired')
              AND NOT (pattern.keywords && $2::TEXT[])
            ORDER BY pattern.created_at DESC, pattern.id DESC
            LIMIT 200
        ), vector_candidates AS (
            SELECT pattern.id, pattern.created_at, FALSE AS keyword_overlap
            FROM decision_patterns pattern
            WHERE pattern.id = ANY($3::BIGINT[])
        ), candidate_union AS (
            SELECT id, created_at, keyword_overlap FROM keyword_candidates
            UNION ALL
            SELECT id, created_at, keyword_overlap FROM recent_fallback
            UNION ALL
            SELECT id, created_at, keyword_overlap FROM vector_candidates
        ), candidates AS (
            SELECT id, MAX(created_at) AS created_at, BOOL_OR(keyword_overlap) AS keyword_overlap
            FROM candidate_union
            GROUP BY id
        )
        SELECT pattern.id, project.id, project.name, version.id, version.version_number,
               pattern.pattern_type, pattern.name, pattern.category, pattern.description, pattern.keywords,
               pattern.success_rate::FLOAT8, pattern.usage_count, pattern.evolution_number, pattern.dsl_snippet
        FROM decision_patterns pattern
        JOIN candidates candidate ON candidate.id = pattern.id
        JOIN decision_projects project ON project.id = pattern.source_project_id
        JOIN decision_versions version ON version.id = pattern.source_version_id
        ORDER BY candidate.keyword_overlap DESC, candidate.created_at DESC, candidate.id DESC
        "#,
    )
    .bind(&claims.sub)
    .bind(query_terms)
    .bind(
        vector_candidates
            .iter()
            .map(|(pattern_id, _)| *pattern_id)
            .collect::<Vec<_>>(),
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("检索决策模式", error))?;
    let mut matches = rows
        .into_iter()
        .map(
            |(
                pattern_id,
                project_id,
                project_name,
                version_id,
                version_number,
                pattern_type,
                name,
                category,
                description,
                keywords,
                success_rate,
                usage_count,
                evolution_number,
                dsl_snippet,
            )| {
                let (lexical_score, lexical_reason) = semantic_pattern_match_score(
                    query_text,
                    &keywords,
                    &category,
                    &pattern_type,
                    &dsl_snippet,
                );
                let vector_score = vector_scores.get(&pattern_id).copied().unwrap_or(0.0);
                let match_score = lexical_score.max(vector_score);
                let recommendation_reason = if vector_score > lexical_score {
                    format!("语义向量相似度排名靠前；{lexical_reason}")
                } else {
                    lexical_reason
                };
                PatternRecommendation {
                    recommendation_id: 0,
                    pattern_id,
                    source_project_id: project_id,
                    source_project_name: project_name,
                    source_version_id: version_id,
                    source_version_number: version_number,
                    pattern_type,
                    name,
                    description,
                    match_score,
                    recommendation_reason,
                    success_rate,
                    usage_count,
                    evolution_number,
                }
            },
        )
        .filter(|item| item.match_score >= 12.0)
        .collect::<Vec<_>>();
    sort_pattern_recommendations(&mut matches);
    matches.truncate(10);
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("记录模式推荐", error))?;
    for item in &mut matches {
        item.recommendation_id = sqlx::query_scalar(
            r#"
            INSERT INTO pattern_recommendations (
                owner_username, project_id, recommended_pattern_id, match_score, recommendation_reason
            ) VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(&claims.sub)
        .bind(query.project_id)
        .bind(item.pattern_id)
        .bind(item.match_score)
        .bind(&item.recommendation_reason)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("记录模式推荐", error))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交模式推荐记录", error))?;
    Ok(Json(matches))
}

async fn debug_decision_pattern_search(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<PatternRecommendationQuery>,
) -> Result<Json<PatternSearchDiagnostics>, ProjectApiError> {
    let query_text = validate_pattern_query(&query.q).map_err(ProjectApiError::bad_request)?;
    if let Some(project_id) = query.project_id {
        ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    }
    let query_terms = semantic_terms(query_text);
    let keyword_candidate_ids = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT pattern.id
        FROM decision_patterns pattern
        JOIN decision_versions version ON version.id = pattern.source_version_id
        WHERE pattern.owner_username = $1
          AND version.status IN ('published', 'retired')
          AND pattern.keywords && $2::TEXT[]
        ORDER BY pattern.created_at DESC, pattern.id DESC
        LIMIT 200
        "#,
    )
    .bind(&claims.sub)
    .bind(query_terms)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取关键词模式候选", error))?;
    let vector_candidates = crate::rag::patterns::semantic_candidates(
        &state.db,
        &claims.sub,
        query_text,
        &crate::rag::config::EmbeddingConfig::from_env(),
    )
    .await;
    let rrf_candidate_ids = fuse_pattern_candidate_ids(&keyword_candidate_ids, &vector_candidates);
    Ok(Json(PatternSearchDiagnostics {
        keyword_candidate_ids,
        vector_candidates,
        rrf_candidate_ids,
    }))
}

fn fuse_pattern_candidate_ids(keyword_ids: &[i64], vector_candidates: &[(i64, f64)]) -> Vec<i64> {
    let mut scores = HashMap::<i64, f64>::new();
    for (rank, pattern_id) in keyword_ids.iter().enumerate() {
        *scores.entry(*pattern_id).or_default() += 1.0 / (60.0 + (rank + 1) as f64);
    }
    for (rank, (pattern_id, _)) in vector_candidates.iter().enumerate() {
        *scores.entry(*pattern_id).or_default() += 1.0 / (60.0 + (rank + 1) as f64);
    }
    let mut ranked = scores.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_id, left_score), (right_id, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left_id.cmp(right_id))
    });
    ranked.into_iter().take(200).map(|(id, _)| id).collect()
}

fn sort_pattern_recommendations(matches: &mut [PatternRecommendation]) {
    matches.sort_by(|left, right| {
        right
            .match_score
            .total_cmp(&left.match_score)
            .then_with(|| left.pattern_id.cmp(&right.pattern_id))
    });
}

async fn submit_pattern_recommendation_feedback(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(recommendation_id): Path<i64>,
    Json(payload): Json<PatternRecommendationFeedback>,
) -> Result<StatusCode, ProjectApiError> {
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存模式推荐反馈", error))?;
    let pattern_id = sqlx::query_scalar::<_, i64>(
        r#"
        UPDATE pattern_recommendations
        SET user_accepted = $1
        WHERE id = $2 AND owner_username = $3
        RETURNING recommended_pattern_id
        "#,
    )
    .bind(payload.accepted)
    .bind(recommendation_id)
    .bind(&claims.sub)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存模式推荐反馈", error))?;
    let Some(pattern_id) = pattern_id else {
        return Err(ProjectApiError::not_found());
    };
    sqlx::query(
        r#"
        UPDATE decision_patterns pattern
        SET success_rate = stats.success_rate
        FROM (
            SELECT recommended_pattern_id,
                   ROUND(
                       100.0 * COUNT(*) FILTER (WHERE user_accepted = TRUE)
                       / NULLIF(COUNT(*) FILTER (WHERE user_accepted IS NOT NULL), 0),
                       2
                   ) AS success_rate
            FROM pattern_recommendations
            WHERE recommended_pattern_id = $1 AND owner_username = $2
            GROUP BY recommended_pattern_id
        ) stats
        WHERE pattern.id = stats.recommended_pattern_id
          AND pattern.owner_username = $2
        "#,
    )
    .bind(pattern_id)
    .bind(&claims.sub)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新模式成功率", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交模式推荐反馈", error))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_decision_pattern_lineage(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(version_id): Path<i64>,
) -> Result<Json<Vec<PatternLineageItem>>, ProjectApiError> {
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            i64,
            i32,
            String,
            String,
            String,
            String,
            String,
            Option<f64>,
            i32,
            i32,
            Option<i64>,
            chrono::DateTime<chrono::Utc>,
        ),
    >(
        r#"
        WITH RECURSIVE pattern_lineage AS (
            SELECT pattern.id, pattern.source_project_id, pattern.source_version_id,
                   pattern.pattern_type, pattern.category, pattern.name,
                   pattern.description, pattern.success_rate::FLOAT8,
                   pattern.usage_count, pattern.evolution_number,
                   pattern.supersedes_pattern_id, pattern.created_at,
                   ARRAY[pattern.id]::BIGINT[]
            FROM decision_patterns pattern
            WHERE pattern.source_version_id = $1
              AND pattern.owner_username = $2
            UNION ALL
            SELECT parent.id, parent.source_project_id, parent.source_version_id,
                   parent.pattern_type, parent.category, parent.name,
                   parent.description, parent.success_rate::FLOAT8,
                   parent.usage_count, parent.evolution_number,
                   parent.supersedes_pattern_id, parent.created_at,
                   child.path || parent.id
            FROM decision_patterns parent
            JOIN pattern_lineage child ON child.supersedes_pattern_id = parent.id
            WHERE parent.owner_username = $2
              AND NOT parent.id = ANY(child.path)
        )
        SELECT lineage.id, lineage.source_project_id, project.name,
               lineage.source_version_id, version.version_number, version.status,
               lineage.pattern_type, lineage.category, lineage.name,
               lineage.description, lineage.success_rate, lineage.usage_count,
               lineage.evolution_number, lineage.supersedes_pattern_id,
               lineage.created_at
        FROM pattern_lineage lineage
        JOIN decision_projects project ON project.id = lineage.source_project_id
        JOIN decision_versions version ON version.id = lineage.source_version_id
        ORDER BY lineage.evolution_number ASC, lineage.id ASC
        LIMIT 100
        "#,
    )
    .bind(version_id)
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取模式演进链", error))?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    pattern_id,
                    source_project_id,
                    source_project_name,
                    source_version_id,
                    source_version_number,
                    version_status,
                    pattern_type,
                    category,
                    name,
                    description,
                    success_rate,
                    usage_count,
                    evolution_number,
                    supersedes_pattern_id,
                    created_at,
                )| PatternLineageItem {
                    pattern_id,
                    source_project_id,
                    source_project_name,
                    source_version_id,
                    source_version_number,
                    version_status,
                    pattern_type,
                    category,
                    name,
                    description,
                    success_rate,
                    usage_count,
                    evolution_number,
                    supersedes_pattern_id,
                    created_at,
                },
            )
            .collect(),
    ))
}

async fn get_pattern_library_analytics(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<PatternLibraryAnalytics>, ProjectApiError> {
    let (total_patterns, total_usage_count, average_success_rate) =
        sqlx::query_as::<_, (i64, i64, Option<f64>)>(
            r#"
            SELECT COUNT(*)::BIGINT,
                   COALESCE(SUM(usage_count), 0)::BIGINT,
                   AVG(success_rate)::FLOAT8
            FROM decision_patterns
            WHERE owner_username = $1
            "#,
        )
        .bind(&claims.sub)
        .fetch_one(&state.db)
        .await
        .map_err(|error| ProjectApiError::database("读取模式统计", error))?;
    let (
        total_recommendations,
        accepted_recommendations,
        rejected_recommendations,
        pending_recommendations,
        average_match_score,
    ) = sqlx::query_as::<_, (i64, i64, i64, i64, Option<f64>)>(
        r#"
        SELECT COUNT(*)::BIGINT,
               COUNT(*) FILTER (WHERE user_accepted IS TRUE)::BIGINT,
               COUNT(*) FILTER (WHERE user_accepted IS FALSE)::BIGINT,
               COUNT(*) FILTER (WHERE user_accepted IS NULL)::BIGINT,
               AVG(match_score)::FLOAT8
        FROM pattern_recommendations
        WHERE owner_username = $1
        "#,
    )
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取模式推荐统计", error))?;
    let top_patterns = sqlx::query_as::<
        _,
        (
            i64,
            String,
            i64,
            i64,
            i32,
            i32,
            Option<f64>,
            Option<chrono::DateTime<chrono::Utc>>,
        ),
    >(
        r#"
        SELECT id, name, source_project_id, source_version_id, evolution_number,
               usage_count, success_rate::FLOAT8, last_used
        FROM decision_patterns
        WHERE owner_username = $1
        ORDER BY usage_count DESC, success_rate DESC NULLS LAST,
                 evolution_number DESC, id DESC
        LIMIT 10
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取高复用模式", error))?
    .into_iter()
    .map(
        |(
            pattern_id,
            name,
            source_project_id,
            source_version_id,
            evolution_number,
            usage_count,
            success_rate,
            last_used,
        )| PatternUsageMetric {
            pattern_id,
            name,
            source_project_id,
            source_version_id,
            evolution_number,
            usage_count,
            success_rate,
            last_used,
        },
    )
    .collect::<Vec<_>>();
    let acceptance_rate =
        calculate_pattern_acceptance_rate(accepted_recommendations, rejected_recommendations);

    Ok(Json(PatternLibraryAnalytics {
        total_patterns,
        total_usage_count,
        average_success_rate,
        total_recommendations,
        accepted_recommendations,
        rejected_recommendations,
        pending_recommendations,
        acceptance_rate,
        average_match_score,
        top_patterns,
    }))
}

fn calculate_pattern_acceptance_rate(accepted: i64, rejected: i64) -> Option<f64> {
    let feedback_count = accepted + rejected;
    (feedback_count > 0)
        .then(|| (accepted as f64 * 100.0 / feedback_count as f64 * 100.0).round() / 100.0)
}

async fn load_published_artifact(
    state: &AppState,
    username: &str,
    project_id: i64,
) -> Result<Option<(i64, i32, String)>, ProjectApiError> {
    ensure_project_permission(state, project_id, username, ProjectPermission::View).await?;
    let row = sqlx::query_as::<_, (Option<i64>, Option<i32>, Option<String>)>(
        r#"
        SELECT project.published_version_id, version.version_number, artifact.dmn_xml
        FROM decision_projects project
        LEFT JOIN decision_versions version
               ON version.id = project.published_version_id AND version.status = 'published'
        LEFT JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE project.id = $1
        "#,
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取已发布决策服务", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    match row {
        (Some(version_id), Some(version_number), Some(xml)) => {
            Ok(Some((version_id, version_number, xml)))
        }
        _ => Ok(None),
    }
}

fn service_field(name: String, type_ref: Option<&str>, source: &str) -> DecisionServiceField {
    DecisionServiceField {
        name,
        type_ref: type_ref.unwrap_or("Any").to_string(),
        source: source.into(),
    }
}

fn collect_required_inputs(
    model: &crate::dmn::DmnModel,
    decision_id: &str,
    visited: &mut HashSet<String>,
    input_ids: &mut HashSet<String>,
) {
    if !visited.insert(decision_id.to_string()) {
        return;
    }
    let Some(decision) = model
        .decisions
        .iter()
        .find(|decision| decision.id == decision_id)
    else {
        return;
    };
    for requirement in &decision.requirements {
        if let Some(input_id) = &requirement.required_input {
            input_ids.insert(input_id.clone());
        }
        if let Some(required_decision) = &requirement.required_decision {
            collect_required_inputs(model, required_decision, visited, input_ids);
        }
    }
}

/// Models generated without an explicit DMN decisionService are still
/// callable: expose each terminal decision as a small default service.
fn ensure_default_decision_services(model: &mut crate::dmn::DmnModel) {
    if !model.decision_services.is_empty() || model.decisions.is_empty() {
        return;
    }
    let referenced_decisions = model
        .decisions
        .iter()
        .flat_map(|decision| {
            decision
                .requirements
                .iter()
                .filter_map(|requirement| requirement.required_decision.as_deref())
        })
        .collect::<HashSet<_>>();
    let terminal_decisions = model
        .decisions
        .iter()
        .filter(|decision| !referenced_decisions.contains(decision.id.as_str()))
        .map(|decision| (decision.id.clone(), decision.name.clone()))
        .collect::<Vec<_>>();
    let decisions = if terminal_decisions.is_empty() {
        model
            .decisions
            .iter()
            .map(|decision| (decision.id.clone(), decision.name.clone()))
            .collect::<Vec<_>>()
    } else {
        terminal_decisions
    };
    for (decision_id, decision_name) in decisions {
        let mut visited = HashSet::new();
        let mut input_ids = HashSet::new();
        collect_required_inputs(model, &decision_id, &mut visited, &mut input_ids);
        let input_data = model
            .input_data
            .iter()
            .filter(|input| input_ids.contains(&input.id))
            .map(|input| input.id.clone())
            .collect();
        model.decision_services.push(crate::dmn::DecisionService {
            id: format!("__default_service_{decision_id}"),
            name: format!("{decision_name}（默认服务）"),
            output_decisions: vec![decision_id],
            encapsulated_decisions: Vec::new(),
            input_decisions: Vec::new(),
            input_data,
        });
    }
}

fn published_service_contracts(
    model: &crate::dmn::DmnModel,
    project_id: i64,
    version_id: i64,
    version_number: i32,
) -> Vec<PublishedDecisionService> {
    model
        .decision_services
        .iter()
        .map(|service| {
            let mut inputs = Vec::new();
            for id in &service.input_data {
                if let Some(input) = model.input_data.iter().find(|input| input.id == *id) {
                    inputs.push(service_field(
                        input.variable.clone().unwrap_or_else(|| input.name.clone()),
                        input.type_ref.as_deref(),
                        "input_data",
                    ));
                }
            }
            for id in &service.input_decisions {
                if let Some(decision) = model.decisions.iter().find(|decision| decision.id == *id) {
                    inputs.push(service_field(
                        decision
                            .variable
                            .clone()
                            .unwrap_or_else(|| decision.name.clone()),
                        decision.type_ref.as_deref(),
                        "input_decision",
                    ));
                }
            }
            let outputs = service
                .output_decisions
                .iter()
                .filter_map(|id| {
                    model
                        .decisions
                        .iter()
                        .find(|decision| decision.id == *id)
                        .map(|decision| {
                            service_field(
                                decision.name.clone(),
                                decision.type_ref.as_deref(),
                                "output_decision",
                            )
                        })
                })
                .collect();
            PublishedDecisionService {
                project_id,
                version_id,
                version_number,
                id: service.id.clone(),
                name: service.name.clone(),
                inputs,
                outputs,
            }
        })
        .collect()
}

async fn list_published_decision_services(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<PublishedDecisionService>>, ProjectApiError> {
    let Some((version_id, version_number, xml)) =
        load_published_artifact(&state, &claims.sub, project_id).await?
    else {
        return Ok(Json(Vec::new()));
    };
    let mut model = crate::dmn::parse_dmn(&xml)
        .map_err(|error| ProjectApiError::bad_request(format!("已发布 DMN 解析失败: {error}")))?;
    ensure_default_decision_services(&mut model);
    crate::dmn::compile_dmn(&model)
        .map_err(|error| ProjectApiError::bad_request(format!("已发布 DMN 编译失败: {error}")))?;
    Ok(Json(published_service_contracts(
        &model,
        project_id,
        version_id,
        version_number,
    )))
}

fn service_output(
    model: &crate::dmn::DmnModel,
    service: &crate::dmn::DecisionService,
    result: &crate::dmn::DmnResult,
) -> Value {
    Value::Object(
        service
            .output_decisions
            .iter()
            .filter_map(|id| {
                let decision = model.decisions.iter().find(|decision| decision.id == *id)?;
                let evaluated = result
                    .decision_results
                    .iter()
                    .find(|evaluated| evaluated.decision_id == *id)?;
                let value =
                    crate::version_verification::feel_value_to_json(&evaluated.value).ok()?;
                Some((decision.name.clone(), value))
            })
            .collect(),
    )
}

fn hash_service_api_key(secret: &str) -> Result<String, ProjectApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| {
            ProjectApiError::coded(
                StatusCode::INTERNAL_SERVER_ERROR,
                "KEY_HASH",
                "生成服务凭证失败",
            )
        })
}

fn verify_service_api_key(hash: &str, secret: &str) -> bool {
    PasswordHash::new(hash).ok().is_some_and(|parsed| {
        Argon2::default()
            .verify_password(secret.as_bytes(), &parsed)
            .is_ok()
    })
}

async fn list_decision_service_api_keys(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DecisionServiceApiKey>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let keys = sqlx::query_as::<_, DecisionServiceApiKey>(
        r#"
        SELECT id, project_id, service_id, name, key_prefix, created_by, created_at,
               last_used_at, expires_at, revoked_at
        FROM decision_service_api_keys
        WHERE project_id = $1 AND revoked_at IS NULL
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取服务调用凭证", error))?;
    Ok(Json(keys))
}

async fn create_decision_service_api_key(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<CreateDecisionServiceApiKey>,
) -> Result<Json<CreatedDecisionServiceApiKey>, ProjectApiError> {
    ensure_project_permission(
        &state,
        project_id,
        &claims.sub,
        ProjectPermission::ManageService,
    )
    .await?;
    let name = payload.name.trim();
    let service_id = payload.service_id.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(ProjectApiError::bad_request(
            "凭证名称不能为空且不能超过 120 个字符",
        ));
    }
    if service_id.is_empty() || service_id.chars().count() > 255 {
        return Err(ProjectApiError::bad_request("决策服务标识无效"));
    }
    if payload
        .expires_at
        .is_some_and(|expires| expires <= chrono::Utc::now())
    {
        return Err(ProjectApiError::bad_request("凭证过期时间必须晚于当前时间"));
    }
    let active_keys = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM decision_service_api_keys WHERE project_id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > NOW())",
    )
    .bind(project_id)
    .fetch_one(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("检查服务调用凭证数量", error))?;
    if active_keys >= 50 {
        return Err(ProjectApiError::conflict(
            "每个项目最多保留 50 个有效调用凭证，请先撤销不用的凭证",
        ));
    }
    let Some((_version_id, _version_number, xml)) =
        load_published_artifact(&state, &claims.sub, project_id).await?
    else {
        return Err(ProjectApiError::bad_request("项目尚未发布决策版本"));
    };
    let mut model = crate::dmn::parse_dmn(&xml)
        .map_err(|error| ProjectApiError::bad_request(format!("已发布 DMN 解析失败: {error}")))?;
    ensure_default_decision_services(&mut model);
    let _service = model
        .decision_services
        .iter()
        .find(|service| service.id == service_id)
        .ok_or_else(|| ProjectApiError::bad_request("已发布版本中不存在该决策服务"))?;
    let secret = format!("dmn_live_{}", Uuid::new_v4().simple());
    let key_prefix = secret.chars().take(20).collect::<String>();
    let key_hash = hash_service_api_key(&secret)?;
    let key = sqlx::query_as::<_, DecisionServiceApiKey>(
        r#"
        INSERT INTO decision_service_api_keys
            (project_id, service_id, name, key_prefix, key_hash, created_by, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, project_id, service_id, name, key_prefix, created_by, created_at,
                  last_used_at, expires_at, revoked_at
        "#,
    )
    .bind(project_id)
    .bind(service_id)
    .bind(name)
    .bind(&key_prefix)
    .bind(key_hash)
    .bind(&claims.sub)
    .bind(payload.expires_at)
    .fetch_one(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("创建服务调用凭证", error))?;
    Ok(Json(CreatedDecisionServiceApiKey { key, secret }))
}

async fn revoke_decision_service_api_key(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, key_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ProjectApiError> {
    ensure_project_permission(
        &state,
        project_id,
        &claims.sub,
        ProjectPermission::ManageService,
    )
    .await?;
    let result = sqlx::query(
        "UPDATE decision_service_api_keys SET revoked_at = COALESCE(revoked_at, NOW()) WHERE id = $1 AND project_id = $2",
    )
    .bind(key_id)
    .bind(project_id)
    .execute(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("撤销服务调用凭证", error))?;
    if result.rows_affected() == 0 {
        return Err(ProjectApiError::coded(
            StatusCode::NOT_FOUND,
            "KEY_NOT_FOUND",
            "服务调用凭证不存在",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn authenticate_service_api_key(
    state: &AppState,
    headers: &HeaderMap,
    project_id: i64,
    service_id: &str,
) -> Result<String, ProjectApiError> {
    let header = headers.get("X-Decision-Key").ok_or_else(|| {
        ProjectApiError::coded(
            StatusCode::UNAUTHORIZED,
            "KEY_REQUIRED",
            "缺少决策服务调用凭证",
        )
    })?;
    let secret = header
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ProjectApiError::coded(
                StatusCode::UNAUTHORIZED,
                "KEY_INVALID",
                "决策服务调用凭证格式无效，请使用生成时显示的完整密钥",
            )
        })?;
    if secret.len() > 128 {
        return Err(ProjectApiError::coded(
            StatusCode::UNAUTHORIZED,
            "KEY_INVALID",
            "决策服务调用凭证无效",
        ));
    }
    let prefix = secret.chars().take(20).collect::<String>();
    let row = sqlx::query_as::<_, (String, String, Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>, String)>(
        "SELECT key_hash, created_by, expires_at, revoked_at, service_id FROM decision_service_api_keys WHERE project_id = $1 AND service_id = $2 AND key_prefix = $3",
    )
    .bind(project_id)
    .bind(service_id)
    .bind(&prefix)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("校验服务调用凭证", error))?
    .ok_or_else(|| ProjectApiError::coded(StatusCode::UNAUTHORIZED, "KEY_INVALID", "决策服务调用凭证无效"))?;
    let (hash, owner, expires_at, revoked_at, stored_service_id) = row;
    if stored_service_id != service_id
        || revoked_at.is_some()
        || expires_at.is_some_and(|expires| expires <= chrono::Utc::now())
        || !verify_service_api_key(&hash, secret)
    {
        return Err(ProjectApiError::coded(
            StatusCode::UNAUTHORIZED,
            "KEY_INVALID",
            "决策服务调用凭证无效",
        ));
    }
    sqlx::query("UPDATE decision_service_api_keys SET last_used_at = NOW() WHERE project_id = $1 AND service_id = $2 AND key_prefix = $3")
        .bind(project_id)
        .bind(service_id)
        .bind(prefix)
        .execute(&state.db)
        .await
        .map_err(|error| ProjectApiError::database("更新服务调用凭证", error))?;
    Ok(owner)
}

async fn execute_public_decision_service(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    headers: HeaderMap,
    Json(payload): Json<ExecuteDecisionService>,
) -> Result<Response, ProjectApiError> {
    let owner =
        authenticate_service_api_key(&state, &headers, project_id, &payload.service_id).await?;
    let Json(run) = execute_published_decision_service(
        State(state),
        Extension(Claims {
            sub: owner,
            exp: usize::MAX,
        }),
        Path(project_id),
        Json(payload),
    )
    .await?;
    let mut response = Json(run).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn execute_published_decision_service(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<ExecuteDecisionService>,
) -> Result<Json<DecisionServiceRun>, ProjectApiError> {
    if !payload.inputs.is_object() {
        return Err(ProjectApiError::bad_request("决策服务输入必须是 JSON 对象"));
    }
    if payload.business_reference.chars().count() > 255 {
        return Err(ProjectApiError::bad_request(
            "业务关联号不能超过 255 个字符",
        ));
    }
    let business_reference = payload.business_reference.trim().to_string();
    let input_size = serde_json::to_vec(&payload.inputs)
        .map_err(|error| ProjectApiError::bad_request(format!("服务输入无法序列化：{error}")))?
        .len();
    if input_size > 1_000_000 {
        return Err(ProjectApiError::bad_request(
            "决策服务输入不能超过 1000000 字节",
        ));
    }
    let Some((version_id, version_number, xml)) =
        load_published_artifact(&state, &claims.sub, project_id).await?
    else {
        return Err(ProjectApiError::bad_request("项目尚未发布决策版本"));
    };
    let mut model = crate::dmn::parse_dmn(&xml)
        .map_err(|error| ProjectApiError::bad_request(format!("已发布 DMN 解析失败: {error}")))?;
    ensure_default_decision_services(&mut model);
    crate::dmn::compile_dmn(&model)
        .map_err(|error| ProjectApiError::bad_request(format!("已发布 DMN 编译失败: {error}")))?;
    let service = model
        .decision_services
        .iter()
        .find(|service| service.id == payload.service_id)
        .cloned()
        .ok_or_else(|| ProjectApiError::bad_request("已发布版本中不存在该决策服务"))?;
    let started = std::time::Instant::now();
    let (status, output, trace) =
        match decision_service_inputs_to_feel(&model, &service, &payload.inputs) {
            Ok(inputs) => {
                let result = crate::dmn::DmnExecutor::new(&model)
                    .evaluate_decision_service_with_details(&service.id, inputs);
                let status = if result.status == crate::dmn::DmnResultStatus::Failed {
                    "failed"
                } else {
                    "succeeded"
                };
                let output = service_output(&model, &service, &result);
                let trace = build_decision_service_trace(
                    &model,
                    &service,
                    version_id,
                    version_number,
                    &payload.inputs,
                    &result,
                );
                (status, output, trace)
            }
            Err(error) => {
                let mut trace = build_rejected_execution_trace(
                    &model,
                    Some(version_id),
                    Some(version_number),
                    &payload.inputs,
                    "SERVICE_INPUT_CONTRACT",
                    &error,
                );
                trace.explanation = format!("决策服务 {} 未执行：{error}", service.name);
                ("failed", json!({ "error": error }), trace)
            }
        };
    let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let trace_json = serde_json::to_value(trace)
        .map_err(|error| ProjectApiError::bad_request(format!("执行轨迹序列化失败：{error}")))?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存决策服务运行记录", error))?;
    let run = sqlx::query_as::<_, DecisionServiceRun>(
        r#"
        INSERT INTO decision_runs (
            project_id, version_id, service_id, service_name, input_json, output_json,
            execution_trace_json, duration_ms, status, executed_by, business_reference
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING id, project_id, version_id, service_id, service_name, input_json,
                  output_json, execution_trace_json, duration_ms, status, executed_by,
                  executed_at, business_reference, actual_outcome_json,
                  outcome_assessment, outcome_note, outcome_occurred_at,
                  outcome_reported_by, outcome_reported_at
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .bind(&service.id)
    .bind(&service.name)
    .bind(payload.inputs)
    .bind(output)
    .bind(trace_json)
    .bind(duration_ms)
    .bind(status)
    .bind(&claims.sub)
    .bind(business_reference)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存决策服务运行记录", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version_id),
        "decision_service.invoked",
        &claims.sub,
        &format!("调用决策服务 {}", service.name),
        json!({
            "runId": run.id,
            "serviceId": service.id,
            "serviceName": service.name,
            "versionNumber": version_number,
            "status": run.status,
            "durationMs": run.duration_ms,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录服务调用审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交决策服务运行记录", error))?;
    Ok(Json(run))
}

async fn list_decision_service_runs(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DecisionServiceRun>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let runs = sqlx::query_as::<_, DecisionServiceRun>(
        r#"
        SELECT id, project_id, version_id, service_id, service_name, input_json,
               output_json, execution_trace_json, duration_ms, status, executed_by,
               executed_at, business_reference, actual_outcome_json,
               outcome_assessment, outcome_note, outcome_occurred_at,
               outcome_reported_by, outcome_reported_at
        FROM decision_runs_partitioned
        WHERE project_id = $1
        ORDER BY executed_at DESC, id DESC
        LIMIT 50
        "#,
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取决策服务运行记录", error))?;
    Ok(Json(runs))
}

fn validate_decision_run_explanation(
    trace: &Value,
    explanation: &crate::models::DecisionRunExplanation,
) -> Result<(), String> {
    if explanation.summary.trim().is_empty() || explanation.summary.chars().count() > 2000 {
        return Err("解释摘要为空或超出长度限制".into());
    }
    let expected = trace
        .get("decisions")
        .and_then(Value::as_array)
        .ok_or_else(|| "运行轨迹缺少决策步骤".to_string())?
        .iter()
        .map(|step| {
            step.get("decisionId")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| "运行轨迹包含无效决策 ID".to_string())
        })
        .collect::<Result<HashSet<_>, _>>()?;
    let mut actual = HashSet::new();
    for step in &explanation.decisions {
        if step.explanation.trim().is_empty()
            || step.explanation.chars().count() > 1200
            || !actual.insert(step.decision_id.as_str())
        {
            return Err("解释步骤重复、为空或超出长度限制".into());
        }
    }
    let expected = expected.iter().map(String::as_str).collect::<HashSet<_>>();
    if actual != expected {
        return Err("解释决策 ID 与执行轨迹不一致".into());
    }
    Ok(())
}

async fn get_decision_run_explanation(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, run_id)): Path<(i64, i64)>,
) -> Result<Json<crate::models::DecisionRunExplanationView>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let explanation = sqlx::query_as::<_, crate::models::DecisionRunExplanationView>(
        r#"SELECT explanation.id, explanation.decision_run_id, explanation.generation_revision, explanation.status,
                  explanation.explanation_json, explanation.model, explanation.prompt_version,
                  explanation.prompt_digest, explanation.trace_digest, explanation.error_message,
                  explanation.created_by, explanation.created_at, explanation.updated_at,
                  explanation.completed_at
           FROM decision_run_explanations explanation
           JOIN decision_runs run ON run.id = explanation.decision_run_id
           WHERE run.project_id = $1 AND run.id = $2"#,
    )
    .bind(project_id)
    .bind(run_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取决策解释", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    Ok(Json(explanation))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecisionExplanationReviewQuery {
    explanation_revision: i64,
}

async fn list_decision_run_explanation_reviews(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, run_id)): Path<(i64, i64)>,
    Query(query): Query<DecisionExplanationReviewQuery>,
) -> Result<Json<Vec<crate::models::DecisionRunExplanationReviewView>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    if query.explanation_revision <= 0 {
        return Err(ProjectApiError::bad_request("解释版本无效"));
    }
    let is_reviewer = project_access::has_permission(
        &state.db,
        project_id,
        &claims.sub,
        ProjectPermission::Review,
    )
    .await
    .map_err(|error| ProjectApiError::database("校验解释审阅权限", error))?;
    // Visibility and revision use one database snapshot, so regeneration between
    // two reads cannot reveal labels from a version the Reviewer never reviewed.
    let reviews = sqlx::query_as::<_, crate::models::DecisionRunExplanationReviewView>(
        decision_explanation::CURRENT_REVIEWS_SQL,
    )
    .bind(project_id)
    .bind(run_id)
    .bind(query.explanation_revision)
    .bind(is_reviewer)
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取解释审阅记录", error))?;
    Ok(Json(reviews))
}

async fn list_decision_explanation_review_export(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<DriftEventReviewPageQuery>,
) -> Result<Json<Vec<crate::models::DecisionExplanationReviewExportRecord>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let (after_id, limit) =
        validate_drift_event_review_page(&query).map_err(ProjectApiError::bad_request)?;
    let is_reviewer = project_access::has_permission(
        &state.db,
        project_id,
        &claims.sub,
        ProjectPermission::Review,
    )
    .await
    .map_err(|error| ProjectApiError::database("校验解释审阅导出权限", error))?;
    let reviews = sqlx::query_as::<_, crate::models::DecisionExplanationReviewExportRecord>(
        decision_explanation::CURRENT_REVIEW_EXPORT_SQL,
    )
    .bind(project_id)
    .bind(after_id)
    .bind(is_reviewer)
    .bind(&claims.sub)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取解释审阅导出记录", error))?;
    Ok(Json(reviews))
}

async fn review_decision_run_explanation(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, run_id)): Path<(i64, i64)>,
    Json(payload): Json<crate::models::DecisionRunExplanationReviewRequest>,
) -> Result<Json<crate::models::DecisionRunExplanationReviewView>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Review).await?;
    if !valid_decision_run_explanation_review_label(&payload.review_label)
        || payload.explanation_revision <= 0
    {
        return Err(ProjectApiError::bad_request("解释审阅结论无效"));
    }
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("开始提交解释审阅", error))?;
    let (explanation_id, generation_revision, explanation_status) =
        sqlx::query_as::<_, (i64, i64, String)>(
            r#"SELECT explanation.id, explanation.generation_revision, explanation.status
           FROM decision_run_explanations explanation
           JOIN decision_runs run ON run.id = explanation.decision_run_id
           WHERE run.project_id = $1 AND run.id = $2
           FOR UPDATE OF explanation"#,
        )
        .bind(project_id)
        .bind(run_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("读取待审阅决策解释", error))?
        .ok_or_else(ProjectApiError::not_found)?;
    if payload.explanation_revision != generation_revision || explanation_status != "succeeded" {
        return Err(ProjectApiError::conflict(
            "解释已更新，请重新查看后再提交审阅",
        ));
    }
    let has_reviewed = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM decision_run_explanation_reviews WHERE explanation_id = $1 AND reviewer_username = $2 AND explanation_revision = $3)",
    )
    .bind(explanation_id)
    .bind(&claims.sub)
    .bind(generation_revision)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("检查重复解释审阅", error))?;
    if has_reviewed {
        return Err(ProjectApiError::conflict(
            "每位审阅者只能提交一次该解释的可读性结论",
        ));
    }
    let review = sqlx::query_as::<_, crate::models::DecisionRunExplanationReviewView>(
        r#"INSERT INTO decision_run_explanation_reviews
               (explanation_id, reviewer_username, review_label, explanation_revision)
           VALUES ($1, $2, $3, $4)
           RETURNING id, explanation_revision, review_label, reviewer_username, created_at"#,
    )
    .bind(explanation_id)
    .bind(&claims.sub)
    .bind(&payload.review_label)
    .bind(generation_revision)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存解释审阅记录", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        None,
        "decision_run.explanation_reviewed",
        &claims.sub,
        "审阅决策运行解释",
        json!({"runId": run_id, "explanationId": explanation_id, "explanationRevision": generation_revision, "reviewLabel": payload.review_label}),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录解释审阅审计", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交解释审阅", error))?;
    Ok(Json(review))
}

fn valid_decision_run_explanation_review_label(label: &str) -> bool {
    matches!(label, "readable" | "needs_revision" | "inconclusive")
}

async fn generate_decision_run_explanation(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<crate::api::LlmState>>,
    Extension(claims): Extension<Claims>,
    Path((project_id, run_id)): Path<(i64, i64)>,
) -> Result<(StatusCode, Json<crate::models::DecisionRunExplanationView>), ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let agent = llm_state.decision_explanation_agent().ok_or_else(|| {
        ProjectApiError::coded(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODEL_UNAVAILABLE",
            "当前未配置决策解释模型",
        )
    })?;
    let (trace, run_status) = sqlx::query_as::<_, (Value, String)>(
        "SELECT execution_trace_json, status FROM decision_runs WHERE project_id = $1 AND id = $2",
    )
    .bind(project_id)
    .bind(run_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取决策运行轨迹", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if run_status != "succeeded" {
        return Err(ProjectApiError::conflict("失败运行不支持生成业务解释"));
    }
    let trace_bytes = serde_json::to_vec(&trace)
        .map_err(|error| ProjectApiError::bad_request(format!("执行轨迹序列化失败：{error}")))?;
    if trace_bytes.len() > 200_000 {
        return Err(ProjectApiError::bad_request("执行轨迹超过解释生成大小限制"));
    }
    let trace_digest = format!("{:x}", Sha256::digest(&trace_bytes));
    let prompt = agent.prompt_asset("decision_explanation");
    let prompt_version = prompt.version.clone();
    let prompt_digest = format!("{:x}", Sha256::digest(prompt.content.as_bytes()));
    let estimated_cost = crate::api::estimated_run_cost_reservation(
        agent.estimated_cost_per_response_usd(),
        agent.maximum_model_response_attempts(),
    );
    let mut transaction =
        crate::api::begin_project_daily_budget_transaction(&state.db, project_id, estimated_cost)
            .await
            .map_err(decision_explanation_budget_error)?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('fuxi_decision_run_explanation:' || $1::TEXT, 0))",
    )
    .bind(run_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("锁定决策解释请求", error))?;
    if let Some(cached) = sqlx::query_as::<_, crate::models::DecisionRunExplanationView>(
        r#"SELECT id, decision_run_id, generation_revision, status, explanation_json, model, prompt_version,
                  prompt_digest, trace_digest, error_message, created_by, created_at,
                  updated_at, completed_at
           FROM decision_run_explanations WHERE decision_run_id = $1 FOR UPDATE"#,
    )
    .bind(run_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取已有决策解释", error))?
    {
        if cached.status == "succeeded"
            && cached.trace_digest.trim() == trace_digest
            && cached.prompt_digest.trim() == prompt_digest
            && cached.prompt_version == prompt_version
        {
            transaction
                .commit()
                .await
                .map_err(|error| ProjectApiError::database("提交决策解释缓存读取", error))?;
            return Ok((StatusCode::OK, Json(cached)));
        }
        if cached.status == "running" {
            return Err(ProjectApiError::conflict("该决策解释正在生成"));
        }
    }
    crate::api::check_orchestration_budgets(
        &mut transaction,
        project_id,
        None,
        None,
        estimated_cost,
    )
    .await
    .map_err(decision_explanation_budget_error)?;
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO agent_orchestrations (project_id, username, trigger_kind, status, current_stage) VALUES ($1, $2, 'decision_explanation', 'running', 'decision_explanation') RETURNING id",
    )
    .bind(project_id)
    .bind(&claims.sub)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("创建决策解释编排", error))?;
    let run_agent_id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO agent_runs (
            project_id, stage, status, model, prompt_version, prompt_digest,
            input_json, usage_json, orchestration_id
        ) VALUES ($1, 'decision_explanation', 'running', $2, $3, $4, $5, $6, $7)
        RETURNING id"#,
    )
    .bind(project_id)
    .bind(agent.model())
    .bind(&prompt_version)
    .bind(&prompt_digest)
    .bind(json!({"decisionRunId": run_id, "traceDigest": trace_digest}))
    .bind(crate::api::project_budget_reservation(estimated_cost))
    .bind(orchestration_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录决策解释 Agent 运行", error))?;
    sqlx::query(
        "INSERT INTO agent_steps (orchestration_id, agent_run_id, step_no, agent_role, task_kind, model, prompt_version, prompt_digest, artifact_kind) VALUES ($1, $2, 1, 'explainer', 'decision_explanation', $3, $4, $5, 'decision_run_explanation')",
    )
    .bind(orchestration_id)
    .bind(run_agent_id)
    .bind(agent.model())
    .bind(&prompt_version)
    .bind(&prompt_digest)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录决策解释 Agent 步骤", error))?;
    let explanation_id = sqlx::query_scalar::<_, i64>(decision_explanation::UPSERT_ATTEMPT_SQL)
        .bind(run_id)
        .bind(agent.model())
        .bind(&prompt_version)
        .bind(&prompt_digest)
        .bind(&trace_digest)
        .bind(&claims.sub)
        .bind(run_agent_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("创建决策解释缓存", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交决策解释任务", error))?;
    let state_for_job = state.clone();
    let username = claims.sub.clone();
    tokio::spawn(async move {
        if let Err(error) = execute_decision_run_explanation_job(
            state_for_job,
            project_id,
            run_id,
            explanation_id,
            run_agent_id,
            orchestration_id,
            username,
            agent,
            trace,
        )
        .await
        {
            eprintln!("保存决策解释作业结果失败：{error:?}");
        }
    });
    let row = sqlx::query_as::<_, crate::models::DecisionRunExplanationView>(
        r#"SELECT id, decision_run_id, generation_revision, status, explanation_json, model, prompt_version,
                  prompt_digest, trace_digest, error_message, created_by, created_at,
                  updated_at, completed_at
           FROM decision_run_explanations WHERE id = $1"#,
    )
    .bind(explanation_id)
    .fetch_one(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取决策解释作业", error))?;
    Ok((StatusCode::ACCEPTED, Json(row)))
}

#[allow(clippy::too_many_arguments)]
async fn execute_decision_run_explanation_job(
    state: AppState,
    project_id: i64,
    run_id: i64,
    explanation_id: i64,
    run_agent_id: i64,
    orchestration_id: i64,
    username: String,
    agent: crate::nl_dsl::agent::LlmAgent,
    trace: Value,
) -> Result<(), ProjectApiError> {
    let running = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM agent_orchestrations WHERE id = $1 AND status = 'running')",
    )
    .bind(orchestration_id)
    .fetch_one(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("检查决策解释运行状态", error))?;
    let generated = if running {
        decision_explanation::until_cancelled(
            agent.explain_decision_run(&trace),
            crate::workflow_api::wait_for_orchestration_cancellation(&state.db, orchestration_id),
        )
        .await
    } else {
        Err(anyhow::anyhow!("决策解释编排已结束"))
    };
    let output = match generated {
        Ok(explanation) => validate_decision_run_explanation(&trace, &explanation)
            .and_then(|()| serde_json::to_value(explanation).map_err(|error| error.to_string())),
        Err(error) => {
            eprintln!("决策解释模型调用失败：{error:#}");
            Err("模型服务未能生成有效解释".into())
        }
    };
    let usage = agent.usage_snapshot();
    let mut finish = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("开始保存决策解释结果", error))?;
    decision_explanation::finish_in_transaction(
        &mut finish,
        &decision_explanation::Attempt {
            project_id,
            decision_run_id: run_id,
            explanation_id,
            agent_run_id: run_agent_id,
            orchestration_id,
        },
        &username,
        &usage,
        output,
    )
    .await?;
    finish
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交决策解释结果", error))
}

fn decision_explanation_budget_error(
    error: crate::api::ProjectTokenBudgetError,
) -> ProjectApiError {
    match error {
        crate::api::ProjectTokenBudgetError::Exceeded { used, limit }
        | crate::api::ProjectTokenBudgetError::OrchestrationTokensExceeded { used, limit } => {
            ProjectApiError::coded(
                StatusCode::TOO_MANY_REQUESTS,
                "MODEL_TOKEN_BUDGET_EXCEEDED",
                format!("模型 Token 预算已用尽（已用 {used}，上限 {limit}）"),
            )
        }
        crate::api::ProjectTokenBudgetError::EstimatedCostExceeded { used, next, limit } => {
            ProjectApiError::coded(
                StatusCode::TOO_MANY_REQUESTS,
                "MODEL_COST_BUDGET_EXCEEDED",
                format!(
                    "模型成本预算不足（已用 ${used:.6}，本次估算 ${next:.6}，上限 ${limit:.6}）"
                ),
            )
        }
        crate::api::ProjectTokenBudgetError::InvalidConfiguration(message) => {
            ProjectApiError::coded(
                StatusCode::SERVICE_UNAVAILABLE,
                "MODEL_BUDGET_CONFIG_INVALID",
                message,
            )
        }
        crate::api::ProjectTokenBudgetError::Database(error) => {
            ProjectApiError::database("检查决策解释模型预算", error)
        }
        crate::api::ProjectTokenBudgetError::RetryAlreadyActive => {
            ProjectApiError::conflict("决策解释运行状态冲突")
        }
    }
}

async fn load_published_analytics(
    state: &AppState,
    username: &str,
    project_id: i64,
) -> Result<DecisionAnalyticsReport, ProjectApiError> {
    use tokio_stream::StreamExt as _;

    let Some((version_id, version_number, xml)) =
        load_published_artifact(state, username, project_id).await?
    else {
        return Err(ProjectApiError::bad_request("项目尚未发布决策版本"));
    };
    let model = crate::dmn::parse_dmn(&xml)
        .map_err(|error| ProjectApiError::bad_request(format!("已发布 DMN 解析失败: {error}")))?;
    crate::dmn::compile_dmn(&model)
        .map_err(|error| ProjectApiError::bad_request(format!("已发布 DMN 编译失败: {error}")))?;
    let generated_at = chrono::Utc::now();
    let window_start = analytics_window_start(generated_at);
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("读取决策运行分析数据", error))?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("读取决策运行分析数据", error))?;
    let thresholds = sqlx::query_as::<_, DecisionAnalyticsThresholds>(
        r#"
        SELECT failure_warning_rate, failure_high_rate, failure_min_runs,
               mismatch_warning_rate, mismatch_high_rate, mismatch_min_samples,
               updated_at
        FROM decision_analytics_thresholds
        WHERE project_id = $1
        "#,
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取项目分析阈值", error))?
    .unwrap_or_default();
    let p95_duration_ms = sqlx::query_scalar::<_, Option<i64>>(
        r#"
        SELECT percentile_disc(0.95) WITHIN GROUP (ORDER BY GREATEST(duration_ms, 0))
        FROM decision_runs_partitioned
        WHERE project_id = $1 AND version_id = $2 AND executed_at >= $3
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .bind(window_start)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取决策运行分析数据", error))?;
    let mut builder = DecisionAnalyticsBuilder::new(
        project_id,
        version_id,
        version_number,
        &model,
        window_start,
        generated_at,
    );
    builder.set_thresholds(thresholds);
    builder.set_exact_p95_duration_ms(p95_duration_ms);
    let mut runs = sqlx::query_as::<_, DecisionAnalyticsRun>(
        r#"
        WITH ordered_runs AS (
            SELECT project_id, input_json, execution_trace_json, duration_ms, status,
                   executed_at, id, business_reference, outcome_assessment,
                   COUNT(*) FILTER (
                       WHERE status = 'succeeded'
                         AND outcome_assessment IN ('confirmed', 'mismatch')
                   ) OVER () AS total_conclusive_feedback,
                   COUNT(*) FILTER (
                       WHERE status = 'succeeded'
                         AND outcome_assessment IN ('confirmed', 'mismatch')
                   ) OVER (
                       ORDER BY executed_at, id
                       ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
                   ) AS conclusive_feedback_seen
            FROM decision_runs_partitioned
            WHERE project_id = $1 AND version_id = $2 AND executed_at >= $3
        ), sized_runs AS (
            SELECT *,
                   octet_length(execution_trace_json::text) AS execution_trace_bytes,
                   CASE WHEN status = 'succeeded'
                              AND outcome_assessment IN ('confirmed', 'mismatch')
                              AND total_conclusive_feedback - conclusive_feedback_seen < 2000
                        THEN octet_length(input_json::text) END AS input_json_bytes
            FROM ordered_runs
        )
        SELECT project_id,
               CASE WHEN status = 'succeeded'
                          AND outcome_assessment IN ('confirmed', 'mismatch')
                          AND total_conclusive_feedback - conclusive_feedback_seen < 2000
                          AND input_json_bytes <= $4
                    THEN input_json ELSE 'null'::jsonb END AS input_json,
               COALESCE(input_json_bytes > $4, FALSE) AS numeric_input_omitted,
               CASE WHEN execution_trace_bytes <= $4
                    THEN execution_trace_json ELSE 'null'::jsonb END AS execution_trace_json,
               COALESCE(execution_trace_bytes > $4, FALSE) AS trace_details_omitted,
               duration_ms, status, executed_at,
               business_reference, outcome_assessment
        FROM sized_runs
        ORDER BY executed_at, id
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .bind(window_start)
    .bind(MAX_ANALYTICS_TRACE_BYTES)
    .fetch(&mut *transaction);
    while let Some(run) = runs.next().await {
        let run = run.map_err(|error| ProjectApiError::database("读取决策运行分析数据", error))?;
        builder.record(run);
    }
    drop(runs);
    let daily_aggregates = sqlx::query_as::<_, DecisionRunDailyAggregate>(
        r#"
        WITH daily AS (
            SELECT run_day, total_runs, succeeded_runs, failed_runs, average_duration_ms
            FROM decision_run_daily_stats
            WHERE project_id = $1 AND version_id = $2
              AND run_day >= $3::DATE
              AND run_day < ($4::DATE + 1)
              AND run_day < (NOW() AT TIME ZONE 'UTC')::DATE
            UNION ALL
            SELECT (executed_at AT TIME ZONE 'UTC')::DATE AS run_day,
                   COUNT(*)::BIGINT AS total_runs,
                   COUNT(*) FILTER (WHERE status = 'succeeded')::BIGINT AS succeeded_runs,
                   COUNT(*) FILTER (WHERE status = 'failed')::BIGINT AS failed_runs,
                   AVG(duration_ms)::DOUBLE PRECISION AS average_duration_ms
            FROM decision_runs_partitioned
            WHERE project_id = $1 AND version_id = $2
              AND executed_at >= (NOW() AT TIME ZONE 'UTC')::DATE AT TIME ZONE 'UTC'
              AND executed_at <= $5
              AND executed_at >= $3
            GROUP BY (executed_at AT TIME ZONE 'UTC')::DATE
        )
        SELECT run_day, total_runs, succeeded_runs, failed_runs, average_duration_ms
        FROM daily
        ORDER BY run_day
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .bind(window_start.date_naive())
    .bind(generated_at.date_naive())
    .bind(generated_at)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取决策运行每日聚合", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("读取决策运行分析数据", error))?;
    let mut report = builder.finish();
    for aggregate in daily_aggregates {
        let Some(day) = report
            .daily_trends
            .iter_mut()
            .find(|day| day.date == aggregate.run_day)
        else {
            continue;
        };
        day.total_runs = usize::try_from(aggregate.total_runs).unwrap_or(0);
        day.succeeded_runs = usize::try_from(aggregate.succeeded_runs).unwrap_or(0);
        day.failed_runs = usize::try_from(aggregate.failed_runs).unwrap_or(0);
        day.average_duration_ms = aggregate.average_duration_ms;
    }
    Ok(report)
}

fn optimization_guidance(code: &str) -> (&'static str, Vec<&'static str>) {
    match code {
        "HIGH_FAILURE_RATE" => (
            "排查并降低线上失败率",
            vec![
                "从失败运行轨迹中归类输入契约、字段映射和表达式错误。",
                "补充拒绝输入、空值和异常分支的业务验收案例。",
                "仅在草稿中修复规则，并重新执行技术核验与业务验收。",
            ],
        ),
        "LOW_FEEDBACK_COVERAGE" => (
            "提高实际业务结果回填覆盖",
            vec![
                "在业务流程中补齐业务关联号与结果回填责任。",
                "优先回填高风险或人工复核的决策结果。",
                "获得足够样本后再评估是否需要调整规则。",
            ],
        ),
        "HIGH_MISMATCH_RATE" => (
            "复核规则与实际业务结果的偏差",
            vec![
                "抽样核对偏差运行的输入、输出、人工判定和业务依据。",
                "将确认的偏差沉淀为可复现业务验收案例。",
                "在草稿中调整条件、阈值或默认分支，并完成验收。",
            ],
        ),
        "RULE_FEEDBACK_MISMATCH" => (
            "复核命中规则与业务结果的偏差",
            vec![
                "抽查该规则命中后被判定为偏差的业务记录及其原始输入。",
                "确认是否为规则条件、优先级或业务结果回填错误。",
                "仅将已核实的差异转为验收案例，再在草稿中调整并复测。",
            ],
        ),
        "RULE_HIT_DRIFT" => (
            "调查规则命中率变化",
            vec![
                "对照前期基线与最近一周的输入分布和上游决策变化。",
                "抽样检查命中与未命中运行，确认是否来自业务流量变化或规则缺口。",
                "仅在业务含义得到确认后调整规则，并补充覆盖两种行为的验收案例。",
            ],
        ),
        "INPUT_DISTRIBUTION_DRIFT" => (
            "核实数值输入分布变化",
            vec![
                "对照近期与前期反馈样本，确认数值字段变化是否来自客户或业务结构变化。",
                "检查变化是否影响现有阈值附近的决策结果，并核对对应业务反馈。",
                "确认真实规则缺口后再修改草稿，并为新旧输入区间补充验收案例。",
            ],
        ),
        "INPUT_FIELD_MISSINGNESS_DRIFT" => (
            "核实数值输入字段缺失率变化",
            vec![
                "检查近期字段缺失是否来自上游数据契约、采集映射或业务流程变化。",
                "对照有值与缺失样本，确认缺失是否集中在特定业务来源或决策结果。",
                "先修复数据质量或补充明确的缺失值业务规则，再通过边界案例验证草稿。",
            ],
        ),
        "HIGH_P95_LATENCY" => (
            "定位高耗时决策路径",
            vec![
                "根据执行轨迹定位重复计算和高复杂度表达式。",
                "简化决策依赖或拆分不必要的计算链路。",
                "以真实输入复测 P95 耗时后再发布。",
            ],
        ),
        "LOW_RULE_COVERAGE" => (
            "补充规则覆盖与测试证据",
            vec![
                "确认长期未命中规则是否仍符合当前业务范围。",
                "为有效规则补充正常、边界和冲突业务验收案例。",
                "删除或调整规则前先完成影响评估与业务确认。",
            ],
        ),
        "FREQUENT_FALLBACK" => (
            "减少默认输出和无匹配分支",
            vec![
                "抽样分析默认输出和无匹配运行的输入组合。",
                "补充缺失条件、允许值或明确的业务兜底规则。",
                "将代表性输入固化为回归验收案例。",
            ],
        ),
        "NO_RECENT_RUNS" => (
            "补齐发布版本的运行观测",
            vec![
                "确认决策服务已被业务流程调用并携带业务关联号。",
                "使用真实或受控样本执行服务调用与结果回填。",
                "累计可用运行样本后重新生成优化建议。",
            ],
        ),
        _ => (
            "复核近期运行趋势",
            vec![
                "查看相关运行记录、执行轨迹和业务反馈。",
                "将确认的问题转化为可验证的规则或流程改进。",
                "在草稿中修改并完成版本核验后再发布。",
            ],
        ),
    }
}

fn optimization_proposal_withdrawal_note(
    proposal_source_version_id: i64,
    current_source_version_id: i64,
    finding_is_active: bool,
) -> Option<&'static str> {
    if proposal_source_version_id != current_source_version_id {
        Some("自动撤回：项目发布版本已变化")
    } else if !finding_is_active {
        Some("自动撤回：当前分析未复现该发现")
    } else {
        None
    }
}

fn optimization_proposal_withdrawal_audit_details(
    proposal_id: i64,
    source_version_id: i64,
    finding_code: &str,
    element: &str,
    resolution_note: &str,
) -> Value {
    json!({
        "proposalId": proposal_id,
        "sourceVersionId": source_version_id,
        "findingCode": finding_code,
        "element": element,
        "resolutionNote": resolution_note,
    })
}

struct OptimizationAutodraftShadowPolicy {
    enabled: bool,
    draft_enabled: bool,
    project_ids: HashSet<i64>,
}

impl OptimizationAutodraftShadowPolicy {
    fn from_environment() -> Self {
        Self::from_values(
            std::env::var("OPTIMIZATION_AUTODRAFT_MODE").ok().as_deref(),
            std::env::var("OPTIMIZATION_AUTODRAFT_PROJECT_IDS")
                .ok()
                .as_deref(),
        )
    }

    fn from_values(mode: Option<&str>, project_ids: Option<&str>) -> Self {
        let mode = mode.map(str::trim).unwrap_or_default();
        let enabled = mode.eq_ignore_ascii_case("shadow") || mode.eq_ignore_ascii_case("draft");
        let draft_enabled = mode.eq_ignore_ascii_case("draft");
        let project_ids = project_ids
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .map(str::parse::<i64>)
                    .collect::<Result<HashSet<_>, _>>()
            })
            .and_then(Result::ok)
            .filter(|ids| !ids.is_empty() && ids.iter().all(|id| *id > 0))
            .unwrap_or_default();
        Self {
            enabled: enabled && !project_ids.is_empty(),
            draft_enabled: draft_enabled && !project_ids.is_empty(),
            project_ids,
        }
    }

    fn includes(&self, project_id: i64) -> bool {
        self.enabled && self.project_ids.contains(&project_id)
    }
}

fn optimization_autodraft_shadow_outcome(
    project_risk: &str,
    finding_code: &str,
    sample_source: &str,
    sample_count: usize,
    simulation_passed: bool,
    change_review: Option<&Value>,
) -> &'static str {
    if project_risk != "low" {
        return "blocked_project_risk";
    }
    if !matches!(finding_code, "HIGH_FAILURE_RATE" | "HIGH_MISMATCH_RATE") {
        return "blocked_finding_code";
    }
    if sample_source != "approved_business_tests" {
        return "blocked_unlabeled_samples";
    }
    if sample_count < MIN_SIMULATION_SAMPLE_COUNT {
        return "blocked_insufficient_samples";
    }
    if !simulation_passed {
        return "blocked_simulation";
    }
    let low_risk_updates_only = change_review.is_some_and(|review| {
        review.get("overallRisk").and_then(Value::as_str) == Some("low")
            && review
                .get("changes")
                .and_then(Value::as_array)
                .is_some_and(|changes| {
                    !changes.is_empty()
                        && changes.iter().all(|change| {
                            change.get("changeType").and_then(Value::as_str) == Some("update")
                        })
                })
    });
    if !low_risk_updates_only {
        return "blocked_change_risk";
    }
    "eligible_shadow_candidate"
}

async fn list_optimization_proposals_for_project(
    state: &AppState,
    username: &str,
    project_id: i64,
) -> Result<Vec<DecisionOptimizationProposal>, ProjectApiError> {
    ensure_project_permission(state, project_id, username, ProjectPermission::View).await?;
    sqlx::query_as::<_, DecisionOptimizationProposal>(
        r#"
        SELECT proposal.id, proposal.project_id, proposal.source_version_id,
               version.version_number AS source_version_number, proposal.finding_code,
               proposal.finding_level, proposal.element, proposal.title, proposal.summary,
               proposal.proposed_actions_json AS proposed_actions,
               proposal.evidence_json AS evidence,
               proposal.analysis_snapshot_json AS analysis_snapshot, proposal.status,
               proposal.generated_by, proposal.generated_at, proposal.resolved_by,
               proposal.resolved_at, proposal.resolution_note, proposal.resulting_version_id,
               proposal.effectiveness_result_json AS effectiveness_result,
               proposal.effectiveness_tracked_at,
               COALESCE(latest_simulation.passed, FALSE) AS latest_simulation_passed
        FROM decision_optimization_proposals proposal
        JOIN decision_versions version ON version.id = proposal.source_version_id
        LEFT JOIN LATERAL (
            SELECT passed FROM optimization_simulations
            WHERE suggestion_id = proposal.id
            ORDER BY created_at DESC, id DESC LIMIT 1
        ) latest_simulation ON TRUE
        WHERE proposal.project_id = $1
        ORDER BY CASE proposal.status WHEN 'open' THEN 0 WHEN 'confirmed' THEN 1 ELSE 2 END,
                 proposal.generated_at DESC, proposal.id DESC
        "#,
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取优化建议", error))
}

async fn list_optimization_proposals(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DecisionOptimizationProposal>>, ProjectApiError> {
    Ok(Json(
        list_optimization_proposals_for_project(&state, &claims.sub, project_id).await?,
    ))
}

async fn optimization_proposal_representative_runs(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: i64,
    version_id: i64,
    finding_code: &str,
    element: &str,
) -> Result<Vec<Value>, ProjectApiError> {
    if !matches!(
        finding_code,
        "HIGH_FAILURE_RATE"
            | "HIGH_MISMATCH_RATE"
            | "RULE_FEEDBACK_MISMATCH"
            | "INPUT_DISTRIBUTION_DRIFT"
            | "INPUT_FIELD_MISSINGNESS_DRIFT"
    ) {
        return Ok(Vec::new());
    }
    let input_path = if matches!(
        finding_code,
        "INPUT_DISTRIBUTION_DRIFT" | "INPUT_FIELD_MISSINGNESS_DRIFT"
    ) {
        let Some(path) = numeric_field_segments(element) else {
            // Ambiguous legacy snapshots cannot supply precise field evidence.
            return Ok(Vec::new());
        };
        path
    } else {
        Vec::new()
    };
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            chrono::DateTime<chrono::Utc>,
            String,
            Option<String>,
            Value,
            Value,
        ),
    >(include_str!("storage/optimization_representative_runs.sql"))
    .bind(project_id)
    .bind(version_id)
    .bind(finding_code)
    .bind(input_path)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取优化建议代表运行", error))?;
    Ok(rows
        .into_iter()
        .map(optimization_proposal_run_evidence)
        .collect())
}

fn optimization_proposal_run_evidence(
    row: (
        i64,
        chrono::DateTime<chrono::Utc>,
        String,
        Option<String>,
        Value,
        Value,
    ),
) -> Value {
    let (run_id, executed_at, status, outcome_assessment, input_fields, output_fields) = row;
    json!({
        "runId": run_id,
        "executedAt": executed_at,
        "status": status,
        "outcomeAssessment": outcome_assessment,
        "inputFields": input_fields,
        "outputFields": output_fields,
    })
}

fn optimization_proposal_evidence(
    finding: &DecisionAnalyticsFinding,
    summary: &DecisionAnalyticsSummary,
    window_start: chrono::DateTime<chrono::Utc>,
    window_days: u32,
    generated_at: chrono::DateTime<chrono::Utc>,
    representative_runs: Vec<Value>,
) -> Value {
    json!({
        "schemaVersion": 1,
        "source": "decision_analytics",
        "finding": finding,
        "summary": summary,
        "windowStart": window_start,
        "windowDays": window_days,
        "generatedAt": generated_at,
        "representativeRuns": representative_runs,
        "privacy": {
            "representativeRunFields": "field_names_only",
            "rawInputValuesIncluded": false,
            "rawOutputValuesIncluded": false,
            "executionTraceIncluded": false,
        },
    })
}

fn optimization_suggestion_map(
    findings: &[DecisionAnalyticsFinding],
    suggestions: Vec<crate::nl_dsl::agent::OptimizationSuggestion>,
) -> HashMap<(String, String), crate::nl_dsl::agent::OptimizationSuggestion> {
    let expected = findings
        .iter()
        .map(|finding| (finding.code.clone(), finding.element.clone()))
        .collect::<HashSet<_>>();
    let mut output = HashMap::new();
    for suggestion in suggestions {
        let key = (suggestion.finding_code.clone(), suggestion.element.clone());
        let valid_text =
            |value: &str, max: usize| !value.trim().is_empty() && value.chars().count() <= max;
        if !expected.contains(&key)
            || !valid_text(&suggestion.title, 160)
            || !valid_text(&suggestion.summary, 800)
            || suggestion.proposed_actions.is_empty()
            || suggestion.proposed_actions.len() > 5
            || suggestion
                .proposed_actions
                .iter()
                .any(|action| !valid_text(action, 240))
            || output.insert(key, suggestion).is_some()
        {
            return HashMap::new();
        }
    }
    if output.len() != expected.len() {
        return HashMap::new();
    }
    output
}

fn optimization_budget_error(error: crate::api::ProjectTokenBudgetError) -> ProjectApiError {
    match error {
        crate::api::ProjectTokenBudgetError::Exceeded { used, limit } => ProjectApiError::coded(
            StatusCode::TOO_MANY_REQUESTS,
            "PROJECT_TOKEN_BUDGET_EXCEEDED",
            format!("项目今日模型 token 预算已用尽（已用 {used}，上限 {limit}）"),
        ),
        crate::api::ProjectTokenBudgetError::EstimatedCostExceeded { used, next, limit } => {
            ProjectApiError::coded(
                StatusCode::TOO_MANY_REQUESTS,
                "PROJECT_COST_BUDGET_EXCEEDED",
                format!(
                    "项目今日模型估算成本预算不足（已用 ${used:.6}，本次至少 ${next:.6}，上限 ${limit:.6}）"
                ),
            )
        }
        crate::api::ProjectTokenBudgetError::InvalidConfiguration(message) => {
            ProjectApiError::coded(
                StatusCode::SERVICE_UNAVAILABLE,
                "MODEL_BUDGET_CONFIG_INVALID",
                message,
            )
        }
        crate::api::ProjectTokenBudgetError::Database(error) => {
            ProjectApiError::database("检查优化建议模型预算", error)
        }
        crate::api::ProjectTokenBudgetError::RetryAlreadyActive
        | crate::api::ProjectTokenBudgetError::OrchestrationTokensExceeded { .. } => {
            ProjectApiError::conflict("优化建议模型运行预算状态不可用")
        }
    }
}

async fn begin_optimization_agent_run(
    state: &AppState,
    project_id: i64,
    username: &str,
    agent: &crate::nl_dsl::agent::LlmAgent,
    input: &Value,
) -> Result<(i64, i64), ProjectApiError> {
    let estimated_cost = crate::api::estimated_run_cost_reservation(
        agent.estimated_cost_per_response_usd(),
        agent.maximum_model_response_attempts(),
    );
    let mut transaction =
        crate::api::begin_project_daily_budget_transaction(&state.db, project_id, estimated_cost)
            .await
            .map_err(optimization_budget_error)?;
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO agent_orchestrations (project_id, username, trigger_kind, status, current_stage) VALUES ($1, $2, 'optimization_proposal', 'running', 'optimization_proposal') RETURNING id",
    )
    .bind(project_id)
    .bind(username)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("创建优化建议编排", error))?;
    let prompt = agent.prompt_asset("optimization_proposal");
    let prompt_digest = format!("{:x}", Sha256::digest(prompt.content.as_bytes()));
    let run_id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO agent_runs (
            project_id, stage, status, model, prompt_version, prompt_digest,
            input_json, usage_json, orchestration_id
        ) VALUES ($1, 'optimization_proposal', 'running', $2, $3, $4, $5, $6, $7)
        RETURNING id"#,
    )
    .bind(project_id)
    .bind(agent.model())
    .bind(&prompt.version)
    .bind(&prompt_digest)
    .bind(input)
    .bind(crate::api::project_budget_reservation(estimated_cost))
    .bind(orchestration_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录优化建议 Agent 运行", error))?;
    sqlx::query(
        "INSERT INTO agent_steps (orchestration_id, agent_run_id, step_no, agent_role, task_kind, model, prompt_version, prompt_digest, artifact_kind) VALUES ($1, $2, 1, 'optimizer', 'optimization_proposal', $3, $4, $5, 'optimization_suggestion')",
    )
    .bind(orchestration_id)
    .bind(run_id)
    .bind(agent.model())
    .bind(&prompt.version)
    .bind(prompt_digest)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录优化建议 Agent 步骤", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交优化建议模型预算", error))?;
    Ok((run_id, orchestration_id))
}

async fn finish_optimization_agent_run(
    state: &AppState,
    run_id: i64,
    orchestration_id: i64,
    agent: &crate::nl_dsl::agent::LlmAgent,
    output: Result<Value, String>,
) -> Result<(), ProjectApiError> {
    let usage = agent.usage_snapshot();
    let (status, output, error) = match output {
        Ok(output) => ("succeeded", Some(output), None),
        Err(message) => (
            "failed",
            None,
            Some(json!({"message": message, "retryable": false})),
        ),
    };
    let digest = output
        .as_ref()
        .map(|value| format!("{:x}", Sha256::digest(value.to_string().as_bytes())));
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("开始完成优化建议 Agent 运行", error))?;
    sqlx::query("UPDATE agent_runs SET status = $1, output_json = $2, error_json = $3, usage_json = $4, finished_at = NOW() WHERE id = $5 AND status = 'running'")
        .bind(status)
        .bind(output)
        .bind(error.clone())
        .bind(&usage)
        .bind(run_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("更新优化建议 Agent 运行", error))?;
    sqlx::query("UPDATE agent_steps SET status = $1, artifact_digest = $2, usage_json = $3, error_json = $4, finished_at = NOW() WHERE agent_run_id = $5 AND status = 'running'")
        .bind(status)
        .bind(digest)
        .bind(usage)
        .bind(error)
        .bind(run_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("更新优化建议 Agent 步骤", error))?;
    sqlx::query("UPDATE agent_orchestrations SET status = $1, finished_at = NOW() WHERE id = $2 AND status = 'running'")
        .bind(status)
        .bind(orchestration_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("更新优化建议编排状态", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交优化建议 Agent 结果", error))
}

async fn generate_optimization_proposals(
    State(state): State<AppState>,
    Extension(llm_state): Extension<Arc<crate::api::LlmState>>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DecisionOptimizationProposal>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let report = load_published_analytics(&state, &claims.sub, project_id).await?;
    let snapshot = serde_json::to_value(&report)
        .map_err(|error| ProjectApiError::bad_request(format!("运行分析快照无效：{error}")))?;
    let mut llm_suggestions = HashMap::new();
    let mut proposal_prompt_version =
        crate::nl_dsl::prompts::OPTIMIZATION_PROPOSAL_PROMPT_VERSION.to_string();
    if !report.findings.is_empty() && report.findings.len() <= 20 {
        if let Some(agent) = llm_state.optimization_proposal_agent() {
            proposal_prompt_version = agent.prompt_asset("optimization_proposal").version.clone();
            let input = json!({
                "windowDays": report.window_days,
                "windowStart": report.window_start,
                "summary": report.summary,
                "findings": report.findings,
            });
            let (run_id, orchestration_id) =
                begin_optimization_agent_run(&state, project_id, &claims.sub, &agent, &input)
                    .await?;
            let generation = agent.generate_optimization_suggestions(
                input["findings"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            );
            tokio::pin!(generation);
            let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));
            let generation_result = loop {
                tokio::select! {
                    result = &mut generation => break result,
                    _ = heartbeat.tick() => {
                        let refreshed = sqlx::query(
                            "UPDATE agent_orchestrations SET heartbeat_at = NOW() WHERE id = $1 AND status = 'running'",
                        )
                        .bind(orchestration_id)
                        .execute(&state.db)
                        .await;
                        match refreshed {
                            Ok(result) if result.rows_affected() == 1 => {}
                            Ok(_) => break Err(anyhow::anyhow!("优化建议编排已结束，停止模型生成")),
                            Err(error) => break Err(anyhow::anyhow!("更新优化建议编排心跳失败: {error}")),
                        }
                    }
                }
            };
            match generation_result {
                Ok(suggestions) => {
                    let mapped = optimization_suggestion_map(&report.findings, suggestions.clone());
                    if mapped.is_empty() {
                        finish_optimization_agent_run(
                            &state,
                            run_id,
                            orchestration_id,
                            &agent,
                            Err("结构化建议未通过发现关联或内容校验".into()),
                        )
                        .await?;
                        eprintln!("优化建议 Agent 输出未通过关联与内容校验，已回退确定性建议");
                    } else {
                        let output = serde_json::to_value(suggestions).unwrap_or(Value::Null);
                        finish_optimization_agent_run(
                            &state,
                            run_id,
                            orchestration_id,
                            &agent,
                            Ok(output),
                        )
                        .await?;
                        llm_suggestions = mapped;
                    }
                }
                Err(error) => {
                    finish_optimization_agent_run(
                        &state,
                        run_id,
                        orchestration_id,
                        &agent,
                        Err(error.to_string()),
                    )
                    .await?;
                    eprintln!("优化建议 Agent 生成失败，已回退确定性建议: {error}");
                }
            }
        }
    }
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("生成优化建议", error))?;
    let current_published_version_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT published_version_id FROM decision_projects WHERE id = $1 FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("确认优化建议分析基线", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if current_published_version_id != Some(report.version_id) {
        return Err(ProjectApiError::conflict(
            "项目发布版本已变化，请重新生成优化建议",
        ));
    }
    let active_findings = report
        .findings
        .iter()
        .map(|finding| (finding.code.clone(), finding.element.clone()))
        .collect::<HashSet<_>>();
    let open_proposals = sqlx::query_as::<_, (i64, i64, String, String)>(
        r#"
        SELECT id, source_version_id, finding_code, element
        FROM decision_optimization_proposals
        WHERE project_id = $1 AND status = 'open'
        FOR UPDATE
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取待处理优化建议", error))?;
    for (proposal_id, source_version_id, finding_code, element) in open_proposals {
        let finding_is_active = active_findings.contains(&(finding_code.clone(), element.clone()));
        let Some(resolution_note) = optimization_proposal_withdrawal_note(
            source_version_id,
            report.version_id,
            finding_is_active,
        ) else {
            continue;
        };
        sqlx::query(
            r#"
            UPDATE decision_optimization_proposals
            SET status = 'withdrawn', resolved_at = NOW(), resolution_note = $1
            WHERE id = $2 AND project_id = $3 AND status = 'open'
            "#,
        )
        .bind(resolution_note)
        .bind(proposal_id)
        .bind(project_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("撤回过期优化建议", error))?;
        insert_audit_event(
            &mut *transaction,
            project_id,
            Some(source_version_id),
            "optimization_proposal.withdrawn",
            &claims.sub,
            resolution_note,
            optimization_proposal_withdrawal_audit_details(
                proposal_id,
                source_version_id,
                &finding_code,
                &element,
                resolution_note,
            ),
        )
        .await
        .map_err(|error| ProjectApiError::database("记录优化建议撤回审计事件", error))?;
    }
    for finding in &report.findings {
        let representative_runs = optimization_proposal_representative_runs(
            &mut transaction,
            project_id,
            report.version_id,
            &finding.code,
            &finding.element,
        )
        .await?;
        let (title, actions) = optimization_guidance(&finding.code);
        let summary = finding
            .numeric_feedback_split_summary()
            .map(|diagnostic| format!("{}\n{}", finding.message, diagnostic))
            .unwrap_or_else(|| finding.message.clone());
        let suggestion = llm_suggestions.get(&(finding.code.clone(), finding.element.clone()));
        let (title, summary, actions, generation_mode) = match suggestion {
            Some(suggestion) => (
                suggestion.title.clone(),
                suggestion.summary.clone(),
                suggestion.proposed_actions.clone(),
                "llm_structured",
            ),
            None => (
                title.to_string(),
                summary,
                actions.into_iter().map(str::to_string).collect(),
                "deterministic_fallback",
            ),
        };
        let actions = serde_json::to_value(actions).map_err(|error| {
            ProjectApiError::bad_request(format!("优化建议序列化失败：{error}"))
        })?;
        let mut evidence = optimization_proposal_evidence(
            finding,
            &report.summary,
            report.window_start,
            report.window_days,
            report.generated_at,
            representative_runs,
        );
        if let Some(evidence) = evidence.as_object_mut() {
            evidence.insert(
                "proposalGeneration".into(),
                json!({
                    "mode": generation_mode,
                    "promptVersion": proposal_prompt_version,
                }),
            );
        }
        sqlx::query(
            r#"
            INSERT INTO decision_optimization_proposals (
                project_id, source_version_id, finding_code, finding_level, element,
                title, summary, proposed_actions_json, evidence_json, analysis_snapshot_json,
                generated_by
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (project_id, source_version_id, finding_code, element)
                WHERE status = 'open'
            DO UPDATE SET finding_level = EXCLUDED.finding_level,
                          title = EXCLUDED.title,
                          summary = EXCLUDED.summary,
                          proposed_actions_json = EXCLUDED.proposed_actions_json,
                          evidence_json = EXCLUDED.evidence_json,
                          analysis_snapshot_json = EXCLUDED.analysis_snapshot_json,
                          generated_at = NOW(), generated_by = EXCLUDED.generated_by
            "#,
        )
        .bind(project_id)
        .bind(report.version_id)
        .bind(&finding.code)
        .bind(&finding.level)
        .bind(&finding.element)
        .bind(title)
        .bind(summary)
        .bind(actions)
        .bind(evidence)
        .bind(&snapshot)
        .bind(&claims.sub)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("保存优化建议", error))?;
    }
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(report.version_id),
        "optimization_proposal.generated",
        &claims.sub,
        &format!(
            "基于发布版本 v{} 更新 {} 条优化建议",
            report.version_number,
            report.findings.len()
        ),
        json!({
            "sourceVersionId": report.version_id,
            "sourceVersionNumber": report.version_number,
            "findingCount": report.findings.len(),
            "windowDays": report.window_days,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录优化建议审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交优化建议", error))?;
    Ok(Json(
        list_optimization_proposals_for_project(&state, &claims.sub, project_id).await?,
    ))
}

async fn simulate_optimization_proposal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, proposal_id, candidate_version_id)): Path<(i64, i64, i64)>,
) -> Result<Json<OptimizationSimulationReport>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let versions = sqlx::query_as::<
        _,
        (
            i64,
            i32,
            String,
            i32,
            String,
            String,
            Option<Value>,
            Option<Value>,
            Option<Value>,
            Option<Value>,
            Option<Value>,
        ),
    >(
        r#"SELECT proposal.source_version_id, baseline.version_number, baseline_artifact.dmn_xml,
                  candidate.version_number, candidate_artifact.dmn_xml, candidate_artifact.dsl_yaml,
                  candidate_artifact.decision_brief_json, candidate_artifact.decision_plan_json,
                  candidate_artifact.model_patch_json, candidate_artifact.knowledge_citations_json,
                  candidate_artifact.change_review_json
           FROM decision_optimization_proposals proposal
           JOIN decision_versions baseline
             ON baseline.id = proposal.source_version_id AND baseline.project_id = proposal.project_id
           JOIN decision_artifacts baseline_artifact ON baseline_artifact.version_id = baseline.id
           JOIN decision_versions candidate
             ON candidate.id = $3 AND candidate.project_id = proposal.project_id
            JOIN decision_projects project
             ON project.id = proposal.project_id
            AND project.published_version_id = proposal.source_version_id
           JOIN decision_artifacts candidate_artifact ON candidate_artifact.version_id = candidate.id
           WHERE proposal.id = $1 AND proposal.project_id = $2
             AND proposal.status = 'open'
             AND baseline.status = 'published'
             AND candidate.base_version_id = proposal.source_version_id
             AND candidate.status = 'draft'"#,
    )
    .bind(proposal_id)
    .bind(project_id)
    .bind(candidate_version_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取优化模拟版本", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (
        baseline_version_id,
        baseline_number,
        baseline_xml,
        candidate_number,
        candidate_xml,
        candidate_dsl,
        candidate_brief,
        candidate_plan,
        candidate_patch,
        candidate_citations,
        candidate_review,
    ) = versions;
    if baseline_version_id == candidate_version_id {
        return Err(ProjectApiError::bad_request("候选版本必须与建议基线不同"));
    }
    let baseline_model = crate::dmn::parse_dmn(&baseline_xml)
        .map_err(|error| ProjectApiError::bad_request(format!("基线 DMN 无效：{error}")))?;
    let candidate_model = crate::dmn::parse_dmn(&candidate_xml)
        .map_err(|error| ProjectApiError::bad_request(format!("候选 DMN 无效：{error}")))?;
    crate::dmn::compile_dmn(&baseline_model)
        .map_err(|error| ProjectApiError::bad_request(format!("基线 DMN 编译失败：{error}")))?;
    crate::dmn::compile_dmn(&candidate_model)
        .map_err(|error| ProjectApiError::bad_request(format!("候选 DMN 编译失败：{error}")))?;

    let mut test_cases = sqlx::query_as::<_, (i64, Value, Value)>(
        r#"SELECT id, input_json, expected_output_json
           FROM decision_test_cases
           WHERE version_id = $1 AND category = 'business' AND approved_by IS NOT NULL
           ORDER BY created_at, id LIMIT 1000"#,
    )
    .bind(baseline_version_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取优化模拟验收集", error))?;
    let sample_source = if test_cases.is_empty() {
        test_cases = sqlx::query_as::<_, (i64, Value, Value)>(
            r#"SELECT id, input_json, output_json
               FROM decision_runs
               WHERE project_id = $1 AND version_id = $2 AND status = 'succeeded'
               ORDER BY executed_at DESC, id DESC LIMIT 1000"#,
        )
        .bind(project_id)
        .bind(baseline_version_id)
        .fetch_all(&state.db)
        .await
        .map_err(|error| ProjectApiError::database("读取优化模拟历史样本", error))?;
        "successful_history_runs"
    } else {
        "approved_business_tests"
    };
    if test_cases.is_empty() {
        return Err(ProjectApiError::bad_request(
            "基线版本没有已确认验收案例或成功历史记录，无法模拟",
        ));
    }

    let (baseline_passed, candidate_passed, total) = tokio::task::spawn_blocking(move || {
        let mut baseline_passed = 0;
        let mut candidate_passed = 0;
        for (source_run_id, input, expected_output) in &test_cases {
            let baseline_case = HistoricalRegressionCase {
                source_run_id: *source_run_id,
                input: input.clone(),
                expected_output: expected_output.clone(),
            };
            let candidate_case = baseline_case.clone();
            if execute_historical_regression_case(
                &baseline_model,
                baseline_version_id,
                baseline_number,
                baseline_case,
            )
            .status
                == "passed"
            {
                baseline_passed += 1;
            }
            if execute_historical_regression_case(
                &candidate_model,
                candidate_version_id,
                candidate_number,
                candidate_case,
            )
            .status
                == "passed"
            {
                candidate_passed += 1;
            }
        }
        (baseline_passed, candidate_passed, test_cases.len())
    })
    .await
    .map_err(|error| {
        ProjectApiError::coded(
            StatusCode::INTERNAL_SERVER_ERROR,
            "OPTIMIZATION_SIMULATION_FAILED",
            format!("优化模拟任务失败：{error}"),
        )
    })?;
    let metrics = calculate_metrics(baseline_passed, candidate_passed, total);
    let passed = passes_simulation(metrics);
    let mut estimated_business_impact = impact_summary(metrics);
    if !passed {
        estimated_business_impact.push_str(" 候选版本一致率低于基线，模拟未通过。");
    }
    let candidate_artifact = json!({
        "versionNumber": candidate_number,
        "dslYaml": candidate_dsl,
        "dmnXml": candidate_xml,
        "decisionBrief": candidate_brief,
        "decisionPlan": candidate_plan,
        "modelPatch": candidate_patch,
        "knowledgeCitations": candidate_citations.unwrap_or_else(|| json!([])),
    });
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存优化模拟结果", error))?;
    let inserted = sqlx::query_as::<_, (i64, chrono::DateTime<chrono::Utc>)>(
        r#"INSERT INTO optimization_simulations (
            suggestion_id, baseline_version_id, version_id, test_set_size,
            baseline_accuracy, simulated_accuracy, accuracy_improvement,
            bias_rate_baseline, bias_rate_simulated, estimated_business_impact,
            test_set_source, created_by, passed, candidate_artifact_json
        ) VALUES (
            $1, $2, $3, $4, $5::FLOAT8::NUMERIC(5,2), $6::FLOAT8::NUMERIC(5,2),
            $7::FLOAT8::NUMERIC(6,2), $8::FLOAT8::NUMERIC(5,2),
            $9::FLOAT8::NUMERIC(5,2), $10, $11, $12, $13, $14
        ) RETURNING id, created_at"#,
    )
    .bind(proposal_id)
    .bind(baseline_version_id)
    .bind(candidate_version_id)
    .bind(total as i32)
    .bind(metrics.baseline_accuracy)
    .bind(metrics.candidate_accuracy)
    .bind(metrics.accuracy_improvement)
    .bind(metrics.baseline_bias_rate)
    .bind(metrics.candidate_bias_rate)
    .bind(&estimated_business_impact)
    .bind(sample_source)
    .bind(&claims.sub)
    .bind(passed)
    .bind(candidate_artifact)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存优化模拟结果", error))?;
    let proposal_withdrawn = if !should_withdraw_proposal(metrics) {
        false
    } else {
        let resolution_note = format!(
            "候选版本 v{candidate_number} 在 {} 个样本上的一致率低于基线 v{baseline_number}，系统已撤回此建议。",
            metrics.test_set_size
        );
        let updated = sqlx::query(
            r#"
            UPDATE decision_optimization_proposals
            SET status = 'withdrawn', resolved_by = $1, resolved_at = NOW(),
                resolution_note = $2
            WHERE id = $3 AND project_id = $4 AND status = 'open'
            "#,
        )
        .bind(&claims.sub)
        .bind(&resolution_note)
        .bind(proposal_id)
        .bind(project_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("撤回模拟未通过的优化建议", error))?;
        let withdrawn = updated.rows_affected() > 0;
        if withdrawn {
            estimated_business_impact.push_str(" 优化建议已自动撤回。");
            insert_audit_event(
                &mut *transaction,
                project_id,
                Some(baseline_version_id),
                "optimization_proposal.simulation_withdrawn",
                &claims.sub,
                &resolution_note,
                json!({
                    "proposalId": proposal_id,
                    "simulationId": inserted.0,
                    "baselineVersionId": baseline_version_id,
                    "candidateVersionId": candidate_version_id,
                    "baselineAccuracy": metrics.baseline_accuracy,
                    "candidateAccuracy": metrics.candidate_accuracy,
                    "sampleCount": metrics.test_set_size,
                }),
            )
            .await
            .map_err(|error| ProjectApiError::database("记录模拟失败撤回审计事件", error))?;
        }
        withdrawn
    };
    let shadow_policy = OptimizationAutodraftShadowPolicy::from_environment();
    let shadow_evaluation = if shadow_policy.includes(project_id) {
        let (project_risk, finding_code) = sqlx::query_as::<_, (String, String)>(
            r#"SELECT project.risk_level, proposal.finding_code
               FROM decision_projects project
               JOIN decision_optimization_proposals proposal
                 ON proposal.project_id = project.id
               WHERE project.id = $1 AND proposal.id = $2"#,
        )
        .bind(project_id)
        .bind(proposal_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("读取自动草稿 shadow 门控", error))?;
        let outcome = optimization_autodraft_shadow_outcome(
            &project_risk,
            &finding_code,
            sample_source,
            total,
            passed,
            candidate_review.as_ref(),
        );
        let saved = sqlx::query(
            r#"INSERT INTO optimization_autodraft_shadow_evaluations (
                   simulation_id, project_id, proposal_id, source_version_id,
                   candidate_version_id, outcome, finding_code, sample_source,
                   test_set_size, baseline_accuracy, simulated_accuracy, evaluated_by
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
               ON CONFLICT (simulation_id) DO NOTHING"#,
        )
        .bind(inserted.0)
        .bind(project_id)
        .bind(proposal_id)
        .bind(baseline_version_id)
        .bind(candidate_version_id)
        .bind(outcome)
        .bind(&finding_code)
        .bind(sample_source)
        .bind(total as i32)
        .bind(metrics.baseline_accuracy)
        .bind(metrics.candidate_accuracy)
        .bind(&claims.sub)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("保存自动草稿 shadow 评估", error))?;
        if saved.rows_affected() > 0 {
            insert_audit_event(
                &mut *transaction,
                project_id,
                Some(baseline_version_id),
                "optimization_proposal.autodraft_shadow_evaluated",
                &claims.sub,
                "记录优化草稿 shadow 门控结果",
                json!({
                    "simulationId": inserted.0,
                    "proposalId": proposal_id,
                    "candidateVersionId": candidate_version_id,
                    "outcome": outcome,
                    "sampleCount": total,
                    "sampleSource": sample_source,
                }),
            )
            .await
            .map_err(|error| ProjectApiError::database("记录自动草稿 shadow 审计", error))?;
        }
        Some(outcome.to_string())
    } else {
        None
    };
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交优化模拟结果", error))?;
    let mut autodraft_created_version_id = None;
    let mut autodraft_error = None;
    if shadow_policy.draft_enabled
        && shadow_evaluation.as_deref() == Some("eligible_shadow_candidate")
    {
        match create_optimization_proposal_draft(
            &state,
            &claims,
            project_id,
            proposal_id,
            true,
            Some(inserted.0),
        )
        .await
        {
            Ok((_, workspace)) => {
                autodraft_created_version_id = workspace.version.as_ref().map(|version| version.id);
            }
            Err(error) => {
                let message = error.message;
                let mut audit =
                    state.db.begin().await.map_err(|db_error| {
                        ProjectApiError::database("记录自动草稿失败", db_error)
                    })?;
                insert_audit_event(
                    &mut *audit,
                    project_id,
                    Some(baseline_version_id),
                    "optimization_proposal.autodraft_failed",
                    &claims.sub,
                    "自动草稿门控通过，但草稿创建失败；需要人工处理",
                    json!({"proposalId": proposal_id, "simulationId": inserted.0, "error": &message}),
                )
                .await
                .map_err(|db_error| ProjectApiError::database("记录自动草稿失败审计", db_error))?;
                audit.commit().await.map_err(|db_error| {
                    ProjectApiError::database("提交自动草稿失败审计", db_error)
                })?;
                autodraft_error = Some(message);
            }
        }
    }
    Ok(Json(OptimizationSimulationReport {
        simulation_id: inserted.0,
        proposal_id,
        baseline_version_id,
        candidate_version_id,
        sample_source: sample_source.into(),
        test_set_size: metrics.test_set_size,
        minimum_sample_count: MIN_SIMULATION_SAMPLE_COUNT,
        baseline_accuracy: metrics.baseline_accuracy,
        simulated_accuracy: metrics.candidate_accuracy,
        accuracy_improvement: metrics.accuracy_improvement,
        bias_rate_baseline: metrics.baseline_bias_rate,
        bias_rate_simulated: metrics.candidate_bias_rate,
        passed,
        proposal_withdrawn,
        shadow_evaluation,
        autodraft_created_version_id,
        autodraft_error,
        estimated_business_impact,
        created_at: Some(inserted.1),
    }))
}

async fn dismiss_optimization_proposal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, proposal_id)): Path<(i64, i64)>,
) -> Result<Json<DecisionOptimizationProposal>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("驳回优化建议", error))?;
    let updated = sqlx::query(
        r#"
        UPDATE decision_optimization_proposals proposal
        SET status = 'dismissed', resolved_by = $1, resolved_at = NOW(),
            resolution_note = '人工驳回'
        WHERE proposal.id = $2 AND proposal.project_id = $3 AND proposal.status = 'open'
        "#,
    )
    .bind(&claims.sub)
    .bind(proposal_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("驳回优化建议", error))?;
    if updated.rows_affected() == 0 {
        return Err(ProjectApiError::bad_request("该优化建议不可再驳回"));
    }
    insert_audit_event(
        &mut *transaction,
        project_id,
        None,
        "optimization_proposal.dismissed",
        &claims.sub,
        "驳回运行优化建议",
        json!({ "proposalId": proposal_id }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录优化建议驳回审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交优化建议驳回", error))?;
    let proposal = list_optimization_proposals_for_project(&state, &claims.sub, project_id)
        .await?
        .into_iter()
        .find(|proposal| proposal.id == proposal_id)
        .ok_or_else(ProjectApiError::not_found)?;
    Ok(Json(proposal))
}

async fn confirm_optimization_proposal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, proposal_id)): Path<(i64, i64)>,
) -> Result<(StatusCode, Json<DecisionWorkspace>), ProjectApiError> {
    create_optimization_proposal_draft(&state, &claims, project_id, proposal_id, false, None).await
}

async fn create_optimization_proposal_draft(
    state: &AppState,
    claims: &Claims,
    project_id: i64,
    proposal_id: i64,
    automated: bool,
    expected_simulation_id: Option<i64>,
) -> Result<(StatusCode, Json<DecisionWorkspace>), ProjectApiError> {
    if automated {
        let policy = OptimizationAutodraftShadowPolicy::from_environment();
        if !policy.draft_enabled || !policy.includes(project_id) {
            return Err(ProjectApiError::bad_request(
                "自动草稿模式或项目白名单已关闭",
            ));
        }
    }
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("确认优化建议", error))?;
    let mut project = sqlx::query_as::<_, DecisionProject>(
        r#"
        SELECT id, owner_username, name, description, risk_level,
               current_draft_version_id, published_version_id, created_at, updated_at
        FROM decision_projects WHERE id = $1 FOR UPDATE
        "#,
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取优化建议项目", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let source_version_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT source_version_id FROM decision_optimization_proposals
        WHERE id = $1 AND project_id = $2 AND status = 'open' FOR UPDATE
        "#,
    )
    .bind(proposal_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取优化建议", error))?
    .ok_or_else(|| ProjectApiError::bad_request("该优化建议不可再确认"))?;
    let (
        simulation_baseline_id,
        candidate_version_id,
        simulation_passed,
        candidate_artifact,
        simulation_id,
    ) = sqlx::query_as::<_, (i64, i64, bool, Value, i64)>(
        r#"
            SELECT baseline_version_id, version_id, passed, candidate_artifact_json, id
            FROM optimization_simulations
            WHERE suggestion_id = $1 AND ($2::BIGINT IS NULL OR id = $2)
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            FOR UPDATE
            "#,
    )
    .bind(proposal_id)
    .bind(expected_simulation_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取优化建议模拟结果", error))?
    .ok_or_else(|| ProjectApiError::bad_request("请先运行候选版本效果模拟"))?;
    if !simulation_passed || simulation_baseline_id != source_version_id {
        return Err(ProjectApiError::bad_request(
            "最近一次候选版本模拟未通过，请重新运行模拟后再确认",
        ));
    }
    if automated {
        let eligible: bool = sqlx::query_scalar(
            r#"SELECT EXISTS (
                   SELECT 1 FROM optimization_autodraft_shadow_evaluations
                   WHERE simulation_id = $1 AND project_id = $2 AND proposal_id = $3
                     AND outcome = 'eligible_shadow_candidate'
               )"#,
        )
        .bind(simulation_id)
        .bind(project_id)
        .bind(proposal_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("复核自动草稿资格", error))?;
        if !eligible {
            return Err(ProjectApiError::bad_request(
                "本次模拟没有通过自动草稿资格门槛",
            ));
        }
    }
    let candidate_version_number = candidate_artifact
        .get("versionNumber")
        .and_then(Value::as_i64)
        .and_then(|number| i32::try_from(number).ok())
        .ok_or_else(|| ProjectApiError::bad_request("通过模拟的候选版本快照无效"))?;
    let candidate_dsl = candidate_artifact
        .get("dslYaml")
        .and_then(Value::as_str)
        .ok_or_else(|| ProjectApiError::bad_request("通过模拟的候选 DSL 快照缺失"))?
        .to_owned();
    let candidate_dmn = candidate_artifact
        .get("dmnXml")
        .and_then(Value::as_str)
        .ok_or_else(|| ProjectApiError::bad_request("通过模拟的候选 DMN 快照缺失"))?
        .to_owned();
    let candidate_brief = candidate_artifact
        .get("decisionBrief")
        .cloned()
        .filter(|value| !value.is_null());
    let candidate_plan = candidate_artifact
        .get("decisionPlan")
        .cloned()
        .filter(|value| !value.is_null());
    let candidate_patch = candidate_artifact
        .get("modelPatch")
        .cloned()
        .filter(|value| !value.is_null());
    let candidate_citations = candidate_artifact
        .get("knowledgeCitations")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let knowledge_citations =
        serde_json::from_value::<Vec<KnowledgeCitation>>(candidate_citations.clone()).map_err(
            |error| ProjectApiError::bad_request(format!("通过模拟的候选知识引用无效：{error}")),
        )?;
    if project.published_version_id != Some(source_version_id) {
        return Err(ProjectApiError::bad_request(
            "建议基于的发布版本已变化，请重新生成优化建议",
        ));
    }
    if project.current_draft_version_id != Some(source_version_id) {
        return Err(ProjectApiError::bad_request(
            "当前已有未发布草稿，请先完成、放弃或发布该草稿后再确认优化建议",
        ));
    }
    let source = sqlx::query_as::<_, (i32, String)>(
        r#"
        SELECT version.version_number, version.status
        FROM decision_versions version
        WHERE version.id = $1 AND version.project_id = $2
        "#,
    )
    .bind(source_version_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取优化建议基线版本", error))?
    .ok_or_else(|| ProjectApiError::bad_request("优化建议基线版本不存在"))?;
    let (source_number, source_status) = source;
    if source_status != "published" {
        return Err(ProjectApiError::bad_request(
            "优化建议只能基于当前已发布版本确认",
        ));
    }
    let source_dsl = sqlx::query_scalar::<_, String>(
        "SELECT dsl_yaml FROM decision_artifacts WHERE version_id = $1",
    )
    .bind(source_version_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取优化建议基线 DSL", error))?
    .ok_or_else(|| ProjectApiError::bad_request("优化建议基线制品不存在"))?;
    let baseline_definitions = nl_dsl::parse_definitions(&source_dsl)
        .map_err(|error| ProjectApiError::bad_request(format!("优化建议基线 DSL 无效：{error}")))?;
    let candidate_definitions = nl_dsl::parse_definitions(&candidate_dsl).map_err(|error| {
        ProjectApiError::bad_request(format!("通过模拟的候选 DSL 无效：{error}"))
    })?;
    let version_number = sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM decision_versions WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("计算优化草稿版本号", error))?;
    let review = build_change_review(
        Some(&baseline_definitions),
        &candidate_definitions,
        None,
        Some(source_number),
        Some(version_number),
    );
    let review_json = serde_json::to_value(&review).map_err(|error| {
        ProjectApiError::bad_request(format!("优化草稿审查序列化失败：{error}"))
    })?;
    let change_summary = json!({
        "optimizationProposalId": proposal_id,
        "sourceVersionId": source_version_id,
        "sourceVersionNumber": source_number,
        "candidateVersionId": candidate_version_id,
        "candidateVersionNumber": candidate_version_number,
        "simulationPassed": true,
        "message": if automated {
            "自动草稿试点根据已通过模拟的候选版本制品创建草稿，后续仍需完成常规验收与复核。"
        } else {
            "根据已通过模拟的候选版本制品创建草稿，后续仍需完成常规验收与复核。"
        },
    });
    let version = sqlx::query_as::<_, DecisionVersion>(
        r#"
        INSERT INTO decision_versions (
            project_id, version_number, status, base_version_id, parent_revision_id,
            change_summary, created_by,
            reviewed_by, reviewed_at
        ) VALUES (
            $1,
            $2,
            'draft',
            $3,
            (SELECT revision_id FROM decision_versions WHERE id = $3),
            $4,
            $5,
            NULL,
            NULL
        )
        RETURNING id, project_id, version_number, status, base_version_id,
                  rollback_source_version_id, rollback_reason, change_summary,
                  created_by, created_at, reviewed_by, reviewed_at
        "#,
    )
    .bind(project_id)
    .bind(version_number)
    .bind(source_version_id)
    .bind(change_summary)
    .bind(&claims.sub)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("创建优化草稿", error))?;
    sqlx::query(
        r#"
        INSERT INTO decision_artifacts (
            version_id, dsl_yaml, dmn_xml, decision_brief_json, decision_plan_json,
            model_patch_json, change_review_json, knowledge_citations_json
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(version.id)
    .bind(&candidate_dsl)
    .bind(&candidate_dmn)
    .bind(candidate_brief)
    .bind(candidate_plan)
    .bind(candidate_patch)
    .bind(review_json)
    .bind(&candidate_citations)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("复制优化草稿制品", error))?;
    sqlx::query(
        r#"
        INSERT INTO decision_test_cases (
            version_id, name, category, input_json, expected_output_json, status, source
        )
        SELECT $1, name, category, input_json, expected_output_json, 'pending', source
        FROM decision_test_cases WHERE version_id = $2 AND category = 'business'
        "#,
    )
    .bind(version.id)
    .bind(source_version_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("复制优化草稿业务验收案例", error))?;
    sqlx::query(
        r#"
        UPDATE decision_optimization_proposals
        SET status = 'confirmed', resolved_by = $1, resolved_at = NOW(),
            resolution_note = $5, resulting_version_id = $2
        WHERE id = $3 AND project_id = $4 AND status = 'open'
        "#,
    )
    .bind(&claims.sub)
    .bind(version.id)
    .bind(proposal_id)
    .bind(project_id)
    .bind(if automated {
        "自动草稿试点创建，待人工验收、复核和发布"
    } else {
        "人工确认并创建优化草稿"
    })
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("确认优化建议", error))?;
    sqlx::query(
        "UPDATE decision_projects SET current_draft_version_id = $1, updated_at = NOW() WHERE id = $2",
    )
    .bind(version.id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新优化草稿版本", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version.id),
        if automated {
            "optimization_proposal.autodraft_created"
        } else {
            "optimization_proposal.confirmed"
        },
        &claims.sub,
        &format!(
            "{}模拟通过的候选版本 v{candidate_version_number} 并创建草稿 v{version_number}",
            if automated { "自动准备" } else { "确认" }
        ),
        json!({
            "proposalId": proposal_id,
            "sourceVersionId": source_version_id,
            "sourceVersionNumber": source_number,
            "simulationCandidateVersionId": candidate_version_id,
            "simulationPassed": true,
            "resultingVersionId": version.id,
            "resultingVersionNumber": version.version_number,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录优化建议确认审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交优化草稿", error))?;
    project.current_draft_version_id = Some(version.id);
    Ok((
        StatusCode::CREATED,
        Json(DecisionWorkspace {
            project,
            version: Some(version),
            dsl_yaml: Some(candidate_dsl),
            dmn_xml: Some(candidate_dmn),
            knowledge_citations,
        }),
    ))
}

async fn get_decision_analytics(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<DecisionAnalyticsReport>, ProjectApiError> {
    Ok(Json(
        load_published_analytics(&state, &claims.sub, project_id).await?,
    ))
}

fn validate_analytics_thresholds(
    thresholds: &DecisionAnalyticsThresholds,
) -> Result<(), ProjectApiError> {
    let pairs = [
        (
            thresholds.failure_warning_rate,
            thresholds.failure_high_rate,
        ),
        (
            thresholds.mismatch_warning_rate,
            thresholds.mismatch_high_rate,
        ),
    ];
    if pairs.iter().any(|(warning, high)| {
        !warning.is_finite()
            || !high.is_finite()
            || *warning <= 0.0
            || *warning >= 1.0
            || *high <= *warning
            || *high > 1.0
    }) {
        return Err(ProjectApiError::bad_request(
            "预警阈值必须在 0 到 1 之间，且高风险阈值必须大于预警阈值",
        ));
    }
    if !(1..=100_000).contains(&thresholds.failure_min_runs)
        || !(1..=100_000).contains(&thresholds.mismatch_min_samples)
    {
        return Err(ProjectApiError::bad_request(
            "失败率和偏差率最小样本数必须在 1 到 100000 之间",
        ));
    }
    Ok(())
}

async fn get_decision_analytics_thresholds(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<DecisionAnalyticsThresholds>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let thresholds = sqlx::query_as::<_, DecisionAnalyticsThresholds>(
        r#"
        SELECT failure_warning_rate, failure_high_rate, failure_min_runs,
               mismatch_warning_rate, mismatch_high_rate, mismatch_min_samples,
               updated_at
        FROM decision_analytics_thresholds
        WHERE project_id = $1
        "#,
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取项目分析阈值", error))?
    .unwrap_or_default();
    Ok(Json(thresholds))
}

async fn update_decision_analytics_thresholds(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(thresholds): Json<DecisionAnalyticsThresholds>,
) -> Result<Json<DecisionAnalyticsThresholds>, ProjectApiError> {
    validate_analytics_thresholds(&thresholds)?;
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存项目分析阈值", error))?;
    let saved = sqlx::query_as::<_, DecisionAnalyticsThresholds>(
        r#"
        INSERT INTO decision_analytics_thresholds (
            project_id, failure_warning_rate, failure_high_rate, failure_min_runs,
            mismatch_warning_rate, mismatch_high_rate, mismatch_min_samples, updated_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (project_id) DO UPDATE SET
            failure_warning_rate = EXCLUDED.failure_warning_rate,
            failure_high_rate = EXCLUDED.failure_high_rate,
            failure_min_runs = EXCLUDED.failure_min_runs,
            mismatch_warning_rate = EXCLUDED.mismatch_warning_rate,
            mismatch_high_rate = EXCLUDED.mismatch_high_rate,
            mismatch_min_samples = EXCLUDED.mismatch_min_samples,
            updated_by = EXCLUDED.updated_by,
            updated_at = NOW()
        RETURNING failure_warning_rate, failure_high_rate, failure_min_runs,
                  mismatch_warning_rate, mismatch_high_rate, mismatch_min_samples,
                  updated_at
        "#,
    )
    .bind(project_id)
    .bind(thresholds.failure_warning_rate)
    .bind(thresholds.failure_high_rate)
    .bind(thresholds.failure_min_runs)
    .bind(thresholds.mismatch_warning_rate)
    .bind(thresholds.mismatch_high_rate)
    .bind(thresholds.mismatch_min_samples)
    .bind(&claims.sub)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存项目分析阈值", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        None,
        "decision_analytics.thresholds_updated",
        &claims.sub,
        "更新项目运行分析阈值",
        serde_json::to_value(&saved).unwrap_or(Value::Null),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录项目分析阈值审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交项目分析阈值", error))?;
    Ok(Json(saved))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecisionCopilotMetric {
    TotalRuns,
    SuccessRate,
    FailureRate,
    MismatchRate,
    FeedbackCoverage,
    AverageLatency,
    P95Latency,
}

impl DecisionCopilotMetric {
    fn key(self) -> &'static str {
        match self {
            Self::TotalRuns => "total_runs",
            Self::SuccessRate => "success_rate",
            Self::FailureRate => "failure_rate",
            Self::MismatchRate => "mismatch_rate",
            Self::FeedbackCoverage => "feedback_coverage",
            Self::AverageLatency => "average_latency",
            Self::P95Latency => "p95_latency",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::TotalRuns => "运行量",
            Self::SuccessRate => "成功率",
            Self::FailureRate => "失败率",
            Self::MismatchRate => "业务偏差率",
            Self::FeedbackCoverage => "反馈覆盖率",
            Self::AverageLatency => "平均耗时",
            Self::P95Latency => "P95 耗时",
        }
    }
}

fn parse_decision_copilot_intent(
    question: &str,
) -> (Option<DecisionCopilotMetric>, Option<u32>, &'static str) {
    let question = question.to_lowercase();
    let command = [
        "请帮我",
        "帮我",
        "帮忙",
        "请",
        "我想",
        "我需要",
        "能否",
        "可以",
        "将",
    ]
    .iter()
    .find_map(|prefix| question.strip_prefix(prefix))
    .unwrap_or(&question)
    .trim_start();
    let imperative_change = ["修改", "调整", "改成", "改为", "设置", "更新", "删除"]
        .iter()
        .any(|verb| command.starts_with(verb));
    let explicit_change = [
        "调整到",
        "改到",
        "改成",
        "改为",
        "调成",
        "设置为",
        "设为",
        "更新为",
    ]
    .iter()
    .any(|phrase| question.contains(phrase));
    let explicit_release_or_approval = [
        "发布模型",
        "发布版本",
        "发布决策",
        "发布当前",
        "发布这个",
        "发布该",
        "审批当前版本",
        "审批这个版本",
        "审批该版本",
        "审批此版本",
        "审批当前模型",
        "审批这个模型",
        "审批该模型",
        "审批方案",
        "批准发布",
    ]
    .iter()
    .any(|phrase| question.contains(phrase));
    if imperative_change || explicit_change || explicit_release_or_approval {
        return (None, None, "routed");
    }
    let candidates = [
        (
            DecisionCopilotMetric::TotalRuns,
            &["运行量", "运行次数", "调用量", "调用次数"][..],
        ),
        (
            DecisionCopilotMetric::SuccessRate,
            &["成功率", "成功比例"][..],
        ),
        (
            DecisionCopilotMetric::FailureRate,
            &["失败率", "失败比例"][..],
        ),
        (
            DecisionCopilotMetric::MismatchRate,
            &["偏差率", "不一致率"][..],
        ),
        (
            DecisionCopilotMetric::FeedbackCoverage,
            &["反馈覆盖率", "回填覆盖率"][..],
        ),
        (
            DecisionCopilotMetric::AverageLatency,
            &["平均耗时", "平均延迟"][..],
        ),
        (
            DecisionCopilotMetric::P95Latency,
            &["p95", "95分位", "95 分位"][..],
        ),
    ];
    let matches = candidates
        .iter()
        .filter(|(_, terms)| terms.iter().any(|term| question.contains(term)))
        .map(|(metric, _)| *metric)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return (None, None, "clarification");
    }
    let days_7 = [
        "近7天",
        "最近7天",
        "过去7天",
        "近 7 天",
        "最近 7 天",
        "过去 7 天",
        "近七天",
        "最近七天",
        "过去七天",
        "近一周",
        "最近一周",
        "过去一周",
    ]
    .iter()
    .any(|term| question.contains(term));
    let days_30 = [
        "近30天",
        "最近30天",
        "过去30天",
        "近 30 天",
        "最近 30 天",
        "过去 30 天",
        "近三十天",
        "最近三十天",
        "过去三十天",
        "近一个月",
        "最近一个月",
        "过去一个月",
    ]
    .iter()
    .any(|term| question.contains(term));
    let unsupported_window = ["昨天", "前天", "今天", "今日", "今年", "去年"]
        .iter()
        .any(|term| question.contains(term))
        || ["周", "月", "季度", "年"]
            .iter()
            .any(|unit| question.contains(unit))
            && !days_7
            && !days_30;
    if days_7 && days_30
        || question.contains('天') && !days_7 && !days_30
        || unsupported_window
        || matches[0] == DecisionCopilotMetric::P95Latency && days_7
    {
        return (None, None, "clarification");
    }
    (
        Some(matches[0]),
        Some(if days_7 { 7 } else { 30 }),
        "answered",
    )
}

fn decision_copilot_enabled_from_value(value: Option<&str>) -> bool {
    match value {
        None => true,
        Some(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
    }
}

fn decision_copilot_enabled_from_environment() -> bool {
    match std::env::var("DECISION_COPILOT_ENABLED") {
        Ok(value) => decision_copilot_enabled_from_value(Some(&value)),
        Err(std::env::VarError::NotPresent) => true,
        Err(std::env::VarError::NotUnicode(_)) => false,
    }
}

fn copilot_rate(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn decision_copilot_query_result(
    report: &DecisionAnalyticsReport,
    metric: DecisionCopilotMetric,
    days: u32,
) -> DecisionCopilotQueryResponse {
    let start_date = report.generated_at.date_naive() - chrono::Duration::days(i64::from(days - 1));
    let window_start = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        start_date.and_hms_opt(0, 0, 0).expect("midnight is valid"),
        chrono::Utc,
    );
    let values = if days == 30 {
        let summary = &report.summary;
        (
            summary.total_runs as u64,
            summary.succeeded_runs as u64,
            summary.failed_runs as u64,
            summary.outcome_reported_runs as u64,
            summary.conclusive_outcome_runs as u64,
            summary.mismatch_runs as u64,
            summary.average_duration_ms,
        )
    } else {
        report
            .daily_trends
            .iter()
            .filter(|day| day.date >= start_date)
            .fold(
                (0_u64, 0_u64, 0_u64, 0_u64, 0_u64, 0_u64, 0.0_f64),
                |(total, succeeded, failed, reported, conclusive, mismatch, latency), day| {
                    (
                        total + day.total_runs as u64,
                        succeeded + day.succeeded_runs as u64,
                        failed + day.failed_runs as u64,
                        reported + day.outcome_reported_runs as u64,
                        conclusive + day.conclusive_outcome_runs as u64,
                        mismatch + day.mismatch_runs as u64,
                        latency + day.average_duration_ms * day.total_runs as f64,
                    )
                },
            )
    };
    let (total, succeeded, failed, reported, conclusive, mismatch, latency) = values;
    let (value, unit, sample_size, numerator, denominator, definition) = match metric {
        DecisionCopilotMetric::TotalRuns => (
            total as f64,
            "次",
            total,
            Some(total),
            Some(total),
            "时间窗内决策服务运行总次数",
        ),
        DecisionCopilotMetric::SuccessRate => (
            copilot_rate(succeeded, total) * 100.0,
            "%",
            total,
            Some(succeeded),
            Some(total),
            "成功运行数 / 全部运行数",
        ),
        DecisionCopilotMetric::FailureRate => (
            copilot_rate(failed, total) * 100.0,
            "%",
            total,
            Some(failed),
            Some(total),
            "失败运行数 / 全部运行数",
        ),
        DecisionCopilotMetric::MismatchRate => (
            copilot_rate(mismatch, conclusive) * 100.0,
            "%",
            conclusive,
            Some(mismatch),
            Some(conclusive),
            "偏差反馈数 / 有效业务反馈数",
        ),
        DecisionCopilotMetric::FeedbackCoverage => (
            copilot_rate(reported, total) * 100.0,
            "%",
            total,
            Some(reported),
            Some(total),
            "已回填运行数 / 全部运行数",
        ),
        DecisionCopilotMetric::AverageLatency => (
            if days == 30 {
                latency
            } else if total > 0 {
                latency / total as f64
            } else {
                0.0
            },
            "毫秒",
            total,
            None,
            None,
            "按运行次数加权的平均执行耗时",
        ),
        DecisionCopilotMetric::P95Latency => (
            report.summary.p95_duration_ms as f64,
            "毫秒",
            total,
            None,
            None,
            "全部运行耗时的第 95 百分位数",
        ),
    };
    let status = if sample_size == 0 {
        "insufficient"
    } else {
        "answered"
    };
    let answer = if sample_size == 0 {
        format!(
            "最近 {days} 天没有足够的运行样本，无法计算{}。",
            metric.label()
        )
    } else if unit == "%" {
        format!(
            "最近 {days} 天的{}为 {:.1}%，样本 {}/{}。",
            metric.label(),
            value,
            numerator.unwrap_or_default(),
            denominator.unwrap_or_default()
        )
    } else if unit == "次" {
        format!("最近 {days} 天的{}为 {} 次。", metric.label(), value as u64)
    } else {
        format!("最近 {days} 天的{}为 {:.1} {unit}。", metric.label(), value)
    };
    DecisionCopilotQueryResponse {
        status: status.into(),
        answer,
        metric: Some(metric.key().into()),
        window_days: Some(days),
        window_start: Some(window_start),
        generated_at: Some(report.generated_at),
        value: Some(value),
        unit: Some(unit.into()),
        sample_size: Some(sample_size),
        numerator,
        denominator,
        definition: Some(definition.into()),
        source: format!("/api/projects/{}/decision-analytics", report.project_id),
    }
}

async fn query_decision_copilot(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<DecisionCopilotQueryRequest>,
) -> Result<Json<DecisionCopilotQueryResponse>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    if !decision_copilot_enabled_from_environment() {
        return Err(ProjectApiError::coded(
            StatusCode::SERVICE_UNAVAILABLE,
            "COPILOT_DISABLED",
            "运营 Copilot 查询当前已关闭",
        ));
    }
    let question = payload.question.trim();
    if question.is_empty() || question.len() > 500 {
        return Err(ProjectApiError::bad_request(
            "问题不能为空且不能超过 500 字节",
        ));
    }
    let (metric, days, intent_status) = parse_decision_copilot_intent(question);
    let response = match (intent_status, metric, days) {
        ("answered", Some(metric), Some(days)) => {
            let report = load_published_analytics(&state, &claims.sub, project_id).await?;
            decision_copilot_query_result(&report, metric, days)
        }
        ("routed", _, _) => DecisionCopilotQueryResponse {
            status: "routed".into(),
            answer: "这是模型变更意图。请转到需求工作流中预览并提交变更；Copilot 不会直接修改或发布模型。".into(),
            metric: None,
            window_days: None,
            window_start: None,
            generated_at: None,
            value: None,
            unit: None,
            sample_size: None,
            numerator: None,
            denominator: None,
            definition: None,
            source: "decision-workflow".into(),
        },
        _ => DecisionCopilotQueryResponse {
            status: "clarification".into(),
            answer: "目前支持查询项目近 7/30 天的运行量、成功率、失败率、业务偏差率、反馈覆盖率、平均耗时，或近 30 天 P95 耗时。请明确一个指标和时间范围。".into(),
            metric: None,
            window_days: None,
            window_start: None,
            generated_at: None,
            value: None,
            unit: None,
            sample_size: None,
            numerator: None,
            denominator: None,
            definition: None,
            source: "decision-analytics".into(),
        },
    };
    let question_digest = format!("{:x}", Sha256::digest(question.as_bytes()));
    insert_audit_event(
        &state.db,
        project_id,
        None,
        "decision_copilot.query",
        &claims.sub,
        "查询只读运营 Copilot",
        json!({
            "status": response.status,
            "metric": response.metric,
            "windowDays": response.window_days,
            "questionSha256": question_digest,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录运营 Copilot 查询审计", error))?;
    Ok(Json(response))
}

const DRIFT_AUTOMATION_FINDING_CODES: &[&str] = &[
    "FAILURE_TREND_UP",
    "FEEDBACK_TREND_DOWN",
    "FREQUENT_FALLBACK",
    "HIGH_FAILURE_RATE",
    "HIGH_MISMATCH_RATE",
    "HIGH_P95_LATENCY",
    "INPUT_DISTRIBUTION_DRIFT",
    "INPUT_FIELD_MISSINGNESS_DRIFT",
    "LOW_FEEDBACK_COVERAGE",
    "LOW_RULE_COVERAGE",
    "NO_RECENT_RUNS",
    "RULE_FEEDBACK_MISMATCH",
    "RULE_HIT_DRIFT",
];

fn validate_drift_automation_rule(
    payload: &UpdateDecisionDriftAutomation,
) -> Result<(Vec<String>, Option<String>), String> {
    if !(1..=720).contains(&payload.cooldown_hours) {
        return Err("漂移自动化冷却时间必须在 1 到 720 小时之间".into());
    }
    if payload.finding_codes.len() > 20
        || (payload.enabled && payload.finding_codes.is_empty())
        || payload
            .finding_codes
            .iter()
            .any(|code| !DRIFT_AUTOMATION_FINDING_CODES.contains(&code.as_str()))
    {
        return Err("漂移信号为空、数量超限或包含不支持的发现类型".into());
    }
    let mut codes = payload.finding_codes.clone();
    codes.sort();
    codes.dedup();
    let webhook_url = payload
        .webhook_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(validate_drift_webhook_url)
        .transpose()?;
    Ok((codes, webhook_url))
}

fn validate_drift_event_review_label(label: &str) -> Result<&'static str, String> {
    match label {
        "confirmed_drift" | "false_positive" | "inconclusive" => Ok(match label {
            "confirmed_drift" => "confirmed_drift",
            "false_positive" => "false_positive",
            _ => "inconclusive",
        }),
        _ => Err("人工判定必须是 confirmed_drift、false_positive 或 inconclusive".into()),
    }
}

fn validate_drift_webhook_url(value: &str) -> Result<String, String> {
    if value.len() > 2048 {
        return Err("Webhook URL 超过 2048 字节".into());
    }
    let url = reqwest::Url::parse(value).map_err(|_| "Webhook URL 无效".to_string())?;
    let host = url
        .host_str()
        .ok_or_else(|| "Webhook URL 缺少主机名".to_string())?
        .to_ascii_lowercase();
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port().is_some()
        || host.parse::<std::net::IpAddr>().is_ok()
    {
        return Err(
            "Webhook 仅接受白名单主机的标准 HTTPS URL，且不能包含凭证、端口、查询或片段".into(),
        );
    }
    let allowed_hosts = std::env::var("DRIFT_WEBHOOK_ALLOWED_HOSTS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<HashSet<_>>();
    if !allowed_hosts.contains(&host) {
        return Err("Webhook 主机不在 DRIFT_WEBHOOK_ALLOWED_HOSTS 白名单中".into());
    }
    let signing_secret = std::env::var("DRIFT_WEBHOOK_SIGNING_SECRET").unwrap_or_default();
    if signing_secret.as_bytes().len() < 32 {
        return Err("配置 Webhook 前必须设置至少 32 字节的 DRIFT_WEBHOOK_SIGNING_SECRET".into());
    }
    Ok(url.to_string())
}

fn drift_event_analytics_report(
    project_id: i64,
    source_version_id: i64,
    mut evidence: Value,
) -> Result<DecisionAnalyticsReport, String> {
    let source_version_number = evidence
        .get("sourceVersionNumber")
        .cloned()
        .ok_or_else(|| "漂移事件缺少源版本号".to_string())?;
    evidence["projectId"] = json!(project_id);
    evidence["versionId"] = json!(source_version_id);
    evidence["versionNumber"] = source_version_number;
    serde_json::from_value(evidence).map_err(|error| format!("漂移事件证据无效：{error}"))
}

async fn get_drift_automation_rule(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Option<DecisionDriftAutomationRule>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let rule = sqlx::query_as::<_, DecisionDriftAutomationRule>(
        "SELECT project_id, enabled, finding_codes, cooldown_hours, webhook_url, next_eligible_at, updated_at FROM decision_drift_automation_rules WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移自动化规则", error))?;
    Ok(Json(rule))
}

async fn update_drift_automation_rule(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(payload): Json<UpdateDecisionDriftAutomation>,
) -> Result<Json<DecisionDriftAutomationRule>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let (finding_codes, webhook_url) =
        validate_drift_automation_rule(&payload).map_err(ProjectApiError::bad_request)?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存漂移自动化规则", error))?;
    let rule = sqlx::query_as::<_, DecisionDriftAutomationRule>(
        r#"
        INSERT INTO decision_drift_automation_rules (
            project_id, enabled, finding_codes, cooldown_hours, webhook_url, updated_by
        ) VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (project_id) DO UPDATE SET
            enabled = EXCLUDED.enabled,
            finding_codes = EXCLUDED.finding_codes,
            cooldown_hours = EXCLUDED.cooldown_hours,
            webhook_url = EXCLUDED.webhook_url,
            updated_by = EXCLUDED.updated_by,
            updated_at = NOW()
        RETURNING project_id, enabled, finding_codes, cooldown_hours, webhook_url,
                  next_eligible_at, updated_at
        "#,
    )
    .bind(project_id)
    .bind(payload.enabled)
    .bind(finding_codes)
    .bind(payload.cooldown_hours)
    .bind(webhook_url)
    .bind(&claims.sub)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存漂移自动化规则", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        None,
        "decision_drift.rule_updated",
        &claims.sub,
        "更新项目漂移扫描规则",
        json!({
            "enabled": rule.enabled,
            "findingCodes": rule.finding_codes,
            "cooldownHours": rule.cooldown_hours,
            "webhookConfigured": rule.webhook_url.is_some(),
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录漂移规则审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交漂移自动化规则", error))?;
    Ok(Json(rule))
}

async fn list_drift_automation_events(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DecisionDriftEvent>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let events = sqlx::query_as::<_, DecisionDriftEvent>(
        r#"
        SELECT event.id, event.project_id, event.source_version_id, event.dedupe_key,
               event.finding_codes, event.evidence_json AS evidence,
               event.action_result_json AS action_result,
               (SELECT jsonb_build_object(
                   'status', delivery.status,
                   'attemptCount', delivery.attempt_count,
                   'lastHttpStatus', delivery.last_http_status,
                   'lastError', delivery.last_error,
                   'nextAttemptAt', delivery.next_attempt_at,
                   'deliveredAt', delivery.delivered_at
               ) FROM decision_drift_webhook_deliveries delivery
                 WHERE delivery.event_id = event.id) AS webhook_delivery,
               event.review_label, event.reviewed_by, event.reviewed_at,
               event.status, event.observed_at
        FROM decision_drift_events event
        WHERE event.project_id = $1
        ORDER BY event.observed_at DESC, event.id DESC
        LIMIT 100
        "#,
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移自动化事件", error))?;
    Ok(Json(events))
}

fn validate_drift_event_review_page(
    query: &DriftEventReviewPageQuery,
) -> Result<(i64, i64), String> {
    let after_id = query.after_id.unwrap_or(0);
    let limit = query.limit.unwrap_or(100);
    if after_id < 0 {
        return Err("afterId 不能为负数".into());
    }
    if !(1..=500).contains(&limit) {
        return Err("limit 必须在 1 到 500 之间".into());
    }
    Ok((after_id, limit))
}

fn validate_optimization_autodraft_shadow_page(
    query: &OptimizationAutodraftShadowPageQuery,
) -> Result<(i64, i64), String> {
    let after_id = query.after_id.unwrap_or(0);
    let limit = query.limit.unwrap_or(100);
    if after_id < 0 {
        return Err("afterId 不能为负数".into());
    }
    if !(1..=500).contains(&limit) {
        return Err("limit 必须在 1 到 500 之间".into());
    }
    Ok((after_id, limit))
}

async fn list_reviewed_drift_events(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<DriftEventReviewPageQuery>,
) -> Result<Json<Vec<DecisionDriftEventReviewRecord>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let (after_id, limit) =
        validate_drift_event_review_page(&query).map_err(ProjectApiError::bad_request)?;
    let events = sqlx::query_as::<_, DecisionDriftEventReviewRecord>(
        r#"
        SELECT id, project_id, source_version_id, review_label, reviewed_by,
               reviewed_at, evidence_json AS evidence
        FROM decision_drift_events
        WHERE project_id = $1 AND id > $2 AND review_label IS NOT NULL
        ORDER BY id
        LIMIT $3
        "#,
    )
    .bind(project_id)
    .bind(after_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取已审阅漂移事件", error))?;
    Ok(Json(events))
}

fn validate_drift_scan_window_review_label(label: &str) -> Result<&'static str, String> {
    match label {
        "drift" | "normal" | "inconclusive" => Ok(match label {
            "drift" => "drift",
            "normal" => "normal",
            _ => "inconclusive",
        }),
        _ => Err("窗口判定必须是 drift、normal 或 inconclusive".into()),
    }
}

async fn list_drift_scan_windows(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<DriftEventReviewPageQuery>,
) -> Result<Json<Vec<DecisionDriftScanWindow>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let (after_id, limit) =
        validate_drift_event_review_page(&query).map_err(ProjectApiError::bad_request)?;
    let windows = sqlx::query_as::<_, DecisionDriftScanWindow>(
        r#"
        SELECT id, project_id, source_version_id, scan_day,
               evidence_json AS evidence, review_label, reviewed_by, reviewed_at
        FROM decision_drift_scan_windows
        WHERE project_id = $1 AND id > $2
        ORDER BY CASE WHEN $4 THEN id END DESC, CASE WHEN NOT $4 THEN id END ASC
        LIMIT $3
        "#,
    )
    .bind(project_id)
    .bind(after_id)
    .bind(limit)
    .bind(query.newest.unwrap_or(false))
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移扫描窗口", error))?;
    Ok(Json(windows))
}

async fn list_optimization_autodraft_shadow_evaluations(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<OptimizationAutodraftShadowPageQuery>,
) -> Result<Json<Vec<OptimizationAutodraftShadowEvaluation>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let (after_id, limit) = validate_optimization_autodraft_shadow_page(&query)
        .map_err(ProjectApiError::bad_request)?;
    let evaluations = sqlx::query_as::<_, OptimizationAutodraftShadowEvaluation>(
        r#"
        SELECT id, simulation_id, project_id, proposal_id, source_version_id,
               candidate_version_id, outcome, finding_code, sample_source,
               test_set_size, baseline_accuracy::FLOAT8 AS baseline_accuracy,
               simulated_accuracy::FLOAT8 AS simulated_accuracy, created_at
        FROM optimization_autodraft_shadow_evaluations
        WHERE project_id = $1 AND id > $2
        ORDER BY id
        LIMIT $3
        "#,
    )
    .bind(project_id)
    .bind(after_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取优化草稿 shadow 评估", error))?;
    Ok(Json(evaluations))
}

async fn review_drift_scan_window(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, window_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateDecisionDriftScanWindowReview>,
) -> Result<Json<DecisionDriftScanWindowReviewResult>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let review_label = validate_drift_scan_window_review_label(&payload.review_label)
        .map_err(ProjectApiError::bad_request)?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存漂移窗口判定", error))?;
    let row = sqlx::query_as::<_, (
        i64,
        Option<String>,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    )>(
        "SELECT source_version_id, review_label, reviewed_by, reviewed_at FROM decision_drift_scan_windows WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(window_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移窗口判定", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (source_version_id, previous_label, previous_reviewer, previous_reviewed_at) = row;
    if previous_label.as_deref() == Some(review_label)
        && previous_reviewer.is_some()
        && previous_reviewed_at.is_some()
    {
        transaction
            .commit()
            .await
            .map_err(|error| ProjectApiError::database("提交漂移窗口幂等判定", error))?;
        return Ok(Json(DecisionDriftScanWindowReviewResult {
            window_id,
            review_label: review_label.into(),
            reviewed_by: previous_reviewer.unwrap_or_default(),
            reviewed_at: previous_reviewed_at.unwrap_or_else(chrono::Utc::now),
        }));
    }
    let result = sqlx::query_as::<_, (String, String, chrono::DateTime<chrono::Utc>)>(
        r#"
        UPDATE decision_drift_scan_windows
        SET review_label = $1, reviewed_by = $2, reviewed_at = NOW()
        WHERE id = $3 AND project_id = $4
        RETURNING review_label, reviewed_by, reviewed_at
        "#,
    )
    .bind(review_label)
    .bind(&claims.sub)
    .bind(window_id)
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新漂移窗口判定", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(source_version_id),
        "decision_drift.window_reviewed",
        &claims.sub,
        "人工标注漂移扫描窗口",
        json!({
            "windowId": window_id,
            "previousLabel": previous_label,
            "reviewLabel": result.0,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录漂移窗口判定审计", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交漂移窗口判定", error))?;
    Ok(Json(DecisionDriftScanWindowReviewResult {
        window_id,
        review_label: result.0,
        reviewed_by: result.1,
        reviewed_at: result.2,
    }))
}

async fn get_drift_event_impact(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, event_id)): Path<(i64, i64)>,
) -> Result<Json<DecisionDriftImpactReport>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let generated_at = chrono::Utc::now();
    let window_start = generated_at - chrono::Duration::days(30);
    let event = sqlx::query_as::<_, (i64, Value)>(
        "SELECT source_version_id, evidence_json FROM decision_drift_events WHERE id = $1 AND project_id = $2",
    )
    .bind(event_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移影响事件", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (source_version_id, evidence) = event;

    let totals = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        r#"
        SELECT COUNT(*)::BIGINT,
               COUNT(*) FILTER (WHERE status = 'succeeded')::BIGINT,
               COUNT(*) FILTER (WHERE status = 'failed')::BIGINT,
               COUNT(*) FILTER (WHERE outcome_assessment IN ('confirmed', 'mismatch'))::BIGINT,
               COUNT(*) FILTER (WHERE outcome_assessment = 'mismatch')::BIGINT
        FROM decision_runs_partitioned
        WHERE project_id = $1 AND version_id = $2 AND executed_at >= $3
        "#,
    )
    .bind(project_id)
    .bind(source_version_id)
    .bind(window_start)
    .fetch_one(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("统计漂移事件运行影响", error))?;

    let elements = sqlx::query_as::<_, DecisionDriftElementImpact>(
        r#"
        WITH targets AS (
            SELECT DISTINCT NULLIF(BTRIM(finding->>'element'), '') AS element,
                   SPLIT_PART(BTRIM(finding->>'element'), '/', 1) AS decision_key,
                   NULLIF(SPLIT_PART(BTRIM(finding->>'element'), '/', 2), '') AS rule_key
            FROM jsonb_array_elements(COALESCE($3->'findings', '[]'::JSONB)) finding
            WHERE NULLIF(BTRIM(finding->>'element'), '') IS NOT NULL
        ), runs AS (
            SELECT id, status, outcome_assessment, execution_trace_json
            FROM decision_runs_partitioned
            WHERE project_id = $1 AND version_id = $2 AND executed_at >= $4
        )
        SELECT target.element,
               COUNT(DISTINCT run.id) FILTER (WHERE impact.matches)::BIGINT AS observed_runs,
               COUNT(DISTINCT run.id) FILTER (WHERE impact.matches AND run.status = 'succeeded')::BIGINT AS succeeded_runs,
               COUNT(DISTINCT run.id) FILTER (WHERE impact.matches AND run.outcome_assessment IN ('confirmed', 'mismatch'))::BIGINT AS feedback_runs,
               COUNT(DISTINCT run.id) FILTER (WHERE impact.matches AND run.outcome_assessment = 'mismatch')::BIGINT AS mismatch_runs,
               COALESCE(ARRAY_AGG(DISTINCT rule.value) FILTER (WHERE impact.matches AND rule.value IS NOT NULL), ARRAY[]::TEXT[]) AS matched_rule_ids
        FROM targets target
        LEFT JOIN runs run ON TRUE
        LEFT JOIN LATERAL jsonb_array_elements(COALESCE(run.execution_trace_json->'decisions', '[]'::JSONB)) step(value)
          ON step.value->>'decisionName' = target.decision_key OR step.value->>'decisionId' = target.decision_key
        LEFT JOIN LATERAL jsonb_array_elements_text(COALESCE(step.value->'matchedRuleIds', '[]'::JSONB)) rule(value)
          ON TRUE
        CROSS JOIN LATERAL (SELECT step.value IS NOT NULL
              AND (target.rule_key IS NULL OR step.value->'matchedRuleIds' ? target.rule_key) AS matches) impact
        GROUP BY target.element
        ORDER BY target.element
        "#,
    )
    .bind(project_id)
    .bind(source_version_id)
    .bind(evidence)
    .bind(window_start)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移影响元素", error))?;

    Ok(Json(DecisionDriftImpactReport {
        event_id,
        source_version_id,
        window_start,
        generated_at,
        total_runs: totals.0,
        succeeded_runs: totals.1,
        failed_runs: totals.2,
        feedback_runs: totals.3,
        mismatch_runs: totals.4,
        elements,
    }))
}

async fn prepare_drift_event_proposals(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, event_id)): Path<(i64, i64)>,
) -> Result<Json<crate::models::DecisionDriftEventActionResult>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("准备漂移事件提案", error))?;
    let event = sqlx::query_as::<_, (i64, String, Value, Option<Value>)>(
        r#"
        SELECT source_version_id, status, evidence_json, action_result_json
        FROM decision_drift_events
        WHERE id = $1 AND project_id = $2
        FOR UPDATE
        "#,
    )
    .bind(event_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移事件", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (source_version_id, status, evidence, prior_action_result) = event;
    if status == "proposal_created" {
        let proposal_ids = prior_action_result
            .as_ref()
            .and_then(|result| result.get("proposalIds"))
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default();
        transaction
            .commit()
            .await
            .map_err(|error| ProjectApiError::database("提交漂移事件幂等响应", error))?;
        return Ok(Json(crate::models::DecisionDriftEventActionResult {
            event_id,
            status,
            proposal_ids,
        }));
    }
    if status != "open" {
        return Err(ProjectApiError::conflict("该漂移事件已完成处置"));
    }
    let published_version_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT published_version_id FROM decision_projects WHERE id = $1 FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("确认漂移事件发布基线", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if published_version_id != Some(source_version_id) {
        return Err(ProjectApiError::conflict(
            "漂移事件基于的发布版本已变化，请重新运行分析",
        ));
    }

    let report = drift_event_analytics_report(project_id, source_version_id, evidence.clone())
        .map_err(ProjectApiError::bad_request)?;
    if report.findings.is_empty() {
        return Err(ProjectApiError::bad_request("漂移事件没有可转为提案的发现"));
    }
    let full_snapshot = serde_json::to_value(&report)
        .map_err(|error| ProjectApiError::bad_request(format!("漂移分析快照无效：{error}")))?;
    let mut proposal_ids = Vec::new();
    for finding in &report.findings {
        let (title, actions) = optimization_guidance(&finding.code);
        let summary = finding
            .numeric_feedback_split_summary()
            .map(|diagnostic| format!("{}\n{}", finding.message, diagnostic))
            .unwrap_or_else(|| finding.message.clone());
        let actions = serde_json::to_value(actions).map_err(|error| {
            ProjectApiError::bad_request(format!("漂移提案动作序列化失败：{error}"))
        })?;
        let proposal_id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO decision_optimization_proposals (
                project_id, source_version_id, finding_code, finding_level, element,
                title, summary, proposed_actions_json, evidence_json,
                analysis_snapshot_json, generated_by
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (project_id, source_version_id, finding_code, element)
                WHERE status = 'open'
            DO UPDATE SET finding_level = EXCLUDED.finding_level,
                          title = EXCLUDED.title,
                          summary = EXCLUDED.summary,
                          proposed_actions_json = EXCLUDED.proposed_actions_json,
                          evidence_json = EXCLUDED.evidence_json,
                          analysis_snapshot_json = EXCLUDED.analysis_snapshot_json,
                          generated_at = NOW(), generated_by = EXCLUDED.generated_by
            RETURNING id
            "#,
        )
        .bind(project_id)
        .bind(source_version_id)
        .bind(&finding.code)
        .bind(&finding.level)
        .bind(&finding.element)
        .bind(title)
        .bind(summary)
        .bind(actions)
        .bind(json!({"finding": finding, "eventId": event_id, "evidence": evidence}))
        .bind(&full_snapshot)
        .bind(&claims.sub)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("保存漂移事件提案", error))?;
        proposal_ids.push(proposal_id);
    }
    let action_result = json!({"proposalIds": proposal_ids});
    sqlx::query(
        r#"
        UPDATE decision_drift_events
        SET status = 'proposal_created', action_result_json = $1, resolved_at = NOW()
        WHERE id = $2 AND project_id = $3 AND status = 'open'
        "#,
    )
    .bind(&action_result)
    .bind(event_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新漂移事件处置状态", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(source_version_id),
        "decision_drift.proposals_prepared",
        &claims.sub,
        "从漂移事件证据创建待人工确认优化提案",
        json!({"eventId": event_id, "proposalIds": proposal_ids}),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录漂移提案审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交漂移事件提案", error))?;
    Ok(Json(crate::models::DecisionDriftEventActionResult {
        event_id,
        status: "proposal_created".into(),
        proposal_ids,
    }))
}

async fn dismiss_drift_event(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, event_id)): Path<(i64, i64)>,
) -> Result<Json<crate::models::DecisionDriftEventActionResult>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("忽略漂移事件", error))?;
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM decision_drift_events WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(event_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移事件", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    if status == "dismissed" {
        transaction
            .commit()
            .await
            .map_err(|error| ProjectApiError::database("提交漂移事件幂等响应", error))?;
        return Ok(Json(crate::models::DecisionDriftEventActionResult {
            event_id,
            status,
            proposal_ids: Vec::new(),
        }));
    }
    if status != "open" {
        return Err(ProjectApiError::conflict("该漂移事件已完成处置"));
    }
    sqlx::query(
        r#"
        UPDATE decision_drift_events
        SET status = 'dismissed', action_result_json = '{"action":"dismissed"}'::JSONB,
            resolved_at = NOW()
        WHERE id = $1 AND project_id = $2 AND status = 'open'
        "#,
    )
    .bind(event_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新漂移事件状态", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        None,
        "decision_drift.dismissed",
        &claims.sub,
        "人工忽略漂移事件",
        json!({"eventId": event_id}),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录漂移事件忽略审计", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交漂移事件忽略", error))?;
    Ok(Json(crate::models::DecisionDriftEventActionResult {
        event_id,
        status: "dismissed".into(),
        proposal_ids: Vec::new(),
    }))
}

async fn review_drift_event(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, event_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateDecisionDriftEventReview>,
) -> Result<Json<DecisionDriftEventReviewResult>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let review_label = validate_drift_event_review_label(&payload.review_label)
        .map_err(ProjectApiError::bad_request)?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存漂移事件人工判定", error))?;
    let event = sqlx::query_as::<_, (
        i64,
        Option<String>,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    )>(
        "SELECT source_version_id, review_label, reviewed_by, reviewed_at FROM decision_drift_events WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(event_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移事件人工判定", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (source_version_id, previous_label, previous_reviewer, previous_reviewed_at) = event;
    if previous_label.as_deref() == Some(review_label)
        && previous_reviewer.is_some()
        && previous_reviewed_at.is_some()
    {
        transaction
            .commit()
            .await
            .map_err(|error| ProjectApiError::database("提交漂移事件幂等判定", error))?;
        return Ok(Json(DecisionDriftEventReviewResult {
            event_id,
            review_label: review_label.into(),
            reviewed_by: previous_reviewer.unwrap_or_default(),
            reviewed_at: previous_reviewed_at.unwrap_or_else(chrono::Utc::now),
        }));
    }
    let result = sqlx::query_as::<_, (String, String, chrono::DateTime<chrono::Utc>)>(
        r#"
        UPDATE decision_drift_events
        SET review_label = $1, reviewed_by = $2, reviewed_at = NOW()
        WHERE id = $3 AND project_id = $4
        RETURNING review_label, reviewed_by, reviewed_at
        "#,
    )
    .bind(review_label)
    .bind(&claims.sub)
    .bind(event_id)
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新漂移事件人工判定", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(source_version_id),
        "decision_drift.reviewed",
        &claims.sub,
        "人工标注漂移检测结果",
        json!({
            "eventId": event_id,
            "previousLabel": previous_label,
            "reviewLabel": result.0,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录漂移事件人工判定审计", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交漂移事件人工判定", error))?;
    Ok(Json(DecisionDriftEventReviewResult {
        event_id,
        review_label: result.0,
        reviewed_by: result.1,
        reviewed_at: result.2,
    }))
}

async fn retry_drift_webhook_delivery(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, event_id)): Path<(i64, i64)>,
) -> Result<Json<crate::models::DecisionDriftEventActionResult>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("重试漂移通知", error))?;
    let delivery = sqlx::query_as::<_, (i64, String)>(
        "SELECT delivery.id, delivery.status FROM decision_drift_webhook_deliveries delivery JOIN decision_drift_events event ON event.id = delivery.event_id WHERE event.id = $1 AND event.project_id = $2 FOR UPDATE OF delivery",
    )
    .bind(event_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取漂移通知状态", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (delivery_id, status) = delivery;
    if status == "pending" {
        transaction
            .commit()
            .await
            .map_err(|error| ProjectApiError::database("提交漂移通知幂等重试", error))?;
        return Ok(Json(crate::models::DecisionDriftEventActionResult {
            event_id,
            status: "queued".into(),
            proposal_ids: Vec::new(),
        }));
    }
    if status != "failed" {
        return Err(ProjectApiError::conflict("只有终态失败的通知可以手动重试"));
    }
    sqlx::query(
        r#"
        UPDATE decision_drift_webhook_deliveries
        SET status = 'pending', attempt_count = 0, next_attempt_at = NOW(),
            locked_at = NULL, last_http_status = NULL, last_error = NULL, delivered_at = NULL
        WHERE id = $1 AND status = 'failed'
        "#,
    )
    .bind(delivery_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("重置漂移通知重试状态", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        None,
        "decision_drift.webhook_retried",
        &claims.sub,
        "将失败的漂移 Webhook 重新加入投递队列",
        json!({"eventId": event_id, "deliveryId": delivery_id}),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录漂移通知重试审计", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交漂移通知重试", error))?;
    Ok(Json(crate::models::DecisionDriftEventActionResult {
        event_id,
        status: "queued".into(),
        proposal_ids: Vec::new(),
    }))
}

pub async fn scan_decision_drift_events_once(state: &AppState) -> Result<usize, String> {
    let mut lease = state
        .db
        .acquire()
        .await
        .map_err(|error| format!("获取漂移扫描租约失败：{error}"))?;
    let acquired = sqlx::query_scalar::<_, bool>(
        "SELECT pg_try_advisory_lock(hashtextextended('fuxi_decision_drift_scan', 0))",
    )
    .fetch_one(&mut *lease)
    .await
    .map_err(|error| format!("获取漂移扫描租约失败：{error}"))?;
    if !acquired {
        return Ok(0);
    }
    let result = scan_decision_drift_events_under_lease(state).await;
    let unlocked = match sqlx::query_scalar::<_, bool>(
        "SELECT pg_advisory_unlock(hashtextextended('fuxi_decision_drift_scan', 0))",
    )
    .fetch_one(&mut *lease)
    .await
    {
        Ok(unlocked) => unlocked,
        Err(error) => {
            eprintln!("释放漂移扫描租约失败，关闭数据库连接：{error}");
            if let Err(close_error) = lease.close().await {
                eprintln!("关闭漂移扫描数据库连接失败：{close_error}");
            }
            return result;
        }
    };
    if !unlocked {
        eprintln!("释放漂移扫描租约未获 PostgreSQL 确认，关闭数据库连接");
        if let Err(error) = lease.close().await {
            eprintln!("关闭漂移扫描数据库连接失败：{error}");
        }
    }
    result
}

async fn scan_decision_drift_events_under_lease(state: &AppState) -> Result<usize, String> {
    sqlx::query("DELETE FROM decision_drift_scan_windows WHERE scan_day < CURRENT_DATE - 365")
        .execute(&state.db)
        .await
        .map_err(|error| format!("清理过期漂移扫描窗口失败：{error}"))?;
    let rules = sqlx::query_as::<_, (i64, String)>(
        r#"
        SELECT rule.project_id, project.owner_username
        FROM decision_drift_automation_rules rule
        JOIN decision_projects project ON project.id = rule.project_id
        WHERE rule.enabled
        ORDER BY rule.project_id
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| format!("读取漂移自动化项目失败：{error}"))?;
    let mut inserted_events = 0;
    for (project_id, username) in rules {
        let report = match load_published_analytics(state, &username, project_id).await {
            Ok(report) => report,
            Err(error) => {
                eprintln!("漂移扫描跳过项目 {project_id}：{}", error.message);
                continue;
            }
        };
        let mut transaction = state
            .db
            .begin()
            .await
            .map_err(|error| format!("开始记录漂移事件失败：{error}"))?;
        let rule = sqlx::query_as::<_, (bool, Vec<String>, i32, Option<String>, Option<chrono::DateTime<chrono::Utc>>)>(
            "SELECT enabled, finding_codes, cooldown_hours, webhook_url, next_eligible_at FROM decision_drift_automation_rules WHERE project_id = $1 FOR UPDATE",
        )
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| format!("读取漂移自动化规则失败：{error}"))?;
        let Some((true, finding_codes, cooldown_hours, webhook_url, next_eligible_at)) = rule
        else {
            transaction
                .rollback()
                .await
                .map_err(|error| format!("结束漂移扫描事务失败：{error}"))?;
            continue;
        };
        let scan_day = report.generated_at.date_naive();
        let report_snapshot = serde_json::to_value(&report)
            .map_err(|error| format!("序列化漂移扫描窗口失败：{error}"))?;
        sqlx::query(
            r#"
            INSERT INTO decision_drift_scan_windows (
                project_id, source_version_id, scan_day, evidence_json
            ) VALUES ($1, $2, $3, $4)
            ON CONFLICT (project_id, source_version_id, scan_day) DO UPDATE
                SET evidence_json = EXCLUDED.evidence_json
                WHERE decision_drift_scan_windows.review_label IS NULL
            "#,
        )
        .bind(project_id)
        .bind(report.version_id)
        .bind(scan_day)
        .bind(report_snapshot)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("保存漂移扫描窗口失败：{error}"))?;
        if next_eligible_at.is_some_and(|next| next > chrono::Utc::now()) {
            transaction
                .commit()
                .await
                .map_err(|error| format!("提交漂移扫描窗口失败：{error}"))?;
            continue;
        }
        let finding_codes_set = finding_codes.iter().collect::<HashSet<_>>();
        let matched_findings = report
            .findings
            .iter()
            .filter(|finding| finding_codes_set.contains(&finding.code))
            .cloned()
            .collect::<Vec<_>>();
        if matched_findings.is_empty() {
            transaction
                .commit()
                .await
                .map_err(|error| format!("提交漂移扫描窗口失败：{error}"))?;
            continue;
        }
        let mut matched_codes = matched_findings
            .iter()
            .map(|finding| finding.code.clone())
            .collect::<Vec<_>>();
        matched_codes.sort();
        matched_codes.dedup();
        let dedupe_key = format!(
            "v{}:{}:{}",
            report.version_id,
            scan_day,
            matched_codes.join(",")
        );
        let evidence = json!({
            "sourceVersionId": report.version_id,
            "sourceVersionNumber": report.version_number,
            "windowStart": report.window_start,
            "windowDays": report.window_days,
            "generatedAt": report.generated_at,
            "summary": report.summary,
            "findings": matched_findings,
        });
        let webhook_payload = json!({
            "schemaVersion": 1,
            "eventId": 0,
            "projectId": project_id,
            "sourceVersionId": report.version_id,
            "sourceVersionNumber": report.version_number,
            "findingCodes": matched_codes,
            "observedAt": report.generated_at,
            "summary": report.summary,
            "findings": evidence["findings"],
        });
        let event_id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO decision_drift_events (
                project_id, source_version_id, dedupe_key, finding_codes, evidence_json
            ) VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (project_id, dedupe_key) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(project_id)
        .bind(report.version_id)
        .bind(&dedupe_key)
        .bind(&matched_codes)
        .bind(evidence.clone())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| format!("保存漂移自动化事件失败：{error}"))?;
        if let Some(event_id) = event_id {
            if let Some(endpoint_url) = webhook_url.as_deref() {
                let mut payload = webhook_payload.clone();
                payload["eventId"] = json!(event_id);
                sqlx::query(
                    r#"
                    INSERT INTO decision_drift_webhook_deliveries (
                        event_id, project_id, endpoint_url, payload_json
                    ) VALUES ($1, $2, $3, $4)
                    ON CONFLICT (event_id) DO NOTHING
                    "#,
                )
                .bind(event_id)
                .bind(project_id)
                .bind(endpoint_url)
                .bind(payload)
                .execute(&mut *transaction)
                .await
                .map_err(|error| format!("创建漂移通知投递任务失败：{error}"))?;
            }
            sqlx::query(
                "UPDATE decision_drift_automation_rules SET next_eligible_at = NOW() + ($1 * INTERVAL '1 hour'), updated_at = NOW() WHERE project_id = $2",
            )
            .bind(cooldown_hours)
            .bind(project_id)
            .execute(&mut *transaction)
            .await
            .map_err(|error| format!("更新漂移自动化冷却时间失败：{error}"))?;
            insert_audit_event(
                &mut *transaction,
                project_id,
                Some(report.version_id),
                "decision_drift.event_created",
                &username,
                "定时漂移扫描记录了待处理事件",
                json!({"eventId": event_id, "findingCodes": matched_codes}),
            )
            .await
            .map_err(|error| format!("记录漂移自动化审计事件失败：{error}"))?;
            inserted_events += 1;
        }
        transaction
            .commit()
            .await
            .map_err(|error| format!("提交漂移自动化事件失败：{error}"))?;
    }
    Ok(inserted_events)
}

const DRIFT_WEBHOOK_MAX_ATTEMPTS: i32 = 8;

#[derive(Debug, sqlx::FromRow)]
struct DriftWebhookDelivery {
    id: i64,
    event_id: i64,
    endpoint_url: String,
    payload_json: Value,
    attempt_count: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriftEventReviewPageQuery {
    after_id: Option<i64>,
    limit: Option<i64>,
    newest: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OptimizationAutodraftShadowPageQuery {
    after_id: Option<i64>,
    limit: Option<i64>,
}

fn drift_webhook_retry_delay(attempt_count: i32) -> std::time::Duration {
    let exponent = attempt_count.saturating_sub(1).clamp(0, 10) as u32;
    std::time::Duration::from_secs((30_u64.saturating_mul(1_u64 << exponent)).min(6 * 60 * 60))
}

fn drift_webhook_signature(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts secrets of every length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn claim_drift_webhook_deliveries(
    state: &AppState,
) -> Result<Vec<DriftWebhookDelivery>, String> {
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| format!("开始领取漂移通知失败：{error}"))?;
    sqlx::query(
        r#"
        UPDATE decision_drift_webhook_deliveries
        SET status = 'failed', locked_at = NULL,
            last_error = COALESCE(last_error, '投递工作进程在最大尝试次数后失联')
        WHERE status = 'sending' AND locked_at < NOW() - INTERVAL '1 minute'
          AND attempt_count >= $1
        "#,
    )
    .bind(DRIFT_WEBHOOK_MAX_ATTEMPTS)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("收敛失联漂移通知失败：{error}"))?;
    let pending = sqlx::query_as::<_, (i64,)>(
        r#"
        SELECT id FROM decision_drift_webhook_deliveries
        WHERE (status = 'pending' AND next_attempt_at <= NOW() AND attempt_count < $1)
           OR (status = 'sending' AND locked_at < NOW() - INTERVAL '1 minute' AND attempt_count < $1)
        ORDER BY next_attempt_at, id
        FOR UPDATE SKIP LOCKED
        LIMIT 10
        "#,
    )
    .bind(DRIFT_WEBHOOK_MAX_ATTEMPTS)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| format!("读取待投递漂移通知失败：{error}"))?;
    let mut deliveries = Vec::with_capacity(pending.len());
    for (id,) in pending {
        let delivery = sqlx::query_as::<_, DriftWebhookDelivery>(
            r#"
            UPDATE decision_drift_webhook_deliveries
            SET status = 'sending', locked_at = NOW(), attempt_count = attempt_count + 1
            WHERE id = $1
            RETURNING id, event_id, endpoint_url, payload_json, attempt_count
            "#,
        )
        .bind(id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| format!("锁定漂移通知失败：{error}"))?;
        deliveries.push(delivery);
    }
    transaction
        .commit()
        .await
        .map_err(|error| format!("提交漂移通知领取失败：{error}"))?;
    Ok(deliveries)
}

async fn deliver_drift_webhook(
    state: &AppState,
    client: &reqwest::Client,
    delivery: DriftWebhookDelivery,
) -> Result<(), String> {
    let secret = std::env::var("DRIFT_WEBHOOK_SIGNING_SECRET").unwrap_or_default();
    if secret.as_bytes().len() < 32 {
        return finish_drift_webhook_delivery(
            state,
            &delivery,
            None,
            "DRIFT_WEBHOOK_SIGNING_SECRET 缺失或短于 32 字节".into(),
        )
        .await;
    }
    if let Err(error) = validate_drift_webhook_url(&delivery.endpoint_url) {
        return finish_drift_webhook_delivery(state, &delivery, None, error).await;
    }
    let body = serde_json::to_vec(&delivery.payload_json)
        .map_err(|error| format!("序列化漂移通知失败：{error}"))?;
    let timestamp = chrono::Utc::now().timestamp();
    let signature = drift_webhook_signature(&secret, timestamp, &body);
    let response = client
        .post(&delivery.endpoint_url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("X-Fuxi-Timestamp", timestamp.to_string())
        .header("X-Fuxi-Signature", format!("sha256={signature}"))
        .header(
            "Idempotency-Key",
            format!("decision-drift-{}", delivery.event_id),
        )
        .body(body)
        .send()
        .await;
    match response {
        Ok(response) if response.status().is_success() => {
            sqlx::query(
                "UPDATE decision_drift_webhook_deliveries SET status = 'delivered', locked_at = NULL, last_http_status = $1, last_error = NULL, delivered_at = NOW() WHERE id = $2 AND status = 'sending'",
            )
            .bind(response.status().as_u16() as i16)
            .bind(delivery.id)
            .execute(&state.db)
            .await
            .map(|_| ())
            .map_err(|error| format!("记录漂移通知成功状态失败：{error}"))
        }
        Ok(response) => {
            let status = response.status().as_u16();
            finish_drift_webhook_delivery(
                state,
                &delivery,
                Some(status as i16),
                format!("Webhook 返回 HTTP {status}"),
            )
            .await
        }
        Err(error) => {
            finish_drift_webhook_delivery(
                state,
                &delivery,
                None,
                error.without_url().to_string(),
            )
            .await
        }
    }
}

async fn finish_drift_webhook_delivery(
    state: &AppState,
    delivery: &DriftWebhookDelivery,
    http_status: Option<i16>,
    error: String,
) -> Result<(), String> {
    let terminal = delivery.attempt_count >= DRIFT_WEBHOOK_MAX_ATTEMPTS;
    let status = if terminal { "failed" } else { "pending" };
    let delay_seconds = drift_webhook_retry_delay(delivery.attempt_count).as_secs() as i64;
    let bounded_error = error.chars().take(512).collect::<String>();
    sqlx::query(
        r#"
        UPDATE decision_drift_webhook_deliveries
        SET status = $1, locked_at = NULL, last_http_status = $2, last_error = $3,
            next_attempt_at = CASE WHEN $1 = 'pending'
                THEN NOW() + ($4 * INTERVAL '1 second') ELSE next_attempt_at END
        WHERE id = $5 AND status = 'sending'
        "#,
    )
    .bind(status)
    .bind(http_status)
    .bind(bounded_error)
    .bind(delay_seconds)
    .bind(delivery.id)
    .execute(&state.db)
    .await
    .map(|_| ())
    .map_err(|error| format!("记录漂移通知失败状态失败：{error}"))
}

async fn dispatch_drift_webhooks_once(
    state: &AppState,
    client: &reqwest::Client,
) -> Result<usize, String> {
    let deliveries = claim_drift_webhook_deliveries(state).await?;
    let count = deliveries.len();
    let mut tasks = tokio::task::JoinSet::new();
    for delivery in deliveries {
        let state = state.clone();
        let client = client.clone();
        tasks.spawn(async move {
            if let Err(error) = deliver_drift_webhook(&state, &client, delivery).await {
                eprintln!("处理漂移 Webhook 失败：{error}");
            }
        });
    }
    while let Some(result) = tasks.join_next().await {
        if let Err(error) = result {
            eprintln!("漂移 Webhook 投递任务异常：{error}");
        }
    }
    Ok(count)
}

pub async fn deliver_drift_webhooks(state: AppState) {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            eprintln!("创建漂移 Webhook 客户端失败：{error}");
            return;
        }
    };
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
    loop {
        interval.tick().await;
        match dispatch_drift_webhooks_once(&state, &client).await {
            Ok(count) if count > 0 => println!("处理 {count} 条漂移 Webhook 投递"),
            Ok(_) => {}
            Err(error) => eprintln!("漂移 Webhook 队列处理失败：{error}"),
        }
    }
}

pub async fn scan_decision_drift_events_daily(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
    loop {
        interval.tick().await;
        match scan_decision_drift_events_once(&state).await {
            Ok(count) if count > 0 => println!("漂移扫描新增 {count} 条待处理事件"),
            Ok(_) => {}
            Err(error) => eprintln!("漂移自动化扫描失败：{error}"),
        }
    }
}

async fn save_decision_run_outcome(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, run_id)): Path<(i64, i64)>,
    Json(payload): Json<SaveDecisionRunOutcome>,
) -> Result<Json<DecisionServiceRun>, ProjectApiError> {
    validate_run_outcome(&payload)?;
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let business_reference = payload.business_reference.trim();
    let note = payload.note.trim();
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存实际业务结果", error))?;
    let run = sqlx::query_as::<_, DecisionServiceRun>(
        r#"
        UPDATE decision_runs run
        SET business_reference=$1, actual_outcome_json=$2, outcome_assessment=$3,
            outcome_note=$4, outcome_occurred_at=$5, outcome_reported_by=$6,
            outcome_reported_at=NOW()
        WHERE run.id=$7 AND run.project_id=$8 AND run.status='succeeded'
        RETURNING run.id, run.project_id, run.version_id, run.service_id,
                  run.service_name, run.input_json, run.output_json,
                  run.execution_trace_json, run.duration_ms, run.status,
                  run.executed_by, run.executed_at, run.business_reference,
                  run.actual_outcome_json, run.outcome_assessment, run.outcome_note,
                  run.outcome_occurred_at, run.outcome_reported_by,
                  run.outcome_reported_at
        "#,
    )
    .bind(business_reference)
    .bind(&payload.actual_outcome)
    .bind(&payload.assessment)
    .bind(note)
    .bind(payload.occurred_at)
    .bind(&claims.sub)
    .bind(run_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存实际业务结果", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(run.version_id),
        "decision_run.outcome_reported",
        &claims.sub,
        &format!("回填决策运行 {} 的实际业务结果", run.id),
        json!({
            "runId": run.id,
            "serviceId": run.service_id,
            "serviceName": run.service_name,
            "businessReference": run.business_reference,
            "assessment": run.outcome_assessment,
            "outcomeOccurredAt": run.outcome_occurred_at,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录实际结果审计事件", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交实际业务结果", error))?;
    Ok(Json(run))
}

async fn store_public_run_outcome(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: i64,
    service_id: &str,
    run_id: i64,
    owner: &str,
    idempotency_key: &str,
    payload: &PublicDecisionRunOutcome,
) -> Result<DecisionRunOutcomeReceipt, ProjectApiError> {
    let digest = public_outcome_digest(payload)?;
    let inserted = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO decision_run_outcome_receipts (
            project_id, service_id, idempotency_key, run_id, payload_digest
        ) VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (project_id, service_id, idempotency_key) DO NOTHING
        RETURNING 1
        "#,
    )
    .bind(project_id)
    .bind(service_id)
    .bind(idempotency_key)
    .bind(run_id)
    .bind(&digest)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| ProjectApiError::database("记录业务结果幂等键", error))?
    .is_some();

    if !inserted {
        let existing = sqlx::query_as::<_, (i64, String)>(
            "SELECT run_id, payload_digest FROM decision_run_outcome_receipts WHERE project_id = $1 AND service_id = $2 AND idempotency_key = $3 FOR UPDATE",
        )
        .bind(project_id)
        .bind(service_id)
        .bind(idempotency_key)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| ProjectApiError::database("校验业务结果幂等请求", error))?
        .ok_or_else(|| ProjectApiError::conflict("业务结果回传状态已变化，请重新读取"))?;
        if existing.0 != run_id || existing.1 != digest {
            return Err(ProjectApiError::coded(
                StatusCode::CONFLICT,
                "IDEMPOTENCY_CONFLICT",
                "Idempotency-Key 已用于不同运行记录或不同业务结果",
            ));
        }
        let row = sqlx::query_as::<_, (String, String, chrono::DateTime<chrono::Utc>)>(
            "SELECT business_reference, outcome_assessment, outcome_reported_at FROM decision_runs WHERE id = $1 AND project_id = $2 AND service_id = $3 AND outcome_reported_at IS NOT NULL",
        )
        .bind(run_id)
        .bind(project_id)
        .bind(service_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| ProjectApiError::database("读取业务结果回执", error))?
        .ok_or_else(|| ProjectApiError::conflict("该运行记录的业务结果已由其他渠道更新"))?;
        return Ok(DecisionRunOutcomeReceipt {
            run_id,
            business_reference: row.0,
            assessment: row.1,
            reported_at: row.2,
            replayed: true,
        });
    }

    let business_reference = payload.business_reference.trim();
    let note = payload.note.trim();
    let run = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            String,
            chrono::DateTime<chrono::Utc>,
            Value,
            Value,
        ),
    >(
        r#"
        UPDATE decision_runs run
        SET business_reference = CASE WHEN $1 = '' THEN run.business_reference ELSE $1 END,
            actual_outcome_json = $2,
            outcome_assessment = $3,
            outcome_note = $4,
            outcome_occurred_at = $5,
            outcome_reported_by = $6,
            outcome_reported_at = NOW()
        WHERE run.id = $7 AND run.project_id = $8 AND run.service_id = $9
          AND run.status = 'succeeded' AND run.outcome_reported_at IS NULL
          AND ($1 = '' OR run.business_reference = '' OR run.business_reference = $1)
        RETURNING run.id, run.version_id,
                  CASE WHEN $1 = '' THEN run.business_reference ELSE $1 END,
                  run.outcome_assessment, run.outcome_reported_at,
                  run.execution_trace_json, run.output_json
        "#,
    )
    .bind(business_reference)
    .bind(&payload.actual_outcome)
    .bind(&payload.assessment)
    .bind(note)
    .bind(payload.occurred_at)
    .bind(owner)
    .bind(run_id)
    .bind(project_id)
    .bind(service_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| ProjectApiError::database("保存外部业务结果", error))?
    .ok_or_else(|| {
        ProjectApiError::coded(
            StatusCode::CONFLICT,
            "OUTCOME_CONFLICT",
            "运行记录不存在、未成功执行、已回填结果或业务关联号不匹配",
        )
    })?;
    let (stored_run_id, version_id, stored_reference, assessment, reported_at, trace, output) = run;
    insert_audit_event(
        &mut **transaction,
        project_id,
        Some(version_id),
        "decision_run.outcome_reported",
        owner,
        &format!("外部系统回填决策运行 {} 的业务结果", stored_run_id),
        json!({
            "runId": stored_run_id,
            "serviceId": service_id,
            "businessReference": stored_reference,
            "assessment": assessment,
            "idempotencyKey": idempotency_key,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录外部业务结果审计", error))?;

    if assessment == "mismatch" {
        let (status, attributed_rule, confidence, evidence) =
            deviation_candidate(&trace, &payload.actual_outcome);
        let evidence = json!({
            "analysis": evidence,
            "expectedOutput": output,
            "actualOutcome": payload.actual_outcome,
        });
        sqlx::query(
            r#"
            INSERT INTO deviation_analyses (
                project_id, decision_run_id, status, attributed_rule,
                confidence, evidence_json
            ) VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (decision_run_id) DO NOTHING
            "#,
        )
        .bind(project_id)
        .bind(stored_run_id)
        .bind(status)
        .bind(attributed_rule)
        .bind(confidence)
        .bind(evidence)
        .execute(&mut **transaction)
        .await
        .map_err(|error| ProjectApiError::database("保存偏差归因候选", error))?;
    }

    Ok(DecisionRunOutcomeReceipt {
        run_id: stored_run_id,
        business_reference: stored_reference,
        assessment,
        reported_at,
        replayed: false,
    })
}

async fn report_public_decision_run_outcome(
    State(state): State<AppState>,
    Path((project_id, run_id)): Path<(i64, i64)>,
    headers: HeaderMap,
    Json(payload): Json<PublicDecisionRunOutcome>,
) -> Result<Response, ProjectApiError> {
    validate_public_run_outcome(&payload)?;
    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ProjectApiError::bad_request("缺少 Idempotency-Key 请求头"))?;
    let idempotency_key = validate_idempotency_key(idempotency_key)?;
    let service_id = sqlx::query_scalar::<_, String>(
        "SELECT service_id FROM decision_runs WHERE id = $1 AND project_id = $2",
    )
    .bind(run_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取业务结果运行范围", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let owner = authenticate_service_api_key(&state, &headers, project_id, &service_id).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("开始外部结果回传", error))?;
    let receipt = store_public_run_outcome(
        &mut transaction,
        project_id,
        &service_id,
        run_id,
        &owner,
        idempotency_key,
        &payload,
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交外部结果回传", error))?;
    let status = if receipt.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    let mut response = (status, Json(receipt)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn report_public_decision_run_outcomes_batch(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    headers: HeaderMap,
    Json(payload): Json<PublicDecisionRunOutcomeBatch>,
) -> Result<Response, ProjectApiError> {
    if payload.service_id.trim().is_empty() || payload.service_id.chars().count() > 255 {
        return Err(ProjectApiError::bad_request("serviceId 无效"));
    }
    if payload.items.is_empty() || payload.items.len() > 100 {
        return Err(ProjectApiError::bad_request(
            "批量回传必须包含 1 至 100 条结果",
        ));
    }
    let mut run_ids = HashSet::new();
    let mut idempotency_keys = HashSet::new();
    for item in &payload.items {
        if item.run_id <= 0 || !run_ids.insert(item.run_id) {
            return Err(ProjectApiError::bad_request(
                "批次中的运行记录 ID 必须唯一且有效",
            ));
        }
        let key = validate_idempotency_key(&item.idempotency_key)?;
        if !idempotency_keys.insert(key.to_owned()) {
            return Err(ProjectApiError::bad_request(
                "批次中的 Idempotency-Key 必须唯一",
            ));
        }
        validate_public_run_outcome(&item.outcome)?;
    }
    let owner =
        authenticate_service_api_key(&state, &headers, project_id, payload.service_id.trim())
            .await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("开始批量结果回传", error))?;
    let mut receipts = Vec::with_capacity(payload.items.len());
    for item in &payload.items {
        receipts.push(
            store_public_run_outcome(
                &mut transaction,
                project_id,
                payload.service_id.trim(),
                item.run_id,
                &owner,
                item.idempotency_key.trim(),
                &item.outcome,
            )
            .await?,
        );
    }
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交批量结果回传", error))?;
    let mut response =
        Json(PublicDecisionRunOutcomeBatchResult { results: receipts }).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn validate_outcome_reconciliation_request(
    request: &PublicDecisionRunOutcomeReconciliationRequest,
) -> Result<(), ProjectApiError> {
    if request.service_id.trim().is_empty() || request.service_id.chars().count() > 255 {
        return Err(ProjectApiError::bad_request("serviceId 无效"));
    }
    if request.run_ids.is_empty() || request.run_ids.len() > 100 {
        return Err(ProjectApiError::bad_request(
            "对账批次必须包含 1 至 100 个运行记录 ID",
        ));
    }
    let mut ids = HashSet::new();
    if request
        .run_ids
        .iter()
        .any(|run_id| *run_id <= 0 || !ids.insert(*run_id))
    {
        return Err(ProjectApiError::bad_request(
            "对账批次中的运行记录 ID 必须唯一且为正整数",
        ));
    }
    Ok(())
}

async fn reconcile_public_decision_run_outcomes(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    headers: HeaderMap,
    Json(request): Json<PublicDecisionRunOutcomeReconciliationRequest>,
) -> Result<Response, ProjectApiError> {
    validate_outcome_reconciliation_request(&request)?;
    let service_id = request.service_id.trim();
    authenticate_service_api_key(&state, &headers, project_id, service_id).await?;
    let results = sqlx::query_as::<_, PublicDecisionRunOutcomeReconciliationItem>(
        r#"
        SELECT requested.run_id,
               CASE
                   WHEN run.id IS NULL THEN 'unavailable'
                   WHEN run.status <> 'succeeded' THEN 'not_succeeded'
                   WHEN run.outcome_reported_at IS NOT NULL THEN 'already_reported'
                   ELSE 'ready'
               END AS state,
               CASE WHEN run.outcome_reported_at IS NOT NULL THEN run.business_reference END AS business_reference,
               CASE WHEN run.outcome_reported_at IS NOT NULL THEN run.outcome_assessment END AS assessment,
               run.outcome_reported_at AS reported_at
        FROM UNNEST($3::BIGINT[]) WITH ORDINALITY AS requested(run_id, request_order)
        LEFT JOIN decision_runs AS run
          ON run.id = requested.run_id
         AND run.project_id = $1
         AND run.service_id = $2
        ORDER BY requested.request_order
        "#,
    )
    .bind(project_id)
    .bind(service_id)
    .bind(&request.run_ids)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("对账外部业务结果", error))?;
    let mut response =
        Json(PublicDecisionRunOutcomeReconciliationResult { results }).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn list_deviation_analyses(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DeviationAnalysis>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let analyses = sqlx::query_as::<_, DeviationAnalysis>(
        r#"
        SELECT id, project_id, decision_run_id, status, attributed_rule,
               confidence, evidence_json AS evidence, created_at,
               review_label, reviewed_by, reviewed_at
        FROM deviation_analyses
        WHERE project_id = $1
        ORDER BY created_at DESC, id DESC
        LIMIT 500
        "#,
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取偏差归因记录", error))?;
    Ok(Json(analyses))
}

async fn list_reviewed_deviation_analyses(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<DriftEventReviewPageQuery>,
) -> Result<Json<Vec<DeviationAnalysisReviewRecord>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    let (after_id, limit) =
        validate_drift_event_review_page(&query).map_err(ProjectApiError::bad_request)?;
    let analyses = sqlx::query_as::<_, DeviationAnalysisReviewRecord>(
        r#"
        SELECT id, project_id, decision_run_id, status, attributed_rule, confidence,
               review_label, reviewed_by, reviewed_at
        FROM deviation_analyses
        WHERE project_id = $1 AND id > $2 AND review_label IS NOT NULL
        ORDER BY id
        LIMIT $3
        "#,
    )
    .bind(project_id)
    .bind(after_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取已审阅偏差归因记录", error))?;
    Ok(Json(analyses))
}

fn validate_deviation_analysis_review(
    status: &str,
    review_label: &str,
) -> Result<&'static str, String> {
    match (status, review_label) {
        ("candidate", "correct") => Ok("correct"),
        ("candidate", "incorrect") => Ok("incorrect"),
        ("candidate", "inconclusive") | ("insufficient_evidence", "inconclusive") => {
            Ok("inconclusive")
        }
        ("candidate" | "insufficient_evidence", _) => Err(
            "候选归因可标记 correct、incorrect 或 inconclusive；证据不足只能标记 inconclusive"
                .into(),
        ),
        _ => Err("不支持该偏差归因状态".into()),
    }
}

async fn review_deviation_analysis(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, analysis_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateDeviationAnalysisReview>,
) -> Result<Json<DeviationAnalysisReviewResult>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("保存偏差归因判定", error))?;
    let row = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<String>,
            Option<String>,
            Option<chrono::DateTime<chrono::Utc>>,
        ),
    >(
        r#"
        SELECT analysis.decision_run_id, analysis.status, analysis.review_label,
               analysis.reviewed_by, analysis.reviewed_at
        FROM deviation_analyses AS analysis
        WHERE analysis.id = $1 AND analysis.project_id = $2
        FOR UPDATE
        "#,
    )
    .bind(analysis_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取偏差归因判定", error))?
    .ok_or_else(ProjectApiError::not_found)?;
    let (run_id, status, previous_label, previous_reviewer, previous_reviewed_at) = row;
    let review_label = validate_deviation_analysis_review(&status, &payload.review_label)
        .map_err(ProjectApiError::bad_request)?;
    if previous_label.as_deref() == Some(review_label)
        && previous_reviewer.is_some()
        && previous_reviewed_at.is_some()
    {
        transaction
            .commit()
            .await
            .map_err(|error| ProjectApiError::database("提交偏差归因幂等判定", error))?;
        return Ok(Json(DeviationAnalysisReviewResult {
            analysis_id,
            review_label: review_label.into(),
            reviewed_by: previous_reviewer.unwrap_or_default(),
            reviewed_at: previous_reviewed_at.unwrap_or_else(chrono::Utc::now),
        }));
    }
    let result = sqlx::query_as::<_, (String, String, chrono::DateTime<chrono::Utc>)>(
        r#"
        UPDATE deviation_analyses
        SET review_label = $1, reviewed_by = $2, reviewed_at = NOW()
        WHERE id = $3 AND project_id = $4
        RETURNING review_label, reviewed_by, reviewed_at
        "#,
    )
    .bind(review_label)
    .bind(&claims.sub)
    .bind(analysis_id)
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("更新偏差归因判定", error))?;
    let version_id = sqlx::query_scalar::<_, i64>(
        "SELECT version_id FROM decision_runs WHERE id = $1 AND project_id = $2",
    )
    .bind(run_id)
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取偏差归因运行版本", error))?;
    insert_audit_event(
        &mut *transaction,
        project_id,
        Some(version_id),
        "decision_deviation.reviewed",
        &claims.sub,
        "人工审阅偏差归因结果",
        json!({
            "analysisId": analysis_id,
            "decisionRunId": run_id,
            "previousLabel": previous_label,
            "reviewLabel": result.0,
        }),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录偏差归因审阅审计", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交偏差归因判定", error))?;
    Ok(Json(DeviationAnalysisReviewResult {
        analysis_id,
        review_label: result.0,
        reviewed_by: result.1,
        reviewed_at: result.2,
    }))
}

pub fn routes(llm_state: Arc<crate::api::LlmState>) -> Router<AppState> {
    Router::new()
        .route("/model-library", get(list_model_library))
        .route("/model-library/analytics", get(get_pattern_library_analytics))
        .route(
            "/model-library/recommendations",
            get(recommend_decision_patterns),
        )
        .route(
            "/model-library/search-debug",
            get(debug_decision_pattern_search),
        )
        .route(
            "/model-library/recommendations/{recommendation_id}/feedback",
            post(submit_pattern_recommendation_feedback),
        )
        .route(
            "/model-library/history/{version_id}/lineage",
            get(list_decision_pattern_lineage),
        )
        .route(
            "/model-library/{source_kind}/{source_id}/instantiate",
            post(instantiate_model_library_item),
        )
        .route(
            "/model-library/templates/{template_id}",
            delete(delete_decision_template),
        )
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/{project_id}",
            get(get_workspace).put(update_project).delete(delete_project),
        )
        .route("/projects/{project_id}/members", get(list_project_members))
        .route(
            "/projects/{project_id}/members/{username}",
            put(save_project_member).delete(delete_project_member),
        )
        .route(
            "/projects/{project_id}/versions",
            get(list_versions).post(create_version),
        )
        .route(
            "/projects/{project_id}/model-changes/preview",
            post(preview_model_change),
        )
        .route(
            "/projects/{project_id}/model-changes/{candidate_id}/commit",
            post(commit_model_change),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/review",
            get(get_version_review),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/behavior-comparison",
            get(get_version_behavior_comparison),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/review-approval",
            post(review_project_version),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/verification",
            get(get_version_verification).post(verify_project_version),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/test-cases",
            get(list_test_cases).post(create_test_case),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/tester-proposals/{proposal_id}/judge",
            post(judge_tester_proposal),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/tester-proposals/{proposal_id}/convert",
            post(convert_tester_proposal),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/test-cases/run",
            post(run_test_cases),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/tester-proposals/run",
            post(run_tester_proposal_cases),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/regression-tests/run",
            post(run_historical_regression_tests),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/test-cases/generate",
            post(generate_test_cases),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/test-cases/{case_id}/approval",
            put(approve_test_case),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/test-cases/{case_id}",
            delete(delete_test_case),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/publish",
            axum::routing::post(publish_project_version),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/rollback",
            post(rollback_project_version),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/templates",
            post(save_decision_template),
        )
        .route(
            "/projects/{project_id}/audit-events",
            get(list_audit_events),
        )
        .route(
            "/projects/{project_id}/decision-services",
            get(list_published_decision_services),
        )
        .route(
            "/projects/{project_id}/decision-service-keys",
            get(list_decision_service_api_keys).post(create_decision_service_api_key),
        )
        .route(
            "/projects/{project_id}/decision-service-keys/{key_id}",
            delete(revoke_decision_service_api_key),
        )
        .route(
            "/projects/{project_id}/decision-services/execute",
            post(execute_published_decision_service),
        )
        .route(
            "/projects/{project_id}/decision-runs",
            get(list_decision_service_runs),
        )
        .route(
            "/projects/{project_id}/decision-runs/{run_id}/explanation",
            get(get_decision_run_explanation).post(generate_decision_run_explanation),
        )
        .route(
            "/projects/{project_id}/decision-runs/{run_id}/explanation/reviews",
            get(list_decision_run_explanation_reviews).post(review_decision_run_explanation),
        )
        .route(
            "/projects/{project_id}/decision-explanation-reviews",
            get(list_decision_explanation_review_export),
        )
        .route(
            "/projects/{project_id}/decision-analytics",
            get(get_decision_analytics),
        )
        .route(
            "/projects/{project_id}/decision-analytics/thresholds",
            get(get_decision_analytics_thresholds).put(update_decision_analytics_thresholds),
        )
        .route(
            "/projects/{project_id}/copilot/query",
            post(query_decision_copilot),
        )
        .route(
            "/projects/{project_id}/drift-automation",
            get(get_drift_automation_rule).put(update_drift_automation_rule),
        )
        .route(
            "/projects/{project_id}/drift-events",
            get(list_drift_automation_events),
        )
        .route(
            "/projects/{project_id}/drift-event-reviews",
            get(list_reviewed_drift_events),
        )
        .route(
            "/projects/{project_id}/drift-scan-windows",
            get(list_drift_scan_windows),
        )
        .route(
            "/projects/{project_id}/optimization-shadow-evaluations",
            get(list_optimization_autodraft_shadow_evaluations),
        )
        .route(
            "/projects/{project_id}/drift-scan-windows/{window_id}/review",
            post(review_drift_scan_window),
        )
        .route(
            "/projects/{project_id}/drift-events/{event_id}/impact",
            get(get_drift_event_impact),
        )
        .route(
            "/projects/{project_id}/drift-events/{event_id}/prepare-proposals",
            post(prepare_drift_event_proposals),
        )
        .route(
            "/projects/{project_id}/drift-events/{event_id}/dismiss",
            post(dismiss_drift_event),
        )
        .route(
            "/projects/{project_id}/drift-events/{event_id}/review",
            post(review_drift_event),
        )
        .route(
            "/projects/{project_id}/drift-events/{event_id}/webhook-retry",
            post(retry_drift_webhook_delivery),
        )
        .route(
            "/projects/{project_id}/optimization-proposals",
            get(list_optimization_proposals),
        )
        .route(
            "/projects/{project_id}/optimization-proposals/generate",
            post(generate_optimization_proposals),
        )
        .route(
            "/projects/{project_id}/optimization-proposals/{proposal_id}/simulate/{candidate_version_id}",
            post(simulate_optimization_proposal),
        )
        .route(
            "/projects/{project_id}/optimization-proposals/{proposal_id}/confirm",
            post(confirm_optimization_proposal),
        )
        .route(
            "/projects/{project_id}/optimization-proposals/{proposal_id}/dismiss",
            post(dismiss_optimization_proposal),
        )
        .route(
            "/projects/{project_id}/decision-runs/{run_id}/outcome",
            put(save_decision_run_outcome),
        )
        .route(
            "/projects/{project_id}/deviation-analyses",
            get(list_deviation_analyses),
        )
        .route(
            "/projects/{project_id}/deviation-analysis-reviews",
            get(list_reviewed_deviation_analyses),
        )
        .route(
            "/projects/{project_id}/deviation-analysis-inventory",
            get(deviation_inventory::list),
        )
        .route(
            "/projects/{project_id}/deviation-analyses/{analysis_id}/review",
            post(review_deviation_analysis),
        )
        .layer(axum::extract::DefaultBodyLimit::max(7_000_000))
        .layer(Extension(llm_state))
}

/// Public execution is protected by a scoped X-Decision-Key, not by the
/// application's login JWT. It is intentionally kept outside the protected
/// router so integrations never need a browser session.
pub fn public_service_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/public/projects/{project_id}/decision-services/execute",
            post(execute_public_decision_service),
        )
        .route(
            "/public/projects/{project_id}/decision-runs/{run_id}/outcome",
            post(report_public_decision_run_outcome),
        )
        .route(
            "/public/projects/{project_id}/decision-runs/outcomes:batch",
            post(report_public_decision_run_outcomes_batch),
        )
        .route(
            "/public/projects/{project_id}/decision-runs/outcomes:reconcile",
            post(reconcile_public_decision_run_outcomes),
        )
        .layer(axum::extract::DefaultBodyLimit::max(7_000_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tester_judge_budget_failures_are_reported_as_budget_limits() {
        let token_error = tester_judge_budget_error(
            crate::api::ProjectTokenBudgetError::OrchestrationTokensExceeded {
                used: 250_000,
                limit: 250_000,
            },
        );
        assert_eq!(token_error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(token_error.code, "TESTER_JUDGE_BUDGET_ORCHESTRATION_TOKENS");
        assert!(token_error.message.contains("人工复核后继续"));

        let cost_error =
            tester_judge_budget_error(crate::api::ProjectTokenBudgetError::EstimatedCostExceeded {
                used: 0.9,
                next: 0.2,
                limit: 1.0,
            });
        assert_eq!(cost_error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(cost_error.code, "TESTER_JUDGE_BUDGET_COST");
    }

    #[tokio::test]
    async fn tester_judge_budget_response_marks_the_skipped_optional_agent() {
        let response = tester_judge_budget_error(
            crate::api::ProjectTokenBudgetError::OrchestrationTokensExceeded {
                used: 250_000,
                limit: 250_000,
            },
        )
        .into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(payload["degradation"]["kind"], "budget_skip");
        assert_eq!(payload["degradation"]["skippedAgent"], "tester_judge");
        assert_eq!(payload["degradation"]["manualReviewRequired"], true);
    }

    #[test]
    fn static_tester_proposals_are_scoped_to_their_draft_version() {
        let proposal = crate::workflow::SemanticTestProposal {
            category: "static_analysis".into(),
            source_reference: "draftVersion:52/staticFinding:MISSING_COVERAGE:Decision:r1".into(),
            ..Default::default()
        };
        assert!(tester_proposal_matches_version(&proposal, 52));
        assert!(!tester_proposal_matches_version(&proposal, 53));

        let malformed = crate::workflow::SemanticTestProposal {
            source_reference: "draftVersion:invalid/staticFinding".into(),
            ..proposal.clone()
        };
        assert!(!tester_proposal_matches_version(&malformed, 52));

        let semantic = crate::workflow::SemanticTestProposal {
            category: "semantic_scenario".into(),
            ..proposal
        };
        assert!(tester_proposal_matches_version(&semantic, 53));
    }

    #[test]
    fn decision_run_explanation_review_only_accepts_known_labels() {
        assert!(valid_decision_run_explanation_review_label("readable"));
        assert!(valid_decision_run_explanation_review_label(
            "needs_revision"
        ));
        assert!(valid_decision_run_explanation_review_label("inconclusive"));
        assert!(!valid_decision_run_explanation_review_label("approved"));
        assert!(!valid_decision_run_explanation_review_label(""));
    }

    #[test]
    fn decision_run_explanation_must_cover_exact_trace_decisions() {
        let trace = json!({
            "decisions": [
                {"decisionId": "eligibility", "decisionName": "Eligibility"},
                {"decisionId": "offer", "decisionName": "Offer"}
            ]
        });
        let valid = crate::models::DecisionRunExplanation {
            summary: "The request qualifies for the standard offer.".into(),
            decisions: vec![
                crate::models::DecisionRunDecisionExplanation {
                    decision_id: "eligibility".into(),
                    explanation: "Eligibility was evaluated successfully.".into(),
                },
                crate::models::DecisionRunDecisionExplanation {
                    decision_id: "offer".into(),
                    explanation: "The standard offer rule matched.".into(),
                },
            ],
        };
        assert!(validate_decision_run_explanation(&trace, &valid).is_ok());

        let missing_step = crate::models::DecisionRunExplanation {
            decisions: valid.decisions[..1].to_vec(),
            ..valid.clone()
        };
        assert!(validate_decision_run_explanation(&trace, &missing_step).is_err());
        let duplicate_step = crate::models::DecisionRunExplanation {
            decisions: vec![valid.decisions[0].clone(), valid.decisions[0].clone()],
            ..valid.clone()
        };
        assert!(validate_decision_run_explanation(&trace, &duplicate_step).is_err());
        let unknown_step = crate::models::DecisionRunExplanation {
            decisions: vec![
                valid.decisions[0].clone(),
                crate::models::DecisionRunDecisionExplanation {
                    decision_id: "invented".into(),
                    explanation: "This step is not in the trace.".into(),
                },
            ],
            ..valid
        };
        assert!(validate_decision_run_explanation(&trace, &unknown_step).is_err());
    }

    #[test]
    fn pattern_candidate_fusion_rewards_overlap_and_keeps_stable_ties() {
        let fused = fuse_pattern_candidate_ids(&[1, 2], &[(2, 91.0), (3, 80.0)]);
        assert_eq!(fused, vec![2, 1, 3]);
    }

    #[test]
    fn pattern_query_deserializes_camel_case_project_id() {
        let query: PatternRecommendationQuery =
            serde_json::from_value(json!({"q": "loan policy", "projectId": 42})).unwrap();
        assert_eq!(query.project_id, Some(42));
    }

    #[test]
    fn pattern_recommendation_ties_use_stable_pattern_id_order() {
        let recommendation = |pattern_id, match_score| PatternRecommendation {
            recommendation_id: 0,
            pattern_id,
            source_project_id: 1,
            source_project_name: "project".into(),
            source_version_id: pattern_id,
            source_version_number: 1,
            pattern_type: "general".into(),
            name: format!("pattern-{pattern_id}"),
            description: String::new(),
            match_score,
            recommendation_reason: String::new(),
            success_rate: None,
            usage_count: 0,
            evolution_number: 1,
        };
        let mut recommendations = vec![
            recommendation(9, 50.0),
            recommendation(3, 50.0),
            recommendation(5, 60.0),
        ];

        sort_pattern_recommendations(&mut recommendations);

        assert_eq!(
            recommendations
                .iter()
                .map(|item| item.pattern_id)
                .collect::<Vec<_>>(),
            [5, 3, 9]
        );
    }

    #[test]
    fn pattern_recommendation_query_is_trimmed_and_bounded_by_characters() {
        assert_eq!(validate_pattern_query("  信贷审批  "), Ok("信贷审批"));
        assert_eq!(validate_pattern_query(" \n "), Err("请提供需求关键词 q"));
        assert_eq!(
            validate_pattern_query(&"界".repeat(501)),
            Err("需求关键词 q 不能超过 500 个字符")
        );
        assert!(validate_pattern_query(&"贷".repeat(500)).is_ok());
    }

    #[test]
    fn trace_input_matching_is_fifo_and_ignores_object_key_order() {
        let baseline = vec![
            (
                1,
                ExecutionTrace {
                    normalized_input: json!({"a": 1, "b": {"x": 2, "y": 3}}),
                    ..Default::default()
                },
            ),
            (
                2,
                ExecutionTrace {
                    normalized_input: json!({"id": 1}),
                    ..Default::default()
                },
            ),
            (
                3,
                ExecutionTrace {
                    normalized_input: json!({"id": 1}),
                    ..Default::default()
                },
            ),
            (
                4,
                ExecutionTrace {
                    normalized_input: json!({"missing": true}),
                    ..Default::default()
                },
            ),
        ];
        let candidates = vec![
            (
                11,
                ExecutionTrace {
                    normalized_input: json!({"b": {"y": 3, "x": 2}, "a": 1}),
                    ..Default::default()
                },
            ),
            (
                12,
                ExecutionTrace {
                    normalized_input: json!({"id": 1}),
                    ..Default::default()
                },
            ),
            (
                13,
                ExecutionTrace {
                    normalized_input: json!({"id": 1}),
                    ..Default::default()
                },
            ),
        ];

        assert_eq!(
            match_trace_inputs(&baseline, &candidates),
            vec![(0, 0), (1, 1), (2, 2)]
        );
    }

    #[test]
    fn extracts_explainable_business_pattern_features() {
        let (kind, keywords) =
            pattern_features("申请人评分达到阈值后进入审批流程", "信贷", "信贷审批");
        assert_eq!(kind, "workflow");
        assert!(keywords.contains(&"审批".to_string()));
        assert!(keywords.contains(&"信贷".to_string()));
    }

    #[test]
    fn pattern_recommendations_require_actual_keyword_overlap() {
        let keywords = vec!["评分".to_string(), "信贷审批".to_string()];
        let (score, reason) = pattern_match_score("信贷评分模型", &keywords, "", "matrix");
        assert_eq!(score, 20.0);
        assert!(reason.contains("关键词"));
        let (no_match, _) = pattern_match_score("旅游报销", &keywords, "", "matrix");
        assert_eq!(no_match, 0.0);
    }

    #[test]
    fn empty_pattern_query_is_not_recommended() {
        let (score, reason) = pattern_match_score("  ", &["审批".into()], "", "workflow");
        assert_eq!(score, 0.0);
        assert!(reason.contains("请输入"));
    }

    #[test]
    fn pattern_acceptance_rate_is_a_percentage() {
        assert_eq!(calculate_pattern_acceptance_rate(1, 1), Some(50.0));
        assert_eq!(calculate_pattern_acceptance_rate(2, 1), Some(66.67));
        assert_eq!(calculate_pattern_acceptance_rate(0, 0), None);
    }

    #[test]
    fn semantic_pattern_matching_uses_cjk_phrases_from_source_text() {
        let terms = semantic_terms("信贷申请按客户等级审批");
        assert!(terms.contains(&"信贷".to_string()));
        assert!(terms.contains(&"审批".to_string()));
        let (score, reason) = semantic_pattern_match_score(
            "客户等级审批流程",
            &[],
            "",
            "workflow",
            "信贷申请按客户等级审批",
        );
        assert!(score > 0.0);
        assert!(reason.contains("业务语义片段"));
    }

    #[test]
    fn optimization_guidance_preserves_human_review_boundary() {
        let (title, actions) = optimization_guidance("HIGH_MISMATCH_RATE");
        assert_eq!(title, "复核规则与实际业务结果的偏差");
        assert!(actions.iter().any(|action| action.contains("草稿")));
        assert!(actions.iter().all(|action| !action.contains("自动发布")));

        let (_, drift_actions) = optimization_guidance("RULE_HIT_DRIFT");
        assert!(
            drift_actions
                .iter()
                .any(|action| action.contains("验收案例"))
        );
        assert!(
            drift_actions
                .iter()
                .all(|action| !action.contains("自动发布"))
        );

        let (_, distribution_actions) = optimization_guidance("INPUT_DISTRIBUTION_DRIFT");
        assert!(
            distribution_actions
                .iter()
                .any(|action| action.contains("阈值"))
        );
        assert!(
            distribution_actions
                .iter()
                .all(|action| !action.contains("自动发布"))
        );
    }

    #[test]
    fn optimization_suggestions_require_exact_unique_finding_matches() {
        let findings = vec![DecisionAnalyticsFinding {
            code: "HIGH_MISMATCH_RATE".into(),
            element: "Approval".into(),
            ..Default::default()
        }];
        let suggestion = crate::nl_dsl::agent::OptimizationSuggestion {
            finding_code: "HIGH_MISMATCH_RATE".into(),
            element: "Approval".into(),
            title: "复核业务偏差".into(),
            summary: "抽查偏差样本并确认业务口径".into(),
            proposed_actions: vec!["补充已确认的偏差案例".into()],
        };

        assert_eq!(
            optimization_suggestion_map(&findings, vec![suggestion.clone()]).len(),
            1
        );
        let mut mismatched = suggestion.clone();
        mismatched.element = "UnknownDecision".into();
        assert!(optimization_suggestion_map(&findings, vec![mismatched]).is_empty());
        assert!(optimization_suggestion_map(&findings, Vec::new()).is_empty());
        assert!(
            optimization_suggestion_map(&findings, vec![suggestion.clone(), suggestion]).is_empty()
        );
    }

    #[test]
    fn optimization_suggestions_reject_empty_or_oversized_actions() {
        let findings = vec![DecisionAnalyticsFinding {
            code: "HIGH_FAILURE_RATE".into(),
            element: String::new(),
            ..Default::default()
        }];
        let mut suggestion = crate::nl_dsl::agent::OptimizationSuggestion {
            finding_code: "HIGH_FAILURE_RATE".into(),
            element: String::new(),
            title: "复核失败原因".into(),
            summary: "按错误类型抽查失败记录".into(),
            proposed_actions: vec!["补充针对性验收案例".into()],
        };
        suggestion.proposed_actions.clear();
        assert!(optimization_suggestion_map(&findings, vec![suggestion.clone()]).is_empty());
        suggestion.proposed_actions = vec!["建议".repeat(121)];
        assert!(optimization_suggestion_map(&findings, vec![suggestion]).is_empty());
    }

    #[test]
    fn drift_automation_rules_accept_only_bounded_supported_signals() {
        let payload = UpdateDecisionDriftAutomation {
            enabled: true,
            finding_codes: vec![
                "RULE_HIT_DRIFT".into(),
                "HIGH_FAILURE_RATE".into(),
                "RULE_HIT_DRIFT".into(),
            ],
            cooldown_hours: 24,
            webhook_url: None,
        };
        assert_eq!(
            validate_drift_automation_rule(&payload).unwrap().0,
            ["HIGH_FAILURE_RATE", "RULE_HIT_DRIFT"]
        );

        let missingness = UpdateDecisionDriftAutomation {
            enabled: true,
            finding_codes: vec!["INPUT_FIELD_MISSINGNESS_DRIFT".into()],
            cooldown_hours: 24,
            webhook_url: None,
        };
        assert_eq!(
            validate_drift_automation_rule(&missingness).unwrap().0,
            ["INPUT_FIELD_MISSINGNESS_DRIFT"]
        );

        let empty = UpdateDecisionDriftAutomation {
            enabled: true,
            finding_codes: Vec::new(),
            cooldown_hours: 24,
            webhook_url: None,
        };
        assert!(validate_drift_automation_rule(&empty).is_err());
        let unsupported = UpdateDecisionDriftAutomation {
            enabled: true,
            finding_codes: vec!["MODEL_PUBLISH".into()],
            cooldown_hours: 24,
            webhook_url: None,
        };
        assert!(validate_drift_automation_rule(&unsupported).is_err());
        let invalid_cooldown = UpdateDecisionDriftAutomation {
            enabled: false,
            finding_codes: Vec::new(),
            cooldown_hours: 721,
            webhook_url: None,
        };
        assert!(validate_drift_automation_rule(&invalid_cooldown).is_err());
    }

    #[test]
    fn input_missingness_drift_has_data_quality_guidance_without_auto_publish() {
        let (title, actions) = optimization_guidance("INPUT_FIELD_MISSINGNESS_DRIFT");
        assert!(title.contains("缺失率"));
        assert!(actions.iter().any(|action| action.contains("数据质量")));
        assert!(actions.iter().all(|action| !action.contains("自动发布")));
    }

    #[test]
    fn drift_webhook_signatures_bind_timestamp_and_body() {
        let first =
            drift_webhook_signature("a sufficiently long test secret value", 123, b"payload");
        assert_eq!(
            first,
            "52f44c6eb87fba22bf31a346f2aab9d5f86687bdf4dcc73c39edc3649799e06e"
        );
        assert_ne!(
            first,
            drift_webhook_signature("a sufficiently long test secret value", 124, b"payload")
        );
        assert_ne!(
            first,
            drift_webhook_signature("a sufficiently long test secret value", 123, b"changed")
        );
        assert_eq!(drift_webhook_retry_delay(1).as_secs(), 30);
        assert_eq!(drift_webhook_retry_delay(8).as_secs(), 3840);
        assert_eq!(drift_webhook_retry_delay(100).as_secs(), 21600);
    }

    #[test]
    fn drift_webhook_url_rejects_non_https_credentials_queries_and_ip_literals() {
        for url in [
            "http://hooks.example.com/path",
            "https://user:password@hooks.example.com/path",
            "https://hooks.example.com/path?token=secret",
            "https://127.0.0.1/path",
        ] {
            assert!(validate_drift_webhook_url(url).is_err(), "accepted {url}");
        }
    }

    #[test]
    fn drift_event_review_accepts_only_supported_labels() {
        for label in ["confirmed_drift", "false_positive", "inconclusive"] {
            assert_eq!(validate_drift_event_review_label(label), Ok(label));
        }
        for label in ["", "dismissed", "true", "CONFIRMED_DRIFT"] {
            assert!(validate_drift_event_review_label(label).is_err());
        }
    }

    #[test]
    fn drift_scan_window_review_accepts_only_supported_labels() {
        for label in ["drift", "normal", "inconclusive"] {
            assert_eq!(validate_drift_scan_window_review_label(label), Ok(label));
        }
        for label in ["", "confirmed_drift", "false_positive", "DRIFT"] {
            assert!(validate_drift_scan_window_review_label(label).is_err());
        }
    }

    #[test]
    fn deviation_analysis_review_labels_match_evidence_status() {
        for label in ["correct", "incorrect", "inconclusive"] {
            assert_eq!(
                validate_deviation_analysis_review("candidate", label),
                Ok(label)
            );
        }
        assert_eq!(
            validate_deviation_analysis_review("insufficient_evidence", "inconclusive"),
            Ok("inconclusive")
        );
        for label in ["", "correct", "incorrect"] {
            if label != "" {
                assert!(
                    validate_deviation_analysis_review("insufficient_evidence", label).is_err()
                );
            }
        }
        assert!(validate_deviation_analysis_review("unknown", "inconclusive").is_err());
        assert!(validate_deviation_analysis_review("candidate", "confirmed_drift").is_err());
    }

    #[test]
    fn decision_copilot_parser_is_whitelisted_and_routes_writes() {
        let supported = [
            (
                "运行次数",
                Some(DecisionCopilotMetric::TotalRuns),
                Some(30),
                "answered",
            ),
            (
                "近 7 天成功比例",
                Some(DecisionCopilotMetric::SuccessRate),
                Some(7),
                "answered",
            ),
            (
                "过去一周失败比例",
                Some(DecisionCopilotMetric::FailureRate),
                Some(7),
                "answered",
            ),
            (
                "近30天不一致率",
                Some(DecisionCopilotMetric::MismatchRate),
                Some(30),
                "answered",
            ),
            (
                "最近一个月回填覆盖率",
                Some(DecisionCopilotMetric::FeedbackCoverage),
                Some(30),
                "answered",
            ),
            (
                "近7天平均延迟",
                Some(DecisionCopilotMetric::AverageLatency),
                Some(7),
                "answered",
            ),
            (
                "最近 P95 耗时",
                Some(DecisionCopilotMetric::P95Latency),
                Some(30),
                "answered",
            ),
        ];
        for (question, expected_metric, expected_days, expected_status) in supported {
            assert_eq!(
                parse_decision_copilot_intent(question),
                (expected_metric, expected_days, expected_status),
                "unexpected parse for {question}"
            );
        }
        assert_eq!(
            parse_decision_copilot_intent("最近7天失败率是多少"),
            (
                Some(DecisionCopilotMetric::FailureRate),
                Some(7),
                "answered"
            )
        );
        assert_eq!(
            parse_decision_copilot_intent("最近 7 天失败率是多少"),
            (
                Some(DecisionCopilotMetric::FailureRate),
                Some(7),
                "answered"
            )
        );
        assert_eq!(
            parse_decision_copilot_intent("最近14天失败率是多少"),
            (None, None, "clarification")
        );
        assert_eq!(
            parse_decision_copilot_intent("上周失败率是多少"),
            (None, None, "clarification")
        );
        for question in [
            "最近 90 天失败率是多少",
            "最近两个月失败率是多少",
            "今年失败率是多少",
            "本月失败率是多少",
            "近两周失败率是多少",
        ] {
            assert_eq!(
                parse_decision_copilot_intent(question),
                (None, None, "clarification"),
                "unsupported time window should clarify: {question}"
            );
        }
        assert_eq!(
            parse_decision_copilot_intent("最近失败率和成功率"),
            (None, None, "clarification")
        );
        assert_eq!(
            parse_decision_copilot_intent("把失败率调整到 5%"),
            (None, None, "routed")
        );
        assert_eq!(
            parse_decision_copilot_intent("最近7天 P95 耗时"),
            (None, None, "clarification")
        );
        assert_eq!(
            parse_decision_copilot_intent("执行 SELECT * FROM users"),
            (None, None, "clarification")
        );
    }

    #[test]
    fn decision_copilot_kill_switch_defaults_on_and_fails_closed() {
        assert!(decision_copilot_enabled_from_value(None));
        for enabled in ["1", "true", "TRUE", "yes", "on"] {
            assert!(decision_copilot_enabled_from_value(Some(enabled)));
        }
        for disabled in ["0", "false", "no", "off", "", "invalid"] {
            assert!(!decision_copilot_enabled_from_value(Some(disabled)));
        }
    }

    #[test]
    fn decision_copilot_golden_intent_accuracy_meets_stage_eight_threshold() {
        let mut total = 0_usize;
        let mut correct = 0_usize;
        let mut mismatches = Vec::new();

        for (line_number, line) in include_str!("../tests/golden/copilot-intents.tsv")
            .lines()
            .enumerate()
        {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 4, "malformed golden row {}", line_number + 1);
            let (expected_status, expected_metric, expected_days, question) =
                (fields[0], fields[1], fields[2], fields[3]);
            let expected_metric = (expected_metric != "-").then_some(expected_metric);
            let expected_days = if expected_days == "-" {
                None
            } else {
                Some(
                    expected_days
                        .parse::<u32>()
                        .expect("valid golden day count"),
                )
            };
            let actual = parse_decision_copilot_intent(question);
            let actual_metric = actual.0.map(DecisionCopilotMetric::key);
            let expected = (expected_metric, expected_days, expected_status);
            let actual = (actual_metric, actual.1, actual.2);
            total += 1;
            if actual == expected {
                correct += 1;
            } else {
                mismatches.push(format!(
                    "line {}: {:?} expected {:?} for {:?}",
                    line_number + 1,
                    actual,
                    expected,
                    question
                ));
            }
        }

        let accuracy = correct as f64 / total as f64;
        eprintln!(
            "Copilot intent golden accuracy: {correct}/{total} ({:.1}%)",
            accuracy * 100.0
        );
        if !mismatches.is_empty() {
            eprintln!("Copilot intent mismatches:\n{}", mismatches.join("\n"));
        }
        assert!(total >= 20, "golden set should cover enough intent cases");
        assert!(
            accuracy >= 0.90,
            "intent accuracy {:.1}% is below 90%:\n{}",
            accuracy * 100.0,
            mismatches.join("\n")
        );
    }

    #[test]
    fn decision_copilot_uses_daily_whitelist_metrics_and_discloses_denominator() {
        let generated_at = chrono::DateTime::parse_from_rfc3339("2026-11-13T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let report = DecisionAnalyticsReport {
            project_id: 1,
            version_id: 2,
            version_number: 3,
            window_days: 30,
            window_start: generated_at - chrono::Duration::days(29),
            generated_at,
            summary: Default::default(),
            numeric_input_omitted_runs: 0,
            trace_details_omitted_runs: 0,
            unknown_result_steps_omitted: 0,
            rule_coverage: Vec::new(),
            result_distributions: Vec::new(),
            daily_trends: vec![
                crate::models::DecisionDailyTrendMetric {
                    date: chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
                    total_runs: 2,
                    succeeded_runs: 1,
                    failed_runs: 1,
                    average_duration_ms: 100.0,
                    ..Default::default()
                },
                crate::models::DecisionDailyTrendMetric {
                    date: chrono::NaiveDate::from_ymd_opt(2026, 11, 13).unwrap(),
                    total_runs: 3,
                    succeeded_runs: 3,
                    failed_runs: 0,
                    average_duration_ms: 300.0,
                    ..Default::default()
                },
            ],
            findings: Vec::new(),
            drift_signals: Vec::new(),
        };
        let result = decision_copilot_query_result(&report, DecisionCopilotMetric::FailureRate, 7);
        assert_eq!(result.status, "answered");
        assert_eq!(result.value, Some(20.0));
        assert_eq!(result.numerator, Some(1));
        assert_eq!(result.denominator, Some(5));
        assert_eq!(result.sample_size, Some(5));
        assert!(result.answer.contains("样本 1/5"));
    }

    #[test]
    fn drift_event_review_pagination_is_bounded() {
        assert_eq!(
            validate_drift_event_review_page(&DriftEventReviewPageQuery {
                after_id: Some(10),
                limit: Some(500),
                newest: None,
            }),
            Ok((10, 500))
        );
        for query in [
            DriftEventReviewPageQuery {
                after_id: Some(-1),
                limit: None,
                newest: None,
            },
            DriftEventReviewPageQuery {
                after_id: None,
                limit: Some(0),
                newest: None,
            },
            DriftEventReviewPageQuery {
                after_id: None,
                limit: Some(501),
                newest: None,
            },
        ] {
            assert!(validate_drift_event_review_page(&query).is_err());
        }
    }

    #[test]
    fn optimization_shadow_history_pagination_is_bounded() {
        assert_eq!(
            validate_optimization_autodraft_shadow_page(&OptimizationAutodraftShadowPageQuery {
                after_id: Some(10),
                limit: Some(500),
            }),
            Ok((10, 500))
        );
        assert_eq!(
            validate_optimization_autodraft_shadow_page(&OptimizationAutodraftShadowPageQuery {
                after_id: None,
                limit: None,
            }),
            Ok((0, 100))
        );
        for query in [
            OptimizationAutodraftShadowPageQuery {
                after_id: Some(-1),
                limit: None,
            },
            OptimizationAutodraftShadowPageQuery {
                after_id: None,
                limit: Some(0),
            },
            OptimizationAutodraftShadowPageQuery {
                after_id: None,
                limit: Some(501),
            },
        ] {
            assert!(validate_optimization_autodraft_shadow_page(&query).is_err());
        }
    }

    #[test]
    fn optimization_shadow_history_does_not_serialize_raw_inputs_or_audit_bodies() {
        let evaluation = OptimizationAutodraftShadowEvaluation {
            id: 1,
            simulation_id: 2,
            project_id: 3,
            proposal_id: 4,
            source_version_id: 5,
            candidate_version_id: 6,
            outcome: "blocked_project_risk".into(),
            finding_code: "HIGH_FAILURE_RATE".into(),
            sample_source: "approved_business_tests".into(),
            test_set_size: 12,
            baseline_accuracy: 92.0,
            simulated_accuracy: 93.0,
            created_at: chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
        };
        let value = serde_json::to_value(evaluation).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.get("simulationId").and_then(Value::as_i64), Some(2));
        assert!(!object.contains_key("inputJson"));
        assert!(!object.contains_key("executionTraceJson"));
        assert!(!object.contains_key("evidence"));
        assert!(!object.contains_key("evaluatedBy"));
    }

    #[test]
    fn drift_event_evidence_reconstructs_a_proposal_ready_report() {
        let report = drift_event_analytics_report(
            7,
            19,
            json!({
                "sourceVersionNumber": 4,
                "windowDays": 30,
                "windowStart": "2026-10-01T00:00:00Z",
                "generatedAt": "2026-10-31T00:00:00Z",
                "summary": {
                    "totalRuns": 20,
                    "succeededRuns": 18,
                    "failedRuns": 2,
                    "outcomeReportedRuns": 10,
                    "confirmedRuns": 7,
                    "mismatchRuns": 3,
                    "inconclusiveRuns": 0,
                    "failureRate": 0.1,
                    "feedbackCoverageRate": 0.5,
                    "mismatchRate": 0.3,
                    "averageDurationMs": 21.0,
                    "p95DurationMs": 48
                },
                "findings": [{
                    "level": "warning",
                    "code": "RULE_HIT_DRIFT",
                    "message": "规则命中变化",
                    "element": "Risk"
                }]
            }),
        )
        .expect("event evidence should reconstruct its analytics snapshot");

        assert_eq!(report.project_id, 7);
        assert_eq!(report.version_id, 19);
        assert_eq!(report.version_number, 4);
        assert_eq!(report.findings[0].code, "RULE_HIT_DRIFT");
        assert!(report.rule_coverage.is_empty());
    }

    #[test]
    fn optimization_proposals_withdraw_when_the_published_baseline_changes() {
        assert_eq!(optimization_proposal_withdrawal_note(12, 12, true), None);
        assert_eq!(
            optimization_proposal_withdrawal_note(12, 12, false),
            Some("自动撤回：当前分析未复现该发现")
        );
        assert_eq!(
            optimization_proposal_withdrawal_note(12, 13, true),
            Some("自动撤回：项目发布版本已变化")
        );
    }

    #[test]
    fn analytics_thresholds_require_bounded_warning_and_higher_high_rates() {
        let defaults = DecisionAnalyticsThresholds::default();
        assert!(validate_analytics_thresholds(&defaults).is_ok());

        let mut invalid = defaults.clone();
        invalid.failure_warning_rate = f64::NAN;
        assert!(validate_analytics_thresholds(&invalid).is_err());

        let mut invalid = defaults.clone();
        invalid.mismatch_high_rate = invalid.mismatch_warning_rate;
        assert!(validate_analytics_thresholds(&invalid).is_err());

        let mut invalid = defaults;
        invalid.failure_high_rate = 1.01;
        assert!(validate_analytics_thresholds(&invalid).is_err());

        let mut invalid = DecisionAnalyticsThresholds::default();
        invalid.failure_min_runs = 0;
        assert!(validate_analytics_thresholds(&invalid).is_err());

        let mut invalid = DecisionAnalyticsThresholds::default();
        invalid.mismatch_min_samples = 100_001;
        assert!(validate_analytics_thresholds(&invalid).is_err());
    }

    #[test]
    fn optimization_proposal_withdrawal_audit_is_specific_and_contains_no_run_evidence() {
        let details = optimization_proposal_withdrawal_audit_details(
            41,
            12,
            "HIGH_FAILURE_RATE",
            "RiskDecision",
            "自动撤回：项目发布版本已变化",
        );

        assert_eq!(details["proposalId"], 41);
        assert_eq!(details["sourceVersionId"], 12);
        assert_eq!(details["findingCode"], "HIGH_FAILURE_RATE");
        assert_eq!(details["element"], "RiskDecision");
        assert_eq!(details["resolutionNote"], "自动撤回：项目发布版本已变化");
        assert!(details.get("evidence").is_none());
        assert!(details.get("representativeRuns").is_none());
    }

    #[test]
    fn optimization_shadow_policy_requires_mode_and_explicit_positive_allowlist() {
        let disabled = OptimizationAutodraftShadowPolicy::from_values(None, Some("7"));
        assert!(!disabled.includes(7));
        let no_allowlist = OptimizationAutodraftShadowPolicy::from_values(Some("shadow"), None);
        assert!(!no_allowlist.includes(7));
        let invalid_allowlist =
            OptimizationAutodraftShadowPolicy::from_values(Some("shadow"), Some("7,invalid"));
        assert!(!invalid_allowlist.includes(7));

        let enabled =
            OptimizationAutodraftShadowPolicy::from_values(Some("SHADOW"), Some("7, 8,7"));
        assert!(enabled.includes(7));
        assert!(enabled.includes(8));
        assert!(!enabled.includes(9));
        assert!(!enabled.draft_enabled);

        let draft = OptimizationAutodraftShadowPolicy::from_values(Some("draft"), Some("7"));
        assert!(draft.includes(7));
        assert!(draft.draft_enabled);
        let draft_without_allowlist =
            OptimizationAutodraftShadowPolicy::from_values(Some("draft"), Some("invalid"));
        assert!(!draft_without_allowlist.includes(7));
        assert!(!draft_without_allowlist.draft_enabled);
    }

    #[test]
    fn optimization_shadow_gate_fails_closed_for_every_unmet_requirement() {
        let low_risk_review = json!({
            "overallRisk": "low",
            "changes": [{"changeType": "update"}]
        });
        let eligible = |review: Option<&Value>| {
            optimization_autodraft_shadow_outcome(
                "low",
                "HIGH_FAILURE_RATE",
                "approved_business_tests",
                10,
                true,
                review,
            )
        };
        assert_eq!(
            eligible(Some(&low_risk_review)),
            "eligible_shadow_candidate"
        );
        assert_eq!(
            optimization_autodraft_shadow_outcome(
                "medium",
                "HIGH_FAILURE_RATE",
                "approved_business_tests",
                10,
                true,
                Some(&low_risk_review),
            ),
            "blocked_project_risk"
        );
        assert_eq!(
            optimization_autodraft_shadow_outcome(
                "low",
                "LOW_RULE_COVERAGE",
                "approved_business_tests",
                10,
                true,
                Some(&low_risk_review),
            ),
            "blocked_finding_code"
        );
        assert_eq!(
            optimization_autodraft_shadow_outcome(
                "low",
                "HIGH_FAILURE_RATE",
                "successful_history_runs",
                10,
                true,
                Some(&low_risk_review),
            ),
            "blocked_unlabeled_samples"
        );
        assert_eq!(
            optimization_autodraft_shadow_outcome(
                "low",
                "HIGH_FAILURE_RATE",
                "approved_business_tests",
                9,
                true,
                Some(&low_risk_review),
            ),
            "blocked_insufficient_samples"
        );
        assert_eq!(
            optimization_autodraft_shadow_outcome(
                "low",
                "HIGH_FAILURE_RATE",
                "approved_business_tests",
                10,
                false,
                Some(&low_risk_review),
            ),
            "blocked_simulation"
        );
        assert_eq!(eligible(None), "blocked_change_risk");
        assert_eq!(
            eligible(Some(
                &json!({"overallRisk":"high","changes":[{"changeType":"update"}]})
            ),),
            "blocked_change_risk"
        );
        assert_eq!(
            eligible(Some(
                &json!({"overallRisk":"low","changes":[{"changeType":"add"}]})
            ),),
            "blocked_change_risk"
        );
    }

    #[test]
    fn optimization_proposal_run_evidence_keeps_field_names_without_payload_values() {
        let evidence = optimization_proposal_run_evidence((
            41,
            chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
            "succeeded".into(),
            Some("mismatch".into()),
            json!(["age", "region"]),
            json!(["decision", "reason"]),
        ));
        assert_eq!(evidence["runId"], 41);
        assert_eq!(evidence["inputFields"], json!(["age", "region"]));
        assert_eq!(evidence["outputFields"], json!(["decision", "reason"]));
        assert!(!evidence.as_object().unwrap().contains_key("inputJson"));
        assert!(!evidence.as_object().unwrap().contains_key("outputJson"));
        assert!(
            !evidence
                .as_object()
                .unwrap()
                .contains_key("executionTraceJson")
        );
    }

    #[test]
    fn optimization_proposal_evidence_is_versioned_and_declares_its_privacy_policy() {
        let finding = DecisionAnalyticsFinding {
            level: "warning".into(),
            code: "HIGH_FAILURE_RATE".into(),
            message: "失败率偏高".into(),
            element: "RiskDecision".into(),
            numeric_feedback_split: None,
        };
        let run = optimization_proposal_run_evidence((
            41,
            chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
            "failed".into(),
            None,
            json!(["age", "region"]),
            json!(["decision"]),
        ));
        let evidence = optimization_proposal_evidence(
            &finding,
            &DecisionAnalyticsSummary::default(),
            chrono::DateTime::from_timestamp(1_799_000_000, 0).unwrap(),
            30,
            chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
            vec![run],
        );

        assert_eq!(evidence["schemaVersion"], 1);
        assert_eq!(evidence["source"], "decision_analytics");
        assert_eq!(
            evidence["privacy"]["representativeRunFields"],
            "field_names_only"
        );
        assert_eq!(evidence["privacy"]["rawInputValuesIncluded"], false);
        assert_eq!(evidence["privacy"]["rawOutputValuesIncluded"], false);
        assert_eq!(evidence["privacy"]["executionTraceIncluded"], false);
        assert_eq!(
            evidence["representativeRuns"][0]["inputFields"],
            json!(["age", "region"])
        );
        assert!(evidence["representativeRuns"][0].get("inputJson").is_none());
        assert!(
            evidence["representativeRuns"][0]
                .get("outputJson")
                .is_none()
        );
        assert!(
            evidence["representativeRuns"][0]
                .get("executionTraceJson")
                .is_none()
        );
        assert!(!evidence.to_string().contains("sensitive input value"));
    }

    #[test]
    fn change_intent_updates_nested_fields_without_before_values() {
        let base = nl_dsl::parse_definitions(
            r#"
name: Approval
inputs:
  - name: Age
    typeRef: number
decisions:
  - name: Result
    typeRef: string
    requiredInputs: [Age]
    logic:
      type: decisionTable
      inputs:
        - expression: Age
      outputs:
        - name: result
          typeRef: string
      rules:
        - inputEntries: [">= 18"]
          outputEntries: ['"Approve"']
"#,
        )
        .unwrap();
        let intent = ModelChangeIntent {
            base_revision: 1,
            base_hash: String::new(),
            changes: vec![crate::models::ModelChangeOperation {
                target: "decision:Result".into(),
                action: "update".into(),
                field: Some("logic.rules[0].inputEntries[0]".into()),
                value: Some(json!(">= 21")),
                reason: "提高门槛".into(),
            }],
        };
        assert!(apply_change_intent(&base, &intent).is_err());

        let prefixed = ModelChangeIntent {
            base_revision: 1,
            base_hash: String::new(),
            changes: vec![crate::models::ModelChangeOperation {
                target: "decision:Result".into(),
                action: "update".into(),
                field: Some("decisions[0].logic.rules[0].inputEntries[0]".into()),
                value: Some(json!(">= 21")),
                reason: "提高门槛".into(),
            }],
        };
        assert!(apply_change_intent(&base, &prefixed).is_err());
    }

    #[test]
    fn change_intent_adds_and_removes_named_elements() {
        let base =
            nl_dsl::parse_definitions("name: Approval\ninputs: []\ndecisions: []\n").unwrap();
        let renamed = apply_change_intent(
            &base,
            &ModelChangeIntent {
                base_revision: 1,
                base_hash: String::new(),
                changes: vec![crate::models::ModelChangeOperation {
                    target: "model:Approval".into(),
                    action: "update".into(),
                    field: Some("description".into()),
                    value: Some(json!("Updated approval model")),
                    reason: String::new(),
                }],
            },
        )
        .unwrap();
        assert_eq!(
            renamed.description.as_deref(),
            Some("Updated approval model")
        );
        let added = apply_change_intent(
            &base,
            &ModelChangeIntent {
                base_revision: 1,
                base_hash: String::new(),
                changes: vec![crate::models::ModelChangeOperation {
                    target: "input:Age".into(),
                    action: "add".into(),
                    field: None,
                    value: Some(json!({"name": "Age", "typeRef": "number"})),
                    reason: String::new(),
                }],
            },
        )
        .unwrap();
        assert_eq!(added.inputs.len(), 1);
        let removed = apply_change_intent(
            &added,
            &ModelChangeIntent {
                base_revision: 1,
                base_hash: String::new(),
                changes: vec![crate::models::ModelChangeOperation {
                    target: "input:Age".into(),
                    action: "remove".into(),
                    field: None,
                    value: None,
                    reason: String::new(),
                }],
            },
        )
        .unwrap();
        assert!(removed.inputs.is_empty());
    }

    #[test]
    fn change_intent_prunes_diagram_edges_when_dependencies_are_removed() {
        let base = nl_dsl::parse_definitions(
            r#"
name: LoanModel
inputs:
  - name: OverdueCount
    typeRef: number
decisions:
  - name: RiskLevel
    typeRef: string
    requiredInputs: [OverdueCount]
    logic:
      type: literalExpression
      expression: '"High"'
  - name: LoanDecision
    typeRef: string
    requiredInputs: [OverdueCount]
    requiredDecisions: [RiskLevel]
    logic:
      type: literalExpression
      expression: '"Approve"'
diagram:
  nodes:
    - element: OverdueCount
      x: 0
      y: 0
    - element: RiskLevel
      x: 100
      y: 0
    - element: LoanDecision
      x: 200
      y: 0
  edges:
    - source: OverdueCount
      target: LoanDecision
      waypoints: [[10, 20]]
    - source: RiskLevel
      target: LoanDecision
      waypoints: [[30, 40]]
"#,
        )
        .unwrap();
        let updated = apply_change_intent(
            &base,
            &ModelChangeIntent {
                base_revision: 1,
                base_hash: String::new(),
                changes: vec![crate::models::ModelChangeOperation {
                    target: "decision:LoanDecision".into(),
                    action: "update".into(),
                    field: Some("requiredInputs".into()),
                    value: Some(json!([])),
                    reason: "移除直接输入依赖".into(),
                }],
            },
        )
        .unwrap();

        let diagram = updated.diagram.as_ref().unwrap();
        assert_eq!(diagram.nodes.len(), 3);
        assert_eq!(diagram.edges.len(), 1);
        assert_eq!(diagram.edges[0].source, "RiskLevel");
        assert_eq!(diagram.edges[0].waypoints, vec![(30.0, 40.0)]);
        nl_dsl::validate(&updated).unwrap();
    }

    #[test]
    fn change_intent_normalizes_integer_input_type_to_feel_number() {
        let base =
            nl_dsl::parse_definitions("name: Approval\ninputs: []\ndecisions: []\n").unwrap();
        let updated = apply_change_intent(
            &base,
            &ModelChangeIntent {
                base_revision: 1,
                base_hash: String::new(),
                changes: vec![crate::models::ModelChangeOperation {
                    target: "input:OrdersLast30Days".into(),
                    action: "add".into(),
                    field: None,
                    value: Some(json!({
                        "name": "OrdersLast30Days",
                        "typeRef": "integer"
                    })),
                    reason: String::new(),
                }],
            },
        )
        .unwrap();
        assert_eq!(updated.inputs[0].type_ref.as_deref(), Some("number"));
    }

    #[test]
    fn change_intent_rejects_unknown_targets_and_fields() {
        let base =
            nl_dsl::parse_definitions("name: Approval\ninputs: []\ndecisions: []\n").unwrap();
        let unknown_target = ModelChangeIntent {
            base_revision: 1,
            base_hash: String::new(),
            changes: vec![crate::models::ModelChangeOperation {
                target: "widget:Age".into(),
                action: "add".into(),
                field: None,
                value: Some(json!({"name": "Age"})),
                reason: String::new(),
            }],
        };
        assert!(apply_change_intent(&base, &unknown_target).is_err());
        let unknown_field = ModelChangeIntent {
            base_revision: 1,
            base_hash: String::new(),
            changes: vec![crate::models::ModelChangeOperation {
                target: "model:Approval".into(),
                action: "update".into(),
                field: Some("secrets".into()),
                value: Some(json!(true)),
                reason: String::new(),
            }],
        };
        assert!(apply_change_intent(&base, &unknown_field).is_err());
    }

    #[test]
    fn change_intent_accepts_declared_element_fields_and_aliases() {
        let base = nl_dsl::parse_definitions(
            "name: Approval\ninputs: []\ndecisions:\n  - name: Result\n    requiredInputs: []\n    requiredDecisions: []\n    requiredKnowledge: []\n    logic:\n      type: literalExpression\n      expression: '1'\n",
        )
        .unwrap();
        let updated = apply_change_intent(
            &base,
            &ModelChangeIntent {
                base_revision: 1,
                base_hash: String::new(),
                changes: vec![crate::models::ModelChangeOperation {
                    target: "decision:Result".into(),
                    action: "update".into(),
                    field: Some("required_decisions".into()),
                    value: Some(json!(["OtherDecision"])),
                    reason: String::new(),
                }],
            },
        )
        .unwrap();
        assert_eq!(
            updated.decisions[0].required_decisions,
            vec!["OtherDecision"]
        );

        let literal_base = nl_dsl::parse_definitions(
            "name: Approval\ndecisions:\n  - name: Result\n    logic:\n      type: literalExpression\n      expression: '1'\n",
        )
        .unwrap();
        let literal_updated = apply_change_intent(
            &literal_base,
            &ModelChangeIntent {
                base_revision: 1,
                base_hash: String::new(),
                changes: vec![crate::models::ModelChangeOperation {
                    target: "decision:Result".into(),
                    action: "update".into(),
                    field: Some("literalExpression.expression".into()),
                    value: Some(json!("2")),
                    reason: String::new(),
                }],
            },
        )
        .unwrap();
        let logic = serde_json::to_value(&literal_updated.decisions[0].logic).unwrap();
        assert_eq!(logic["expression"], json!("2"));
    }

    #[test]
    fn change_intent_base_requires_revision_and_hash() {
        let intent = ModelChangeIntent {
            base_revision: 7,
            base_hash: "abc".into(),
            changes: Vec::new(),
        };
        assert!(validate_intent_base(&intent, 7, "abc").is_ok());
        let error = validate_intent_base(&intent, 8, "abc").unwrap_err();
        assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
        assert_eq!(error.code, "BASE_CONFLICT");
        let error = validate_intent_base(&intent, 7, "def").unwrap_err();
        assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    }

    #[test]
    fn change_intent_array_paths_are_rejected_as_patch_targets() {
        let base = nl_dsl::parse_definitions(
            "name: Approval\ndecisions:\n  - name: Result\n    logic:\n      type: literalExpression\n      expression: '1'\n",
        )
        .unwrap();
        for field in [
            "logic.rules[0]",
            "logic.rules[0].outputEntries[0]",
            "logic.rules.0.outputEntries.0",
            "decisions[0].logic.expression",
            "logic.rules]",
        ] {
            let intent = ModelChangeIntent {
                base_revision: 1,
                base_hash: String::new(),
                changes: vec![crate::models::ModelChangeOperation {
                    target: "decision:Result".into(),
                    action: "update".into(),
                    field: Some(field.into()),
                    value: Some(json!("x")),
                    reason: String::new(),
                }],
            };
            let error = apply_change_intent(&base, &intent).unwrap_err();
            assert_eq!(error.code, "PATCH_TARGET");
            assert!(error.message.contains(field), "{}", error.message);
            if field.contains('[') {
                assert!(error.message.contains("target=decision:Result"));
                assert!(error.message.contains("完整数组"));
                assert!(error.message.contains("保留未修改项及顺序"));
            }
        }
    }

    #[test]
    fn change_intent_updates_complete_rule_array_and_preserves_other_rules() {
        let base = nl_dsl::parse_definitions(
            r#"
name: Approval
inputs:
  - name: Age
    typeRef: number
decisions:
  - name: Result
    typeRef: string
    requiredInputs: [Age]
    logic:
      type: decisionTable
      hitPolicy: FIRST
      inputs:
        - expression: Age
          typeRef: number
      outputs:
        - name: result
          typeRef: string
      rules:
        - inputEntries: [">= 18"]
          outputEntries: ['"Approve"']
        - inputEntries: ["-"]
          outputEntries: ['"Reject"']
"#,
        )
        .unwrap();
        let original = serde_json::to_value(&base).unwrap();
        let mut rules = original["decisions"][0]["logic"]["rules"].clone();
        rules[0]["inputEntries"][0] = json!(">= 21");
        let intent = ModelChangeIntent {
            base_revision: 1,
            base_hash: String::new(),
            changes: vec![crate::models::ModelChangeOperation {
                target: "decision:Result".into(),
                action: "update".into(),
                field: Some("logic.rules".into()),
                value: Some(rules.clone()),
                reason: "提高门槛，保留其他规则".into(),
            }],
        };
        let updated = apply_change_intent(&base, &intent).unwrap();
        let updated_document = serde_json::to_value(&updated).unwrap();
        assert_eq!(updated_document["decisions"][0]["logic"]["rules"], rules);
        assert_eq!(
            updated_document["decisions"][0]["logic"]["rules"][1],
            original["decisions"][0]["logic"]["rules"][1]
        );
        assert_eq!(updated_document["inputs"], original["inputs"]);
        assert_eq!(serde_json::to_value(&base).unwrap(), original);
        nl_dsl::validate(&updated).unwrap();
        canonical_dmn_xml(&updated).expect("whole-array update should compile to DMN");
    }

    #[test]
    fn project_validation_rejects_invalid_name_and_risk() {
        let mut payload = CreateDecisionProject {
            name: " ".to_string(),
            description: String::new(),
            risk_level: "medium".to_string(),
        };
        assert!(validate_project(&payload).is_err());
        payload.name = "Loan Approval".to_string();
        payload.risk_level = "critical".to_string();
        assert!(validate_project(&payload).is_err());
        payload.risk_level = "high".to_string();
        assert!(validate_project(&payload).is_ok());
    }

    #[test]
    fn version_validation_requires_both_artifacts() {
        let payload = CreateDecisionVersion {
            dsl_yaml: "name: Example".to_string(),
            dmn_xml: String::new(),
            change_summary: None,
            decision_brief: None,
            decision_plan: None,
            candidate_id: None,
            knowledge_citations: Vec::new(),
            change_confirmed: true,
        };
        assert!(validate_version(&payload).is_err());
    }

    #[test]
    fn version_validation_requires_explicit_change_confirmation() {
        let payload = CreateDecisionVersion {
            dsl_yaml: "name: Example\ninputs: []\ndecisions: []\n".to_string(),
            dmn_xml: "<definitions />".to_string(),
            change_summary: None,
            decision_brief: None,
            decision_plan: None,
            candidate_id: None,
            knowledge_citations: Vec::new(),
            change_confirmed: false,
        };
        let error = validate_version(&payload).unwrap_err();
        assert!(error.message.contains("确认版本差异"));
    }

    #[test]
    fn canonical_dmn_is_regenerated_from_updated_boolean_input_dsl() {
        let definitions = nl_dsl::parse_definitions(
            r#"
name: Deployment
namespace: https://example.com/deployment
inputs:
  - name: IsProductionEnvironment
    typeRef: boolean
decisions:
  - name: DeploymentMode
    typeRef: string
    requiredInputs: [IsProductionEnvironment]
    logic:
      type: decisionTable
      inputs:
        - expression: IsProductionEnvironment
          typeRef: boolean
      outputs:
        - name: mode
          typeRef: string
      rules:
        - inputEntries: ["true"]
          outputEntries: ['"production"']
        - inputEntries: ["false"]
          outputEntries: ['"non-production"']
"#,
        )
        .expect("updated DSL should parse");
        nl_dsl::validate(&definitions).expect("updated DSL should validate");

        let xml = canonical_dmn_xml(&definitions)
            .expect("canonical XML should be generated and compiled from DSL");
        assert!(xml.contains("name=\"IsProductionEnvironment\" typeRef=\"feel:boolean\""));

        let model = crate::dmn::parse_dmn(&xml).expect("canonical XML should parse");
        let inputs = json_inputs_to_feel(&model, &json!({ "IsProductionEnvironment": true }))
            .expect("boolean JSON input should convert to FEEL boolean");
        let result = crate::dmn::DmnExecutor::new(&model).evaluate_all(inputs);
        assert_eq!(result.status, crate::dmn::DmnResultStatus::Succeeded);
    }

    #[test]
    fn canonical_dmn_rejects_unknown_bkm_invocation_binding() {
        let mut definitions = nl_dsl::parse_definitions(
            r#"
name: CostModel
inputs:
  - name: EstimatedMonthlyCost
    typeRef: number
bkms:
  - name: BKM_CalculateCostRatio
    typeRef: number
    parameters:
      - name: EstimatedMonthlyCost
        typeRef: number
    logic:
      type: literalExpression
      expression: EstimatedMonthlyCost / 100
decisions:
  - name: CostRatio
    typeRef: number
    requiredInputs: [EstimatedMonthlyCost]
    requiredKnowledge: [BKM_CalculateCostRatio]
    logic:
      type: invocation
      bkm: BKM_CalculateCostRatio
      bindings:
        - parameter: estimatedMonthlyCost
          expression: EstimatedMonthlyCost
"#,
        )
        .expect("invalid invocation DSL should still parse");

        let error = canonical_dmn_xml(&definitions)
            .expect_err("unknown BKM invocation binding must block submission");
        assert!(
            error
                .message
                .contains("unknown invocation binding 'estimatedMonthlyCost'")
        );
        assert_eq!(
            normalize_case_insensitive_invocation_bindings(&mut definitions),
            1
        );
        let nl_dsl::BoxedExpression::Invocation { bindings, .. } = &definitions.decisions[0].logic
        else {
            panic!("CostRatio should remain an invocation");
        };
        assert_eq!(bindings[0].parameter, "EstimatedMonthlyCost");
        canonical_dmn_xml(&definitions)
            .expect("unambiguous casing correction should compile successfully");
    }

    #[test]
    fn publish_validation_requires_explicit_confirmation() {
        let error = validate_publish(&PublishDecisionVersion {
            publish_confirmed: false,
        })
        .unwrap_err();
        assert!(error.message.contains("确认验收报告"));
        assert!(
            validate_publish(&PublishDecisionVersion {
                publish_confirmed: true,
            })
            .is_ok()
        );
    }

    #[test]
    fn rollback_validation_requires_confirmation_and_reason() {
        let mut payload = RollbackDecisionVersion {
            reason: "恢复已验证的稳定规则".into(),
            rollback_confirmed: false,
        };
        assert!(validate_rollback(&payload).is_err());
        payload.rollback_confirmed = true;
        assert!(validate_rollback(&payload).is_ok());
        payload.reason = " ".into();
        assert!(validate_rollback(&payload).is_err());
    }

    #[test]
    fn model_library_requires_explicit_instantiation_confirmation() {
        let mut payload = InstantiateModelLibraryItem {
            project_name: "贷款审批复用".into(),
            description: String::new(),
            instantiate_confirmed: false,
        };
        assert!(validate_instantiation(&payload).is_err());
        payload.instantiate_confirmed = true;
        assert!(validate_instantiation(&payload).is_ok());
    }

    #[test]
    fn run_outcome_validation_requires_governed_business_feedback() {
        let mut payload = SaveDecisionRunOutcome {
            business_reference: String::new(),
            actual_outcome: json!({"approved": true}),
            assessment: "confirmed".into(),
            note: String::new(),
            occurred_at: None,
        };
        assert!(validate_run_outcome(&payload).is_err());

        payload.business_reference = "APPLICATION-2026-001".into();
        assert!(validate_run_outcome(&payload).is_ok());

        payload.actual_outcome = json!("approved");
        assert!(validate_run_outcome(&payload).is_err());
        payload.actual_outcome = json!({"approved": true});
        payload.assessment = "unknown".into();
        assert!(validate_run_outcome(&payload).is_err());
        payload.assessment = "mismatch".into();
        payload.occurred_at = Some(chrono::Utc::now() + chrono::Duration::hours(1));
        assert!(validate_run_outcome(&payload).is_err());
    }

    #[test]
    fn public_outcome_validation_bounds_payload_and_requires_valid_idempotency_keys() {
        let mut payload = PublicDecisionRunOutcome {
            business_reference: "APPLICATION-1".into(),
            actual_outcome: json!({"approved": true}),
            assessment: "confirmed".into(),
            note: String::new(),
            occurred_at: None,
        };
        assert!(validate_public_run_outcome(&payload).is_ok());
        assert!(validate_idempotency_key("batch-1:row_2.v3").is_ok());
        assert!(validate_idempotency_key(" ").is_err());
        assert!(validate_idempotency_key("bad key").is_err());

        payload.actual_outcome = json!("approved");
        assert!(validate_public_run_outcome(&payload).is_err());
    }

    #[test]
    fn public_outcome_reconciliation_bounds_and_deduplicates_run_ids() {
        let valid = PublicDecisionRunOutcomeReconciliationRequest {
            service_id: "decision-service".into(),
            run_ids: vec![10, 11, 12],
        };
        assert!(validate_outcome_reconciliation_request(&valid).is_ok());

        let mut invalid = valid.clone();
        invalid.run_ids = vec![];
        assert!(validate_outcome_reconciliation_request(&invalid).is_err());

        let mut invalid = valid.clone();
        invalid.run_ids = vec![10, 10];
        assert!(validate_outcome_reconciliation_request(&invalid).is_err());

        let mut invalid = valid.clone();
        invalid.run_ids = vec![0];
        assert!(validate_outcome_reconciliation_request(&invalid).is_err());

        let mut invalid = valid;
        invalid.run_ids = (1..=101).collect();
        assert!(validate_outcome_reconciliation_request(&invalid).is_err());
    }

    #[test]
    fn public_outcome_digest_is_stable_for_equivalent_payloads() {
        let payload = PublicDecisionRunOutcome {
            business_reference: " ref-1 ".into(),
            actual_outcome: json!({"approved": true, "score": 10}),
            assessment: "confirmed".into(),
            note: " checked ".into(),
            occurred_at: None,
        };
        let mut normalized = payload.clone();
        normalized.business_reference = "ref-1".into();
        normalized.note = "checked".into();
        assert_eq!(
            public_outcome_digest(&payload).unwrap(),
            public_outcome_digest(&normalized).unwrap()
        );
    }

    #[test]
    fn deviation_attribution_returns_low_confidence_rule_candidates_only() {
        let trace = json!({"decisions":[{
            "decisionId":"decision-risk",
            "decisionName":"RiskLevel",
            "result":"low",
            "matchedRuleIds":["rule-low"]
        }]});
        let (status, rule, confidence, evidence) =
            deviation_candidate(&trace, &json!({"RiskLevel":"high"}));
        assert_eq!(status, "candidate");
        assert_eq!(rule.as_deref(), Some("rule-low"));
        assert_eq!(confidence, 0.35);
        assert_eq!(evidence["candidateRules"][0]["ruleId"], "rule-low");

        let (status, rule, confidence, _) =
            deviation_candidate(&trace, &json!({"Unrelated":"value"}));
        assert_eq!(status, "insufficient_evidence");
        assert!(rule.is_none());
        assert_eq!(confidence, 0.0);
    }

    #[test]
    fn generated_case_values_support_common_unary_tests() {
        assert_eq!(representative_unary_value(">= 10"), Some(json!(10.0)));
        assert_eq!(representative_unary_value("> 10"), Some(json!(10.00001)));
        assert_eq!(representative_unary_value("[10..20]"), Some(json!(15.0)));
        assert_eq!(representative_unary_value("in [10..20]"), Some(json!(15.0)));
        assert_eq!(representative_unary_value("not(< 40)"), Some(json!(40.0)));
        assert_eq!(
            representative_unary_value("not(< 40) and < 80"),
            Some(json!(79.99992))
        );
        assert_eq!(
            representative_unary_value("not(>= 40)"),
            Some(json!(39.99996))
        );
        assert_eq!(representative_unary_value("!= 10"), Some(json!(10.00001)));
        assert_eq!(
            representative_unary_value("in (\"A\", \"B\")"),
            Some(json!("A"))
        );
        assert_eq!(
            representative_unary_value("[\"A\", \"B\"]"),
            Some(json!("A"))
        );
        assert_eq!(
            representative_unary_value("date(\"2026-01-01\")"),
            Some(json!("2026-01-01"))
        );
        assert_eq!(representative_unary_value("true"), Some(json!(true)));
        assert_eq!(representative_unary_value("= \"A\""), Some(json!("A")));
        assert_eq!(
            representative_unary_value("not(= \"A\")"),
            Some(json!("sample"))
        );
        assert_eq!(representative_unary_value("not(1)"), None);
    }

    #[test]
    fn generated_cases_cover_string_equality_and_negation() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="kind" name="kind"><variable name="kind" typeRef="feel:string"/></inputData>
                <decision id="route" name="route">
                    <informationRequirement><requiredInput href="#kind"/></informationRequirement>
                    <decisionTable hitPolicy="FIRST">
                        <input><inputExpression><text>kind</text></inputExpression></input>
                        <output name="route"/>
                        <rule id="known"><inputEntry><text>"A"</text></inputEntry><outputEntry><text>"known"</text></outputEntry></rule>
                        <rule id="other"><inputEntry><text>not(= "A")</text></inputEntry><outputEntry><text>"other"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );
        let model = crate::dmn::parse_dmn(&xml).expect("parse string model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate cases");
        assert_eq!(generated.candidates.len(), 2, "{:?}", generated.notes);
        assert_eq!(generated.skipped_rule_count, 0);
    }

    #[test]
    fn static_tester_suggestions_are_verified_conflict_and_coverage_witnesses() {
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
        let model = crate::dmn::parse_dmn(&xml).expect("parse conflicting model");
        let findings =
            crate::version_verification::analyze_static_rules(&xml).expect("analyze static rules");
        let suggestions = suggested_inputs_for_static_findings(&model, 52, &findings);
        let conflict = findings
            .iter()
            .find(|finding| finding.code == "UNIQUE_RULE_CONFLICT")
            .expect("unique overlap finding");
        let conflict_input = suggestions
            .get(&crate::workflow_api::static_analysis_source_reference(
                52, conflict,
            ))
            .expect("conflict witness input");
        let result = crate::dmn::DmnExecutor::new(&model)
            .evaluate_all(json_inputs_to_feel(&model, conflict_input).unwrap());
        assert_eq!(
            result.first_error().map(|error| error.code),
            Some("DMN-EVAL-HIT-POLICY")
        );

        let coverage = findings
            .iter()
            .find(|finding| finding.code == "MISSING_COVERAGE")
            .expect("coverage finding");
        let coverage_input = suggestions
            .get(&crate::workflow_api::static_analysis_source_reference(
                52, coverage,
            ))
            .expect("unmatched coverage witness input");
        let result = crate::dmn::DmnExecutor::new(&model)
            .evaluate_all(json_inputs_to_feel(&model, coverage_input).unwrap());
        assert!(
            result
                .decision_by_id("result")
                .unwrap()
                .matched_rule_ids
                .is_empty()
        );
    }

    #[test]
    fn generated_cases_preserve_boolean_rule_inputs() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="production" name="Production"><variable name="IsProductionEnvironment" typeRef="feel:boolean"/></inputData>
                <decision id="mode" name="Mode">
                    <informationRequirement><requiredInput href="#production"/></informationRequirement>
                    <decisionTable hitPolicy="FIRST">
                        <input><inputExpression typeRef="feel:boolean"><text>IsProductionEnvironment</text></inputExpression></input>
                        <output name="mode" typeRef="feel:string"/>
                        <rule id="production"><inputEntry><text>true</text></inputEntry><outputEntry><text>"production"</text></outputEntry></rule>
                        <rule id="non-production"><inputEntry><text>false</text></inputEntry><outputEntry><text>"non-production"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );
        let model = crate::dmn::parse_dmn(&xml).expect("parse boolean model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate cases");

        assert_eq!(generated.skipped_rule_count, 0, "{:?}", generated.notes);
        assert_eq!(generated.candidates.len(), 2, "{:?}", generated.notes);
        assert!(
            generated
                .candidates
                .iter()
                .any(|case| case.input_json["IsProductionEnvironment"] == json!(true))
        );
        assert!(
            generated
                .candidates
                .iter()
                .any(|case| case.input_json["IsProductionEnvironment"] == json!(false))
        );
    }

    #[test]
    fn generated_cases_use_column_constraints_for_wildcard_rules() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="age" name="age"><variable name="age" typeRef="feel:number"/></inputData>
                <decision id="risk" name="risk">
                    <informationRequirement><requiredInput href="#age"/></informationRequirement>
                    <decisionTable>
                        <input><inputExpression><text>age</text></inputExpression><inputValues><text>[18..65]</text></inputValues></input>
                        <output name="risk"/><rule id="adult"><inputEntry><text>-</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );
        let model = crate::dmn::parse_dmn(&xml).expect("parse wildcard model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate case");
        assert_eq!(generated.total_rule_count, 1);
        assert_eq!(generated.skipped_rule_count, 0);
        assert_eq!(generated.candidates.len(), 1);
        assert_eq!(generated.candidates[0].input_json["age"], json!(41.5));
    }

    #[test]
    fn generated_cases_use_declared_type_for_unconstrained_wildcards() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="age" name="age"><variable name="age" typeRef="feel:number"/></inputData>
                <decision id="risk" name="risk">
                    <informationRequirement><requiredInput href="#age"/></informationRequirement>
                    <decisionTable><input><inputExpression><text>age</text></inputExpression></input>
                        <output name="risk"/><rule id="any-age"><inputEntry><text>-</text></inputEntry><outputEntry><text>"known"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );
        let model = crate::dmn::parse_dmn(&xml).expect("parse wildcard model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate case");
        assert_eq!(generated.candidates.len(), 1);
        assert_eq!(generated.skipped_rule_count, 0);
        assert_eq!(generated.candidates[0].input_json["age"], json!(1));
    }

    #[test]
    fn generated_cases_use_input_type_when_wildcard_column_defaults_to_any() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="device" name="Device"><variable name="usesNewDevice" typeRef="feel:boolean"/></inputData>
                <decision id="result" name="Result">
                    <informationRequirement><requiredInput href="#device"/></informationRequirement>
                    <decisionTable hitPolicy="FIRST">
                        <input><inputExpression typeRef="feel:Any"><text>usesNewDevice</text></inputExpression></input>
                        <output name="result" typeRef="feel:string"/>
                        <rule id="any-device"><inputEntry><text>-</text></inputEntry><outputEntry><text>"ok"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );
        let model = crate::dmn::parse_dmn(&xml).expect("parse wildcard boolean model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate case");
        assert_eq!(generated.skipped_rule_count, 0, "{:?}", generated.notes);
        assert_eq!(generated.candidates.len(), 1, "{:?}", generated.notes);
        assert_eq!(
            generated.candidates[0].input_json["usesNewDevice"],
            json!(false)
        );
    }

    #[test]
    fn generated_cases_ignore_unrelated_decision_failures() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="age" name="age"><variable name="age" typeRef="feel:number"/></inputData>
                <decision id="target" name="target">
                    <informationRequirement><requiredInput href="#age"/></informationRequirement>
                    <decisionTable><input><inputExpression><text>age</text></inputExpression></input>
                        <output name="target"/><rule id="adult"><inputEntry><text>&gt;= 18</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
                <decision id="broken" name="broken"><literalExpression><text>substring("x", 0)</text></literalExpression></decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );
        let model = crate::dmn::parse_dmn(&xml).expect("parse model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate cases");
        assert_eq!(generated.total_rule_count, 1);
        assert_eq!(generated.skipped_rule_count, 0);
        assert_eq!(generated.candidates.len(), 1);
    }

    #[test]
    fn generated_cases_keep_a_baseline_for_dependency_tables() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="age" name="age"><variable name="age" typeRef="number"/></inputData>
                <decision id="eligible" name="eligible">
                    <informationRequirement><requiredInput href="#age"/></informationRequirement>
                    <literalExpression><text>age &gt;= 18</text></literalExpression>
                </decision>
                <decision id="route" name="route">
                    <informationRequirement><requiredDecision href="#eligible"/></informationRequirement>
                    <decisionTable>
                        <input><inputExpression><text>eligible</text></inputExpression></input>
                        <output name="route"/>
                        <rule id="adult"><inputEntry><text>true</text></inputEntry><outputEntry><text>"adult"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );
        let model = crate::dmn::parse_dmn(&xml).expect("parse dependency model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate cases");
        assert!(
            generated
                .candidates
                .iter()
                .any(|candidate| candidate.fallback && candidate.rule_key == "route:adult")
        );
    }

    #[test]
    fn generated_cases_avoid_zero_denominators_in_dependent_decisions() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="cost" name="TotalCost"><variable name="TotalCost" typeRef="number"/></inputData>
                <inputData id="amount" name="TotalAmount"><variable name="TotalAmount" typeRef="number"/></inputData>
                <decision id="ratio" name="CostRatio">
                    <informationRequirement><requiredInput href="#cost"/></informationRequirement>
                    <informationRequirement><requiredInput href="#amount"/></informationRequirement>
                    <literalExpression><text>TotalCost / TotalAmount</text></literalExpression>
                </decision>
                <decision id="band" name="RatioBand">
                    <informationRequirement><requiredDecision href="#ratio"/></informationRequirement>
                    <decisionTable hitPolicy="FIRST">
                        <input><inputExpression typeRef="number"><text>CostRatio</text></inputExpression></input>
                        <output name="RatioBand" typeRef="string"/>
                        <rule id="safe"><inputEntry><text>&gt;= 1</text></inputEntry><outputEntry><text>"safe"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );
        let model = crate::dmn::parse_dmn(&xml).expect("parse ratio model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate ratio case");
        let candidate = generated
            .candidates
            .iter()
            .find(|candidate| candidate.rule_key == "band:safe")
            .expect("dependent rule should have a candidate");
        assert_eq!(candidate.input_json["TotalAmount"], json!(1));
        let inputs = json_inputs_to_feel(&model, &candidate.input_json).expect("typed candidate");
        let result = crate::dmn::DmnExecutor::new(&model).evaluate_all(inputs);
        assert!(
            !result.has_errors(),
            "unexpected candidate errors: {:?}",
            result.messages
        );
    }

    #[test]
    fn generated_business_cases_cover_reachable_builtin_rules() {
        for template in builtin_templates() {
            let snapshot = builtin_snapshot(&template).expect("built-in template snapshot");
            let model = crate::dmn::parse_dmn(&snapshot.dmn_xml).expect("parse generated DMN");
            let generated = generate_business_candidates(&model, 1, 1)
                .expect("generate deterministic business cases");
            assert_eq!(
                generated.candidates.len(),
                generated.total_rule_count,
                "{} should cover every reachable rule",
                template.name
            );
            assert_eq!(generated.skipped_rule_count, 0);
            assert!(generated.candidates.iter().all(|candidate| {
                candidate.input_json.is_object()
                    && candidate
                        .expected_output_json
                        .as_object()
                        .is_some_and(|expected| !expected.is_empty())
            }));
        }
    }

    #[test]
    fn generated_cases_cover_dependent_loan_rules() {
        let model = crate::dmn::parse_dmn(include_str!("../resource/loan_0812.dmn"))
            .expect("parse loan model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate loan cases");
        assert!(generated.candidates.len() >= 8, "{:?}", generated.notes);
        assert!(
            generated
                .candidates
                .iter()
                .any(|candidate| candidate.rule_key == "decision_7:rule_48")
        );
        assert!(
            generated
                .candidates
                .iter()
                .any(|candidate| candidate.rule_key == "decision_7:rule_63")
        );
    }

    #[test]
    fn generated_cases_cover_ecommerce_dependency_rules() {
        let xml = format!(
            r##"<definitions xmlns="{}">
                <inputData id="userAge" name="UserAge"><variable name="UserAge" typeRef="number"/></inputData>
                <inputData id="accountAge" name="AccountAgeDays"><variable name="AccountAgeDays" typeRef="number"/></inputData>
                <inputData id="frequency" name="OrderFrequency"><variable name="OrderFrequency" typeRef="number"/></inputData>
                <inputData id="average" name="AvgPurchaseAmount"><variable name="AvgPurchaseAmount" typeRef="number"/></inputData>
                <inputData id="amount" name="OrderAmount"><variable name="OrderAmount" typeRef="number"/></inputData>
                <inputData id="shipping" name="ShippingCost"><variable name="ShippingCost" typeRef="number"/></inputData>
                <inputData id="device" name="IsNewDevice"><variable name="IsNewDevice" typeRef="boolean"/></inputData>
                <decision id="shippingRatio" name="ShippingRatio">
                    <informationRequirement><requiredInput href="#shipping"/></informationRequirement>
                    <informationRequirement><requiredInput href="#amount"/></informationRequirement>
                    <literalExpression><text>ShippingCost / OrderAmount</text></literalExpression>
                </decision>
                <decision id="risk" name="OrderRiskGrade">
                    <informationRequirement><requiredInput href="#frequency"/></informationRequirement>
                    <informationRequirement><requiredInput href="#accountAge"/></informationRequirement>
                    <informationRequirement><requiredDecision href="#shippingRatio"/></informationRequirement>
                    <decisionTable hitPolicy="FIRST">
                        <input><inputExpression typeRef="number"><text>OrderFrequency</text></inputExpression></input>
                        <input><inputExpression typeRef="number"><text>AccountAgeDays</text></inputExpression></input>
                        <input><inputExpression typeRef="number"><text>ShippingRatio</text></inputExpression></input>
                        <output name="OrderRiskGrade" typeRef="string"/>
                        <rule id="high-frequency"><inputEntry><text>&gt; 20</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"High"</text></outputEntry></rule>
                        <rule id="high-shipping"><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>&gt; 0.3</text></inputEntry><outputEntry><text>"High"</text></outputEntry></rule>
                        <rule id="medium-frequency"><inputEntry><text>&gt; 10</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"Medium"</text></outputEntry></rule>
                        <rule id="medium-account"><inputEntry><text>-</text></inputEntry><inputEntry><text>&lt; 7</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"Medium"</text></outputEntry></rule>
                        <rule id="low"><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"Low"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
                <decision id="eligibility" name="OrderEligibility">
                    <informationRequirement><requiredInput href="#userAge"/></informationRequirement>
                    <informationRequirement><requiredInput href="#accountAge"/></informationRequirement>
                    <informationRequirement><requiredInput href="#average"/></informationRequirement>
                    <informationRequirement><requiredInput href="#amount"/></informationRequirement>
                    <decisionTable hitPolicy="FIRST">
                        <input><inputExpression typeRef="number"><text>UserAge</text></inputExpression></input>
                        <input><inputExpression typeRef="number"><text>AccountAgeDays</text></inputExpression></input>
                        <input><inputExpression typeRef="number"><text>OrderAmount</text></inputExpression></input>
                        <output name="OrderEligibility" typeRef="string"/>
                        <rule id="underage"><inputEntry><text>&lt; 14</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"Reject"</text></outputEntry></rule>
                        <rule id="new-account"><inputEntry><text>-</text></inputEntry><inputEntry><text>&lt; 1</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"Reject"</text></outputEntry></rule>
                        <rule id="large-order"><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>&gt; AvgPurchaseAmount * 5</text></inputEntry><outputEntry><text>"ManualReview"</text></outputEntry></rule>
                        <rule id="eligible"><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"Eligible"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
                <decision id="final" name="FinalOrderDecision">
                    <informationRequirement><requiredDecision href="#risk"/></informationRequirement>
                    <informationRequirement><requiredDecision href="#eligibility"/></informationRequirement>
                    <informationRequirement><requiredInput href="#device"/></informationRequirement>
                    <decisionTable hitPolicy="FIRST">
                        <input><inputExpression typeRef="string"><text>OrderEligibility</text></inputExpression></input>
                        <input><inputExpression typeRef="string"><text>OrderRiskGrade</text></inputExpression></input>
                        <input><inputExpression typeRef="boolean"><text>IsNewDevice</text></inputExpression></input>
                        <output name="FinalOrderDecision" typeRef="string"/>
                        <rule id="reject-eligibility"><inputEntry><text>"Reject"</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"Reject"</text></outputEntry></rule>
                        <rule id="reject-high-risk"><inputEntry><text>-</text></inputEntry><inputEntry><text>"High"</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"Reject"</text></outputEntry></rule>
                        <rule id="review-large-order"><inputEntry><text>"ManualReview"</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"ManualReview"</text></outputEntry></rule>
                        <rule id="review-new-device"><inputEntry><text>-</text></inputEntry><inputEntry><text>"Medium"</text></inputEntry><inputEntry><text>true</text></inputEntry><outputEntry><text>"ManualReview"</text></outputEntry></rule>
                        <rule id="approve"><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><inputEntry><text>-</text></inputEntry><outputEntry><text>"Approve"</text></outputEntry></rule>
                    </decisionTable>
                </decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        );
        let model = crate::dmn::parse_dmn(&xml).expect("parse ecommerce model");
        let generated = generate_business_candidates(&model, 1, 1).expect("generate cases");
        assert_eq!(generated.total_rule_count, 14, "{:?}", generated.notes);
        assert_eq!(generated.skipped_rule_count, 0, "{:?}", generated.notes);
        assert_eq!(generated.candidates.len(), 14, "{:?}", generated.notes);
        for rule_key in [
            "risk:high-shipping",
            "eligibility:large-order",
            "final:reject-eligibility",
            "final:review-new-device",
            "final:approve",
        ] {
            assert!(
                generated
                    .candidates
                    .iter()
                    .any(|candidate| candidate.rule_key == rule_key),
                "missing {rule_key}"
            );
        }
    }

    #[test]
    fn builtin_templates_parse_generate_compile_and_pass_samples() {
        for template in builtin_templates() {
            let snapshot = builtin_snapshot(&template).expect("built-in template snapshot");
            let model = crate::dmn::parse_dmn(&snapshot.dmn_xml).expect("parse generated DMN");
            crate::dmn::compile_dmn(&model).expect("compile generated DMN");
            assert!(!snapshot.test_cases.is_empty());
            for (index, sample) in snapshot.test_cases.into_iter().enumerate() {
                let now = chrono::Utc::now();
                let test_case = DecisionTestCase {
                    id: index as i64 + 1,
                    version_id: 1,
                    name: sample.name,
                    category: sample.category,
                    input_json: sample.input_json,
                    expected_output_json: sample.expected_output_json,
                    actual_output_json: None,
                    execution_trace_json: None,
                    status: "pending".into(),
                    source: sample.source,
                    approved_by: None,
                    approved_at: None,
                    created_at: now,
                    updated_at: now,
                };
                let (status, actual, _) = execute_test_case(&model, 1, &test_case);
                assert_eq!(status, "passed", "{} produced {actual}", test_case.name);
            }
        }
    }

    #[test]
    fn tester_proposal_case_executes_against_dmn_and_produces_a_trace() {
        let template = builtin_templates().into_iter().next().expect("template");
        let snapshot = builtin_snapshot(&template).expect("built-in template snapshot");
        let model = crate::dmn::parse_dmn(&snapshot.dmn_xml).expect("parse generated DMN");
        let sample = snapshot.test_cases.into_iter().next().expect("sample case");
        let now = chrono::Utc::now();
        let test_case = DecisionTestCase {
            id: 77,
            version_id: 1,
            name: sample.name,
            category: "business".into(),
            input_json: sample.input_json,
            expected_output_json: sample.expected_output_json,
            actual_output_json: None,
            execution_trace_json: None,
            status: "pending".into(),
            source: "tester_proposal".into(),
            approved_by: None,
            approved_at: None,
            created_at: now,
            updated_at: now,
        };

        let (status, actual, trace) = execute_test_case(&model, 1, &test_case);

        assert_eq!(status, "passed");
        assert!(actual.is_object());
        assert_eq!(trace["output"], actual);
        assert_eq!(test_case.source, "tester_proposal");
    }

    #[test]
    fn tester_report_records_execution_status_only_for_linked_cases() {
        let mut report = TesterReport {
            schema_version: 1,
            proposals: vec![
                crate::workflow::SemanticTestProposal {
                    id: "linked".into(),
                    test_case_id: Some(10),
                    ..Default::default()
                },
                crate::workflow::SemanticTestProposal {
                    id: "unlinked".into(),
                    ..Default::default()
                },
            ],
        };
        let statuses = HashMap::from([(10, "passed".to_string()), (11, "failed".to_string())]);

        apply_tester_execution_statuses(&mut report, &statuses);

        assert_eq!(
            report.proposals[0].execution_status.as_deref(),
            Some("passed")
        );
        assert_eq!(report.proposals[1].execution_status, None);
    }

    #[test]
    fn tester_judge_requires_two_matching_confident_votes() {
        use crate::workflow::TesterJudgeVote;

        let vote = |assessment: &str| TesterJudgeVote {
            assessment: assessment.into(),
            reason: "有明确依据".into(),
        };
        let review = tester_judge_review_from_votes(
            vec![vote("plausible"), vote("plausible")],
            "input".into(),
            "expected".into(),
        );
        assert_eq!(review.status, "no_obvious_issue");

        let review = tester_judge_review_from_votes(
            vec![vote("suspicious"), vote("suspicious")],
            "input".into(),
            "expected".into(),
        );
        assert_eq!(review.status, "suspicious");

        for votes in [
            vec![vote("plausible"), vote("suspicious")],
            vec![vote("insufficient_evidence"), vote("insufficient_evidence")],
        ] {
            let review = tester_judge_review_from_votes(votes, "input".into(), "expected".into());
            assert_eq!(review.status, "inconclusive");
        }
    }

    #[test]
    fn business_acceptance_gate_requires_passed_and_approved_cases() {
        assert_eq!(business_acceptance_gate(0, 0, 0).status, "failed");
        assert_eq!(business_acceptance_gate(2, 1, 1).status, "failed");
        assert_eq!(business_acceptance_gate(2, 2, 1).status, "failed");
        assert_eq!(business_acceptance_gate(2, 2, 2).status, "passed");
    }

    #[test]
    fn expected_output_can_assert_a_subset_of_actual_decisions() {
        let actual = json!({
            "Eligibility": "approved",
            "RiskGrade": "low",
            "Details": { "score": 88, "source": "model" }
        });
        assert!(json_contains(
            &actual,
            &json!({ "Eligibility": "approved", "Details": { "score": 88 } })
        ));
        assert!(!json_contains(
            &actual,
            &json!({ "Eligibility": "rejected" })
        ));
    }

    #[test]
    fn test_case_validation_requires_object_contracts() {
        let valid = CreateDecisionTestCase {
            name: "批准路径".into(),
            input_json: json!({ "score": 88 }),
            expected_output_json: json!({ "Eligibility": "approved" }),
        };
        assert!(validate_test_case(&valid).is_ok());
        let invalid = CreateDecisionTestCase {
            name: String::new(),
            input_json: json!([]),
            expected_output_json: json!("approved"),
        };
        assert!(validate_test_case(&invalid).is_err());
    }

    #[test]
    fn published_service_contract_exposes_only_declared_interface() {
        let model = crate::dmn::parse_dmn(&format!(
            r##"<definitions xmlns="{}" name="ServiceModel">
            <inputData id="amount" name="Amount"><variable name="amount" typeRef="number"/></inputData>
            <decision id="internal" name="Internal"><variable name="internal" typeRef="number"/><informationRequirement><requiredInput href="#amount"/></informationRequirement><literalExpression><text>amount * 2</text></literalExpression></decision>
            <decision id="output" name="Result"><variable name="result" typeRef="number"/><informationRequirement><requiredDecision href="#internal"/></informationRequirement><literalExpression><text>internal + 1</text></literalExpression></decision>
            <decisionService id="pricing" name="Pricing"><outputDecision href="#output"/><encapsulatedDecision href="#internal"/><inputData href="#amount"/></decisionService>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        ))
        .unwrap();
        let contracts = published_service_contracts(&model, 3, 8, 2);
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].id, "pricing");
        assert_eq!(contracts[0].inputs[0].name, "amount");
        assert_eq!(contracts[0].inputs[0].type_ref, "number");
        assert_eq!(contracts[0].outputs[0].name, "Result");
        assert!(
            contracts[0]
                .outputs
                .iter()
                .all(|field| field.name != "internal")
        );
    }

    #[test]
    fn published_service_contract_creates_default_service_for_terminal_decision() {
        let mut model = crate::dmn::parse_dmn(&format!(
            r##"<definitions xmlns="{}" name="DecisionModel">
            <inputData id="score" name="Score"><variable name="score" typeRef="number"/></inputData>
            <decision id="result" name="Result"><variable name="result" typeRef="string"/><informationRequirement><requiredInput href="#score"/></informationRequirement><literalExpression><text>if score >= 60 then "Pass" else "Fail"</text></literalExpression></decision>
            </definitions>"##,
            crate::dmn::DMN_NAMESPACE
        ))
        .unwrap();
        ensure_default_decision_services(&mut model);
        let contracts = published_service_contracts(&model, 3, 8, 2);
        assert_eq!(contracts.len(), 1);
        assert!(contracts[0].id.starts_with("__default_service_"));
        assert_eq!(contracts[0].name, "Result（默认服务）");
        assert_eq!(contracts[0].inputs[0].name, "score");
        assert_eq!(contracts[0].outputs[0].name, "Result");
    }

    #[test]
    fn service_api_key_is_argon2_hashed_and_prefix_is_non_secret() {
        let secret = format!("dmn_live_{}", Uuid::new_v4().simple());
        let hash = hash_service_api_key(&secret).expect("hash should succeed");
        assert_ne!(hash, secret);
        assert!(verify_service_api_key(&hash, &secret));
        assert!(!verify_service_api_key(&hash, "dmn_live_wrong"));
        let prefix = secret.chars().take(20).collect::<String>();
        assert_eq!(prefix.len(), 20);
        assert!(!hash.contains(&prefix));
    }

    #[test]
    fn historical_regression_gate_requires_complete_passing_results() {
        assert_eq!(
            historical_regression_gate_error(0, 0),
            Some("发布前必须先运行至少一条历史回归用例")
        );
        assert_eq!(
            historical_regression_gate_error(3, 2),
            Some("历史回归测试存在失败、执行错误或未完成用例，不能发布")
        );
        assert_eq!(historical_regression_gate_error(3, 3), None);
    }

    #[test]
    fn historical_regression_gate_is_on_by_default_and_only_explicitly_disabled() {
        assert!(historical_regression_gate_enabled_for(None));
        assert!(historical_regression_gate_enabled_for(Some("invalid")));
        assert!(!historical_regression_gate_enabled_for(Some("false")));
        assert!(!historical_regression_gate_enabled_for(Some(" OFF ")));
    }
}
