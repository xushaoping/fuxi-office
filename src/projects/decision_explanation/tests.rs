use super::*;
use std::{cell::Cell, future::pending, time::Duration};

#[tokio::test]
async fn decision_explanation_cancellation_drops_generation_and_wins_ready_race() {
    struct Guard<'a>(&'a Cell<bool>);
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let dropped = Cell::new(false);
    let (send, receive) = tokio::sync::oneshot::channel();
    let result = until_cancelled(
        async {
            let _guard = Guard(&dropped);
            send.send(()).unwrap();
            pending::<anyhow::Result<()>>().await
        },
        async { receive.await.map_err(anyhow::Error::from) },
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("取消"));
    assert!(dropped.get());

    let started = Cell::new(false);
    assert!(
        until_cancelled(
            async {
                started.set(true);
                Ok(())
            },
            async { Ok(()) },
        )
        .await
        .is_err()
    );
    assert!(!started.get());
    assert!(
        until_cancelled(async { Ok(()) }, async {
            Err(anyhow::anyhow!("database unavailable"))
        })
        .await
        .is_err()
    );
    assert_eq!(
        until_cancelled(async { Ok(42) }, pending::<anyhow::Result<()>>())
            .await
            .unwrap(),
        42
    );
}

async fn add_attempt(
    connection: &mut sqlx::PgConnection,
    project_id: i64,
    decision_run_id: i64,
) -> (i64, i64) {
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO agent_orchestrations (project_id, username, trigger_kind, current_stage) VALUES ($1, 'admin', 'decision_explanation', 'decision_explanation') RETURNING id",
    ).bind(project_id).fetch_one(&mut *connection).await.unwrap();
    let run_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO agent_runs (project_id, orchestration_id, stage, status, input_json) VALUES ($1, $2, 'decision_explanation', 'running', $3) RETURNING id",
    ).bind(project_id).bind(orchestration_id).bind(json!({"decisionRunId":decision_run_id})).fetch_one(&mut *connection).await.unwrap();
    sqlx::query("INSERT INTO agent_steps (orchestration_id, agent_run_id, step_no, agent_role, task_kind) VALUES ($1, $2, 1, 'explainer', 'decision_explanation')")
        .bind(orchestration_id).bind(run_id).execute(&mut *connection).await.unwrap();
    (orchestration_id, run_id)
}

async fn cache_state(
    connection: &mut sqlx::PgConnection,
    id: i64,
) -> (String, Option<Value>, Option<String>, Option<i64>) {
    sqlx::query_as("SELECT status, explanation_json, error_message, agent_run_id FROM decision_run_explanations WHERE id = $1")
        .bind(id).fetch_one(connection).await.unwrap()
}

async fn statuses(
    connection: &mut sqlx::PgConnection,
    attempt: &Attempt,
) -> (String, String, String) {
    sqlx::query_as("SELECT orchestration.status, run.status, step.status FROM agent_orchestrations orchestration JOIN agent_runs run ON run.orchestration_id = orchestration.id JOIN agent_steps step ON step.agent_run_id = run.id WHERE orchestration.id = $1 AND run.id = $2")
        .bind(attempt.orchestration_id).bind(attempt.agent_run_id).fetch_one(connection).await.unwrap()
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to an isolated migrated database"]
async fn decision_explanation_postgres_attempt_guards() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&std::env::var("TEST_DATABASE_URL").expect("isolated TEST_DATABASE_URL required"))
        .await
        .unwrap();
    let mut fixture = db.begin().await.unwrap();
    let project_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO decision_projects (owner_username, name) VALUES ('admin', 'Explanation attempt regression') RETURNING id",
    ).fetch_one(&mut *fixture).await.unwrap();
    let version_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO decision_versions (project_id, version_number, status, created_by) VALUES ($1, 1, 'draft', 'admin') RETURNING id",
    ).bind(project_id).fetch_one(&mut *fixture).await.unwrap();
    let decision_run_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO decision_runs (project_id, version_id, service_id, service_name, input_json, output_json, execution_trace_json, duration_ms, status, executed_by) VALUES ($1, $2, 'svc', 'test', '{}', '{}', '{}', 1, 'succeeded', 'admin') RETURNING id",
    ).bind(project_id).bind(version_id).fetch_one(&mut *fixture).await.unwrap();
    let (orchestration_id, agent_run_id) =
        add_attempt(&mut fixture, project_id, decision_run_id).await;
    let explanation_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO decision_run_explanations (decision_run_id, agent_run_id, status, prompt_version, prompt_digest, trace_digest, created_by) VALUES ($1, $2, 'running', 'test-v1', repeat('a',64), repeat('b',64), 'admin') RETURNING id",
    ).bind(decision_run_id).bind(agent_run_id).fetch_one(&mut *fixture).await.unwrap();
    let attempt = Attempt {
        project_id,
        decision_run_id,
        explanation_id,
        agent_run_id,
        orchestration_id,
    };
    fixture.commit().await.unwrap();
    let output = json!({"summary":"fixture explanation", "decisions":[]});
    let usage = json!({"promptTokens":13,"completionTokens":8});

    // Cache, runtime states and audit commit/roll back together for both outcomes.
    for result in [Ok(output.clone()), Err("invalid model output".to_string())] {
        let expected_status = if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        };
        let mut transaction = db.begin().await.unwrap();
        assert!(
            finish_in_transaction(&mut transaction, &attempt, "admin", &usage, result)
                .await
                .unwrap()
        );
        assert_eq!(
            statuses(&mut transaction, &attempt).await,
            (
                expected_status.into(),
                expected_status.into(),
                expected_status.into()
            )
        );
        assert_eq!(
            cache_state(&mut transaction, explanation_id).await.0,
            expected_status
        );
        let audit_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE project_id = $1 AND event_type = 'decision_run.explanation_generated'")
            .bind(project_id).fetch_one(&mut *transaction).await.unwrap();
        assert_eq!(audit_count, 1);
        transaction.rollback().await.unwrap();
        let mut check = db.acquire().await.unwrap();
        assert_eq!(
            statuses(&mut check, &attempt).await,
            ("running".into(), "running".into(), "running".into())
        );
        assert_eq!(cache_state(&mut check, explanation_id).await.0, "running");
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE project_id = $1")
                .bind(project_id)
                .fetch_one(&mut *check)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }

    for terminal in ["cancelled", "failed"] {
        let mut transaction = db.begin().await.unwrap();
        sqlx::query(
            "UPDATE agent_orchestrations SET status = $1, finished_at = NOW() WHERE id = $2",
        )
        .bind(terminal)
        .bind(orchestration_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query("UPDATE agent_runs SET status = $1, error_json = '{\"message\":\"original terminal error\"}', finished_at = NOW() WHERE id = $2")
            .bind(terminal).bind(agent_run_id).execute(&mut *transaction).await.unwrap();
        sqlx::query(
            "UPDATE agent_steps SET status = $1, finished_at = NOW() WHERE agent_run_id = $2",
        )
        .bind(terminal)
        .bind(agent_run_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(
            crate::workflow_api::stop_decision_explanation_attempts(
                &mut transaction,
                &[orchestration_id],
                "stopped fixture"
            )
            .await
            .unwrap(),
            1
        );
        let before = cache_state(&mut transaction, explanation_id).await;
        assert!(
            !finish_in_transaction(
                &mut transaction,
                &attempt,
                "admin",
                &usage,
                Ok(output.clone())
            )
            .await
            .unwrap()
        );
        assert_eq!(cache_state(&mut transaction, explanation_id).await, before);
        assert_eq!(
            statuses(&mut transaction, &attempt).await,
            (terminal.into(), terminal.into(), terminal.into())
        );
        let saved: (Option<Value>, Option<Value>, Option<Value>) = sqlx::query_as(
            "SELECT usage_json, output_json, error_json FROM agent_runs WHERE id = $1",
        )
        .bind(agent_run_id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(
            saved,
            (
                Some(usage.clone()),
                None,
                Some(json!({"message":"original terminal error"}))
            )
        );
        let step: (Option<Value>, Option<String>) = sqlx::query_as(
            "SELECT usage_json, artifact_digest FROM agent_steps WHERE agent_run_id = $1",
        )
        .bind(agent_run_id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(step, (Some(usage.clone()), None));
        transaction.rollback().await.unwrap();
    }

    // Even an old attempt incorrectly left running cannot write into a retry's cache.
    let mut transaction = db.begin().await.unwrap();
    let (_new_orchestration, new_run) =
        add_attempt(&mut transaction, project_id, decision_run_id).await;
    sqlx::query("UPDATE decision_run_explanations SET agent_run_id = $1 WHERE id = $2")
        .bind(new_run)
        .bind(explanation_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
    let before = cache_state(&mut transaction, explanation_id).await;
    assert_eq!(
        crate::workflow_api::stop_decision_explanation_attempts(
            &mut transaction,
            &[orchestration_id],
            "old cancellation"
        )
        .await
        .unwrap(),
        0
    );
    assert!(
        !finish_in_transaction(
            &mut transaction,
            &attempt,
            "admin",
            &usage,
            Ok(output.clone())
        )
        .await
        .unwrap()
    );
    assert_eq!(cache_state(&mut transaction, explanation_id).await, before);
    assert_eq!(
        statuses(&mut transaction, &attempt).await,
        ("failed".into(), "failed".into(), "failed".into())
    );
    transaction.rollback().await.unwrap();

    // Actual stale recovery must also respect ownership; the fresh retry stays live.
    let mut transaction = db.begin().await.unwrap();
    let (new_orchestration, new_run) =
        add_attempt(&mut transaction, project_id, decision_run_id).await;
    let retry = Attempt {
        orchestration_id: new_orchestration,
        agent_run_id: new_run,
        ..attempt
    };
    sqlx::query("UPDATE decision_run_explanations SET agent_run_id = $1 WHERE id = $2")
        .bind(new_run)
        .bind(explanation_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE agent_orchestrations SET heartbeat_at = NOW() - INTERVAL '5 minutes' WHERE id = $1",
    )
    .bind(orchestration_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(
        crate::workflow_api::recover_stale_orchestrations(&db, Duration::from_secs(120))
            .await
            .unwrap()
            >= 1
    );
    let mut check = db.acquire().await.unwrap();
    assert_eq!(
        statuses(&mut check, &attempt).await,
        ("failed".into(), "failed".into(), "failed".into())
    );
    assert_eq!(
        statuses(&mut check, &retry).await,
        ("running".into(), "running".into(), "running".into())
    );
    assert_eq!(cache_state(&mut check, explanation_id).await.0, "running");
    drop(check);
    sqlx::query(
        "UPDATE agent_orchestrations SET heartbeat_at = NOW() - INTERVAL '5 minutes' WHERE id = $1",
    )
    .bind(new_orchestration)
    .execute(&db)
    .await
    .unwrap();
    assert!(
        crate::workflow_api::recover_stale_orchestrations(&db, Duration::from_secs(120))
            .await
            .unwrap()
            >= 1
    );
    let mut transaction = db.begin().await.unwrap();
    let before = cache_state(&mut transaction, explanation_id).await;
    assert_eq!(before.0, "failed");
    assert!(
        !finish_in_transaction(&mut transaction, &retry, "admin", &usage, Ok(output))
            .await
            .unwrap()
    );
    assert_eq!(cache_state(&mut transaction, explanation_id).await, before);
    transaction.commit().await.unwrap();

    // Versions restrict run deletion, so remove immutable run fixtures first.
    sqlx::query("DELETE FROM decision_runs WHERE project_id = $1")
        .bind(project_id)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM decision_projects WHERE id = $1")
        .bind(project_id)
        .execute(&db)
        .await
        .unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to an isolated migrated database"]
async fn decision_explanation_postgres_review_revisions() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_DATABASE_URL").expect("isolated TEST_DATABASE_URL required"))
        .await
        .unwrap();
    let mut transaction = db.begin().await.unwrap();
    let project_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO decision_projects (owner_username, name) VALUES ('admin', 'Explanation revision query regression') RETURNING id",
    ).fetch_one(&mut *transaction).await.unwrap();
    let version_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO decision_versions (project_id, version_number, created_by) VALUES ($1, 1, 'admin') RETURNING id",
    ).bind(project_id).fetch_one(&mut *transaction).await.unwrap();
    let decision_run_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO decision_runs (project_id, version_id, service_id, service_name, input_json, output_json, execution_trace_json, duration_ms, status, executed_by) VALUES ($1, $2, 'svc', 'test', '{}', '{}', '{}', 1, 'succeeded', 'admin') RETURNING id",
    ).bind(project_id).bind(version_id).fetch_one(&mut *transaction).await.unwrap();
    let (_, agent_run) = add_attempt(&mut transaction, project_id, decision_run_id).await;
    let explanation_id = sqlx::query_scalar::<_, i64>(UPSERT_ATTEMPT_SQL)
        .bind(decision_run_id)
        .bind("fixture-model")
        .bind("fixture-v1")
        .bind("a".repeat(64))
        .bind("b".repeat(64))
        .bind("admin")
        .bind(agent_run)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("UPDATE decision_run_explanations SET status = 'succeeded', explanation_json = '{\"summary\":\"version one\"}', completed_at = NOW(), created_at = NOW() - INTERVAL '1 year' WHERE id = $1")
        .bind(explanation_id).execute(&mut *transaction).await.unwrap();
    sqlx::query("INSERT INTO decision_run_explanation_reviews (explanation_id, explanation_revision, reviewer_username, review_label) VALUES ($1, 1, 'admin', 'needs_revision')")
        .bind(explanation_id).execute(&mut *transaction).await.unwrap();
    let first =
        sqlx::query_as::<_, crate::models::DecisionRunExplanationReviewView>(CURRENT_REVIEWS_SQL)
            .bind(project_id)
            .bind(decision_run_id)
            .bind(1_i64)
            .bind(true)
            .bind("admin")
            .fetch_all(&mut *transaction)
            .await
            .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].review_label, "needs_revision");
    let hidden =
        sqlx::query_as::<_, crate::models::DecisionRunExplanationReviewView>(CURRENT_REVIEWS_SQL)
            .bind(project_id)
            .bind(decision_run_id)
            .bind(1_i64)
            .bind(true)
            .bind("not-yet-reviewed")
            .fetch_all(&mut *transaction)
            .await
            .unwrap();
    assert!(hidden.is_empty());

    let (_, next_agent_run) = add_attempt(&mut transaction, project_id, decision_run_id).await;
    let reused_id = sqlx::query_scalar::<_, i64>(UPSERT_ATTEMPT_SQL)
        .bind(decision_run_id)
        .bind("new-model")
        .bind("fixture-v2")
        .bind("c".repeat(64))
        .bind("b".repeat(64))
        .bind("admin")
        .bind(next_agent_run)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(reused_id, explanation_id);
    let current: (i64, String, bool) = sqlx::query_as("SELECT generation_revision, status, created_at >= NOW() - INTERVAL '1 minute' FROM decision_run_explanations WHERE id = $1")
        .bind(explanation_id).fetch_one(&mut *transaction).await.unwrap();
    assert_eq!(current, (2, "running".into(), true));
    sqlx::query("UPDATE decision_run_explanations SET status = 'succeeded', explanation_json = '{\"summary\":\"version two\"}', completed_at = NOW() WHERE id = $1")
        .bind(explanation_id).execute(&mut *transaction).await.unwrap();
    // Reviewing v1 never grants visibility into v2, even for the same Reviewer.
    for revision in [1_i64, 2] {
        let reviews = sqlx::query_as::<_, crate::models::DecisionRunExplanationReviewView>(
            CURRENT_REVIEWS_SQL,
        )
        .bind(project_id)
        .bind(decision_run_id)
        .bind(revision)
        .bind(true)
        .bind("admin")
        .fetch_all(&mut *transaction)
        .await
        .unwrap();
        assert!(reviews.is_empty());
    }
    let exports = sqlx::query_as::<_, crate::models::DecisionExplanationReviewExportRecord>(
        CURRENT_REVIEW_EXPORT_SQL,
    )
    .bind(project_id)
    .bind(0_i64)
    .bind(false)
    .bind("admin")
    .bind(500_i64)
    .fetch_all(&mut *transaction)
    .await
    .unwrap();
    assert!(exports.is_empty());
    sqlx::query("INSERT INTO decision_run_explanation_reviews (explanation_id, explanation_revision, reviewer_username, review_label) VALUES ($1, 2, 'admin', 'readable')")
        .bind(explanation_id).execute(&mut *transaction).await.unwrap();
    let reviews =
        sqlx::query_as::<_, crate::models::DecisionRunExplanationReviewView>(CURRENT_REVIEWS_SQL)
            .bind(project_id)
            .bind(decision_run_id)
            .bind(2_i64)
            .bind(true)
            .bind("admin")
            .fetch_all(&mut *transaction)
            .await
            .unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].explanation_revision, 2);
    assert_eq!(reviews[0].review_label, "readable");
    for (is_reviewer, username, expected_count) in [
        (false, "viewer", 1),
        (true, "admin", 1),
        (true, "unreviewed", 0),
    ] {
        let exports = sqlx::query_as::<_, crate::models::DecisionExplanationReviewExportRecord>(
            CURRENT_REVIEW_EXPORT_SQL,
        )
        .bind(project_id)
        .bind(0_i64)
        .bind(is_reviewer)
        .bind(username)
        .bind(500_i64)
        .fetch_all(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(exports.len(), expected_count);
        if let Some(export) = exports.first() {
            assert_eq!(export.explanation_revision, 2);
            assert_eq!(export.review_label, "readable");
        }
    }
    let wrong_project =
        sqlx::query_as::<_, crate::models::DecisionRunExplanationReviewView>(CURRENT_REVIEWS_SQL)
            .bind(project_id + 999_999)
            .bind(decision_run_id)
            .bind(2_i64)
            .bind(false)
            .bind("admin")
            .fetch_all(&mut *transaction)
            .await
            .unwrap();
    assert!(wrong_project.is_empty());
    let all_review_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM decision_run_explanation_reviews WHERE explanation_id = $1",
    )
    .bind(explanation_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(all_review_count, 2);
    transaction.rollback().await.unwrap();
    db.close().await;
}
