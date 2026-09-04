-- Published decision service execution history.

CREATE TABLE IF NOT EXISTS decision_runs (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE RESTRICT,
    service_id VARCHAR(255) NOT NULL,
    service_name VARCHAR(255) NOT NULL,
    input_json JSONB NOT NULL,
    output_json JSONB NOT NULL,
    execution_trace_json JSONB NOT NULL,
    duration_ms BIGINT NOT NULL CHECK (duration_ms >= 0),
    status VARCHAR(16) NOT NULL CHECK (status IN ('succeeded', 'failed')),
    executed_by VARCHAR(255) NOT NULL REFERENCES users(username),
    executed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS decision_runs_project_executed_idx
    ON decision_runs (project_id, executed_at DESC);

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_runs TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE decision_runs_id_seq TO :"app_user";
\endif
