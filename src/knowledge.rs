use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde_json::{Value, json};

use crate::models::{
    KnowledgeCitation, KnowledgeIndexStatus, KnowledgeSource, KnowledgeSourceIndexStatus,
    SaveKnowledgeSource,
};
use crate::rag::{KnowledgeChunker, config::EmbeddingConfig};
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

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
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

async fn replace_source_chunks(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source_id: i64,
    chunks: &[crate::rag::chunker::Chunk],
    embeddings: Option<&[Vec<f32>]>,
    embedding_model: Option<&str>,
) -> Result<(), sqlx::Error> {
    let has_vector_column = crate::rag::retriever::has_vector_column(&mut **tx).await;
    sqlx::query("DELETE FROM knowledge_chunks WHERE knowledge_source_id = $1")
        .bind(source_id)
        .execute(&mut **tx)
        .await?;
    for (index, chunk) in chunks.iter().enumerate() {
        let embedding = embeddings.and_then(|items| items.get(index)).map(|values| {
            format!(
                "[{}]",
                values
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        });
        if has_vector_column {
            sqlx::query(
                "INSERT INTO knowledge_chunks (knowledge_source_id, chunk_index, content, token_count, metadata, embedding, embedding_model) VALUES ($1, $2, $3, $4, $5, $6::vector, $7)",
            )
            .bind(source_id)
            .bind(index as i32)
            .bind(&chunk.content)
            .bind(chunk.token_count as i32)
            .bind(&chunk.metadata)
            .bind(embedding)
            .bind(embedding_model)
            .execute(&mut **tx)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO knowledge_chunks (knowledge_source_id, chunk_index, content, token_count, metadata) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(source_id)
            .bind(index as i32)
            .bind(&chunk.content)
            .bind(chunk.token_count as i32)
            .bind(&chunk.metadata)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

async fn prepare_source_chunks(
    db: &sqlx::PgPool,
    content: &str,
) -> (
    Vec<crate::rag::chunker::Chunk>,
    Option<Vec<Vec<f32>>>,
    Option<String>,
) {
    let chunks = KnowledgeChunker::new(400, 50).chunk(content);
    let config = EmbeddingConfig::from_env();
    if !config.enabled || chunks.is_empty() {
        return (chunks, None, None);
    }
    if !crate::rag::retriever::has_vector_column(db).await {
        eprintln!("pgvector 不可用，知识源将仅使用关键词检索");
        return (chunks, None, None);
    }
    let texts = chunks
        .iter()
        .map(|chunk| chunk.content.clone())
        .collect::<Vec<_>>();
    match config
        .embed_texts(&EmbeddingConfig::http_client(), &texts)
        .await
    {
        Ok(Some(embeddings)) => (chunks, Some(embeddings), Some(config.model)),
        Ok(None) => (chunks, None, None),
        Err(error) => {
            eprintln!("Embedding 写入失败，知识源仍以关键词检索方式保存：{error}");
            (chunks, None, None)
        }
    }
}

pub(crate) async fn backfill_knowledge_chunks(db: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    let source_ids = sqlx::query_scalar::<_, i64>(
        "SELECT source.id FROM knowledge_sources source WHERE NOT EXISTS (SELECT 1 FROM knowledge_chunks chunk WHERE chunk.knowledge_source_id = source.id) ORDER BY source.id",
    )
    .fetch_all(db)
    .await?;
    let mut backfilled = 0;
    for source_id in source_ids {
        let mut tx = db.begin().await?;
        let content = sqlx::query_scalar::<_, String>(
            "SELECT content FROM knowledge_sources WHERE id = $1 FOR UPDATE",
        )
        .bind(source_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(content) = content {
            let has_chunks = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM knowledge_chunks WHERE knowledge_source_id = $1)",
            )
            .bind(source_id)
            .fetch_one(&mut *tx)
            .await?;
            if !has_chunks {
                let chunks = KnowledgeChunker::new(400, 50).chunk(&content);
                replace_source_chunks(&mut tx, source_id, &chunks, None, None).await?;
                backfilled += 1;
            }
        }
        tx.commit().await?;
    }
    let embedding_count =
        backfill_missing_chunk_embeddings(db, &EmbeddingConfig::from_env()).await?;
    if embedding_count > 0 {
        println!("已为 {embedding_count} 个知识分块补齐 Embedding 向量");
    }
    match crate::rag::patterns::backfill_embeddings(db, &EmbeddingConfig::from_env()).await {
        Ok(count) if count > 0 => println!("已为 {count} 个决策模式补齐 Embedding 向量"),
        Ok(_) => {}
        Err(error) => eprintln!("决策模式 Embedding 回填失败：{error}"),
    }
    Ok(backfilled)
}

pub(crate) async fn backfill_knowledge_chunks_daily(db: sqlx::PgPool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
    interval.tick().await;
    loop {
        interval.tick().await;
        match backfill_knowledge_chunks(&db).await {
            Ok(count) if count > 0 => println!("已为 {count} 个知识源补齐检索分块"),
            Ok(_) => {}
            Err(error) => eprintln!("每日知识检索回填失败：{error}"),
        }
    }
}

async fn backfill_missing_chunk_embeddings(
    db: &sqlx::PgPool,
    config: &EmbeddingConfig,
) -> Result<u64, sqlx::Error> {
    if !config.enabled {
        return Ok(0);
    }
    if !crate::rag::retriever::has_vector_column(db).await {
        eprintln!("pgvector 不可用，跳过知识分块向量回填并继续关键词检索");
        return Ok(0);
    }
    let source_ids = sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT knowledge_source_id FROM knowledge_chunks WHERE embedding IS NULL OR embedding_model IS DISTINCT FROM $1 ORDER BY knowledge_source_id",
    )
    .bind(&config.model)
    .fetch_all(db)
    .await?;
    if source_ids.is_empty() {
        return Ok(0);
    }
    if config.api_key.is_none() {
        eprintln!("EMBEDDING_ENABLED=true 但未配置 OPENAI_API_KEY，跳过已有知识分块的向量回填");
        return Ok(0);
    }

    let client = EmbeddingConfig::http_client();
    let mut updated_count = 0;
    for source_id in source_ids {
        loop {
            let chunks = sqlx::query_as::<_, (i64, String)>(
                "SELECT id, content FROM knowledge_chunks WHERE knowledge_source_id = $1 AND (embedding IS NULL OR embedding_model IS DISTINCT FROM $2) ORDER BY chunk_index LIMIT 64",
            )
            .bind(source_id)
            .bind(&config.model)
            .fetch_all(db)
            .await?;
            if chunks.is_empty() {
                break;
            }
            let texts = chunks
                .iter()
                .map(|(_, content)| content.clone())
                .collect::<Vec<_>>();
            let embeddings = match config.embed_texts(&client, &texts).await {
                Ok(Some(embeddings)) => embeddings,
                Ok(None) => break,
                Err(error) => {
                    eprintln!("知识源 {source_id} 的 Embedding 回填失败，保留关键词检索：{error}");
                    break;
                }
            };
            let mut batch_updated = 0;
            for ((chunk_id, content), embedding) in chunks.iter().zip(embeddings) {
                let vector = crate::rag::retriever::vector_literal(&embedding);
                let result = sqlx::query(
                    "UPDATE knowledge_chunks SET embedding = $1::vector, embedding_model = $2, updated_at = NOW() WHERE id = $3 AND content = $4 AND (embedding IS NULL OR embedding_model IS DISTINCT FROM $2)",
                )
                .bind(vector)
                .bind(&config.model)
                .bind(chunk_id)
                .bind(content)
                .execute(db)
                .await?;
                batch_updated += result.rows_affected();
            }
            updated_count += batch_updated;
            if batch_updated == 0 {
                break;
            }
        }
    }
    Ok(updated_count)
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

async fn knowledge_index_status(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<KnowledgeIndexStatus>, KnowledgeApiError> {
    let embedding = EmbeddingConfig::from_env();
    let vector_columns = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid = to_regclass('public.knowledge_chunks') AND attname = 'embedding' AND NOT attisdropped) AND EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid = to_regclass('public.knowledge_chunks') AND attname = 'embedding_model' AND NOT attisdropped)",
    )
    .fetch_one(&state.db)
    .await
    .map_err(|error| KnowledgeApiError::database("检查知识向量索引", error))?;

    let query = if vector_columns {
        r#"
        SELECT source.id AS source_id,
               COUNT(chunk.id)::BIGINT AS chunk_count,
               COUNT(chunk.id) FILTER (WHERE chunk.embedding IS NOT NULL)::BIGINT AS embedded_chunk_count,
               COUNT(chunk.id) FILTER (
                   WHERE chunk.embedding IS NOT NULL AND chunk.embedding_model = $2
               )::BIGINT AS current_model_chunk_count
        FROM knowledge_sources source
        LEFT JOIN knowledge_chunks chunk ON chunk.knowledge_source_id = source.id
        WHERE source.owner_username = $1
        GROUP BY source.id
        ORDER BY source.id
        "#
    } else {
        r#"
        SELECT source.id AS source_id,
               COUNT(chunk.id)::BIGINT AS chunk_count,
               0::BIGINT AS embedded_chunk_count,
               0::BIGINT AS current_model_chunk_count
        FROM knowledge_sources source
        LEFT JOIN knowledge_chunks chunk ON chunk.knowledge_source_id = source.id
        WHERE source.owner_username = $1
        GROUP BY source.id
        ORDER BY source.id
        "#
    };
    let mut request = sqlx::query_as::<_, KnowledgeSourceIndexStatus>(query).bind(&claims.sub);
    if vector_columns {
        request = request.bind(&embedding.model);
    }
    let sources = request
        .fetch_all(&state.db)
        .await
        .map_err(|error| KnowledgeApiError::database("读取知识索引状态", error))?;
    Ok(Json(KnowledgeIndexStatus {
        embedding_enabled: embedding.enabled,
        vector_available: vector_columns,
        current_model: embedding.model,
        sources,
    }))
}

async fn create_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<SaveKnowledgeSource>,
) -> Result<(StatusCode, Json<KnowledgeSource>), KnowledgeApiError> {
    validate_source(&payload)?;
    let (chunks, embeddings, embedding_model) =
        prepare_source_chunks(&state.db, payload.content.trim()).await;
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
    replace_source_chunks(
        &mut tx,
        source.id,
        &chunks,
        embeddings.as_deref(),
        embedding_model.as_deref(),
    )
    .await
    .map_err(|error| KnowledgeApiError::database("保存知识分块", error))?;
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
    let (chunks, embeddings, embedding_model) =
        prepare_source_chunks(&state.db, payload.content.trim()).await;
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
    replace_source_chunks(
        &mut tx,
        source.id,
        &chunks,
        embeddings.as_deref(),
        embedding_model.as_deref(),
    )
    .await
    .map_err(|error| KnowledgeApiError::database("更新知识分块", error))?;
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

async fn reindex_source(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(source_id): Path<i64>,
) -> Result<StatusCode, KnowledgeApiError> {
    let config = EmbeddingConfig::from_env();
    if !config.enabled {
        return Err(KnowledgeApiError::unavailable(
            "请先启用 Embedding 后再重建向量索引",
        ));
    }
    if !crate::rag::retriever::has_vector_column(&state.db).await {
        return Err(KnowledgeApiError::unavailable(
            "pgvector 索引不可用，无法重建向量",
        ));
    }
    let content = sqlx::query_scalar::<_, String>(
        "SELECT content FROM knowledge_sources WHERE id = $1 AND owner_username = $2",
    )
    .bind(source_id)
    .bind(&claims.sub)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| KnowledgeApiError::database("读取待重建知识源", error))?
    .ok_or_else(KnowledgeApiError::not_found)?;
    let chunks = KnowledgeChunker::new(400, 50).chunk(&content);
    if chunks.is_empty() {
        return Err(KnowledgeApiError::bad_request("知识源没有可建立索引的正文"));
    }
    let texts = chunks
        .iter()
        .map(|chunk| chunk.content.clone())
        .collect::<Vec<_>>();
    let embeddings = match config
        .embed_texts(&EmbeddingConfig::http_client(), &texts)
        .await
    {
        Ok(Some(embeddings)) => embeddings,
        Ok(None) => {
            return Err(KnowledgeApiError::unavailable("Embedding 当前未启用"));
        }
        Err(error) => {
            eprintln!("知识源 {source_id} 手动重建 Embedding 失败：{error}");
            return Err(KnowledgeApiError::unavailable(
                "Embedding 生成失败，请检查服务配置或稍后重试",
            ));
        }
    };

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|error| KnowledgeApiError::database("开始重建知识索引", error))?;
    let current_content = sqlx::query_scalar::<_, String>(
        "SELECT content FROM knowledge_sources WHERE id = $1 AND owner_username = $2 FOR UPDATE",
    )
    .bind(source_id)
    .bind(&claims.sub)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| KnowledgeApiError::database("确认知识源内容", error))?
    .ok_or_else(KnowledgeApiError::not_found)?;
    if current_content != content {
        return Err(KnowledgeApiError::conflict(
            "知识源在重建期间已更新，请重新发起索引重建",
        ));
    }
    replace_source_chunks(
        &mut tx,
        source_id,
        &chunks,
        Some(&embeddings),
        Some(&config.model),
    )
    .await
    .map_err(|error| KnowledgeApiError::database("保存重建知识索引", error))?;
    audit_knowledge(
        &mut *tx,
        Some(source_id),
        &claims.sub,
        "knowledge.reindexed",
        "重建企业知识源向量索引",
        json!({"chunkCount": chunks.len(), "embeddingModel": config.model}),
    )
    .await
    .map_err(|error| KnowledgeApiError::database("记录知识索引审计", error))?;
    tx.commit()
        .await
        .map_err(|error| KnowledgeApiError::database("提交知识索引重建", error))?;
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

pub(crate) async fn knowledge_context_for_query(
    db: &sqlx::PgPool,
    sources: &[KnowledgeSource],
    query: &str,
) -> String {
    let source_ids = sources.iter().map(|source| source.id).collect::<Vec<_>>();
    match crate::rag::retriever::search(db, query, &source_ids, 10).await {
        Ok(results) if !results.is_empty() => {
            let mut output = String::new();
            for chunk in results {
                let section = chunk
                    .metadata
                    .get("section")
                    .and_then(Value::as_str)
                    .filter(|section| !section.is_empty())
                    .map(|section| format!(" / {section}"))
                    .unwrap_or_default();
                let block = format!(
                    "### [{}] {}{}\n{}\n\n",
                    chunk.source_type, chunk.source_name, section, chunk.content
                );
                let remaining = 30_000usize.saturating_sub(output.chars().count());
                if remaining == 0 {
                    break;
                }
                output.extend(block.chars().take(remaining));
            }
            output
        }
        Ok(_) => String::new(),
        Err(error) => {
            eprintln!("知识分块检索失败，回退到知识源上下文：{error}");
            knowledge_context(sources)
        }
    }
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
            "/knowledge-sources/index-status",
            get(knowledge_index_status),
        )
        .route(
            "/knowledge-sources/{source_id}",
            put(update_source).delete(archive_source),
        )
        .route(
            "/knowledge-sources/{source_id}/reindex",
            post(reindex_source),
        )
        .route(
            "/projects/{project_id}/knowledge-sources",
            get(list_project_sources),
        )
        .route(
            "/projects/{project_id}/knowledge-sources/{source_id}",
            post(attach_source).delete(detach_source),
        )
        .route(
            "/projects/{project_id}/knowledge-search-debug",
            get(debug_project_search),
        )
}

#[derive(serde::Deserialize)]
struct SearchDebugQuery {
    q: String,
}

async fn debug_project_search(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<SearchDebugQuery>,
) -> Result<Json<crate::rag::retriever::SearchDiagnostics>, KnowledgeApiError> {
    if query.q.trim().is_empty() || query.q.len() > 2_000 {
        return Err(KnowledgeApiError::bad_request(
            "检索调试查询不能为空且不能超过 2000 字节",
        ));
    }
    let allowed = crate::project_access::has_permission(
        &state.db,
        project_id,
        &claims.sub,
        crate::project_access::ProjectPermission::View,
    )
    .await
    .map_err(|error| KnowledgeApiError::database("校验检索调试权限", error))?;
    if !allowed {
        return Err(KnowledgeApiError::not_found());
    }
    let sources = effective_project_sources(&state.db, project_id, &claims.sub)
        .await
        .map_err(|error| KnowledgeApiError::database("读取项目有效知识源", error))?;
    let source_ids = sources.iter().map(|source| source.id).collect::<Vec<_>>();
    let diagnostics =
        crate::rag::retriever::search_with_diagnostics(&state.db, &query.q, &source_ids, 10)
            .await
            .map_err(|error| KnowledgeApiError::database("执行知识检索诊断", error))?;
    Ok(Json(diagnostics))
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
