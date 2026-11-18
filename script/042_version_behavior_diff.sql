BEGIN;

CREATE TABLE IF NOT EXISTS version_behavior_diff (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    baseline_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    candidate_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    baseline_run_id BIGINT REFERENCES decision_runs(id) ON DELETE SET NULL,
    candidate_run_id BIGINT REFERENCES decision_runs(id) ON DELETE SET NULL,
    test_case_id BIGINT REFERENCES decision_test_cases(id) ON DELETE SET NULL,
    baseline_output JSONB NOT NULL,
    candidate_output JSONB NOT NULL,
    diff_json JSONB NOT NULL,
    risk_level TEXT NOT NULL CHECK (risk_level IN ('low', 'medium', 'high')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (baseline_version_id <> candidate_version_id),
    UNIQUE (baseline_run_id, candidate_run_id)
);

CREATE INDEX IF NOT EXISTS version_behavior_diff_candidate_idx
    ON version_behavior_diff(candidate_version_id, created_at DESC);
CREATE INDEX IF NOT EXISTS version_behavior_diff_project_idx
    ON version_behavior_diff(project_id, created_at DESC);
\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE version_behavior_diff TO :"app_user";
GRANT USAGE, SELECT, UPDATE
    ON SEQUENCE version_behavior_diff_id_seq TO :"app_user";
\endif

COMMIT;
