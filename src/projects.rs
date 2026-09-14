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
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::decision_analytics::{analytics_window_start, build_decision_analytics};
use crate::execution_trace::{
    build_decision_service_trace, build_execution_trace, build_rejected_execution_trace,
};
use crate::knowledge::{citations, effective_project_sources};
use crate::models::{
    ApproveDecisionTestCase, AuditEvent, CreateDecisionProject, CreateDecisionServiceApiKey,
    CreateDecisionTestCase, CreateDecisionVersion, CreatedDecisionServiceApiKey,
    DecisionAnalyticsReport, DecisionOptimizationProposal, DecisionProject, DecisionServiceApiKey,
    DecisionServiceField, DecisionServiceRun, DecisionTestCase, DecisionVersion, DecisionWorkspace,
    ExecuteDecisionService, GenerateDecisionTestCasesResult, InstantiateModelLibraryItem,
    KnowledgeCitation, ModelChangeIntent, ModelChangePreview, ModelLibraryItem,
    ModelTemplateTestCase, ProjectMember, PublishDecisionVersion, PublishedDecisionService,
    RollbackDecisionVersion, SaveDecisionRunOutcome, SaveDecisionTemplate, SaveProjectMember,
    VerificationGate, VersionChangeReview, VersionVerificationReport,
};
use crate::nl_dsl::{self, hash::definitions_hash};
use crate::project_access::{self, ProjectPermission};
use crate::types::{AppState, Claims};
use crate::version_review::build_change_review;
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
        (
            self.status,
            Json(json!({ "ok": false, "error": self.message, "code": self.code })),
        )
            .into_response()
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
    if !payload
        .expected_output_json
        .as_object()
        .is_some_and(|expected| !expected.is_empty())
    {
        return Err(ProjectApiError::bad_request(
            "期望输出必须是至少包含一个决策断言的 JSON 对象",
        ));
    }
    Ok(())
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

    let confirmed_workflow = sqlx::query_as::<_, (serde_json::Value, serde_json::Value)>(
        r#"
        SELECT decision_brief_json, decision_plan_json
        FROM decision_workflows
        WHERE project_id = $1 AND status = 'confirmed' AND decision_plan_json IS NOT NULL
        "#,
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取已确认决策方案", error))?;
    let (decision_brief, decision_plan) = confirmed_workflow
        .map(|(brief, plan)| (Some(brief), Some(plan)))
        .unwrap_or((
            payload.decision_brief.clone(),
            payload.decision_plan.clone(),
        ));

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
            if value.starts_with(&prefix) {
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
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(ProjectApiError::patch_target("ChangeIntent 字段路径无效"));
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
                if field.contains('[') {
                    return Err(ProjectApiError::patch_target(
                        "不允许使用数组下标定位字段，请使用元素级变更或重新生成意图",
                    ));
                }
                let root_field = field
                    .split('.')
                    .next()
                    .unwrap_or(field.as_str())
                    .split('[')
                    .next()
                    .unwrap_or(field.as_str());
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
    Ok(definitions)
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
        "number" | "integer" | "long" | "double" | "boolean" | "string" | "any"
        | "date" | "time" | "date and time" | "datetime"
        | "duration" | "daytimeduration" | "yearmonthduration"
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
        .map(|value| value.as_str().parse::<f64>().ok())
        .flatten()
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
            let Ok(regex) = Regex::new(r"(>=|>|<=|<|=|!=|<>)\s*(-?\d+(?:\.\d+)?)") else {
                continue;
            };
            for capture in regex.captures_iter(expression) {
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
            let repeated_input = input_key.as_ref().is_some_and(|input_key| {
                !expanded_external_inputs.insert(input_key.clone())
            });
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

/// Evaluate a candidate input and return diagnostic information about why it failed.
fn evaluate_rule_candidate_with_diagnostic(
    model: &crate::dmn::DmnModel,
    decision_id: &str,
    rule_id: &str,
    input: &Value,
) -> Result<(Value, crate::dmn::DmnResult), String> {
    let inputs = json_inputs_to_feel(model, input)
        .map_err(|e| format!("输入转换失败: {}", e))?;
    let result = crate::dmn::DmnExecutor::new(model).evaluate_all(inputs);

    // Check if any decision failed with dependency issues
    for decision_result in &result.decision_results {
        if decision_result.status == crate::dmn::DecisionStatus::Failed
            || decision_result.status == crate::dmn::DecisionStatus::Skipped {
            // Find error messages for this decision
            let errors: Vec<_> = result.messages.iter()
                .filter(|msg| msg.source.decision_id.as_deref() == Some(&decision_result.decision_id))
                .map(|msg| format!("{}: {}", msg.code, msg.message))
                .collect();
            if !errors.is_empty() {
                return Err(format!("决策 '{}' 失败: {}", decision_result.decision_name, errors.join("; ")));
            }
        }
    }

    let target = result.decision_by_id(decision_id)
        .ok_or_else(|| format!("找不到目标决策 {}", decision_id))?;

    if target.status != crate::dmn::DecisionStatus::Succeeded {
        return Err(format!("目标决策状态: {:?}", target.status));
    }

    if !target.matched_rule_ids.iter().any(|matched| matched == rule_id) {
        return Err(format!("规则未命中，实际命中: {:?}", target.matched_rule_ids));
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
                        model, &decision.id, &rule_id, &input
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
        ORDER BY created_at, id
        "#,
    )
    .bind(version_id)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取待运行验收案例", error))?;
    if cases.is_empty() {
        return Err(ProjectApiError::bad_request("请先添加至少一个业务验收案例"));
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
    for (case_id, (status, actual, trace)) in results {
        sqlx::query(
            r#"
            UPDATE decision_test_cases
            SET actual_output_json = $1, execution_trace_json = $2, status = $3,
                approved_by = NULL, approved_at = NULL, updated_at = NOW()
            WHERE id = $4 AND version_id = $5
            "#,
        )
        .bind(actual)
        .bind(trace)
        .bind(status)
        .bind(case_id)
        .bind(version_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("更新业务验收结果", error))?;
    }
    invalidate_verification(&mut transaction, version_id).await?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("提交业务验收结果", error))?;
    list_test_cases(
        State(state),
        Extension(claims),
        Path((project_id, version_id)),
    )
    .await
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
            String,
        ),
    >(
        r#"
        SELECT version.status, artifact.verification_report_json, version.reviewed_by,
               version.reviewed_at, project.current_draft_version_id, version.created_by
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
    let (status, report_json, reviewed_by, reviewed_at, current_draft_version_id, created_by) = row;
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
        FROM decision_runs
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

async fn load_published_analytics(
    state: &AppState,
    username: &str,
    project_id: i64,
) -> Result<DecisionAnalyticsReport, ProjectApiError> {
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
    let runs = sqlx::query_as::<_, DecisionServiceRun>(
        r#"
        SELECT id, project_id, version_id, service_id, service_name, input_json,
               output_json, execution_trace_json, duration_ms, status, executed_by,
               executed_at, business_reference, actual_outcome_json,
               outcome_assessment, outcome_note, outcome_occurred_at,
               outcome_reported_by, outcome_reported_at
        FROM decision_runs
        WHERE project_id = $1 AND version_id = $2 AND executed_at >= $3
        ORDER BY executed_at, id
        "#,
    )
    .bind(project_id)
    .bind(version_id)
    .bind(window_start)
    .fetch_all(&state.db)
    .await
    .map_err(|error| ProjectApiError::database("读取决策运行分析数据", error))?;
    Ok(build_decision_analytics(
        project_id,
        version_id,
        version_number,
        &model,
        &runs,
        window_start,
        generated_at,
    ))
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
               proposal.resolved_at, proposal.resolution_note, proposal.resulting_version_id
        FROM decision_optimization_proposals proposal
        JOIN decision_versions version ON version.id = proposal.source_version_id
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

async fn generate_optimization_proposals(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<DecisionOptimizationProposal>>, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::Model).await?;
    let report = load_published_analytics(&state, &claims.sub, project_id).await?;
    let snapshot = serde_json::to_value(&report)
        .map_err(|error| ProjectApiError::bad_request(format!("运行分析快照无效：{error}")))?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("生成优化建议", error))?;
    for finding in &report.findings {
        let (title, actions) = optimization_guidance(&finding.code);
        let actions = serde_json::to_value(actions).map_err(|error| {
            ProjectApiError::bad_request(format!("优化建议序列化失败：{error}"))
        })?;
        let evidence = json!({
            "finding": finding,
            "summary": report.summary,
            "windowStart": report.window_start,
            "windowDays": report.window_days,
            "generatedAt": report.generated_at,
        });
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
        .bind(&finding.message)
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
    let source = sqlx::query_as::<
        _,
        (
            i32,
            String,
            Option<Value>,
            Option<Value>,
            String,
            String,
            Value,
        ),
    >(
        r#"
        SELECT version.version_number, version.status, artifact.decision_brief_json,
               artifact.decision_plan_json, artifact.dsl_yaml, artifact.dmn_xml,
               artifact.knowledge_citations_json
        FROM decision_versions version
        JOIN decision_artifacts artifact ON artifact.version_id = version.id
        WHERE version.id = $1 AND version.project_id = $2
        "#,
    )
    .bind(source_version_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("读取优化建议基线版本", error))?
    .ok_or_else(|| ProjectApiError::bad_request("优化建议基线版本不存在"))?;
    let (
        source_number,
        source_status,
        decision_brief,
        decision_plan,
        dsl_yaml,
        dmn_xml,
        knowledge_json,
    ) = source;
    if source_status != "published" {
        return Err(ProjectApiError::bad_request(
            "优化建议只能基于当前已发布版本确认",
        ));
    }
    let definitions = nl_dsl::parse_definitions(&dsl_yaml)
        .map_err(|error| ProjectApiError::bad_request(format!("优化建议基线 DSL 无效：{error}")))?;
    let version_number = sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM decision_versions WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| ProjectApiError::database("计算优化草稿版本号", error))?;
    let review = build_change_review(
        Some(&definitions),
        &definitions,
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
        "message": "根据已确认的运行优化建议创建草稿；模型尚未修改，需在建模页完成变更。",
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
        ) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7)
        "#,
    )
    .bind(version.id)
    .bind(&dsl_yaml)
    .bind(&dmn_xml)
    .bind(decision_brief)
    .bind(decision_plan)
    .bind(review_json)
    .bind(&knowledge_json)
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
            resolution_note = '人工确认并创建优化草稿', resulting_version_id = $2
        WHERE id = $3 AND project_id = $4 AND status = 'open'
        "#,
    )
    .bind(&claims.sub)
    .bind(version.id)
    .bind(proposal_id)
    .bind(project_id)
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
        "optimization_proposal.confirmed",
        &claims.sub,
        &format!("确认优化建议并从 v{source_number} 创建草稿 v{version_number}"),
        json!({
            "proposalId": proposal_id,
            "sourceVersionId": source_version_id,
            "sourceVersionNumber": source_number,
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
    let knowledge_citations = serde_json::from_value::<Vec<KnowledgeCitation>>(knowledge_json)
        .map_err(|error| ProjectApiError::bad_request(format!("优化草稿知识引用无效：{error}")))?;
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

async fn get_decision_analytics(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<DecisionAnalyticsReport>, ProjectApiError> {
    Ok(Json(
        load_published_analytics(&state, &claims.sub, project_id).await?,
    ))
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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/model-library", get(list_model_library))
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
            "/projects/{project_id}/versions/{version_id}/test-cases/run",
            post(run_test_cases),
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
            "/projects/{project_id}/decision-analytics",
            get(get_decision_analytics),
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
        .layer(axum::extract::DefaultBodyLimit::max(1_100_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimization_guidance_preserves_human_review_boundary() {
        let (title, actions) = optimization_guidance("HIGH_MISMATCH_RATE");
        assert_eq!(title, "复核规则与实际业务结果的偏差");
        assert!(actions.iter().any(|action| action.contains("草稿")));
        assert!(actions.iter().all(|action| !action.contains("自动发布")));
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
        let intent = ModelChangeIntent {
            base_revision: 1,
            base_hash: String::new(),
            changes: vec![crate::models::ModelChangeOperation {
                target: "decision:Result".into(),
                action: "update".into(),
                field: Some("logic.rules[0]".into()),
                value: Some(json!("x")),
                reason: String::new(),
            }],
        };
        let error = apply_change_intent(&base, &intent).unwrap_err();
        assert_eq!(error.code, "PATCH_TARGET");
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
}
