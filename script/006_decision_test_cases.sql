-- Persistent business acceptance cases for decision versions.
-- Run this migration against the application database before deploying this version.

CREATE TABLE IF NOT EXISTS decision_test_cases (
    id BIGSERIAL PRIMARY KEY,
    version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    name VARCHAR(160) NOT NULL,
    category VARCHAR(24) NOT NULL DEFAULT 'business'
        CHECK (category IN ('business', 'boundary', 'regression')),
    input_json JSONB NOT NULL,
    expected_output_json JSONB NOT NULL,
    actual_output_json JSONB,
    execution_trace_json JSONB,
    status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'passed', 'failed', 'error')),
    source VARCHAR(16) NOT NULL DEFAULT 'manual'
        CHECK (source IN ('manual', 'generated')),
    approved_by VARCHAR(255) REFERENCES users(username) ON DELETE SET NULL,
    approved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE decision_test_cases
    ADD COLUMN IF NOT EXISTS execution_trace_json JSONB;

CREATE INDEX IF NOT EXISTS decision_test_cases_version_created_idx
    ON decision_test_cases (version_id, created_at);

-- Optional application-role grants. Invoke psql with -v app_user=fuxi when needed.
\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_test_cases TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE decision_test_cases_id_seq TO :"app_user";
\endif
