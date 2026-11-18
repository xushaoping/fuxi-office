use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageQuery {
    after_id: Option<i64>,
    through_id: Option<i64>,
    limit: Option<i64>,
}

impl PageQuery {
    fn validate(&self) -> Result<(i64, i64), ProjectApiError> {
        let after = self.after_id.unwrap_or(0);
        let limit = self.limit.unwrap_or(500);
        if after < 0 || self.through_id.is_some_and(|through| through < after) {
            return Err(ProjectApiError::bad_request(
                "归因清单要求 0 <= afterId <= throughId",
            ));
        }
        if !(1..=500).contains(&limit) {
            return Err(ProjectApiError::bad_request("limit 必须在 1 到 500 之间"));
        }
        Ok((after, limit))
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(super) struct InventoryRecord {
    id: i64,
    project_id: i64,
    decision_run_id: i64,
    status: String,
    confidence: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InventoryPage {
    through_id: i64,
    records: Vec<InventoryRecord>,
}

const PAGE_SQL: &str = r#"
    SELECT id, project_id, decision_run_id, status, confidence
    FROM deviation_analyses
    WHERE project_id = $1 AND id > $2 AND id <= $3
    ORDER BY id
    LIMIT $4
"#;

async fn read_page(
    connection: &mut sqlx::PgConnection,
    project_id: i64,
    query: &PageQuery,
) -> Result<InventoryPage, ProjectApiError> {
    let (after, limit) = query.validate()?;
    let through_id = match query.through_id {
        Some(id) => id,
        None => sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(id), 0) FROM deviation_analyses WHERE project_id = $1",
        )
        .bind(project_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(|error| ProjectApiError::database("读取归因清单边界", error))?,
    };
    let records = sqlx::query_as::<_, InventoryRecord>(PAGE_SQL)
        .bind(project_id)
        .bind(after)
        .bind(through_id)
        .bind(limit)
        .fetch_all(connection)
        .await
        .map_err(|error| ProjectApiError::database("读取归因候选清单", error))?;
    Ok(InventoryPage {
        through_id,
        records,
    })
}

pub(super) async fn list(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<PageQuery>,
) -> Result<Response, ProjectApiError> {
    ensure_project_permission(&state, project_id, &claims.sub, ProjectPermission::View).await?;
    query.validate()?;
    let mut transaction = state
        .db
        .begin()
        .await
        .map_err(|error| ProjectApiError::database("读取归因清单快照", error))?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(|error| ProjectApiError::database("设置归因清单快照", error))?;
    let page = read_page(&mut transaction, project_id, &query).await?;
    transaction
        .commit()
        .await
        .map_err(|error| ProjectApiError::database("完成归因清单读取", error))?;
    let mut response = Json(page).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_cursor_bounds() {
        for (after, through, limit) in [(-1, None, 1), (5, Some(4), 1), (0, None, 501)] {
            assert!(
                PageQuery {
                    after_id: Some(after),
                    through_id: through,
                    limit: Some(limit),
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            PageQuery {
                after_id: Some(0),
                through_id: Some(0),
                limit: Some(500),
            }
            .validate()
            .is_ok()
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to an isolated migrated database"]
    async fn deviation_inventory_postgres_project_scope_and_frozen_cursor() {
        let db = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(
                &std::env::var("TEST_DATABASE_URL").expect("isolated TEST_DATABASE_URL required"),
            )
            .await
            .unwrap();
        let mut tx = db.begin().await.unwrap();
        let project_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO decision_projects (owner_username, name) VALUES ('admin', 'Deviation inventory regression') RETURNING id",
        ).fetch_one(&mut *tx).await.unwrap();
        let version_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO decision_versions (project_id, version_number, status, created_by) VALUES ($1, 1, 'draft', 'admin') RETURNING id",
        ).bind(project_id).fetch_one(&mut *tx).await.unwrap();
        let mut ids = Vec::new();
        for (status, rule, review) in [
            ("candidate", Some("private rule"), Some("correct")),
            ("candidate", Some("other private rule"), None),
            ("insufficient_evidence", None, None),
        ] {
            let run_id = sqlx::query_scalar::<_, i64>(
                "INSERT INTO decision_runs (project_id, version_id, service_id, service_name, input_json, output_json, execution_trace_json, duration_ms, status, executed_by) VALUES ($1, $2, 'svc', 'test', '{}', '{}', '{}', 1, 'succeeded', 'admin') RETURNING id",
            ).bind(project_id).bind(version_id).fetch_one(&mut *tx).await.unwrap();
            let id = sqlx::query_scalar::<_, i64>(
                "INSERT INTO deviation_analyses (project_id, decision_run_id, status, attributed_rule, confidence, review_label, reviewed_by, reviewed_at) VALUES ($1,$2,$3,$4,0.5,$5, CASE WHEN $5 IS NOT NULL THEN 'admin' END, CASE WHEN $5 IS NOT NULL THEN NOW() END) RETURNING id",
            ).bind(project_id).bind(run_id).bind(status).bind(rule).bind(review).fetch_one(&mut *tx).await.unwrap();
            ids.push(id);
        }
        let first = read_page(
            &mut tx,
            project_id,
            &PageQuery {
                after_id: None,
                through_id: None,
                limit: Some(1),
            },
        )
        .await
        .unwrap();
        assert_eq!(first.through_id, ids[2]);
        assert_eq!(first.records[0].id, ids[0]);
        let frozen = read_page(
            &mut tx,
            project_id,
            &PageQuery {
                after_id: Some(ids[0]),
                through_id: Some(ids[1]),
                limit: Some(500),
            },
        )
        .await
        .unwrap();
        assert_eq!(frozen.records.len(), 1);
        assert_eq!(frozen.records[0].id, ids[1]);
        let all = read_page(
            &mut tx,
            project_id,
            &PageQuery {
                after_id: None,
                through_id: None,
                limit: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(all.records.len(), 3);
        assert_eq!(all.records[2].status, "insufficient_evidence");
        let serialized = serde_json::to_string(&all).unwrap();
        for secret in [
            "private rule",
            "\"reviewLabel\"",
            "\"reviewedBy\"",
            "\"evidence\"",
        ] {
            assert!(!serialized.contains(secret));
        }
        assert!(
            read_page(
                &mut tx,
                -1,
                &PageQuery {
                    after_id: None,
                    through_id: None,
                    limit: None,
                }
            )
            .await
            .unwrap()
            .records
            .is_empty()
        );
        tx.rollback().await.unwrap();
    }
}
