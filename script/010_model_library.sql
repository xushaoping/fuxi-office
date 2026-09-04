-- Persistent decision-template snapshots used by the model library.

BEGIN;

CREATE TABLE IF NOT EXISTS decision_templates (
    id BIGSERIAL PRIMARY KEY,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    name VARCHAR(120) NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    industry VARCHAR(80) NOT NULL DEFAULT '',
    risk_level VARCHAR(16) NOT NULL DEFAULT 'medium'
        CHECK (risk_level IN ('low', 'medium', 'high')),
    source_project_id BIGINT REFERENCES decision_projects(id) ON DELETE SET NULL,
    source_version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    requirement_text TEXT,
    decision_brief_json JSONB,
    decision_plan_json JSONB,
    dsl_yaml TEXT NOT NULL,
    dmn_xml TEXT NOT NULL,
    test_cases_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS decision_templates_owner_updated_idx
    ON decision_templates (owner_username, updated_at DESC, id DESC);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_templates TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE decision_templates_id_seq TO :"app_user";
\endif
