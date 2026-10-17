-- Upgrade an existing fuxi_dmn database with project/version persistence.
-- Run while connected to fuxi_dmn, optionally with: -v app_user=fuxi

BEGIN;

CREATE TABLE IF NOT EXISTS decision_projects (
    id BIGSERIAL PRIMARY KEY,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    name VARCHAR(120) NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    risk_level VARCHAR(16) NOT NULL DEFAULT 'medium'
        CHECK (risk_level IN ('low', 'medium', 'high')),
    current_draft_version_id BIGINT,
    published_version_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS decision_projects_owner_updated_idx
    ON decision_projects (owner_username, updated_at DESC);

CREATE TABLE IF NOT EXISTS decision_versions (
    id BIGSERIAL PRIMARY KEY,
    revision_id VARCHAR(64) NOT NULL DEFAULT md5(random()::text || clock_timestamp()::text),
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    version_number INTEGER NOT NULL CHECK (version_number > 0),
    status VARCHAR(16) NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'validated', 'reviewed', 'published', 'retired')),
    base_version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    change_summary JSONB,
    created_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_by VARCHAR(255) REFERENCES users(username),
    reviewed_at TIMESTAMPTZ,
    UNIQUE (revision_id),
    UNIQUE (project_id, version_number)
);

CREATE INDEX IF NOT EXISTS decision_versions_project_created_idx
    ON decision_versions (project_id, created_at DESC);

ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS revision_id VARCHAR(64);
UPDATE decision_versions
SET revision_id = md5(random()::text || clock_timestamp()::text)
WHERE revision_id IS NULL;
ALTER TABLE decision_versions
    ALTER COLUMN revision_id SET DEFAULT md5(random()::text || clock_timestamp()::text),
    ALTER COLUMN revision_id SET NOT NULL;
ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS parent_revision_id VARCHAR(64);
CREATE UNIQUE INDEX IF NOT EXISTS decision_versions_revision_id_idx
    ON decision_versions(revision_id);

CREATE TABLE IF NOT EXISTS decision_artifacts (
    id BIGSERIAL PRIMARY KEY,
    version_id BIGINT NOT NULL UNIQUE REFERENCES decision_versions(id) ON DELETE CASCADE,
    decision_brief_json JSONB,
    decision_plan_json JSONB,
    model_patch_json JSONB,
    change_review_json JSONB,
    dsl_yaml TEXT NOT NULL,
    dmn_xml TEXT NOT NULL,
    verification_report_json JSONB,
    content_hash VARCHAR(128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE decision_artifacts
    ADD COLUMN IF NOT EXISTS model_patch_json JSONB;
ALTER TABLE decision_artifacts
    ADD COLUMN IF NOT EXISTS change_review_json JSONB;

CREATE TABLE IF NOT EXISTS agent_runs (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    stage VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    model VARCHAR(255),
    prompt_version VARCHAR(64),
    input_json JSONB,
    output_json JSONB,
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    error_json JSONB,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS agent_runs_project_started_idx
    ON agent_runs (project_id, started_at DESC);

CREATE TABLE IF NOT EXISTS decision_workflows (
    project_id BIGINT PRIMARY KEY REFERENCES decision_projects(id) ON DELETE CASCADE,
    base_revision BIGINT,
    original_requirement_text TEXT,
    status VARCHAR(24) NOT NULL
        CHECK (status IN ('analyzed', 'clarification', 'planned', 'confirmed')),
    requirement_text TEXT NOT NULL,
    decision_brief_json JSONB NOT NULL,
    clarifications_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    answers_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    decision_plan_json JSONB,
    confirmed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_projects_current_draft_fk'
    ) THEN
        ALTER TABLE decision_projects
            ADD CONSTRAINT decision_projects_current_draft_fk
            FOREIGN KEY (current_draft_version_id)
            REFERENCES decision_versions(id) ON DELETE SET NULL;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_projects_published_fk'
    ) THEN
        ALTER TABLE decision_projects
            ADD CONSTRAINT decision_projects_published_fk
            FOREIGN KEY (published_version_id)
            REFERENCES decision_versions(id) ON DELETE SET NULL;
    END IF;
END $$;

COMMIT;

\if :{?app_user}
GRANT USAGE ON SCHEMA public TO :"app_user";
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
    decision_projects,
    decision_versions,
    decision_artifacts,
    agent_runs,
    decision_workflows
TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO :"app_user";
\endif
