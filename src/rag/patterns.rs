use sqlx::PgPool;

use super::{config::EmbeddingConfig, retriever::vector_literal};

pub async fn has_vector_column<'e, E>(executor: E) -> bool
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid = to_regclass('public.decision_patterns') AND attname = 'embedding' AND NOT attisdropped)",
    )
    .fetch_one(executor)
    .await
    .unwrap_or(false)
}

pub async fn semantic_candidates(
    db: &PgPool,
    owner: &str,
    query: &str,
    config: &EmbeddingConfig,
) -> Vec<(i64, f64)> {
    if !config.enabled || !has_vector_column(db).await {
        return Vec::new();
    }
    let client = EmbeddingConfig::http_client();
    let embedding = match config.embed_texts(&client, &[query.to_string()]).await {
        Ok(Some(mut values)) => values.remove(0),
        Ok(None) => return Vec::new(),
        Err(error) => {
            eprintln!("模式向量检索失败，继续使用关键词语义检索：{error}");
            return Vec::new();
        }
    };
    let vector = vector_literal(&embedding);
    match sqlx::query_as::<_, (i64, f64)>(
        r#"
        SELECT pattern.id,
               GREATEST(0.0, LEAST(100.0, (1.0 - (pattern.embedding <=> $2::vector)) * 100.0))::FLOAT8
        FROM decision_patterns pattern
        JOIN decision_versions version ON version.id = pattern.source_version_id
        WHERE pattern.owner_username = $1
          AND version.status IN ('published', 'retired')
          AND pattern.embedding IS NOT NULL
          AND pattern.embedding_model = $3
        ORDER BY pattern.embedding <=> $2::vector, pattern.id
        LIMIT 200
        "#,
    )
    .bind(owner)
    .bind(vector)
    .bind(&config.model)
    .fetch_all(db)
    .await
    {
        Ok(ids) => ids,
        Err(error) => {
            eprintln!("模式向量数据库检索失败，继续使用关键词语义检索：{error}");
            Vec::new()
        }
    }
}

pub async fn backfill_embeddings(
    db: &PgPool,
    config: &EmbeddingConfig,
) -> Result<u64, sqlx::Error> {
    if !config.enabled || !has_vector_column(db).await || config.api_key.is_none() {
        return Ok(0);
    }
    let client = EmbeddingConfig::http_client();
    let mut updated_count = 0;
    loop {
        let rows = sqlx::query_as::<_, (i64, String)>(
            r#"
            SELECT id, concat_ws(E'\n', name, category, description, dsl_snippet)
            FROM decision_patterns
            WHERE embedding IS NULL OR embedding_model IS DISTINCT FROM $1
            ORDER BY id
            LIMIT 64
            "#,
        )
        .bind(&config.model)
        .fetch_all(db)
        .await?;
        if rows.is_empty() {
            break;
        }
        let texts = rows
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<Vec<_>>();
        let embeddings = match config.embed_texts(&client, &texts).await {
            Ok(Some(embeddings)) => embeddings,
            Ok(None) => break,
            Err(error) => {
                eprintln!("模式 Embedding 回填失败，保留待处理状态：{error}");
                break;
            }
        };
        let mut batch_updated = 0;
        for ((pattern_id, text), embedding) in rows.iter().zip(embeddings) {
            let result = sqlx::query(
                "UPDATE decision_patterns SET embedding = $1::vector, embedding_model = $2 WHERE id = $3 AND concat_ws(E'\\n', name, category, description, dsl_snippet) = $4 AND (embedding IS NULL OR embedding_model IS DISTINCT FROM $2)",
            )
            .bind(vector_literal(&embedding))
            .bind(&config.model)
            .bind(pattern_id)
            .bind(text)
            .execute(db)
            .await?;
            batch_updated += result.rows_affected();
        }
        updated_count += batch_updated;
        if batch_updated == 0 {
            break;
        }
    }
    Ok(updated_count)
}
