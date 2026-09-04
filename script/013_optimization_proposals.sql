-- Traceable runtime optimization proposals. Confirmation creates a draft only;
-- it never changes or publishes a decision model automatically.

BEGIN;

CREATE TABLE IF NOT EXISTS decision_optimization_proposals (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    source_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    finding_code VARCHAR(64) NOT NULL,
    finding_level VARCHAR(16) NOT NULL,
    element VARCHAR(255) NOT NULL DEFAULT '',
    title VARCHAR(240) NOT NULL,
    summary TEXT NOT NULL,
    proposed_actions_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    evidence_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    analysis_snapshot_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    status VARCHAR(16) NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'confirmed', 'dismissed')),
    generated_by VARCHAR(255) NOT NULL REFERENCES users(username),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_by VARCHAR(255) REFERENCES users(username),
    resolved_at TIMESTAMPTZ,
    resolution_note TEXT NOT NULL DEFAULT '',
    resulting_version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS decision_optimization_proposals_project_generated_idx
    ON decision_optimization_proposals (project_id, generated_at DESC, id DESC);
CREATE UNIQUE INDEX IF NOT EXISTS decision_optimization_proposals_open_source_finding_idx
    ON decision_optimization_proposals (project_id, source_version_id, finding_code, element)
    WHERE status = 'open';

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_optimization_proposals TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE decision_optimization_proposals_id_seq TO :"app_user";
\endif
