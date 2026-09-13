-- Run this file while connected to the default `postgres` database.
-- `\gexec` creates the application database only when it does not exist.
SELECT 'CREATE DATABASE fuxi_dmn'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'fuxi_dmn')\gexec
\connect fuxi_dmn


CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    username VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    role VARCHAR(16) NOT NULL DEFAULT 'user'
        CHECK (role IN ('admin', 'user')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS role VARCHAR(16) NOT NULL DEFAULT 'user';
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_role_check;
ALTER TABLE users
    ADD CONSTRAINT users_role_check CHECK (role IN ('admin', 'user'));

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

CREATE TABLE IF NOT EXISTS project_members (
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    role VARCHAR(16) NOT NULL CHECK (role IN ('modeler', 'reviewer', 'approver', 'viewer')),
    added_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, username, role)
);

CREATE INDEX IF NOT EXISTS project_members_username_idx
    ON project_members (username, project_id);

CREATE TABLE IF NOT EXISTS decision_versions (
    id BIGSERIAL PRIMARY KEY,
    revision_id VARCHAR(64) NOT NULL DEFAULT md5(random()::text || clock_timestamp()::text),
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    version_number INTEGER NOT NULL CHECK (version_number > 0),
    status VARCHAR(16) NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'validated', 'reviewed', 'published', 'retired')),
    base_version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    rollback_source_version_id BIGINT,
    rollback_reason TEXT,
    change_summary JSONB,
    created_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_by VARCHAR(255) REFERENCES users(username),
    reviewed_at TIMESTAMPTZ,
    UNIQUE (revision_id),
    UNIQUE (project_id, version_number)
);

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

CREATE INDEX IF NOT EXISTS decision_versions_project_created_idx
    ON decision_versions (project_id, created_at DESC);

ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS rollback_source_version_id BIGINT;
ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS rollback_reason TEXT;

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
    executed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    business_reference VARCHAR(255) NOT NULL DEFAULT '',
    actual_outcome_json JSONB,
    outcome_assessment VARCHAR(16)
        CHECK (outcome_assessment IS NULL OR outcome_assessment IN ('confirmed', 'mismatch', 'inconclusive')),
    outcome_note TEXT NOT NULL DEFAULT '',
    outcome_occurred_at TIMESTAMPTZ,
    outcome_reported_by VARCHAR(255) REFERENCES users(username),
    outcome_reported_at TIMESTAMPTZ
);

ALTER TABLE decision_runs
    ADD COLUMN IF NOT EXISTS business_reference VARCHAR(255) NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS actual_outcome_json JSONB,
    ADD COLUMN IF NOT EXISTS outcome_assessment VARCHAR(16),
    ADD COLUMN IF NOT EXISTS outcome_note TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS outcome_occurred_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS outcome_reported_by VARCHAR(255) REFERENCES users(username),
    ADD COLUMN IF NOT EXISTS outcome_reported_at TIMESTAMPTZ;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_runs_outcome_assessment_check'
    ) THEN
        ALTER TABLE decision_runs
            ADD CONSTRAINT decision_runs_outcome_assessment_check
            CHECK (outcome_assessment IS NULL OR outcome_assessment IN ('confirmed', 'mismatch', 'inconclusive'));
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS decision_runs_project_executed_idx
    ON decision_runs (project_id, executed_at DESC);
CREATE INDEX IF NOT EXISTS decision_runs_project_outcome_idx
    ON decision_runs (project_id, outcome_assessment, executed_at DESC);

-- Public, login-free invocation credentials. Only a keyed hash is persisted;
-- the secret is shown once when a credential is created.
CREATE TABLE IF NOT EXISTS decision_service_api_keys (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    service_id VARCHAR(255) NOT NULL,
    name VARCHAR(120) NOT NULL,
    key_prefix VARCHAR(24) NOT NULL UNIQUE,
    key_hash VARCHAR(255) NOT NULL,
    created_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS decision_service_api_keys_scope_idx
    ON decision_service_api_keys (project_id, service_id, revoked_at);

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

CREATE TABLE IF NOT EXISTS audit_events (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    event_type VARCHAR(64) NOT NULL,
    actor_username VARCHAR(255) NOT NULL,
    summary VARCHAR(240) NOT NULL,
    details_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS audit_events_project_created_idx
    ON audit_events (project_id, created_at DESC, id DESC);

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

CREATE TABLE IF NOT EXISTS knowledge_sources (
    id BIGSERIAL PRIMARY KEY,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    source_type VARCHAR(16) NOT NULL CHECK (source_type IN ('term', 'field', 'policy')),
    name VARCHAR(160) NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL,
    version_label VARCHAR(80) NOT NULL DEFAULT '',
    source_uri TEXT NOT NULL DEFAULT '',
    valid_from DATE,
    valid_until DATE,
    status VARCHAR(16) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (valid_until IS NULL OR valid_from IS NULL OR valid_until >= valid_from)
);

CREATE INDEX IF NOT EXISTS knowledge_sources_owner_updated_idx
    ON knowledge_sources (owner_username, updated_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS project_knowledge_sources (
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    knowledge_source_id BIGINT NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    attached_by VARCHAR(255) NOT NULL REFERENCES users(username),
    attached_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, knowledge_source_id)
);

CREATE INDEX IF NOT EXISTS project_knowledge_sources_source_idx
    ON project_knowledge_sources (knowledge_source_id, project_id);

CREATE TABLE IF NOT EXISTS knowledge_audit_events (
    id BIGSERIAL PRIMARY KEY,
    knowledge_source_id BIGINT REFERENCES knowledge_sources(id) ON DELETE SET NULL,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    event_type VARCHAR(64) NOT NULL,
    summary VARCHAR(240) NOT NULL,
    details_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS knowledge_audit_events_owner_created_idx
    ON knowledge_audit_events (owner_username, created_at DESC, id DESC);

ALTER TABLE decision_artifacts
    ADD COLUMN IF NOT EXISTS knowledge_citations_json JSONB NOT NULL DEFAULT '[]'::JSONB;
ALTER TABLE decision_templates
    ADD COLUMN IF NOT EXISTS knowledge_citations_json JSONB NOT NULL DEFAULT '[]'::JSONB;

CREATE TABLE IF NOT EXISTS decision_workflows (
    project_id BIGINT PRIMARY KEY REFERENCES decision_projects(id) ON DELETE CASCADE,
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
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_versions_rollback_source_fk'
    ) THEN
        ALTER TABLE decision_versions
            ADD CONSTRAINT decision_versions_rollback_source_fk
            FOREIGN KEY (rollback_source_version_id)
            REFERENCES decision_versions(id) ON DELETE SET NULL;
    END IF;
END $$;

-- Persisted model-change preview candidates (safe preview/commit workflow).
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
UPDATE model_change_candidates candidate
SET base_revision_id = version.revision_id
FROM decision_versions version
WHERE candidate.base_version_id = version.id
  AND candidate.base_revision_id IS NULL;
ALTER TABLE model_change_candidates
    ALTER COLUMN base_revision_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS model_change_candidates_project_idx
    ON model_change_candidates(project_id, created_at DESC);

-- Optional application-role grants. Invoke with `-v app_user=fuxi` when the
-- tables are created by a different administrator role.
\if :{?app_user}
GRANT USAGE ON SCHEMA public TO :"app_user";
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
    users,
    decision_projects,
    project_members,
    decision_versions,
    decision_artifacts,
    decision_test_cases,
    decision_runs,
    decision_optimization_proposals,
    audit_events,
    decision_templates,
    knowledge_sources,
    project_knowledge_sources,
    knowledge_audit_events,
    agent_runs,
    decision_workflows,
    model_change_candidates,
    decision_service_api_keys
TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO :"app_user";
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO :"app_user";
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO :"app_user";
\endif

-- 插入默认管理员账号
-- 默认密码: admin1234 (生产环境必须立即修改!)
-- 密码哈希使用 bcrypt，cost=12
INSERT INTO users (username, password_hash, role, created_at)
VALUES (
    'admin',
    '$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/LewY5GyYIkJdx.HvG', --admin1234
    'admin',
    NOW()
)
ON CONFLICT (username) DO UPDATE
SET
    password_hash = EXCLUDED.password_hash,
    role = EXCLUDED.role;

INSERT INTO users (username, password_hash, role, created_at)
VALUES (
           'fuxi',
           '$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/LewY5GyYIkJdx.HvG', --admin1234
           'admin',
           NOW()
       )
  ON CONFLICT (username) DO UPDATE
  SET
      password_hash = EXCLUDED.password_hash,
  role = EXCLUDED.role;

