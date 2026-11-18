BEGIN;

CREATE TABLE IF NOT EXISTS decision_run_explanations (
    id BIGSERIAL PRIMARY KEY,
    decision_run_id BIGINT NOT NULL UNIQUE
        REFERENCES decision_runs(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
    explanation_json JSONB,
    model TEXT,
    prompt_version TEXT NOT NULL,
    prompt_digest CHAR(64) NOT NULL CHECK (prompt_digest ~ '^[0-9a-f]{64}$'),
    trace_digest CHAR(64) NOT NULL CHECK (trace_digest ~ '^[0-9a-f]{64}$'),
    error_message TEXT,
    created_by TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    CHECK (
        (status = 'succeeded' AND explanation_json IS NOT NULL AND completed_at IS NOT NULL)
        OR (status <> 'succeeded' AND explanation_json IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS decision_run_explanations_status_idx
    ON decision_run_explanations (status, updated_at)
    WHERE status = 'running';

COMMENT ON TABLE decision_run_explanations IS
    'On-demand business explanations derived from immutable decision execution traces.';

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_run_explanations TO :"app_user";
GRANT USAGE, SELECT ON SEQUENCE decision_run_explanations_id_seq TO :"app_user";
\endif

COMMIT;
