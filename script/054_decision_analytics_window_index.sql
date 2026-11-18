-- Match published-version analytics filtering and stable execution ordering.
-- Keep this migration outside a transaction so concurrent creation avoids
-- blocking inserts and updates while the index is built on existing data.
CREATE INDEX CONCURRENTLY IF NOT EXISTS decision_runs_project_version_executed_idx
    ON decision_runs (project_id, version_id, executed_at, id);
