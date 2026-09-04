use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde_json::{Value, json};

use crate::models::{KnowledgeCitation, KnowledgeSource, SaveKnowledgeSource};
use crate::types::{AppState, Claims};

#[derive(Debug)]
struct KnowledgeApiError {
    status: StatusCode,
    message: String,
}

impl KnowledgeApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "知识源不存在".into(),
        }
    }

    fn database(operation: &str, error: sqlx::Error) -> Self {
        eprintln!("数据库操作失败 [{operation}]: {error}");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("{operation}失败：{error}"),
        }
    }
}

impl IntoResponse for KnowledgeApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "ok": false, "error": self.message })),
        )
            .into_response()
    }
}

fn validate_source(payload: &SaveKnowledgeSource) -> Result<(), KnowledgeApiError> {
    if !matches!(payload.source_type.as_str(), "term" | "field" | "policy") {
        return Err(KnowledgeApiError::bad_request(
            "知识类型必须是 term、field 或 policy",
        ));
    }
    if payload.name.trim().is_empty() || payload.name.chars().count() > 160 {
        return Err(KnowledgeApiError::bad_request(
            "知识名称不能为空且不能超过 160 个字符",
        ));
    }
    if payload.content.trim().is_empty() || payload.content.len() > 50_000 {
        return Err(KnowledgeApiError::bad_request(
            "知识内容不能为空且不能超过 50000 字节",
        ));
    }
    if payload.description.len() > 4_000 || payload.version_label.chars().count() > 80 {
        return Err(KnowledgeApiError::bad_request("知识描述或版本标签过长"));
    }
    if payload.source_uri.len() > 2_000 {
        return Err(KnowledgeApiError::bad_request("来源地址不能超过 2000 字节"));
    }
    if !matches!(payload.status.as_str(), "active" | "archived") {
        return Err(KnowledgeApiError::bad_request(
            "知识状态必须是 active 或 archived",
        ));
    }
    if payload
        .valid_from
        .zip(payload.valid_until)
        .is_some_and(|(from, until)| until < from)
    {
        return Err(KnowledgeApiError::bad_request(
            "有效期结束日期不能早于开始日期",
        ));
    }
    Ok(())
}

const SOURCE_SELECT: &str = r#"
    SELECT knowledge_sources.id, knowledge_sources.owner_username,
           knowledge_sources.source_type, knowledge_sources.name,
           knowledge_sources.description, knowledge_sources.content,
           knowledge_sources.version_label, knowledge_sources.source_uri,
           knowledge_sources.valid_from, knowledge_sources.valid_until,
           knowledge_sources.status,
           knowledge_sources.status = 'active'
             AND (knowledge_sources.valid_from IS NULL OR knowledge_sources.valid_from <= CURRENT_DATE)
             AND (knowledge_sources.valid_until IS NULL OR knowledge_sources.valid_until >= CURRENT_DATE) AS is_effective,
           knowledge_sources.created_at, knowledge_sources.updated_at
    FROM knowledge_sources
"#;

async fn audit_knowledge(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    source_id: Option<i64>,
    owner: &str,
    event_type: &str,
    summary: &str,
    details: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO knowledge_audit_events (knowledge_source_id, owner_username, event_type, summary, details_json) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(source_id).bind(owner).bind(event_type).bind(summary).bind(details)
    .execute(executor).await.map(|_| ())
}

async fn audit_project(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    project_id: i64,
    actor: &str,
    event_type: &str,
    summary: &str,
    details: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (project_id, event_type, actor_username, summary, details_json) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(project_id).bind(event_type).bind(actor).bind(summary).bind(details)
    .execute(executor).await.map(|_| ())
}

async fn list_sources(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<KnowledgeSource>>, KnowledgeApiError> {
    let query =
        format!("{SOURCE_SELECT} WHERE owner_username = $1 ORDER BY updated_at DESC, id DESC");
    let sources = sqlx::query_as::<_, KnowledgeSource>(&query)
        .bind(&claims.sub)
        .fetch_all(&state.db)
        .await
        .map_err(|error| KnowledgeApiError::database("读取企业知识库", error))?;
    Ok(Json(sources))
}

async fn create_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<SaveKnowledgeSource>,
) -> Result<(StatusCode, Json<KnowledgeSource>), KnowledgeApiError> {
    validate_source(&payload)?;
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|error| KnowledgeApiError::database("创建知识源", error))?;
    let source = sqlx::query_as::<_, KnowledgeSource>(
        r#"
        INSERT INTO knowledge_sources (
            owner_username, source_type, name, description, content, version_label,
            source_uri, valid_from, valid_until, status
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        RETURNING id, owner_username, source_type, name, description, content,
                  version_label, source_uri, valid_from, valid_until, status,
                  status = 'active' AND (valid_from IS NULL OR valid_from <= CURRENT_DATE)
                    AND (valid_until IS NULL OR valid_until >= CURRENT_DATE) AS is_effective,
                  created_at, updated_at
        "#,
    )
    .bind(&claims.sub)
    .bind(&payload.source_type)
    .bind(payload.name.trim())
    .bind(payload.description.trim())
    .bind(payload.content.trim())
    .bind(payload.version_label.trim())
    .bind(payload.source_uri.trim())
    .bind(payload.valid_from)
    .bind(payload.valid_until)
    .bind(&payload.status)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| KnowledgeApiError::database("创建知识源", error))?;
    audit_knowledge(&mut *tx, Some(source.id), &claims.sub, "knowledge.created", "创建企业知识源",
        json!({"name": source.name, "sourceType": source.source_type, "version": source.version_label}))
        .await.map_err(|error| KnowledgeApiError::database("记录知识审计", error))?;
    tx.commit()
        .await
        .map_err(|error| KnowledgeApiError::database("提交知识源", error))?;
    Ok((StatusCode::CREATED, Json(source)))
}

async fn update_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(source_id): Path<i64>,
    Json(payload): Json<SaveKnowledgeSource>,
) -> Result<Json<KnowledgeSource>, KnowledgeApiError> {
    validate_source(&payload)?;
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|error| KnowledgeApiError::database("更新知识源", error))?;
    let source = sqlx::query_as::<_, KnowledgeSource>(
        r#"
        UPDATE knowledge_sources SET source_type=$1, name=$2, description=$3, content=$4,
            version_label=$5, source_uri=$6, valid_from=$7, valid_until=$8, status=$9, updated_at=NOW()
        WHERE id=$10 AND owner_username=$11
        RETURNING id, owner_username, source_type, name, description, content,
                  version_label, source_uri, valid_from, valid_until, status,
                  status = 'active' AND (valid_from IS NULL OR valid_from <= CURRENT_DATE)
                    AND (valid_until IS NULL OR valid_until >= CURRENT_DATE) AS is_effective,
                  created_at, updated_at
        "#,
    )
    .bind(&payload.source_type).bind(payload.name.trim()).bind(payload.description.trim())
    .bind(payload.content.trim()).bind(payload.version_label.trim()).bind(payload.source_uri.trim())
    .bind(payload.valid_from).bind(payload.valid_until).bind(&payload.status).bind(source_id).bind(&claims.sub)
    .fetch_optional(&mut *tx).await.map_err(|error| KnowledgeApiError::database("更新知识源", error))?
    .ok_or_else(KnowledgeApiError::not_found)?;
    audit_knowledge(
        &mut *tx,
        Some(source.id),
        &claims.sub,
        "knowledge.updated",
        "更新企业知识源",
        json!({"name": source.name, "status": source.status, "version": source.version_label}),
    )
    .await
    .map_err(|error| KnowledgeApiError::database("记录知识审计", error))?;
    tx.commit()
        .await
        .map_err(|error| KnowledgeApiError::database("提交知识源更新", error))?;
    Ok(Json(source))
}

async fn archive_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(source_id): Path<i64>,
) -> Result<StatusCode, KnowledgeApiError> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|error| KnowledgeApiError::database("归档知识源", error))?;
    let name = sqlx::query_scalar::<_, String>(
        "UPDATE knowledge_sources SET status='archived', updated_at=NOW() WHERE id=$1 AND owner_username=$2 RETURNING name",
    ).bind(source_id).bind(&claims.sub).fetch_optional(&mut *tx).await
        .map_err(|error| KnowledgeApiError::database("归档知识源", error))?.ok_or_else(KnowledgeApiError::not_found)?;
    audit_knowledge(
        &mut *tx,
        Some(source_id),
        &claims.sub,
        "knowledge.archived",
        "归档企业知识源",
        json!({"name": name}),
    )
    .await
    .map_err(|error| KnowledgeApiError::database("记录知识审计", error))?;
    tx.commit()
        .await
        .map_err(|error| KnowledgeApiError::database("提交知识归档", error))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_project_sources(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<KnowledgeSource>>, KnowledgeApiError> {
    let query = format!(
        r#"{SOURCE_SELECT}
        JOIN project_knowledge_sources link ON link.knowledge_source_id = knowledge_sources.id
        JOIN decision_projects project ON project.id = link.project_id
        WHERE link.project_id=$1 AND project.owner_username=$2 AND knowledge_sources.owner_username=$2
        ORDER BY knowledge_sources.updated_at DESC, knowledge_sources.id DESC"#
    );
    let sources = sqlx::query_as::<_, KnowledgeSource>(&query)
        .bind(project_id)
        .bind(&claims.sub)
        .fetch_all(&state.db)
        .await
        .map_err(|error| KnowledgeApiError::database("读取项目知识源", error))?;
    Ok(Json(sources))
}

async fn attach_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, source_id)): Path<(i64, i64)>,
) -> Result<StatusCode, KnowledgeApiError> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|error| KnowledgeApiError::database("绑定知识源", error))?;
    let source_name = sqlx::query_scalar::<_, String>(
        r#"
        SELECT source.name FROM knowledge_sources source
        JOIN decision_projects project ON project.id=$1
        WHERE source.id=$2 AND source.owner_username=$3 AND project.owner_username=$3
    "#,
    )
    .bind(project_id)
    .bind(source_id)
    .bind(&claims.sub)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| KnowledgeApiError::database("确认知识源权限", error))?
    .ok_or_else(KnowledgeApiError::not_found)?;
    sqlx::query("INSERT INTO project_knowledge_sources (project_id, knowledge_source_id, attached_by) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(project_id).bind(source_id).bind(&claims.sub).execute(&mut *tx).await
        .map_err(|error| KnowledgeApiError::database("绑定知识源", error))?;
    audit_project(
        &mut *tx,
        project_id,
        &claims.sub,
        "knowledge.attached",
        "绑定项目知识源",
        json!({"sourceId":source_id,"name":source_name}),
    )
    .await
    .map_err(|error| KnowledgeApiError::database("记录项目知识审计", error))?;
    tx.commit()
        .await
        .map_err(|error| KnowledgeApiError::database("提交知识绑定", error))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path((project_id, source_id)): Path<(i64, i64)>,
) -> Result<StatusCode, KnowledgeApiError> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|error| KnowledgeApiError::database("解绑知识源", error))?;
    let deleted = sqlx::query(r#"
        DELETE FROM project_knowledge_sources link USING decision_projects project, knowledge_sources source
        WHERE link.project_id=$1 AND link.knowledge_source_id=$2 AND project.id=link.project_id
          AND source.id=link.knowledge_source_id AND project.owner_username=$3 AND source.owner_username=$3
    "#).bind(project_id).bind(source_id).bind(&claims.sub).execute(&mut *tx).await
        .map_err(|error| KnowledgeApiError::database("解绑知识源", error))?;
    if deleted.rows_affected() == 0 {
        return Err(KnowledgeApiError::not_found());
    }
    audit_project(
        &mut *tx,
        project_id,
        &claims.sub,
        "knowledge.detached",
        "解绑项目知识源",
        json!({"sourceId":source_id}),
    )
    .await
    .map_err(|error| KnowledgeApiError::database("记录项目知识审计", error))?;
    tx.commit()
        .await
        .map_err(|error| KnowledgeApiError::database("提交知识解绑", error))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn effective_project_sources(
    db: &sqlx::PgPool,
    project_id: i64,
    _username: &str,
) -> Result<Vec<KnowledgeSource>, sqlx::Error> {
    let query = format!(
        r#"{SOURCE_SELECT}
        JOIN project_knowledge_sources link ON link.knowledge_source_id=knowledge_sources.id
        JOIN decision_projects project ON project.id=link.project_id
        WHERE link.project_id=$1 AND knowledge_sources.owner_username=project.owner_username
          AND knowledge_sources.status='active'
          AND (knowledge_sources.valid_from IS NULL OR knowledge_sources.valid_from <= CURRENT_DATE)
          AND (knowledge_sources.valid_until IS NULL OR knowledge_sources.valid_until >= CURRENT_DATE)
        ORDER BY knowledge_sources.source_type, knowledge_sources.name, knowledge_sources.id"#
    );
    sqlx::query_as::<_, KnowledgeSource>(&query)
        .bind(project_id)
        .fetch_all(db)
        .await
}

pub(crate) fn knowledge_context(sources: &[KnowledgeSource]) -> String {
    let mut output = String::new();
    for source in sources.iter().take(20) {
        let content = source.content.chars().take(6_000).collect::<String>();
        let section = format!(
            "### [{}] {} (版本: {}; 有效期: {} 至 {}; 来源: {})\n{}\n\n",
            source.source_type,
            source.name,
            if source.version_label.is_empty() {
                "未标注"
            } else {
                &source.version_label
            },
            source
                .valid_from
                .map(|date| date.to_string())
                .unwrap_or_else(|| "未限制".into()),
            source
                .valid_until
                .map(|date| date.to_string())
                .unwrap_or_else(|| "未限制".into()),
            if source.source_uri.is_empty() {
                "内部维护"
            } else {
                &source.source_uri
            },
            content,
        );
        let remaining = 30_000usize.saturating_sub(output.chars().count());
        if remaining == 0 {
            break;
        }
        output.extend(section.chars().take(remaining));
    }
    output
}

pub(crate) fn citations(sources: &[KnowledgeSource]) -> Vec<KnowledgeCitation> {
    sources
        .iter()
        .map(|source| KnowledgeCitation {
            source_id: source.id,
            source_type: source.source_type.clone(),
            name: source.name.clone(),
            version_label: source.version_label.clone(),
            source_uri: source.source_uri.clone(),
            valid_from: source.valid_from,
            valid_until: source.valid_until,
            source_updated_at: Some(source.updated_at),
        })
        .collect()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/knowledge-sources", get(list_sources).post(create_source))
        .route(
            "/knowledge-sources/{source_id}",
            put(update_source).delete(archive_source),
        )
        .route(
            "/projects/{project_id}/knowledge-sources",
            get(list_project_sources),
        )
        .route(
            "/projects/{project_id}/knowledge-sources/{source_id}",
            post(attach_source).delete(detach_source),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: i64, content: &str) -> KnowledgeSource {
        let now = chrono::Utc::now();
        KnowledgeSource {
            id,
            owner_username: "owner".into(),
            source_type: "policy".into(),
            name: format!("制度 {id}"),
            description: String::new(),
            content: content.into(),
            version_label: "2026.1".into(),
            source_uri: format!("https://example.test/{id}"),
            valid_from: None,
            valid_until: None,
            status: "active".into(),
            is_effective: true,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn validates_dates_and_required_fields() {
        let payload = SaveKnowledgeSource {
            source_type: "policy".into(),
            name: "授信制度".into(),
            description: String::new(),
            content: "信用分低于 600 分时拒绝".into(),
            version_label: "2026.1".into(),
            source_uri: String::new(),
            valid_from: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
            valid_until: Some(chrono::NaiveDate::from_ymd_opt(2025, 12, 31).unwrap()),
            status: "active".into(),
        };
        assert!(validate_source(&payload).is_err());
    }

    #[test]
    fn context_limits_source_and_total_size() {
        let sources = (1..=25)
            .map(|id| source(id, &"规则内容".repeat(3_000)))
            .collect::<Vec<_>>();
        let context = knowledge_context(&sources);
        assert!(context.chars().count() <= 30_000);
        assert!(context.contains("制度 1"));
        assert!(!context.contains("制度 21"));
    }

    #[test]
    fn citations_snapshot_metadata_without_content() {
        let snapshots = citations(&[source(7, "敏感制度正文")]);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].source_id, 7);
        let json = serde_json::to_string(&snapshots).unwrap();
        assert!(!json.contains("敏感制度正文"));
    }
}
