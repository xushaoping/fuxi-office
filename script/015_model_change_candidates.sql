-- Persisted preview/commit candidates for server-side model changes.
BEGIN;

CREATE INDEX IF NOT EXISTS decision_artifacts_content_hash_idx
    ON decision_artifacts(version_id, content_hash);

CREATE TABLE IF NOT EXISTS model_change_candidates (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    base_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    base_revision_id VARCHAR(64) NOT NULL,
    base_hash VARCHAR(128) NOT NULL,
    dsl_yaml TEXT NOT NULL,
    dmn_xml TEXT NOT NULL,
    change_json JSONB NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'committing', 'committed', 'expired')),
    created_by VARCHAR(255) NOT NULL REFERENCES users(username),
    resulting_version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

ALTER TABLE model_change_candidates
    ADD COLUMN IF NOT EXISTS base_revision_id VARCHAR(64);

CREATE INDEX IF NOT EXISTS model_change_candidates_project_idx
    ON model_change_candidates(project_id, created_at DESC);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE model_change_candidates TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE model_change_candidates_id_seq TO :"app_user";
\endif
