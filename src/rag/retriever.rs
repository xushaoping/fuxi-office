use std::collections::HashMap;

use serde_json::Value;
use sqlx::PgPool;

use super::config::EmbeddingConfig;

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct RetrievedChunk {
    pub id: i64,
    pub source_name: String,
    pub source_type: String,
    pub content: String,
    pub metadata: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchDiagnostics {
    pub keyword_results: Vec<RetrievedChunk>,
    pub vector_results: Vec<RetrievedChunk>,
    pub fused_results: Vec<RetrievedChunk>,
    pub vector_status: &'static str,
    pub minimum_vector_similarity: Option<f64>,
}

pub async fn has_vector_column<'e, E>(executor: E) -> bool
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid = to_regclass('public.knowledge_chunks') AND attname = 'embedding' AND NOT attisdropped)",
    )
    .fetch_one(executor)
    .await
    .unwrap_or(false)
}

pub async fn search(
    db: &PgPool,
    query: &str,
    source_ids: &[i64],
    top_k: usize,
) -> Result<Vec<RetrievedChunk>, sqlx::Error> {
    Ok(search_with_diagnostics(db, query, source_ids, top_k)
        .await?
        .fused_results)
}

pub async fn search_with_diagnostics(
    db: &PgPool,
    query: &str,
    source_ids: &[i64],
    top_k: usize,
) -> Result<SearchDiagnostics, sqlx::Error> {
    if query.trim().is_empty() || source_ids.is_empty() || top_k == 0 {
        return Ok(SearchDiagnostics {
            keyword_results: Vec::new(),
            vector_results: Vec::new(),
            fused_results: Vec::new(),
            vector_status: "not_run",
            minimum_vector_similarity: None,
        });
    }
    let limit = i64::try_from(top_k.min(100)).unwrap_or(100);
    let keyword_results = sqlx::query_as::<_, RetrievedChunk>(
        r#"
        SELECT chunk.id, source.name AS source_name,
               source.source_type, chunk.content, chunk.metadata,
               NULL::REAL AS similarity
        FROM knowledge_chunks chunk
        JOIN knowledge_sources source ON source.id = chunk.knowledge_source_id
        WHERE chunk.knowledge_source_id = ANY($1)
          AND to_tsvector('simple', chunk.content) @@ plainto_tsquery('simple', $2)
        ORDER BY ts_rank_cd(to_tsvector('simple', chunk.content), plainto_tsquery('simple', $2)) DESC,
                 chunk.id
        LIMIT $3
        "#,
    )
    .bind(source_ids)
    .bind(query)
    .bind(limit)
    .fetch_all(db)
    .await?;

    let config = EmbeddingConfig::from_env();
    let minimum_vector_similarity = config.enabled.then(|| config.min_vector_similarity());
    let (vector_results, vector_status) = if !config.enabled {
        (Vec::new(), "disabled")
    } else if !has_vector_column(db).await {
        (Vec::new(), "pgvector_unavailable")
    } else {
        match config
            .embed_texts(&EmbeddingConfig::http_client(), &[query.to_string()])
            .await
        {
            Ok(Some(mut embeddings)) => {
                let vector = vector_literal(&embeddings.remove(0));
                match sqlx::query_as::<_, RetrievedChunk>(
                    r#"
                    SELECT chunk.id, source.name AS source_name,
                           source.source_type, chunk.content, chunk.metadata,
                           (1.0 - (chunk.embedding <=> $2::vector))::REAL AS similarity
                    FROM knowledge_chunks chunk
                    JOIN knowledge_sources source ON source.id = chunk.knowledge_source_id
                    WHERE chunk.knowledge_source_id = ANY($1)
                      AND chunk.embedding IS NOT NULL
                      AND chunk.embedding_model = $4
                      AND 1.0 - (chunk.embedding <=> $2::vector) >= $5
                    ORDER BY chunk.embedding <=> $2::vector, chunk.id
                    LIMIT $3
                    "#,
                )
                .bind(source_ids)
                .bind(vector)
                .bind(limit)
                .bind(&config.model)
                .bind(config.min_vector_similarity())
                .fetch_all(db)
                .await
                {
                    Ok(rows) if rows.is_empty() => (rows, "no_matches"),
                    Ok(rows) => (rows, "ready"),
                    Err(error) => {
                        eprintln!("向量检索失败，继续使用关键词检索：{error}");
                        (Vec::new(), "query_error")
                    }
                }
            }
            Ok(None) => (Vec::new(), "provider_error"),
            Err(error) => {
                eprintln!("查询 Embedding 失败，继续使用关键词检索：{error}");
                (Vec::new(), "provider_error")
            }
        }
    };

    let fused_results = rrf_fusion(keyword_results.clone(), vector_results.clone(), top_k);
    Ok(SearchDiagnostics {
        keyword_results,
        vector_results,
        fused_results,
        vector_status,
        minimum_vector_similarity,
    })
}

pub(crate) fn vector_literal(values: &[f32]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn rrf_fusion(
    keyword_results: Vec<RetrievedChunk>,
    vector_results: Vec<RetrievedChunk>,
    top_k: usize,
) -> Vec<RetrievedChunk> {
    let mut ranked = HashMap::<i64, (RetrievedChunk, f64)>::new();
    for (results, weight) in [(keyword_results, 1.0), (vector_results, 1.0)] {
        for (index, chunk) in results.into_iter().enumerate() {
            let score = weight / (60.0 + (index + 1) as f64);
            ranked
                .entry(chunk.id)
                .and_modify(|(current_chunk, current)| {
                    *current += score;
                    if current_chunk.similarity.is_none() {
                        current_chunk.similarity = chunk.similarity;
                    }
                })
                .or_insert((chunk, score));
        }
    }
    let mut results = ranked.into_values().collect::<Vec<_>>();
    results.sort_by(|(left_chunk, left_score), (right_chunk, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left_chunk.id.cmp(&right_chunk.id))
    });
    results
        .into_iter()
        .take(top_k)
        .map(|(chunk, _)| chunk)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{RetrievedChunk, rrf_fusion, vector_literal};

    fn chunk(id: i64) -> RetrievedChunk {
        RetrievedChunk {
            id,
            source_name: "Policy".into(),
            source_type: "policy".into(),
            content: format!("chunk {id}"),
            metadata: json!({}),
            similarity: None,
        }
    }

    #[test]
    fn rrf_fusion_rewards_documents_present_in_both_rankings() {
        let ranked = rrf_fusion(vec![chunk(1), chunk(2)], vec![chunk(2), chunk(3)], 3);
        assert_eq!(
            ranked.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
    }

    #[test]
    fn rrf_fusion_retains_vector_similarity_for_shared_results() {
        let mut vector_chunk = chunk(2);
        vector_chunk.similarity = Some(0.73);
        let ranked = rrf_fusion(vec![chunk(1), chunk(2)], vec![vector_chunk], 2);
        assert_eq!(ranked[0].id, 2);
        assert_eq!(ranked[0].similarity, Some(0.73));
        assert_eq!(ranked[1].similarity, None);
    }

    #[test]
    fn vector_literal_uses_postgres_vector_syntax() {
        assert_eq!(vector_literal(&[0.25, -1.0]), "[0.25,-1]");
    }
}
