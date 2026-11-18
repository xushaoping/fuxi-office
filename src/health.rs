//! Runtime readiness uses the same read-only database contract as release checks.
use anyhow::{Context, Result, bail};
use axum::{Extension, Json, http::StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

const BASELINE_SQL: &str = include_str!("../script/check-deployment-baseline.sql");
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct Readiness {
    pool: PgPool,
    draining: Arc<AtomicBool>,
}

impl Readiness {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self {
            pool,
            draining: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn drain(&self) {
        self.draining.store(true, Ordering::Release);
    }

    async fn check(&self) -> Result<()> {
        if self.draining.load(Ordering::Acquire) {
            bail!("service is draining");
        }
        check_deployment_baseline(&self.pool).await?;
        // A request already checking the database must also observe shutdown.
        if self.draining.load(Ordering::Acquire) {
            bail!("service is draining");
        }
        Ok(())
    }
}

pub(crate) async fn ready(Extension(readiness): Extension<Readiness>) -> (StatusCode, Json<Value>) {
    match readiness.check().await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "ready": true }))),
        // Internal schema, role and connection diagnostics stay in startup/release
        // logs. The unauthenticated probe only exposes the readiness verdict.
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "ready": false })),
        ),
    }
}

fn baseline_query(sql: &str) -> Result<&str> {
    // The psql wrapper has a separate database/role result and explicit rollback.
    // Execute only its baseline result set inside our own read-only transaction.
    let start = sql
        .find("WITH expected_objects(")
        .context("deployment baseline query marker missing")?;
    let query = sql[start..]
        .trim()
        .strip_suffix("ROLLBACK;")
        .context("deployment baseline rollback marker missing")?
        .trim();
    if !query.ends_with("ORDER BY feature;") {
        bail!("unexpected deployment baseline query layout");
    }
    Ok(query)
}

fn validate_rows(rows: &[(String, String, String, String)]) -> Result<()> {
    if rows.is_empty() {
        bail!("deployment baseline returned no checks");
    }
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|(feature, object, status, _)| match status.as_str() {
            "present" | "optional_present" | "optional_missing" => None,
            _ => Some(format!("{feature} ({object}): {status}")),
        })
        .collect();
    if !failures.is_empty() {
        bail!("deployment baseline failed: {}", failures.join(", "));
    }
    Ok(())
}

pub(crate) async fn check_deployment_baseline(pool: &PgPool) -> Result<()> {
    tokio::time::timeout(PROBE_TIMEOUT, async {
        let mut transaction = pool
            .begin()
            .await
            .context("acquiring readiness connection")?;
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("SET LOCAL statement_timeout = '4000ms'")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("SET LOCAL lock_timeout = '1000ms'")
            .execute(&mut *transaction)
            .await?;
        let rows =
            sqlx::query_as::<_, (String, String, String, String)>(baseline_query(BASELINE_SQL)?)
                .fetch_all(&mut *transaction)
                .await
                .context("querying deployment baseline")?;
        transaction.rollback().await?;
        validate_rows(&rows)
    })
    .await
    .context("deployment readiness timed out")?
}

pub(crate) struct ShutdownSignal {
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
}

impl ShutdownSignal {
    pub(crate) fn register() -> Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            terminate: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("registering SIGTERM handler")?,
        })
    }

    pub(crate) async fn wait(&mut self) -> Result<()> {
        #[cfg(unix)]
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("waiting for Ctrl-C"),
            _ = self.terminate.recv() => Ok(()),
        }
        #[cfg(not(unix))]
        tokio::signal::ctrl_c().await.context("waiting for Ctrl-C")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(status: &str) -> (String, String, String, String) {
        (
            "check".into(),
            "object".into(),
            status.into(),
            "details".into(),
        )
    }

    #[test]
    fn baseline_rejects_missing_unknown_and_empty_results() {
        assert!(validate_rows(&[]).is_err());
        assert!(validate_rows(&[row("missing")]).is_err());
        assert!(validate_rows(&[row("unexpected")]).is_err());
        assert!(
            validate_rows(&[
                row("present"),
                row("optional_missing"),
                row("optional_present")
            ])
            .is_ok()
        );
    }

    #[test]
    fn baseline_reuses_release_query_without_psql_wrapper() {
        let query = baseline_query(BASELINE_SQL).unwrap();
        assert!(query.starts_with("WITH expected_objects("));
        assert!(query.contains("explanation_revision"));
        assert!(!query.contains("BEGIN TRANSACTION"));
        assert!(!query.contains("ROLLBACK"));
        assert!(baseline_query("SELECT 1").is_err());
        assert!(baseline_query("WITH expected_objects(x) AS (VALUES (1)) SELECT 1;").is_err());
    }

    #[tokio::test]
    async fn draining_readiness_is_immediately_unavailable_without_database_access() {
        let pool = PgPool::connect_lazy("postgresql://unused:unused@127.0.0.1:1/unused").unwrap();
        let readiness = Readiness::new(pool);
        let other_handle = readiness.clone();
        readiness.drain();
        let (status, Json(body)) =
            tokio::time::timeout(Duration::from_millis(100), ready(Extension(other_handle)))
                .await
                .expect("draining probe must not wait for PostgreSQL");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, json!({"ok": false, "ready": false}));
    }

    #[tokio::test]
    async fn closed_database_is_unavailable_without_public_diagnostics() {
        let pool = PgPool::connect_lazy("postgresql://unused:unused@127.0.0.1:1/unused").unwrap();
        pool.close().await;
        let (status, Json(body)) = ready(Extension(Readiness::new(pool))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, json!({"ok": false, "ready": false}));
    }

    #[tokio::test]
    #[ignore = "requires isolated, fully migrated HEALTH_TEST_DATABASE_URL using the runtime role"]
    async fn deployment_baseline_accepts_migrated_runtime_database() {
        let url =
            std::env::var("HEALTH_TEST_DATABASE_URL").expect("isolated database URL required");
        let pool = PgPool::connect(&url).await.unwrap();
        check_deployment_baseline(&pool).await.unwrap();
        let (status, _) = ready(Extension(Readiness::new(pool.clone()))).await;
        assert_eq!(status, StatusCode::OK);
        pool.close().await;
    }

    #[tokio::test]
    #[ignore = "requires isolated, empty HEALTH_INCOMPLETE_TEST_DATABASE_URL"]
    async fn deployment_baseline_rejects_incomplete_database() {
        let url = std::env::var("HEALTH_INCOMPLETE_TEST_DATABASE_URL")
            .expect("isolated incomplete database URL required");
        let pool = PgPool::connect(&url).await.unwrap();
        assert!(check_deployment_baseline(&pool).await.is_err());
        let (status, Json(body)) = ready(Extension(Readiness::new(pool.clone()))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, json!({"ok": false, "ready": false}));
        pool.close().await;
    }
}
