BEGIN;

CREATE TABLE IF NOT EXISTS regression_test_sources (
    id BIGSERIAL PRIMARY KEY,
    candidate_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    source_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    source_run_id BIGINT NOT NULL REFERENCES decision_runs(id) ON DELETE CASCADE,
    test_input JSONB NOT NULL,
    expected_output JSONB NOT NULL,
    actual_output JSONB,
    execution_trace_json JSONB,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'passed', 'failed', 'error')),
    risk_level TEXT NOT NULL DEFAULT 'low'
        CHECK (risk_level IN ('low', 'medium', 'high')),
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (candidate_version_id, source_run_id)
);

CREATE INDEX IF NOT EXISTS regression_test_sources_candidate_idx
    ON regression_test_sources(candidate_version_id, status);

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE regression_test_sources TO :"app_user";
GRANT USAGE, SELECT, UPDATE
    ON SEQUENCE regression_test_sources_id_seq TO :"app_user";
\endif

COMMIT;
