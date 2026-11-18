use super::*;

pub(super) const CURRENT_REVIEWS_SQL: &str = r#"SELECT review.id, review.explanation_revision, review.review_label,
                  review.reviewer_username, review.created_at
           FROM decision_run_explanation_reviews review
           JOIN decision_run_explanations explanation ON explanation.id = review.explanation_id
           JOIN decision_runs run ON run.id = explanation.decision_run_id
           WHERE run.project_id = $1 AND run.id = $2
             AND explanation.status = 'succeeded'
             AND explanation.generation_revision = $3
             AND review.explanation_revision = explanation.generation_revision
             AND ($4 = FALSE OR EXISTS (
                 SELECT 1 FROM decision_run_explanation_reviews own_review
                 WHERE own_review.explanation_id = explanation.id
                   AND own_review.explanation_revision = explanation.generation_revision
                   AND own_review.reviewer_username = $5
             ))
           ORDER BY (review.reviewer_username = $5) DESC, review.created_at DESC, review.id DESC
           LIMIT 50"#;

pub(super) const CURRENT_REVIEW_EXPORT_SQL: &str = r#"SELECT review.id, review.explanation_id, review.explanation_revision,
                  review.reviewer_username, review.review_label
           FROM decision_run_explanation_reviews review
           JOIN decision_run_explanations explanation ON explanation.id = review.explanation_id
           JOIN decision_runs run ON run.id = explanation.decision_run_id
           WHERE run.project_id = $1 AND review.id > $2
             AND explanation.status = 'succeeded'
             AND review.explanation_revision = explanation.generation_revision
             AND ($3 = FALSE OR EXISTS (
                 SELECT 1
                 FROM decision_run_explanation_reviews own_review
                 WHERE own_review.explanation_id = review.explanation_id
                   AND own_review.explanation_revision = review.explanation_revision
                   AND own_review.reviewer_username = $4
             ))
           ORDER BY review.id
           LIMIT $5"#;

pub(super) const UPSERT_ATTEMPT_SQL: &str = r#"INSERT INTO decision_run_explanations (
            decision_run_id, status, model, prompt_version, prompt_digest,
            trace_digest, created_by, agent_run_id
        ) VALUES ($1, 'running', $2, $3, $4, $5, $6, $7)
        ON CONFLICT (decision_run_id) DO UPDATE SET
            generation_revision = decision_run_explanations.generation_revision + 1,
            status = 'running', explanation_json = NULL, model = EXCLUDED.model,
            prompt_version = EXCLUDED.prompt_version, prompt_digest = EXCLUDED.prompt_digest,
            trace_digest = EXCLUDED.trace_digest, error_message = NULL,
            created_by = EXCLUDED.created_by, agent_run_id = EXCLUDED.agent_run_id,
            created_at = NOW(), updated_at = NOW(), completed_at = NULL
        RETURNING id"#;

pub(super) struct Attempt {
    pub project_id: i64,
    pub decision_run_id: i64,
    pub explanation_id: i64,
    pub agent_run_id: i64,
    pub orchestration_id: i64,
}

pub(super) async fn until_cancelled<T>(
    work: impl std::future::Future<Output = anyhow::Result<T>>,
    cancellation: impl std::future::Future<Output = anyhow::Result<()>>,
) -> anyhow::Result<T> {
    tokio::select! {
        biased;
        cancelled = cancellation => {
            cancelled?;
            Err(anyhow::anyhow!("决策解释编排已由用户取消"))
        }
        result = work => result,
    }
}

pub(super) async fn finish_in_transaction(
    connection: &mut sqlx::PgConnection,
    attempt: &Attempt,
    username: &str,
    usage: &Value,
    output: Result<Value, String>,
) -> Result<bool, ProjectApiError> {
    // Share the orchestration -> run -> step -> cache order with cancellation/recovery.
    let orchestration_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM agent_orchestrations WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(attempt.orchestration_id)
    .bind(attempt.project_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| ProjectApiError::database("锁定决策解释编排", error))?;
    let run_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM agent_runs WHERE id = $1 AND orchestration_id = $2 AND project_id = $3 AND stage = 'decision_explanation' FOR UPDATE",
    )
    .bind(attempt.agent_run_id)
    .bind(attempt.orchestration_id)
    .bind(attempt.project_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| ProjectApiError::database("锁定决策解释运行", error))?;
    if orchestration_status.is_none() || run_status.is_none() {
        return Ok(false);
    }
    sqlx::query(
        "SELECT id FROM agent_steps WHERE agent_run_id = $1 AND orchestration_id = $2 FOR UPDATE",
    )
    .bind(attempt.agent_run_id)
    .bind(attempt.orchestration_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| ProjectApiError::database("锁定决策解释步骤", error))?;
    let cache = sqlx::query_as::<_, (String, Option<i64>)>(
        "SELECT status, agent_run_id FROM decision_run_explanations WHERE id = $1 AND decision_run_id = $2 FOR UPDATE",
    )
    .bind(attempt.explanation_id)
    .bind(attempt.decision_run_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| ProjectApiError::database("锁定决策解释缓存", error))?;
    let owns_running_cache = cache
        .as_ref()
        .is_some_and(|(status, owner)| status == "running" && *owner == Some(attempt.agent_run_id));

    if orchestration_status.as_deref() != Some("running")
        || run_status.as_deref() != Some("running")
    {
        // Late results can add observed cost, but cannot change terminal outputs/errors.
        sqlx::query("UPDATE agent_runs SET usage_json = $1 WHERE id = $2")
            .bind(usage)
            .bind(attempt.agent_run_id)
            .execute(&mut *connection)
            .await
            .map_err(|error| ProjectApiError::database("补记终止解释运行用量", error))?;
        sqlx::query("UPDATE agent_steps SET usage_json = $1 WHERE agent_run_id = $2 AND orchestration_id = $3")
            .bind(usage).bind(attempt.agent_run_id).bind(attempt.orchestration_id)
            .execute(&mut *connection).await
            .map_err(|error| ProjectApiError::database("补记终止解释步骤用量", error))?;
        if owns_running_cache {
            sqlx::query("UPDATE decision_run_explanations SET status = 'failed', explanation_json = NULL, error_message = '解释编排已停止；可重新发起', updated_at = NOW(), completed_at = NOW() WHERE id = $1 AND agent_run_id = $2 AND status = 'running'")
                .bind(attempt.explanation_id).bind(attempt.agent_run_id)
                .execute(&mut *connection).await
                .map_err(|error| ProjectApiError::database("收敛终止解释缓存", error))?;
        }
        return Ok(false);
    }

    let output = if owns_running_cache {
        output
    } else {
        Err("解释缓存已结束或由后续任务接管，丢弃旧任务结果".into())
    };
    let (status, explanation, error_message) = match output {
        Ok(value) => ("succeeded", Some(value), None),
        Err(message) => ("failed", None, Some(message)),
    };
    let error = error_message
        .as_ref()
        .map(|message| json!({"message":message,"retryable":false}));
    let digest = explanation
        .as_ref()
        .map(|value| format!("{:x}", Sha256::digest(value.to_string().as_bytes())));
    sqlx::query("UPDATE agent_runs SET status = $1, output_json = $2, error_json = $3, usage_json = $4, finished_at = NOW() WHERE id = $5 AND status = 'running'")
        .bind(status).bind(&explanation).bind(&error).bind(usage).bind(attempt.agent_run_id)
        .execute(&mut *connection).await
        .map_err(|error| ProjectApiError::database("完成决策解释运行", error))?;
    sqlx::query("UPDATE agent_steps SET status = $1, artifact_digest = $2, usage_json = $3, error_json = $4, finished_at = NOW() WHERE agent_run_id = $5 AND orchestration_id = $6 AND status = 'running'")
        .bind(status).bind(digest).bind(usage).bind(&error).bind(attempt.agent_run_id).bind(attempt.orchestration_id)
        .execute(&mut *connection).await
        .map_err(|error| ProjectApiError::database("完成决策解释步骤", error))?;
    if owns_running_cache {
        sqlx::query("UPDATE decision_run_explanations SET status = $1, explanation_json = $2, error_message = $3, updated_at = NOW(), completed_at = NOW() WHERE id = $4 AND agent_run_id = $5 AND status = 'running'")
            .bind(status).bind(&explanation).bind(&error_message).bind(attempt.explanation_id).bind(attempt.agent_run_id)
            .execute(&mut *connection).await
            .map_err(|error| ProjectApiError::database("保存决策解释结果", error))?;
    }
    sqlx::query("UPDATE agent_orchestrations SET status = $1, finished_at = NOW() WHERE id = $2 AND status = 'running'")
        .bind(status).bind(attempt.orchestration_id)
        .execute(&mut *connection).await
        .map_err(|error| ProjectApiError::database("完成决策解释编排", error))?;
    insert_audit_event(
        &mut *connection,
        attempt.project_id,
        None,
        if owns_running_cache { "decision_run.explanation_generated" } else { "decision_run.explanation_discarded" },
        username,
        &format!("决策运行 {} 的解释作业{}", attempt.decision_run_id, if owns_running_cache { "已结束" } else { "结果已丢弃" }),
        json!({"runId":attempt.decision_run_id,"explanationId":attempt.explanation_id,"agentRunId":attempt.agent_run_id,"orchestrationId":attempt.orchestration_id,"status":status}),
    )
    .await
    .map_err(|error| ProjectApiError::database("记录决策解释审计", error))?;
    Ok(owns_running_cache)
}

#[cfg(test)]
mod tests;
