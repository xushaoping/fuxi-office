use super::*;
use crate::workflow::{SemanticTestProposal, TesterJudgeVote};
use std::{cell::Cell, future::pending, time::Duration};

fn vote() -> TesterJudgeVote {
    TesterJudgeVote {
        assessment: "plausible".into(),
        reason: "有已确认规则支持".into(),
    }
}

#[tokio::test]
async fn tester_judge_cancellation_drops_in_flight_sample() {
    struct OnDrop<'a>(&'a Cell<bool>);
    impl Drop for OnDrop<'_> {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let started = Cell::new(0);
    let dropped = Cell::new(false);
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let cancel_tx = Cell::new(Some(cancel_tx));
    let result = tester_judge_sample_votes(
        || async { Ok(()) },
        || async {
            started.set(started.get() + 1);
            let _guard = OnDrop(&dropped);
            cancel_tx.take().unwrap().send(()).unwrap();
            pending::<anyhow::Result<TesterJudgeVote>>().await
        },
        async {
            cancel_rx.await?;
            Ok(())
        },
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("取消"));
    assert_eq!(started.get(), 1);
    assert!(dropped.get());
}

#[tokio::test]
async fn tester_judge_checks_state_before_each_sample_and_stops_on_bad_output() {
    for invalid_first_vote in [false, true] {
        let calls = Cell::new(0);
        let result = tester_judge_sample_votes(
            || async {
                anyhow::ensure!(calls.get() == 0, "编排已取消");
                Ok(())
            },
            || async {
                calls.set(calls.get() + 1);
                if invalid_first_vote {
                    Ok(TesterJudgeVote {
                        reason: " ".into(),
                        ..vote()
                    })
                } else {
                    Ok(vote())
                }
            },
            pending::<anyhow::Result<()>>(),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }
    let votes = tester_judge_sample_votes(
        || async { Ok(()) },
        || async { Ok(vote()) },
        pending::<anyhow::Result<()>>(),
    )
    .await
    .unwrap();
    assert_eq!(votes.len(), 2);
}

#[test]
fn tester_judge_context_changes_with_business_material_but_ignores_old_reviews() {
    let candidate = ConvertTesterProposalRequest {
        input_json: json!({"age":25}),
        expected_output_json: json!({"eligible":true}),
    };
    let mut proposal = SemanticTestProposal {
        title: "成人资格".into(),
        ..Default::default()
    };
    let original = tester_judge_context(
        "年龄至少18岁",
        &json!({}),
        &json!([]),
        &None,
        &proposal,
        &candidate,
    );
    proposal.judge_review = Some(TesterJudgeReview::default());
    assert_eq!(
        original,
        tester_judge_context(
            "年龄至少18岁",
            &json!({}),
            &json!([]),
            &None,
            &proposal,
            &candidate
        )
    );
    assert_ne!(
        original,
        tester_judge_context(
            "年龄至少30岁",
            &json!({}),
            &json!([]),
            &None,
            &proposal,
            &candidate
        )
    );
    proposal.rationale = "新增业务依据".into();
    assert_ne!(
        original,
        tester_judge_context(
            "年龄至少18岁",
            &json!({}),
            &json!([]),
            &None,
            &proposal,
            &candidate
        )
    );
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to an isolated migrated database"]
async fn tester_judge_postgres_heartbeat_and_terminal_state_guards() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&std::env::var("TEST_DATABASE_URL").expect("isolated TEST_DATABASE_URL required"))
        .await
        .unwrap();
    let mut fixture = db.begin().await.unwrap();
    let project_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO decision_projects (owner_username, name) VALUES ('admin', 'Tester Judge isolation regression') RETURNING id",
    ).fetch_one(&mut *fixture).await.unwrap();
    let orchestration_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO agent_orchestrations (project_id, username, trigger_kind, current_stage, heartbeat_at) VALUES ($1, 'admin', 'tester_judge', 'tester_judge', NOW() - INTERVAL '5 minutes') RETURNING id",
    ).bind(project_id).fetch_one(&mut *fixture).await.unwrap();
    let run_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO agent_runs (project_id, orchestration_id, stage, status) VALUES ($1, $2, 'tester_judge', 'running') RETURNING id",
    ).bind(project_id).bind(orchestration_id).fetch_one(&mut *fixture).await.unwrap();
    sqlx::query("INSERT INTO agent_steps (orchestration_id, agent_run_id, step_no, agent_role, task_kind) VALUES ($1, $2, 1, 'tester_judge', 'tester_expected_output_judge')")
        .bind(orchestration_id).bind(run_id).execute(&mut *fixture).await.unwrap();
    fixture.commit().await.unwrap();

    // A rolled-back success must leave the run and step running too.
    let usage = json!({"promptTokens":11,"completionTokens":7});
    let mut transaction = db.begin().await.unwrap();
    ensure_tester_judge_running(&mut transaction, orchestration_id)
        .await
        .unwrap();
    finish_tester_judge_agent_run_in_transaction(
        &mut transaction,
        run_id,
        orchestration_id,
        &usage,
        Ok(json!({"status":"no_obvious_issue"})),
    )
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM agent_runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(status, "running");

    let monitor = crate::workflow_api::wait_for_orchestration_cancellation(&db, orchestration_id);
    let cancel_after_heartbeat = async {
        loop {
            let refreshed: bool = sqlx::query_scalar("SELECT heartbeat_at > NOW() - INTERVAL '10 seconds' FROM agent_orchestrations WHERE id = $1")
                .bind(orchestration_id).fetch_one(&db).await.unwrap();
            if refreshed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut transaction = db.begin().await.unwrap();
        sqlx::query("UPDATE agent_orchestrations SET status = 'cancelled', finished_at = NOW() WHERE id = $1")
            .bind(orchestration_id).execute(&mut *transaction).await.unwrap();
        sqlx::query(
            "UPDATE agent_runs SET status = 'cancelled', finished_at = NOW() WHERE id = $1",
        )
        .bind(run_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query("UPDATE agent_steps SET status = 'cancelled', finished_at = NOW() WHERE agent_run_id = $1")
            .bind(run_id).execute(&mut *transaction).await.unwrap();
        transaction.commit().await.unwrap();
    };
    let (stopped, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(monitor, cancel_after_heartbeat)
    })
    .await
    .expect("heartbeat/cancellation timed out");
    stopped.unwrap();

    // Late model output must not resurrect cancellation or stale recovery.
    for terminal in ["cancelled", "failed"] {
        let mut transaction = db.begin().await.unwrap();
        sqlx::query("UPDATE agent_orchestrations SET status = $1 WHERE id = $2")
            .bind(terminal)
            .bind(orchestration_id)
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE agent_runs SET status = $1 WHERE id = $2")
            .bind(terminal)
            .bind(run_id)
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE agent_steps SET status = $1 WHERE agent_run_id = $2")
            .bind(terminal)
            .bind(run_id)
            .execute(&mut *transaction)
            .await
            .unwrap();
        assert!(
            ensure_tester_judge_running(&mut transaction, orchestration_id)
                .await
                .is_err()
        );
        finish_tester_judge_agent_run_in_transaction(
            &mut transaction,
            run_id,
            orchestration_id,
            &usage,
            Ok(json!({"status":"no_obvious_issue"})),
        )
        .await
        .unwrap();
        let run: (String, Option<Value>, Option<Value>) =
            sqlx::query_as("SELECT status, output_json, usage_json FROM agent_runs WHERE id = $1")
                .bind(run_id)
                .fetch_one(&mut *transaction)
                .await
                .unwrap();
        assert_eq!(run.0, terminal);
        assert_eq!(run.1, None);
        assert_eq!(run.2, Some(usage.clone()));
        let step: (String, Option<String>) = sqlx::query_as(
            "SELECT status, artifact_digest FROM agent_steps WHERE agent_run_id = $1",
        )
        .bind(run_id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(step, (terminal.into(), None));
        transaction.rollback().await.unwrap();
    }
    sqlx::query("DELETE FROM decision_projects WHERE id = $1")
        .bind(project_id)
        .execute(&db)
        .await
        .unwrap();
    db.close().await;
}
