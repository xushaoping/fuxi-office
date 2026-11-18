use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

struct AbortTaskOnDrop(tokio::task::JoinHandle<()>);

pub(crate) const EVALUATION_JOB_STALE_AFTER: Duration = Duration::from_secs(120);

pub(crate) async fn schema_ready(db: &sqlx::PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT to_regclass('public.prompt_assets') IS NOT NULL AND to_regclass('public.prompt_evaluation_jobs') IS NOT NULL",
    )
    .fetch_one(db)
    .await
}

impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

use crate::{
    api::LlmState,
    feel::FeelValue,
    models::{
        PromptAssetAdminView as PromptAssetView, PromptAssetEventAdminView as PromptAssetEventView,
    },
    nl_dsl::{
        dmn::generate_dmn_xml,
        parse_definitions,
        prompts::{PromptAsset, PromptCatalog},
        validate,
    },
    types::{AppState, Claims},
    workflow::DecisionBrief,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptEventsQuery {
    prompt_key: Option<String>,
}

fn digest(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn valid_prompt_key(key: &str) -> bool {
    matches!(
        key,
        "requirement_analysis"
            | "decision_planning"
            | "dsl_generation"
            | "change_intent"
            | "optimization_proposal"
            | "decision_explanation"
            | "tester_judge"
    )
}

fn validate_draft(
    payload: &crate::models::CreatePromptDraftRequest,
) -> Result<(), (StatusCode, String)> {
    if !valid_prompt_key(&payload.prompt_key) {
        return Err((StatusCode::BAD_REQUEST, "未知的提示词类别".into()));
    }
    if payload.version.is_empty()
        || payload.version.len() > 64
        || !payload
            .version
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
        || !payload.version.as_bytes()[0].is_ascii_alphanumeric()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "版本号须为 1-64 位字母、数字、点、下划线或连字符，且以字母或数字开头".into(),
        ));
    }
    if payload.content.trim().is_empty() || payload.content.len() > 100_000 {
        return Err((
            StatusCode::BAD_REQUEST,
            "提示词内容不能为空且不能超过 100000 字节".into(),
        ));
    }
    Ok(())
}

async fn require_admin(state: &AppState, claims: &Claims) -> Result<(), (StatusCode, String)> {
    let allowed = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM users WHERE username = $1 AND role = 'admin')",
    )
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(|error| database_error("验证管理员权限", error))?;
    if allowed {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "仅管理员可管理提示词资产".into()))
    }
}

fn database_error(action: &str, error: impl std::fmt::Display) -> (StatusCode, String) {
    eprintln!("{action}失败: {error}");
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{action}失败"))
}

pub async fn ensure_builtin_assets(db: &sqlx::PgPool) -> Result<PromptCatalog, sqlx::Error> {
    let catalog = PromptCatalog::builtins();
    let actor = sqlx::query_scalar::<_, String>(
        "SELECT username FROM users WHERE role = 'admin' ORDER BY username LIMIT 1",
    )
    .fetch_optional(db)
    .await?;
    let Some(actor) = actor else {
        return Ok(catalog);
    };
    let mut transaction = db.begin().await?;
    for asset in catalog.assets() {
        let content_digest = digest(&asset.content);
        let inserted = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO prompt_assets (
                prompt_key, version, content, content_digest, status, created_by,
                evaluation_json, activated_at
            )
            SELECT $1, $2, $3, $4, 'active', $5,
                   '{"evaluationType":"code_shipped_baseline"}'::JSONB, NOW()
            WHERE NOT EXISTS (
                SELECT 1 FROM prompt_assets WHERE prompt_key = $1 AND status = 'active'
            )
            ON CONFLICT (prompt_key, version) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(&asset.key)
        .bind(&asset.version)
        .bind(&asset.content)
        .bind(content_digest)
        .bind(&actor)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(id) = inserted {
            sqlx::query("INSERT INTO prompt_asset_events (prompt_asset_id, event_type, actor_username, details_json) VALUES ($1, 'activated', $2, $3)")
                .bind(id)
                .bind(&actor)
                .bind(json!({"source":"builtin_bootstrap", "version":asset.version}))
                .execute(&mut *transaction)
                .await?;
        }
    }
    transaction.commit().await?;
    Ok(catalog)
}

pub async fn load_active_catalog(db: &sqlx::PgPool, fallback: PromptCatalog) -> PromptCatalog {
    let rows = match sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT prompt_key, version, content, content_digest FROM prompt_assets WHERE status = 'active'",
    )
    .fetch_all(db)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("读取 active 提示词版本失败，继续使用代码内置版本: {error}");
            return fallback;
        }
    };
    let mut assets = rows
        .into_iter()
        .filter_map(|(key, version, content, content_digest)| {
            if digest(&content) != content_digest.trim() {
                eprintln!("active 提示词 {key}/{version} 的 SHA-256 不匹配，忽略该版本");
                return None;
            }
            Some((
                key.clone(),
                PromptAsset {
                    key,
                    version,
                    content,
                },
            ))
        })
        .collect::<HashMap<_, _>>();
    PromptCatalog {
        requirement_analysis: assets
            .remove("requirement_analysis")
            .unwrap_or(fallback.requirement_analysis),
        decision_planning: assets
            .remove("decision_planning")
            .unwrap_or(fallback.decision_planning),
        dsl_generation: assets
            .remove("dsl_generation")
            .unwrap_or(fallback.dsl_generation),
        change_intent: assets
            .remove("change_intent")
            .unwrap_or(fallback.change_intent),
        optimization_proposal: assets
            .remove("optimization_proposal")
            .unwrap_or(fallback.optimization_proposal),
        decision_explanation: assets
            .remove("decision_explanation")
            .unwrap_or(fallback.decision_explanation),
        tester_judge: assets
            .remove("tester_judge")
            .unwrap_or(fallback.tester_judge),
    }
}

async fn list_assets(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<PromptAssetView>>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let assets = sqlx::query_as::<_, PromptAssetView>(
        r#"SELECT id, prompt_key, version, content, content_digest, status,
                  evaluation_json AS evaluation, created_by, created_at, evaluated_at,
                  activated_at, retired_at
           FROM prompt_assets ORDER BY prompt_key, created_at DESC, id DESC"#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| database_error("读取提示词资产", error))?;
    Ok(Json(assets))
}

async fn list_events(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<PromptEventsQuery>,
) -> Result<Json<Vec<PromptAssetEventView>>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    if query
        .prompt_key
        .as_deref()
        .is_some_and(|key| !valid_prompt_key(key))
    {
        return Err((StatusCode::BAD_REQUEST, "未知的提示词类别".into()));
    }
    let events = sqlx::query_as::<_, PromptAssetEventView>(
        r#"SELECT event.id, event.prompt_asset_id, asset.prompt_key, asset.version,
                  event.event_type, event.actor_username, event.details_json AS details,
                  event.created_at
           FROM prompt_asset_events event
           JOIN prompt_assets asset ON asset.id = event.prompt_asset_id
           WHERE ($1::TEXT IS NULL OR asset.prompt_key = $1)
           ORDER BY event.id DESC LIMIT 200"#,
    )
    .bind(query.prompt_key)
    .fetch_all(&state.db)
    .await
    .map_err(|error| database_error("读取提示词发布历史", error))?;
    Ok(Json(events))
}

async fn create_draft(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<crate::models::CreatePromptDraftRequest>,
) -> Result<(StatusCode, Json<PromptAssetView>), (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    validate_draft(&payload)?;
    let content_digest = digest(&payload.content);
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| database_error("创建提示词草稿", error))?;
    let row = sqlx::query_as::<_, PromptAssetView>(
        r#"INSERT INTO prompt_assets (prompt_key, version, content, content_digest, created_by)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id, prompt_key, version, content, content_digest, status,
                     evaluation_json AS evaluation, created_by, created_at, evaluated_at,
                     activated_at, retired_at"#,
    )
    .bind(&payload.prompt_key)
    .bind(&payload.version)
    .bind(&payload.content)
    .bind(content_digest.clone())
    .bind(&claims.sub)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| {
        if let sqlx::Error::Database(database) = &error
            && database.code().as_deref() == Some("23505")
        {
            return (StatusCode::CONFLICT, "该提示词版本已存在".into());
        }
        database_error("创建提示词草稿", error)
    })?;
    sqlx::query("INSERT INTO prompt_asset_events (prompt_asset_id, event_type, actor_username, details_json) VALUES ($1, 'draft_created', $2, $3)")
        .bind(row.id)
        .bind(&claims.sub)
        .bind(json!({"digest":content_digest}))
        .execute(&mut *transaction)
        .await
        .map_err(|error| database_error("记录提示词草稿", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| database_error("创建提示词草稿", error))?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn evaluate_draft(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    axum::extract::Path(asset_id): axum::extract::Path<i64>,
    Json(payload): Json<crate::models::PromptEvaluationRequest>,
) -> Result<Json<PromptAssetView>, (StatusCode, String)> {
    if payload.passed {
        return Err((
            StatusCode::BAD_REQUEST,
            "人工登记不能标记为通过；请运行服务端真实 Golden 评测".into(),
        ));
    }
    persist_evaluation(State(state), Extension(claims), asset_id, payload, false).await
}

async fn persist_evaluation(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    asset_id: i64,
    payload: crate::models::PromptEvaluationRequest,
    live_golden: bool,
) -> Result<Json<PromptAssetView>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    if payload.passed && !live_golden {
        return Err((
            StatusCode::BAD_REQUEST,
            "只有服务端真实 Golden 评测可登记通过".into(),
        ));
    }
    if live_golden
        && payload
            .report
            .get("evaluationType")
            .and_then(serde_json::Value::as_str)
            != Some("llm_live_golden")
    {
        return Err((StatusCode::BAD_REQUEST, "真实模型评测报告类型无效".into()));
    }
    if asset_id <= 0
        || payload.case_count == 0
        || payload.dataset_hash.len() != 64
        || !payload
            .dataset_hash
            .chars()
            .all(|value| value.is_ascii_hexdigit())
        || !payload.report.is_object()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "评测报告必须包含数据集 SHA-256、非零样本数和 JSON 对象".into(),
        ));
    }
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| database_error("登记提示词评测", error))?;
    let (prompt_key, version, content, content_digest) = sqlx::query_as::<
        _,
        (String, String, String, String),
    >(
        "SELECT prompt_key, version, content, content_digest FROM prompt_assets WHERE id = $1 AND status = 'draft' FOR UPDATE",
    )
    .bind(asset_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| database_error("读取待评测提示词草稿", error))?
    .ok_or((
        StatusCode::CONFLICT,
        "只有草稿状态的提示词可登记评测".into(),
    ))?;
    if digest(&content) != content_digest.trim() {
        return Err((
            StatusCode::CONFLICT,
            "提示词内容摘要不匹配，拒绝登记评测".into(),
        ));
    }
    let evaluation = json!({
        "promptKey": prompt_key,
        "promptVersion": version,
        "promptDigest": content_digest.trim(),
        "datasetHash": payload.dataset_hash.to_ascii_lowercase(),
        "caseCount": payload.case_count,
        "passed": payload.passed,
        "evaluationType": if live_golden { "llm_live_golden" } else { "manual_review" },
        "report": payload.report,
    });
    let row = sqlx::query_as::<_, PromptAssetView>(
        r#"UPDATE prompt_assets SET status = CASE WHEN $2 THEN 'evaluated' ELSE 'draft' END,
                  evaluation_json = $3, evaluated_at = NOW()
           WHERE id = $1 AND status = 'draft'
           RETURNING id, prompt_key, version, content, content_digest, status,
                     evaluation_json AS evaluation, created_by, created_at, evaluated_at,
                     activated_at, retired_at"#,
    )
    .bind(asset_id)
    .bind(payload.passed)
    .bind(&evaluation)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| database_error("登记提示词评测", error))?
    .ok_or((
        StatusCode::CONFLICT,
        "只有草稿状态的提示词可登记评测".into(),
    ))?;
    sqlx::query("INSERT INTO prompt_asset_events (prompt_asset_id, event_type, actor_username, details_json) VALUES ($1, 'evaluated', $2, $3)")
        .bind(asset_id)
        .bind(&claims.sub)
        .bind(&evaluation)
        .execute(&mut *transaction)
        .await
        .map_err(|error| database_error("记录提示词评测", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| database_error("登记提示词评测", error))?;
    Ok(Json(row))
}

fn normalized_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn planning_decision_matches(
    decision: &crate::workflow::PlannedDecision,
    expected_name: &str,
    expected_type: &str,
    expected_expression: &str,
    expected_dependencies: &str,
) -> bool {
    let expected_dependencies = expected_dependencies
        .split(',')
        .filter(|value| !value.is_empty())
        .map(normalized_name)
        .collect::<BTreeSet<_>>();
    let actual_dependencies = decision
        .dependencies
        .iter()
        .map(|value| normalized_name(value))
        .collect::<BTreeSet<_>>();
    normalized_name(&decision.name) == normalized_name(expected_name)
        && decision.output_type.eq_ignore_ascii_case(expected_type)
        && decision.expression_type == expected_expression
        && actual_dependencies == expected_dependencies
}

fn meets_evaluation_threshold(
    case_count: u32,
    passed_cases: u32,
    minimum_cases: u32,
    threshold: f64,
) -> bool {
    case_count >= minimum_cases
        && passed_cases <= case_count
        && f64::from(passed_cases) / f64::from(case_count.max(1)) >= threshold
}

fn evaluation_allows_activation(
    status: &str,
    passed: bool,
    evaluation_type: Option<&str>,
    rollback: bool,
    shipped_baseline: bool,
) -> bool {
    if rollback {
        status == "retired"
            && (shipped_baseline || (passed && evaluation_type == Some("llm_live_golden")))
    } else {
        status == "evaluated" && passed && evaluation_type == Some("llm_live_golden")
    }
}

fn requirement_analysis_contract_valid(
    analysis: &crate::workflow::RequirementAnalysis,
    expected_decision: &str,
    expected_type: &str,
    expected_dependencies: &[&str],
) -> bool {
    let brief = &analysis.brief;
    let input_names = brief
        .inputs
        .iter()
        .map(|field| normalized_name(&field.name))
        .collect::<BTreeSet<_>>();
    let expected_inputs = expected_dependencies
        .iter()
        .map(|name| normalized_name(name))
        .collect::<BTreeSet<_>>();
    let has_expected_output = brief.outputs.iter().any(|field| {
        normalized_name(&field.name) == normalized_name(expected_decision)
            && normalized_name(&field.type_ref) == normalized_name(expected_type)
    });
    let clarification_ids = analysis
        .clarifications
        .iter()
        .map(|item| item.id.trim())
        .collect::<BTreeSet<_>>();
    !brief.name.trim().is_empty()
        && brief.name.chars().count() <= 100
        && !brief.goal.trim().is_empty()
        && brief.goal.chars().count() <= 300
        && !brief.description.trim().is_empty()
        && brief.description.chars().count() <= 600
        && brief.inputs.len() <= 20
        && brief.outputs.len() <= 20
        && brief.constraints.len() <= 12
        && brief.terms.len() <= 20
        && brief.assumptions.len() <= 12
        && matches!(brief.risk_level.as_str(), "low" | "medium" | "high")
        && expected_inputs.is_subset(&input_names)
        && has_expected_output
        && analysis.clarifications.len() <= 8
        && clarification_ids.len() == analysis.clarifications.len()
        && analysis.clarifications.iter().all(|item| {
            !item.id.trim().is_empty()
                && !item.question.trim().is_empty()
                && item.question.chars().count() <= 240
                && item.options.len() <= 8
        })
}

fn change_intent_contract_valid(
    intent: &crate::models::ModelChangeIntent,
    expected_target: &str,
    expected_field: &str,
    expected_value: &str,
) -> bool {
    intent.base_revision > 0
        && intent.base_hash.len() == 64
        && intent
            .base_hash
            .chars()
            .all(|value| value.is_ascii_hexdigit())
        && intent.changes.len() == 1
        && intent.changes.iter().all(|change| {
            change.target == expected_target
                && change.action == "update"
                && change.field.as_deref() == Some(expected_field)
                && change.value.as_ref().and_then(serde_json::Value::as_str) == Some(expected_value)
                && !change.reason.trim().is_empty()
                && change.reason.chars().count() <= 240
        })
}

fn validate_golden_rows<'a>(
    dataset_text: &'a str,
    field_count: usize,
    label: &str,
) -> Result<Vec<&'a str>, String> {
    let rows = dataset_text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    let mut ids = std::collections::HashSet::new();
    for row in &rows {
        let fields = row.split('\t').collect::<Vec<_>>();
        if fields.len() != field_count || fields.iter().any(|field| field.trim().is_empty()) {
            return Err(format!("{label}评测集字段格式错误"));
        }
        if !ids.insert(fields[0]) {
            return Err(format!("{label}评测集包含重复 ID"));
        }
    }
    if rows.is_empty() {
        return Err(format!("{label}评测集为空"));
    }
    Ok(rows)
}

async fn evaluate_requirement_analysis_prompt(
    agent: &crate::nl_dsl::agent::LlmAgent,
    db: &sqlx::PgPool,
    job_id: i64,
) -> Result<serde_json::Value, String> {
    let dataset = include_bytes!("../tests/golden/planning-contracts.tsv");
    let dataset_text =
        std::str::from_utf8(dataset).map_err(|_| "需求分析评测集不是 UTF-8".to_string())?;
    let rows = validate_golden_rows(dataset_text, 6, "规划")?;
    let golden_case_count = rows.len();
    let started = Instant::now();
    let mut cases = Vec::new();
    let mut passed_cases = 0_u32;
    let mut consecutive_generation_failures = 0_u8;
    for line in rows {
        let fields = line.splitn(6, '\t').collect::<Vec<_>>();
        let [
            id,
            requirement,
            expected_decision,
            expected_type,
            _,
            dependencies,
        ] = <[&str; 6]>::try_from(fields).map_err(|_| "规划评测集字段数错误".to_string())?;
        let case_started = Instant::now();
        let result = agent.analyze_requirement(requirement, None, None).await;
        let (matched, failure) = match result {
            Ok(analysis) => {
                let expected_dependencies = dependencies.split(',').collect::<Vec<_>>();
                let valid = requirement_analysis_contract_valid(
                    &analysis,
                    expected_decision,
                    expected_type,
                    &expected_dependencies,
                );
                (valid, (!valid).then_some("contract_mismatch"))
            }
            Err(_) => {
                consecutive_generation_failures = consecutive_generation_failures.saturating_add(1);
                (false, Some("generation_failed"))
            }
        };
        if matched {
            passed_cases += 1;
        }
        if failure != Some("generation_failed") {
            consecutive_generation_failures = 0;
        }
        cases.push(serde_json::json!({
            "id": id,
            "passed": matched,
            "failureCode": failure,
            "durationMs": case_started.elapsed().as_millis(),
        }));
        update_evaluation_job_progress(db, job_id, cases.len() as i32).await?;
        if consecutive_generation_failures >= 3 {
            break;
        }
    }
    let case_count = cases.len() as u32;
    let threshold = 0.80;
    Ok(serde_json::json!({
        "evaluationType": "llm_live_golden",
        "task": "requirement_analysis",
        "evaluationScope": "contract_only_expected_field_anchors_not_semantic_quality",
        "promotionEligible": false,
        "dataset": "planning-contracts.tsv",
        "datasetHash": digest_bytes(dataset),
        "caseCount": case_count,
        "goldenCaseCount": golden_case_count,
        "stoppedEarly": cases.len() < golden_case_count,
        "passedCases": passed_cases,
        "passRate": if case_count == 0 { 0.0 } else { f64::from(passed_cases) / f64::from(case_count) },
        "threshold": threshold,
        "contractPassed": meets_evaluation_threshold(case_count, passed_cases, 20, threshold),
        "passed": false,
        "durationMs": started.elapsed().as_millis(),
        "cases": cases,
    }))
}

async fn evaluate_change_intent_prompt(
    agent: &crate::nl_dsl::agent::LlmAgent,
    db: &sqlx::PgPool,
    job_id: i64,
) -> Result<serde_json::Value, String> {
    let dataset = include_bytes!("../tests/golden/change-intents.tsv");
    let dataset_text =
        std::str::from_utf8(dataset).map_err(|_| "Change Intent 评测集不是 UTF-8".to_string())?;
    let rows = validate_golden_rows(dataset_text, 5, "Change Intent")?;
    let golden_case_count = rows.len();
    let existing_model = include_str!("loan.yaml");
    let started = Instant::now();
    let mut cases = Vec::new();
    let mut passed_cases = 0_u32;
    let mut consecutive_generation_failures = 0_u8;
    for line in rows {
        let fields = line.splitn(5, '\t').collect::<Vec<_>>();
        let [id, requirement, target, field, expected_value] = <[&str; 5]>::try_from(fields)
            .map_err(|_| "Change Intent 评测集字段数错误".to_string())?;
        let case_started = Instant::now();
        let result = agent
            .generate_change_intent(requirement, existing_model, 1, None, None)
            .await;
        let (matched, failure) = match result {
            Ok(intent) => {
                let valid = change_intent_contract_valid(&intent, target, field, expected_value);
                (valid, (!valid).then_some("contract_mismatch"))
            }
            Err(_) => {
                consecutive_generation_failures = consecutive_generation_failures.saturating_add(1);
                (false, Some("generation_failed"))
            }
        };
        if matched {
            passed_cases += 1;
        }
        if failure != Some("generation_failed") {
            consecutive_generation_failures = 0;
        }
        cases.push(serde_json::json!({
            "id": id,
            "passed": matched,
            "failureCode": failure,
            "durationMs": case_started.elapsed().as_millis(),
        }));
        update_evaluation_job_progress(db, job_id, cases.len() as i32).await?;
        if consecutive_generation_failures >= 3 {
            break;
        }
    }
    let case_count = cases.len() as u32;
    let threshold = 0.80;
    Ok(serde_json::json!({
        "evaluationType": "llm_live_golden",
        "task": "change_intent",
        "evaluationScope": "contract_only_single_patch_target_and_value_not_semantic_quality",
        "promotionEligible": false,
        "dataset": "change-intents.tsv",
        "datasetHash": digest_bytes(dataset),
        "caseCount": case_count,
        "goldenCaseCount": golden_case_count,
        "stoppedEarly": cases.len() < golden_case_count,
        "passedCases": passed_cases,
        "passRate": if case_count == 0 { 0.0 } else { f64::from(passed_cases) / f64::from(case_count) },
        "threshold": threshold,
        "contractPassed": meets_evaluation_threshold(case_count, passed_cases, 20, threshold),
        "passed": false,
        "durationMs": started.elapsed().as_millis(),
        "cases": cases,
    }))
}

async fn evaluate_planning_prompt(
    agent: &crate::nl_dsl::agent::LlmAgent,
    db: &sqlx::PgPool,
    job_id: i64,
) -> Result<serde_json::Value, String> {
    let dataset = include_bytes!("../tests/golden/planning-contracts.tsv");
    let dataset_text =
        std::str::from_utf8(dataset).map_err(|_| "规划评测集不是 UTF-8".to_string())?;
    let golden_case_count = dataset_text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .count();
    let started = Instant::now();
    let mut cases = Vec::new();
    let mut passed_cases = 0_u32;
    let mut consecutive_generation_failures = 0_u8;
    for line in dataset_text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields = line.splitn(6, '\t').collect::<Vec<_>>();
        if fields.len() != 6 {
            return Err("规划评测集包含格式错误的行".into());
        }
        let [
            id,
            requirement,
            expected_name,
            expected_type,
            expected_expression,
            expected_dependencies,
        ] = <[&str; 6]>::try_from(fields).map_err(|_| "规划评测集字段数错误".to_string())?;
        let brief = DecisionBrief {
            name: id.replace('-', "_"),
            goal: requirement.into(),
            description: requirement.into(),
            ..Default::default()
        };
        let case_started = Instant::now();
        let output = agent.plan_decision_model(&brief, &[], None, None).await;
        let mut matched = false;
        let mut failure = None;
        match output {
            Ok(plan) => {
                matched = plan.decisions.iter().any(|decision| {
                    planning_decision_matches(
                        decision,
                        expected_name,
                        expected_type,
                        expected_expression,
                        expected_dependencies,
                    )
                });
                if !matched {
                    failure = Some("plan_contract_mismatch");
                }
            }
            Err(_) => {
                failure = Some("generation_failed");
                consecutive_generation_failures = consecutive_generation_failures.saturating_add(1);
            }
        }
        if matched {
            consecutive_generation_failures = 0;
        }
        if matched {
            passed_cases += 1;
        }
        cases.push(serde_json::json!({
            "id": id,
            "passed": matched,
            "failureCode": failure,
            "durationMs": case_started.elapsed().as_millis(),
        }));
        update_evaluation_job_progress(db, job_id, cases.len() as i32).await?;
        if consecutive_generation_failures >= 3 {
            break;
        }
    }
    let case_count = cases.len() as u32;
    let pass_rate = if case_count == 0 {
        0.0
    } else {
        f64::from(passed_cases) / f64::from(case_count)
    };
    let threshold = 0.80;
    Ok(serde_json::json!({
        "evaluationType": "llm_live_golden",
        "task": "decision_planning",
        "dataset": "planning-contracts.tsv",
        "datasetHash": digest_bytes(dataset),
        "caseCount": case_count,
        "goldenCaseCount": golden_case_count,
        "stoppedEarly": cases.len() < golden_case_count,
        "passedCases": passed_cases,
        "passRate": pass_rate,
        "threshold": threshold,
        "passed": meets_evaluation_threshold(case_count, passed_cases, 20, threshold),
        "durationMs": started.elapsed().as_millis(),
        "cases": cases,
    }))
}

async fn evaluate_dsl_prompt(
    agent: &crate::nl_dsl::agent::LlmAgent,
    db: &sqlx::PgPool,
    job_id: i64,
) -> Result<serde_json::Value, String> {
    let dataset = include_bytes!("../tests/golden/age-thresholds.tsv");
    let dataset_text =
        std::str::from_utf8(dataset).map_err(|_| "DSL 评测集不是 UTF-8".to_string())?;
    let golden_case_count = dataset_text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .count();
    let started = Instant::now();
    let mut cases = Vec::new();
    let mut passed_cases = 0_u32;
    let mut consecutive_generation_failures = 0_u8;
    for line in dataset_text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields = line.splitn(5, '\t').collect::<Vec<_>>();
        if fields.len() != 5 {
            return Err("DSL 评测集包含格式错误的行".into());
        }
        let [id, _threshold, probe_age, expected, requirement] =
            <[&str; 5]>::try_from(fields).map_err(|_| "DSL 评测集字段数错误".to_string())?;
        let case_started = Instant::now();
        let result = async {
            let generated = agent.generate_dsl(requirement, None).await.map_err(|_| "generation_failed")?;
            let definitions = parse_definitions(&generated.yaml).map_err(|_| "invalid_dsl")?;
            validate(&definitions).map_err(|_| "invalid_dsl")?;
            let xml = generate_dmn_xml(&definitions).map_err(|_| "compile_failed")?;
            let model = crate::dmn::parse_dmn(&xml).map_err(|_| "compile_failed")?;
            let age = probe_age.parse::<f64>().map_err(|_| "invalid_golden_case")?;
            let numeric_inputs = definitions
                .inputs
                .iter()
                .filter(|input| input.type_ref.as_deref().is_some_and(|value| {
                    value.eq_ignore_ascii_case("number") || value.eq_ignore_ascii_case("feel:number")
                }))
                .map(|input| (input.name.clone(), FeelValue::Number(age)))
                .collect::<HashMap<_, _>>();
            if numeric_inputs.is_empty() {
                return Err("missing_numeric_input");
            }
            let evaluation = crate::dmn::DmnExecutor::new(&model).evaluate_all(numeric_inputs);
            if evaluation.status == crate::dmn::DmnResultStatus::Succeeded
                && evaluation.decision_results.iter().any(|decision| {
                    matches!(&decision.value, FeelValue::String(value) if value.eq_ignore_ascii_case(expected))
                })
            {
                Ok(())
            } else {
                Err("behavior_mismatch")
            }
        }
        .await;
        let matched = result.is_ok();
        if matches!(result, Err("generation_failed")) {
            consecutive_generation_failures = consecutive_generation_failures.saturating_add(1);
        } else {
            consecutive_generation_failures = 0;
        }
        if matched {
            passed_cases += 1;
        }
        cases.push(serde_json::json!({
            "id": id,
            "passed": matched,
            "failureCode": result.err(),
            "durationMs": case_started.elapsed().as_millis(),
        }));
        update_evaluation_job_progress(db, job_id, cases.len() as i32).await?;
        if consecutive_generation_failures >= 3 {
            break;
        }
    }
    let case_count = cases.len() as u32;
    let pass_rate = if case_count == 0 {
        0.0
    } else {
        f64::from(passed_cases) / f64::from(case_count)
    };
    let threshold = 0.80;
    Ok(serde_json::json!({
        "evaluationType": "llm_live_golden",
        "task": "dsl_generation",
        "dataset": "age-thresholds.tsv",
        "datasetHash": digest_bytes(dataset),
        "caseCount": case_count,
        "goldenCaseCount": golden_case_count,
        "stoppedEarly": cases.len() < golden_case_count,
        "passedCases": passed_cases,
        "passRate": pass_rate,
        "threshold": threshold,
        "passed": meets_evaluation_threshold(case_count, passed_cases, 50, threshold),
        "durationMs": started.elapsed().as_millis(),
        "cases": cases,
    }))
}

fn optimization_suggestion_contract_valid(
    suggestion: &crate::nl_dsl::agent::OptimizationSuggestion,
    finding_code: &str,
    element: &str,
) -> bool {
    let valid_text = |value: &str, max_chars: usize| {
        !value.trim().is_empty() && value.chars().count() <= max_chars
    };
    suggestion.finding_code == finding_code
        && suggestion.element == element
        && valid_text(&suggestion.title, 160)
        && valid_text(&suggestion.summary, 800)
        && (1..=5).contains(&suggestion.proposed_actions.len())
        && suggestion.proposed_actions.iter().all(|action| {
            valid_text(action, 240)
                && ![
                    "自动发布",
                    "绕过验收",
                    "无需复核",
                    "证明因果",
                    "直接修改生产",
                ]
                .iter()
                .any(|forbidden| action.contains(forbidden))
        })
}

async fn evaluate_optimization_prompt(
    agent: &crate::nl_dsl::agent::LlmAgent,
    db: &sqlx::PgPool,
    job_id: i64,
) -> Result<serde_json::Value, String> {
    let dataset = include_bytes!("../tests/golden/optimization-proposals.tsv");
    let dataset_text =
        std::str::from_utf8(dataset).map_err(|_| "优化建议评测集不是 UTF-8".to_string())?;
    let rows = dataset_text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    let golden_case_count = rows.len();
    let started = Instant::now();
    let mut cases = Vec::new();
    let mut passed_cases = 0_u32;
    let mut consecutive_generation_failures = 0_u8;
    for line in rows {
        let fields = line.splitn(5, '\t').collect::<Vec<_>>();
        if fields.len() != 5 {
            return Err("优化建议评测集包含格式错误的行".into());
        }
        let [id, finding_code, element, level, message] =
            <[&str; 5]>::try_from(fields).map_err(|_| "优化建议评测集字段数错误".to_string())?;
        let finding = serde_json::json!({
            "findingCode": finding_code,
            "element": element,
            "level": level,
            "message": message,
        });
        let case_started = Instant::now();
        let result = agent
            .generate_optimization_suggestions(std::slice::from_ref(&finding))
            .await;
        let (matched, failure) = match result {
            Ok(suggestions) if suggestions.len() == 1 => {
                let valid =
                    optimization_suggestion_contract_valid(&suggestions[0], finding_code, element);
                (valid, (!valid).then_some("contract_mismatch"))
            }
            Ok(_) => (false, Some("result_count_mismatch")),
            Err(_) => {
                consecutive_generation_failures = consecutive_generation_failures.saturating_add(1);
                (false, Some("generation_failed"))
            }
        };
        if matched {
            passed_cases += 1;
        }
        if failure != Some("generation_failed") {
            consecutive_generation_failures = 0;
        }
        cases.push(serde_json::json!({
            "id": id,
            "passed": matched,
            "failureCode": failure,
            "durationMs": case_started.elapsed().as_millis(),
        }));
        update_evaluation_job_progress(db, job_id, cases.len() as i32).await?;
        if consecutive_generation_failures >= 3 {
            break;
        }
    }
    let case_count = cases.len() as u32;
    let pass_rate = if case_count == 0 {
        0.0
    } else {
        f64::from(passed_cases) / f64::from(case_count)
    };
    let threshold = 0.80;
    Ok(serde_json::json!({
        "evaluationType": "llm_live_golden",
        "task": "optimization_proposal",
        "evaluationScope": "structured_contract_and_human_gate_policy",
        "dataset": "optimization-proposals.tsv",
        "datasetHash": digest_bytes(dataset),
        "caseCount": case_count,
        "goldenCaseCount": golden_case_count,
        "stoppedEarly": cases.len() < golden_case_count,
        "passedCases": passed_cases,
        "passRate": pass_rate,
        "threshold": threshold,
        "passed": meets_evaluation_threshold(case_count, passed_cases, 20, threshold),
        "durationMs": started.elapsed().as_millis(),
        "cases": cases,
    }))
}

fn tester_judge_contract_valid(
    vote: &crate::workflow::TesterJudgeVote,
    expected_assessment: &str,
) -> bool {
    vote.assessment == expected_assessment
        && !vote.reason.trim().is_empty()
        && vote.reason.chars().count() <= 600
        && !["自动发布", "绕过审批", "跳过验收", "无需复核"]
            .iter()
            .any(|forbidden| vote.reason.contains(forbidden))
}

fn tester_judge_golden_context(line: &str) -> Result<(&str, serde_json::Value), String> {
    let fields = line.split('\t').collect::<Vec<_>>();
    let [
        _id,
        assessment,
        requirement,
        input_json,
        expected_json,
        _reference_reason,
    ] = <[&str; 6]>::try_from(fields).map_err(|_| "Tester Judge 评测集字段数错误".to_string())?;
    if !matches!(
        assessment,
        "plausible" | "suspicious" | "insufficient_evidence"
    ) {
        return Err("Tester Judge 评测集包含未知判定标签".into());
    }
    let input = serde_json::from_str::<serde_json::Value>(input_json)
        .map_err(|_| "Tester Judge 评测集输入 JSON 无效".to_string())?;
    let expected = serde_json::from_str::<serde_json::Value>(expected_json)
        .map_err(|_| "Tester Judge 评测集期望输出 JSON 无效".to_string())?;
    if !input.is_object() || expected.as_object().is_none_or(|value| value.is_empty()) {
        return Err("Tester Judge 评测集输入/期望输出结构无效".into());
    }
    Ok((
        assessment,
        json!({
            "requirement": requirement,
            "brief": {"name": "Synthetic Tester Judge evaluation"},
            "confirmedAnswers": [],
            "decisionPlan": {"decisions": []},
            "proposal": {
                "category": "semantic",
                "title": "Check expected output against confirmed requirements",
                "rationale": "Review the candidate case using only confirmed requirements",
                "sourceReference": "synthetic_contract_fixture",
                "inputFields": [],
            },
            "candidateCase": {"input": input, "expectedOutput": expected},
        }),
    ))
}

async fn evaluate_tester_judge_prompt(
    agent: &crate::nl_dsl::agent::LlmAgent,
    db: &sqlx::PgPool,
    job_id: i64,
) -> Result<serde_json::Value, String> {
    let dataset = include_bytes!("../tests/golden/tester-judges.tsv");
    let dataset_text =
        std::str::from_utf8(dataset).map_err(|_| "Tester Judge 评测集不是 UTF-8".to_string())?;
    let rows = validate_golden_rows(dataset_text, 6, "Tester Judge")?;
    let contexts = rows
        .iter()
        .map(|row| tester_judge_golden_context(row))
        .collect::<Result<Vec<_>, _>>()?;
    let started = Instant::now();
    let mut cases = Vec::new();
    let mut passed_cases = 0_u32;
    let mut consecutive_generation_failures = 0_u8;
    for (line, (expected_assessment, context)) in rows.iter().zip(contexts.iter()) {
        let id = line.split('\t').next().unwrap_or_default();
        let case_started = Instant::now();
        let result = agent.judge_tester_expected_output(context).await;
        let (matched, failure) = match result {
            Ok(vote) => {
                let valid = tester_judge_contract_valid(&vote, expected_assessment);
                (
                    valid,
                    (!valid).then_some("assessment_or_safety_contract_mismatch"),
                )
            }
            Err(_) => {
                consecutive_generation_failures = consecutive_generation_failures.saturating_add(1);
                (false, Some("generation_failed"))
            }
        };
        if matched {
            passed_cases += 1;
        }
        if failure != Some("generation_failed") {
            consecutive_generation_failures = 0;
        }
        cases.push(serde_json::json!({
            "id": id,
            "passed": matched,
            "failureCode": failure,
            "durationMs": case_started.elapsed().as_millis(),
        }));
        update_evaluation_job_progress(db, job_id, cases.len() as i32).await?;
        if consecutive_generation_failures >= 3 {
            break;
        }
    }
    let case_count = cases.len() as u32;
    let pass_rate = if case_count == 0 {
        0.0
    } else {
        f64::from(passed_cases) / f64::from(case_count)
    };
    let threshold = 0.80;
    Ok(serde_json::json!({
        "evaluationType": "llm_live_golden",
        "task": "tester_judge",
        "evaluationScope": "synthetic_explicit_rule_cases_not_business_quality_or_adoption",
        "dataset": "tester-judges.tsv",
        "datasetHash": digest_bytes(dataset),
        "caseCount": case_count,
        "goldenCaseCount": rows.len(),
        "stoppedEarly": cases.len() < rows.len(),
        "passedCases": passed_cases,
        "passRate": pass_rate,
        "threshold": threshold,
        "passed": meets_evaluation_threshold(case_count, passed_cases, 20, threshold),
        "durationMs": started.elapsed().as_millis(),
        "cases": cases,
    }))
}

fn decision_explanation_contract_valid(
    explanation: &crate::models::DecisionRunExplanation,
    expected_ids: &[String],
) -> bool {
    let expected = expected_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let actual = explanation
        .decisions
        .iter()
        .map(|step| step.decision_id.as_str())
        .collect::<BTreeSet<_>>();
    !explanation.summary.trim().is_empty()
        && explanation.summary.chars().count() <= 2000
        && actual.len() == explanation.decisions.len()
        && actual == expected
        && explanation.decisions.iter().all(|step| {
            !step.explanation.trim().is_empty()
                && step.explanation.chars().count() <= 1200
                && !["自动发布", "绕过审批", "跳过验收", "保证盈利", "证明因果"]
                    .iter()
                    .any(|forbidden| step.explanation.contains(forbidden))
        })
}

async fn evaluate_decision_explanation_prompt(
    agent: &crate::nl_dsl::agent::LlmAgent,
    db: &sqlx::PgPool,
    job_id: i64,
) -> Result<serde_json::Value, String> {
    let dataset = include_bytes!("../tests/golden/decision-explanations.tsv");
    let dataset_text =
        std::str::from_utf8(dataset).map_err(|_| "决策解释评测集不是 UTF-8".to_string())?;
    let rows = dataset_text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    let started = Instant::now();
    let mut cases = Vec::new();
    let mut passed_cases = 0_u32;
    let mut consecutive_generation_failures = 0_u8;
    for line in &rows {
        let fields = line.splitn(2, '\t').collect::<Vec<_>>();
        if fields.len() != 2 {
            return Err("决策解释评测集包含格式错误的行".into());
        }
        let [id, decision_ids] =
            <[&str; 2]>::try_from(fields).map_err(|_| "决策解释评测集字段数错误".to_string())?;
        let expected_ids = decision_ids
            .split(',')
            .map(str::trim)
            .map(str::to_string)
            .collect::<Vec<_>>();
        if expected_ids.is_empty()
            || expected_ids.iter().any(String::is_empty)
            || expected_ids.iter().collect::<BTreeSet<_>>().len() != expected_ids.len()
        {
            return Err("决策解释评测集包含无效或重复决策 ID".into());
        }
        let trace = serde_json::json!({
            "modelName": "GoldenDecision",
            "status": "succeeded",
            "decisions": expected_ids.iter().enumerate().map(|(index, decision_id)| serde_json::json!({
                "order": index + 1,
                "decisionId": decision_id,
                "decisionName": decision_id,
                "logicType": "decisionTable",
                "status": "matched",
                "outcome": if index % 2 == 0 { "approved" } else { "review" },
                "dependencies": [],
                "matchedRuleIds": [format!("{decision_id}-rule")]
            })).collect::<Vec<_>>()
        });
        let case_started = Instant::now();
        let result = agent.explain_decision_run(&trace).await;
        let (matched, failure) = match result {
            Ok(explanation) => {
                let valid = decision_explanation_contract_valid(&explanation, &expected_ids);
                (
                    valid,
                    (!valid).then_some("trace_or_safety_contract_mismatch"),
                )
            }
            Err(_) => {
                consecutive_generation_failures = consecutive_generation_failures.saturating_add(1);
                (false, Some("generation_failed"))
            }
        };
        if matched {
            passed_cases += 1;
        }
        if failure != Some("generation_failed") {
            consecutive_generation_failures = 0;
        }
        cases.push(serde_json::json!({
            "id": id,
            "passed": matched,
            "failureCode": failure,
            "durationMs": case_started.elapsed().as_millis(),
        }));
        update_evaluation_job_progress(db, job_id, cases.len() as i32).await?;
        if consecutive_generation_failures >= 3 {
            break;
        }
    }
    let case_count = cases.len() as u32;
    let pass_rate = if case_count == 0 {
        0.0
    } else {
        f64::from(passed_cases) / f64::from(case_count)
    };
    let threshold = 0.80;
    Ok(serde_json::json!({
        "evaluationType": "llm_live_golden",
        "task": "decision_explanation",
        "evaluationScope": "trace_coverage_and_safety_contract_not_semantic_quality",
        "dataset": "decision-explanations.tsv",
        "datasetHash": digest_bytes(dataset),
        "caseCount": case_count,
        "goldenCaseCount": rows.len(),
        "stoppedEarly": cases.len() < rows.len(),
        "passedCases": passed_cases,
        "passRate": pass_rate,
        "threshold": threshold,
        "passed": meets_evaluation_threshold(case_count, passed_cases, 20, threshold),
        "durationMs": started.elapsed().as_millis(),
        "cases": cases,
    }))
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

async fn update_evaluation_job_progress(
    db: &sqlx::PgPool,
    job_id: i64,
    completed_cases: i32,
) -> Result<(), String> {
    let cancel_requested = sqlx::query_scalar::<_, bool>(
        "UPDATE prompt_evaluation_jobs SET completed_cases = $2, heartbeat_at = NOW() WHERE id = $1 AND status = 'running' RETURNING cancel_requested",
    )
    .bind(job_id)
    .bind(completed_cases)
    .fetch_optional(db)
    .await
    .map_err(|_| "保存评测作业进度失败".to_string())?
    .ok_or_else(|| "评测作业已停止".to_string())?;
    if cancel_requested {
        return Err("评测作业已取消".into());
    }
    Ok(())
}

async fn run_live_evaluation(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Extension(llm_state): Extension<Arc<LlmState>>,
    axum::extract::Path(asset_id): axum::extract::Path<i64>,
) -> Result<(StatusCode, Json<crate::models::PromptEvaluationJobView>), (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let (prompt_key, version, content, content_digest) = sqlx::query_as::<
        _,
        (String, String, String, String),
    >(
        "SELECT prompt_key, version, content, content_digest FROM prompt_assets WHERE id = $1 AND status = 'draft'",
    )
    .bind(asset_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| database_error("读取待评测提示词草稿", error))?
    .ok_or((StatusCode::CONFLICT, "只有草稿状态的提示词可运行评测".into()))?;
    if digest(&content) != content_digest.trim() {
        return Err((
            StatusCode::CONFLICT,
            "提示词内容摘要不匹配，拒绝运行评测".into(),
        ));
    }
    let evaluator = llm_state.prompt_evaluation_agent(&prompt_key).ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "当前未配置该任务的模型服务".into(),
    ))?;
    let total_cases = match prompt_key.as_str() {
        "requirement_analysis" => 20,
        "decision_planning" => 20,
        "dsl_generation" => 50,
        "change_intent" => 20,
        "optimization_proposal" => 20,
        "decision_explanation" => 20,
        "tester_judge" => 20,
        _ => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                "该提示词类别尚无带人工期望结果的在线 Golden 评测集".into(),
            ));
        }
    };
    let job = sqlx::query_as::<_, crate::models::PromptEvaluationJobView>(
        r#"INSERT INTO prompt_evaluation_jobs (prompt_asset_id, created_by, total_cases)
           VALUES ($1, $2, $3)
           RETURNING id, prompt_asset_id, status, completed_cases, total_cases,
                     result_json AS result, error_message, cancel_requested,
                     created_at, started_at, heartbeat_at, finished_at"#,
    )
    .bind(asset_id)
    .bind(&claims.sub)
    .bind(total_cases)
    .fetch_one(&state.db)
    .await
    .map_err(|error| {
        if let sqlx::Error::Database(database) = &error
            && database.code().as_deref() == Some("23505")
        {
            return (StatusCode::CONFLICT, "该草稿已有正在运行的评测作业".into());
        }
        database_error("创建提示词评测作业", error)
    })?;
    let state_for_job = state.clone();
    let claims_for_job = claims.clone();
    let evaluator = evaluator.clone();
    tokio::spawn(async move {
        execute_live_evaluation_job(
            state_for_job,
            claims_for_job,
            evaluator,
            job.id,
            asset_id,
            prompt_key,
            version,
            content,
            content_digest,
        )
        .await;
    });
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn execute_live_evaluation_job(
    state: AppState,
    claims: Claims,
    evaluator: crate::nl_dsl::agent::LlmAgent,
    job_id: i64,
    asset_id: i64,
    prompt_key: String,
    version: String,
    content: String,
    content_digest: String,
) {
    let result = async {
        let started = sqlx::query("UPDATE prompt_evaluation_jobs SET status = 'running', started_at = NOW(), heartbeat_at = NOW() WHERE id = $1 AND status = 'pending' AND cancel_requested = FALSE")
            .bind(job_id)
            .execute(&state.db)
            .await
            .map_err(|_| "启动评测作业失败".to_string())?;
        if started.rows_affected() == 0 {
            return Err("评测作业已停止".into());
        }
        let heartbeat_pool = state.db.clone();
        let heartbeat = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                let refreshed = sqlx::query(
                    "UPDATE prompt_evaluation_jobs SET heartbeat_at = NOW() WHERE id = $1 AND status = 'running'",
                )
                .bind(job_id)
                .execute(&heartbeat_pool)
                .await
                .is_ok_and(|result| result.rows_affected() == 1);
                if !refreshed {
                    break;
                }
            }
        });
        let _heartbeat_guard = AbortTaskOnDrop(heartbeat);
        let mut catalog = load_active_catalog(&state.db, PromptCatalog::builtins()).await;
        let candidate = PromptAsset {
            key: prompt_key.clone(),
            version: version.clone(),
            content,
        };
        match prompt_key.as_str() {
            "requirement_analysis" => catalog.requirement_analysis = candidate,
            "decision_planning" => catalog.decision_planning = candidate,
            "dsl_generation" => catalog.dsl_generation = candidate,
            "change_intent" => catalog.change_intent = candidate,
            "optimization_proposal" => catalog.optimization_proposal = candidate,
            "decision_explanation" => catalog.decision_explanation = candidate,
            "tester_judge" => catalog.tester_judge = candidate,
            _ => return Err("该提示词类别尚无在线 Golden 评测集".to_string()),
        }
        let agent = evaluator.fork_for_run().with_prompt_catalog(catalog);
        let model = agent.model().to_string();
        let evaluation = match prompt_key.as_str() {
            "requirement_analysis" => {
                evaluate_requirement_analysis_prompt(&agent, &state.db, job_id).await
            }
            "decision_planning" => evaluate_planning_prompt(&agent, &state.db, job_id).await,
            "dsl_generation" => evaluate_dsl_prompt(&agent, &state.db, job_id).await,
            "change_intent" => evaluate_change_intent_prompt(&agent, &state.db, job_id).await,
            "optimization_proposal" => {
                evaluate_optimization_prompt(&agent, &state.db, job_id).await
            }
            "decision_explanation" => {
                evaluate_decision_explanation_prompt(&agent, &state.db, job_id).await
            }
            "tester_judge" => evaluate_tester_judge_prompt(&agent, &state.db, job_id).await,
            _ => unreachable!("unsupported prompt key was rejected"),
        }?;
        let mut job_transaction = state
            .db
            .begin()
            .await
            .map_err(|_| "读取评测取消状态失败".to_string())?;
        let cancel_requested = sqlx::query_scalar::<_, bool>(
            "SELECT cancel_requested FROM prompt_evaluation_jobs WHERE id = $1 AND status = 'running' FOR UPDATE",
        )
        .bind(job_id)
        .fetch_optional(&mut *job_transaction)
        .await
        .map_err(|_| "读取评测取消状态失败".to_string())?
        .unwrap_or(true);
        if cancel_requested {
            return Err("评测作业已取消".into());
        }
        let case_count = evaluation["caseCount"].as_u64().unwrap_or(0) as u32;
        if case_count == 0 {
            return Err("评测未产生有效样本".to_string());
        }
        let payload = crate::models::PromptEvaluationRequest {
            dataset_hash: evaluation["datasetHash"]
                .as_str()
                .unwrap_or_default()
                .into(),
            case_count,
            passed: evaluation["passed"].as_bool().unwrap_or(false),
            report: json!({
                "evaluationType": "llm_live_golden",
                "promptKey": prompt_key,
                "promptVersion": version,
                "promptDigest": content_digest,
                "jobId": job_id,
                "model": model,
                "metrics": evaluation,
                "usage": agent.usage_snapshot(),
            }),
        };
        let row = persist_evaluation(
            State(state.clone()),
            Extension(claims.clone()),
            asset_id,
            payload,
            true,
        )
        .await
        .map_err(|_| "登记真实模型评测结果失败".to_string())?;
        let evaluation = row.0.evaluation.unwrap_or_else(|| json!({}));
        sqlx::query("UPDATE prompt_evaluation_jobs SET status = 'succeeded', result_json = $2, heartbeat_at = NOW(), finished_at = NOW() WHERE id = $1 AND status = 'running' AND cancel_requested = FALSE")
            .bind(job_id)
            .bind(evaluation)
            .execute(&mut *job_transaction)
            .await
            .map_err(|_| "保存评测作业结果失败".to_string())?;
        job_transaction
            .commit()
            .await
            .map_err(|_| "提交评测作业结果失败".to_string())?;
        Ok::<(), String>(())
    }
    .await;
    if let Err(message) = result {
        let cancelled = message == "评测作业已取消";
        let _ = sqlx::query("UPDATE prompt_evaluation_jobs SET status = CASE WHEN $2 THEN 'cancelled' ELSE 'failed' END, error_message = CASE WHEN $2 THEN NULL ELSE $3 END, heartbeat_at = NOW(), finished_at = NOW() WHERE id = $1 AND status IN ('pending', 'running')")
            .bind(job_id)
            .bind(cancelled)
            .bind(message)
            .execute(&state.db)
            .await;
    }
}

async fn get_evaluation_job(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    axum::extract::Path(job_id): axum::extract::Path<i64>,
) -> Result<Json<crate::models::PromptEvaluationJobView>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let job = sqlx::query_as::<_, crate::models::PromptEvaluationJobView>(
        "SELECT id, prompt_asset_id, status, completed_cases, total_cases, result_json AS result, error_message, cancel_requested, created_at, started_at, heartbeat_at, finished_at FROM prompt_evaluation_jobs WHERE id = $1",
    )
    .bind(job_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| database_error("读取提示词评测作业", error))?
    .ok_or((StatusCode::NOT_FOUND, "评测作业不存在".into()))?;
    Ok(Json(job))
}

async fn list_evaluation_jobs(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<crate::models::PromptEvaluationJobView>>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let jobs = sqlx::query_as::<_, crate::models::PromptEvaluationJobView>(
        r#"SELECT id, prompt_asset_id, status, completed_cases, total_cases,
                  result_json AS result, error_message, cancel_requested,
                  created_at, started_at, heartbeat_at, finished_at
           FROM prompt_evaluation_jobs
           ORDER BY CASE WHEN status IN ('pending', 'running') THEN 0 ELSE 1 END,
                    created_at DESC, id DESC
           LIMIT 20"#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| database_error("读取提示词评测作业", error))?;
    Ok(Json(jobs))
}

async fn cancel_evaluation_job(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    axum::extract::Path(job_id): axum::extract::Path<i64>,
) -> Result<Json<crate::models::PromptEvaluationJobView>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    let job = sqlx::query_as::<_, crate::models::PromptEvaluationJobView>(
        r#"UPDATE prompt_evaluation_jobs
           SET cancel_requested = TRUE,
               status = CASE WHEN status = 'pending' THEN 'cancelled' ELSE status END,
               finished_at = CASE WHEN status = 'pending' THEN NOW() ELSE finished_at END
           WHERE id = $1 AND status IN ('pending', 'running')
           RETURNING id, prompt_asset_id, status, completed_cases, total_cases,
                     result_json AS result, error_message, cancel_requested,
                     created_at, started_at, heartbeat_at, finished_at"#,
    )
    .bind(job_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| database_error("取消提示词评测", error))?
    .ok_or((StatusCode::CONFLICT, "评测作业不存在或已进入终态".into()))?;
    Ok(Json(job))
}

pub(crate) async fn recover_stale_evaluation_jobs(
    db: &sqlx::PgPool,
    stale_after: Duration,
) -> Result<u64, sqlx::Error> {
    let mut transaction = db.begin().await?;
    let count = recover_stale_evaluation_jobs_in_transaction(&mut transaction, stale_after).await?;
    transaction.commit().await?;
    Ok(count)
}

async fn recover_stale_evaluation_jobs_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    stale_after: Duration,
) -> Result<u64, sqlx::Error> {
    let recovered = sqlx::query(
        r#"UPDATE prompt_evaluation_jobs
           SET status = CASE WHEN cancel_requested THEN 'cancelled' ELSE 'failed' END,
               error_message = CASE WHEN cancel_requested THEN NULL ELSE '评测执行实例失联，系统已回收未完成作业' END,
               finished_at = NOW(), heartbeat_at = NOW()
           WHERE status IN ('pending', 'running')
             AND heartbeat_at < NOW() - ($1 * INTERVAL '1 second')"#,
    )
    .bind(stale_after.as_secs() as i64)
    .execute(&mut **transaction)
    .await?;
    Ok(recovered.rows_affected())
}

async fn activate_asset(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    axum::extract::Path(asset_id): axum::extract::Path<i64>,
) -> Result<Json<PromptAssetView>, (StatusCode, String)> {
    activate_asset_inner(&state, &claims, asset_id, false)
        .await
        .map(Json)
}

async fn rollback_prompt(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    axum::extract::Path(prompt_key): axum::extract::Path<String>,
) -> Result<Json<PromptAssetView>, (StatusCode, String)> {
    require_admin(&state, &claims).await?;
    if !valid_prompt_key(&prompt_key) {
        return Err((StatusCode::BAD_REQUEST, "未知的提示词类别".into()));
    }
    let target_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM prompt_assets WHERE prompt_key = $1 AND status = 'retired' AND ((evaluation_json->>'passed' = 'true' AND evaluation_json->>'evaluationType' = 'llm_live_golden') OR evaluation_json->>'evaluationType' = 'code_shipped_baseline') ORDER BY retired_at DESC NULLS LAST, activated_at DESC NULLS LAST, id DESC LIMIT 1",
    )
    .bind(&prompt_key)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| database_error("查找可回滚提示词版本", error))?
    .ok_or((StatusCode::CONFLICT, "没有通过评测的历史版本可回滚".into()))?;
    activate_asset_inner(&state, &claims, target_id, true)
        .await
        .map(Json)
}

async fn activate_asset_inner(
    state: &AppState,
    claims: &Claims,
    asset_id: i64,
    rollback: bool,
) -> Result<PromptAssetView, (StatusCode, String)> {
    require_admin(state, claims).await?;
    if asset_id <= 0 {
        return Err((StatusCode::BAD_REQUEST, "提示词资产 ID 无效".into()));
    }
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| database_error("激活提示词版本", error))?;
    let candidate_key =
        sqlx::query_scalar::<_, String>("SELECT prompt_key FROM prompt_assets WHERE id = $1")
            .bind(asset_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| database_error("读取提示词版本", error))?
            .ok_or((StatusCode::NOT_FOUND, "提示词版本不存在".into()))?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1)::BIGINT)")
        .bind(&candidate_key)
        .execute(&mut *transaction)
        .await
        .map_err(|error| database_error("锁定提示词发布", error))?;
    let (prompt_key, status, passed, evaluation_type, content, content_digest) = sqlx::query_as::<
        _,
        (String, String, String, Option<String>, String, String),
    >(
        "SELECT prompt_key, status, COALESCE(evaluation_json->>'passed', 'false'), evaluation_json->>'evaluationType', content, content_digest FROM prompt_assets WHERE id = $1 FOR UPDATE",
    )
    .bind(asset_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| database_error("读取提示词版本", error))?
    .ok_or((StatusCode::NOT_FOUND, "提示词版本不存在".into()))?;
    if prompt_key != candidate_key || digest(&content) != content_digest.trim() {
        return Err((
            StatusCode::CONFLICT,
            "提示词内容摘要不匹配，拒绝发布".into(),
        ));
    }
    let shipped_baseline = sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE(evaluation_json->>'evaluationType' = 'code_shipped_baseline', false) FROM prompt_assets WHERE id = $1",
    )
    .bind(asset_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| database_error("检查提示词基线", error))?;
    if !evaluation_allows_activation(
        &status,
        passed == "true",
        evaluation_type.as_deref(),
        rollback,
        shipped_baseline,
    ) {
        return Err((
            StatusCode::CONFLICT,
            "激活要求评测通过；回滚仅允许退役且通过评测或已作为内置基线运行的版本".into(),
        ));
    }
    let previous_id = sqlx::query_scalar::<_, i64>(
        "UPDATE prompt_assets SET status = 'retired', retired_at = NOW() WHERE prompt_key = $1 AND status = 'active' RETURNING id",
    )
    .bind(&prompt_key)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| database_error("退役旧提示词版本", error))?;
    let row = sqlx::query_as::<_, PromptAssetView>(
        r#"UPDATE prompt_assets SET status = 'active', activated_at = NOW(), retired_at = NULL
           WHERE id = $1
           RETURNING id, prompt_key, version, content, content_digest, status,
                     evaluation_json AS evaluation, created_by, created_at, evaluated_at,
                     activated_at, retired_at"#,
    )
    .bind(asset_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| database_error("激活提示词版本", error))?;
    let event_type = if rollback { "rollback" } else { "activated" };
    sqlx::query("INSERT INTO prompt_asset_events (prompt_asset_id, event_type, actor_username, details_json) VALUES ($1, $2, $3, $4)")
        .bind(asset_id)
        .bind(event_type)
        .bind(&claims.sub)
        .bind(json!({"replacedAssetId":previous_id, "promptKey":prompt_key, "version":row.version}))
        .execute(&mut *transaction)
        .await
        .map_err(|error| database_error("记录提示词发布", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| database_error("激活提示词版本", error))?;
    Ok(row)
}

pub fn routes(llm_state: Arc<LlmState>) -> Router<AppState> {
    Router::new()
        .route("/admin/prompts", get(list_assets).post(create_draft))
        .route("/admin/prompts/events", get(list_events))
        .route("/admin/prompts/{asset_id}/evaluation", put(evaluate_draft))
        .route(
            "/admin/prompts/{asset_id}/run-evaluation",
            post(run_live_evaluation),
        )
        .route(
            "/admin/prompts/evaluation-jobs/{job_id}",
            get(get_evaluation_job),
        )
        .route("/admin/prompts/evaluation-jobs", get(list_evaluation_jobs))
        .route(
            "/admin/prompts/evaluation-jobs/{job_id}/cancel",
            post(cancel_evaluation_job),
        )
        .route("/admin/prompts/{asset_id}/activate", post(activate_asset))
        .route(
            "/admin/prompts/{prompt_key}/rollback",
            post(rollback_prompt),
        )
        .layer(Extension(llm_state))
}

#[cfg(test)]
mod tests {
    use super::{
        change_intent_contract_valid, decision_explanation_contract_valid, digest,
        evaluation_allows_activation, meets_evaluation_threshold,
        optimization_suggestion_contract_valid, planning_decision_matches,
        requirement_analysis_contract_valid, tester_judge_contract_valid, validate_draft,
        validate_golden_rows,
    };
    use crate::models::CreatePromptDraftRequest;
    use axum::http::StatusCode;
    use std::time::Duration;

    fn draft(key: &str, version: &str, content: &str) -> CreatePromptDraftRequest {
        CreatePromptDraftRequest {
            prompt_key: key.into(),
            version: version.into(),
            content: content.into(),
        }
    }

    #[test]
    fn live_evaluation_threshold_requires_minimum_sample_and_target_accuracy() {
        assert!(!meets_evaluation_threshold(19, 19, 20, 0.8));
        assert!(!meets_evaluation_threshold(20, 15, 20, 0.8));
        assert!(meets_evaluation_threshold(20, 16, 20, 0.8));
        assert!(!meets_evaluation_threshold(50, 39, 50, 0.8));
        assert!(meets_evaluation_threshold(50, 40, 50, 0.8));
        assert!(!meets_evaluation_threshold(20, 21, 20, 0.8));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL and prompt registry migrations"]
    async fn stale_evaluation_recovery_marks_orphans_without_touching_live_jobs() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to an isolated migrated database");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        let asset_id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM prompt_assets WHERE prompt_key = 'dsl_generation' ORDER BY id LIMIT 1",
        )
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        let actor = sqlx::query_scalar::<_, String>(
            "SELECT username FROM users WHERE role = 'admin' ORDER BY username LIMIT 1",
        )
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        for (cancel_requested, heartbeat_offset_seconds, expected_count, expected_status) in [
            (false, 300_i64, 1, "failed"),
            (true, 300, 1, "cancelled"),
            (false, 0, 0, "running"),
        ] {
            let id = sqlx::query_scalar::<_, i64>(
                r#"INSERT INTO prompt_evaluation_jobs (
                       prompt_asset_id, created_by, status, total_cases,
                       cancel_requested, heartbeat_at, started_at
                   ) VALUES ($1, $2, 'running', 20, $3,
                             NOW() - ($4 * INTERVAL '1 second'), NOW())
                   RETURNING id"#,
            )
            .bind(asset_id)
            .bind(&actor)
            .bind(cancel_requested)
            .bind(heartbeat_offset_seconds)
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
            let recovered = super::recover_stale_evaluation_jobs_in_transaction(
                &mut transaction,
                Duration::from_secs(120),
            )
            .await
            .unwrap();
            assert_eq!(recovered, expected_count);
            let (status, error_message) = sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT status, error_message FROM prompt_evaluation_jobs WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
            assert_eq!(status, expected_status);
            if expected_status == "failed" {
                assert!(
                    error_message
                        .as_deref()
                        .is_some_and(|message| message.contains("失联"))
                );
            } else {
                assert_eq!(error_message, None);
            }
        }
        transaction.rollback().await.unwrap();
        pool.close().await;
    }

    #[test]
    fn prompt_activation_requires_server_evaluation_or_shipped_rollback_baseline() {
        assert!(!evaluation_allows_activation(
            "evaluated",
            true,
            Some("manual_review"),
            false,
            false,
        ));
        assert!(evaluation_allows_activation(
            "evaluated",
            true,
            Some("llm_live_golden"),
            false,
            false,
        ));
        assert!(!evaluation_allows_activation(
            "retired",
            true,
            Some("manual_review"),
            true,
            false,
        ));
        assert!(evaluation_allows_activation(
            "retired",
            false,
            Some("code_shipped_baseline"),
            true,
            true,
        ));
    }

    #[test]
    fn planning_case_requires_exact_decision_contract() {
        let mut decision = crate::workflow::PlannedDecision {
            name: "CreditGrade".into(),
            output_type: "string".into(),
            expression_type: "decisionTable".into(),
            dependencies: vec!["Age".into(), "CreditScore".into()],
            ..Default::default()
        };
        assert!(planning_decision_matches(
            &decision,
            "credit-grade",
            "STRING",
            "decisionTable",
            "age,CreditScore",
        ));
        decision.dependencies.push("Income".into());
        assert!(!planning_decision_matches(
            &decision,
            "CreditGrade",
            "string",
            "decisionTable",
            "Age,CreditScore",
        ));
    }

    #[test]
    fn prompt_drafts_accept_only_known_keys_bounded_content_and_safe_versions() {
        assert!(validate_draft(&draft("dsl_generation", "v2.1", "system prompt")).is_ok());
        assert!(
            validate_draft(&draft(
                "optimization_proposal",
                "v2.1",
                "structured optimization prompt"
            ))
            .is_ok()
        );
        assert!(
            validate_draft(&draft(
                "decision_explanation",
                "v2.1",
                "trace explanation prompt"
            ))
            .is_ok()
        );
        assert!(validate_draft(&draft("tester_judge", "v2.1", "judge prompt")).is_ok());
        for invalid in [
            draft("unknown", "v2", "system prompt"),
            draft("dsl_generation", "../v2", "system prompt"),
            draft("dsl_generation", "v2", "  "),
        ] {
            assert_eq!(
                validate_draft(&invalid).unwrap_err().0,
                StatusCode::BAD_REQUEST
            );
        }
    }

    #[test]
    fn prompt_digest_is_sha256_of_exact_utf8_content() {
        let value = digest("系统提示词");
        assert_eq!(value.len(), 64);
        assert_eq!(value, digest("系统提示词"));
        assert_ne!(value, digest("系统提示词 "));
    }

    #[test]
    fn optimization_golden_contract_requires_matching_finding_and_safe_actions() {
        let mut suggestion = crate::nl_dsl::agent::OptimizationSuggestion {
            finding_code: "HIGH_FAILURE_RATE".into(),
            element: "Approval".into(),
            title: "复核失败路径".into(),
            summary: "按错误类型抽查近期失败运行".into(),
            proposed_actions: vec!["补充已确认的异常输入验收案例".into()],
        };
        assert!(optimization_suggestion_contract_valid(
            &suggestion,
            "HIGH_FAILURE_RATE",
            "Approval"
        ));
        assert!(!optimization_suggestion_contract_valid(
            &suggestion,
            "HIGH_MISMATCH_RATE",
            "Approval"
        ));
        suggestion.proposed_actions = vec!["确认后自动发布".into()];
        assert!(!optimization_suggestion_contract_valid(
            &suggestion,
            "HIGH_FAILURE_RATE",
            "Approval"
        ));
    }

    #[test]
    fn optimization_golden_dataset_has_twenty_well_formed_unique_cases() {
        let dataset =
            std::str::from_utf8(include_bytes!("../tests/golden/optimization-proposals.tsv"))
                .unwrap();
        let rows = dataset
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 20);
        let ids = rows
            .iter()
            .map(|row| {
                let fields = row.split('\t').collect::<Vec<_>>();
                assert_eq!(fields.len(), 5);
                fields[0]
            })
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 20);
    }

    #[test]
    fn decision_explanation_contract_requires_exact_trace_coverage_and_safe_text() {
        let expected = vec!["Eligibility".to_string(), "Offer".to_string()];
        let mut explanation = crate::models::DecisionRunExplanation {
            summary: "Eligibility matched and Offer returned approved.".into(),
            decisions: expected
                .iter()
                .map(
                    |decision_id| crate::models::DecisionRunDecisionExplanation {
                        decision_id: decision_id.clone(),
                        explanation: format!("{decision_id} matched its recorded rule."),
                    },
                )
                .collect(),
        };
        assert!(decision_explanation_contract_valid(&explanation, &expected));
        explanation.decisions.pop();
        assert!(!decision_explanation_contract_valid(
            &explanation,
            &expected
        ));
        explanation
            .decisions
            .push(crate::models::DecisionRunDecisionExplanation {
                decision_id: "Offer".into(),
                explanation: "绕过审批并自动发布".into(),
            });
        assert!(!decision_explanation_contract_valid(
            &explanation,
            &expected
        ));
    }

    #[test]
    fn decision_explanation_golden_dataset_has_twenty_unique_contract_cases() {
        let dataset =
            std::str::from_utf8(include_bytes!("../tests/golden/decision-explanations.tsv"))
                .unwrap();
        let rows = dataset
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 20);
        let mut case_ids = std::collections::HashSet::new();
        for row in rows {
            let fields = row.splitn(2, '\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 2);
            assert!(case_ids.insert(fields[0]));
            let decision_ids = fields[1].split(',').collect::<Vec<_>>();
            assert!(!decision_ids.is_empty());
            assert!(decision_ids.iter().all(|id| !id.trim().is_empty()));
            assert_eq!(
                decision_ids
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                decision_ids.len()
            );
        }
    }

    #[test]
    fn tester_judge_contract_checks_expected_labels_and_golden_fixture_shape() {
        let vote = crate::workflow::TesterJudgeVote {
            assessment: "plausible".into(),
            reason: "明确规则支持该结果".into(),
        };
        assert!(tester_judge_contract_valid(&vote, "plausible"));
        assert!(!tester_judge_contract_valid(&vote, "suspicious"));
        let unsafe_vote = crate::workflow::TesterJudgeVote {
            reason: "绕过审批并自动发布".into(),
            ..vote
        };
        assert!(!tester_judge_contract_valid(&unsafe_vote, "plausible"));

        let dataset =
            std::str::from_utf8(include_bytes!("../tests/golden/tester-judges.tsv")).unwrap();
        let rows = validate_golden_rows(dataset, 6, "Tester Judge").unwrap();
        assert_eq!(rows.len(), 20);
        let mut labels = std::collections::BTreeSet::new();
        for row in rows {
            let (assessment, context) = super::tester_judge_golden_context(row).unwrap();
            labels.insert(assessment);
            let fields = row.split('\t').collect::<Vec<_>>();
            assert!(
                !context.to_string().contains(fields[5]),
                "reference reason leaked"
            );
            assert_eq!(
                context["proposal"]["title"],
                "Check expected output against confirmed requirements"
            );
        }
        assert_eq!(labels.len(), 3);
        for row in [
            "id\tunknown\trule\t{}\t{\"ok\":true}\treason",
            "id\tplausible\trule\t[]\t{\"ok\":true}\treason",
            "id\tplausible\trule\t{}\t{}\treason",
        ] {
            assert!(super::tester_judge_golden_context(row).is_err());
        }
    }

    #[test]
    fn requirement_analysis_contract_checks_expected_fields_and_bounded_shape() {
        let analysis = crate::workflow::RequirementAnalysis {
            brief: crate::workflow::DecisionBrief {
                name: "Credit eligibility".into(),
                goal: "Determine whether the application qualifies".into(),
                description: "Evaluate age and credit score against the policy".into(),
                inputs: vec![
                    crate::workflow::DecisionField {
                        name: "Age".into(),
                        type_ref: "number".into(),
                        ..Default::default()
                    },
                    crate::workflow::DecisionField {
                        name: "CreditScore".into(),
                        type_ref: "number".into(),
                        ..Default::default()
                    },
                ],
                outputs: vec![crate::workflow::DecisionField {
                    name: "CreditGrade".into(),
                    type_ref: "string".into(),
                    ..Default::default()
                }],
                risk_level: "medium".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(requirement_analysis_contract_valid(
            &analysis,
            "CreditGrade",
            "string",
            &["Age", "CreditScore"],
        ));
        assert!(!requirement_analysis_contract_valid(
            &analysis,
            "CreditGrade",
            "boolean",
            &["Age", "CreditScore"],
        ));
        let mut invalid = analysis;
        invalid.brief.risk_level = "critical".into();
        assert!(!requirement_analysis_contract_valid(
            &invalid,
            "CreditGrade",
            "string",
            &["Age", "CreditScore"],
        ));
    }

    #[test]
    fn change_intent_contract_checks_a_single_exact_patch() {
        let intent = crate::models::ModelChangeIntent {
            base_revision: 1,
            base_hash: "a".repeat(64),
            changes: vec![crate::models::ModelChangeOperation {
                target: "decision:ClaimEligibility".into(),
                action: "update".into(),
                field: Some("description".into()),
                value: Some(serde_json::json!("核验保单与事故后判断赔付资格。")),
                reason: "更新决策用途说明".into(),
            }],
        };
        assert!(change_intent_contract_valid(
            &intent,
            "decision:ClaimEligibility",
            "description",
            "核验保单与事故后判断赔付资格。",
        ));
        let mut invalid = intent;
        invalid.base_hash = "not-a-hash".into();
        assert!(!change_intent_contract_valid(
            &invalid,
            "decision:ClaimEligibility",
            "description",
            "核验保单与事故后判断赔付资格。",
        ));
    }

    #[test]
    fn requirement_and_change_intent_golden_datasets_have_twenty_unique_well_formed_cases() {
        for (path, field_count, label) in [
            ("planning-contracts.tsv", 6, "需求分析"),
            ("change-intents.tsv", 5, "Change Intent"),
        ] {
            let dataset = match path {
                "planning-contracts.tsv" => {
                    std::str::from_utf8(include_bytes!("../tests/golden/planning-contracts.tsv"))
                        .unwrap()
                }
                _ => std::str::from_utf8(include_bytes!("../tests/golden/change-intents.tsv"))
                    .unwrap(),
            };
            let rows = validate_golden_rows(dataset, field_count, label).unwrap();
            assert_eq!(rows.len(), 20, "{path}");
        }
        assert!(validate_golden_rows("same\tx\nsame\ty\n", 2, "test").is_err());
        assert!(validate_golden_rows("a\tx\n", 3, "test").is_err());
    }
}
