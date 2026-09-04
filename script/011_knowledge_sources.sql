-- Governed enterprise knowledge sources, project citations and version snapshots.

BEGIN;

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

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
    knowledge_sources,
    project_knowledge_sources,
    knowledge_audit_events
TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE
    knowledge_sources_id_seq,
    knowledge_audit_events_id_seq
TO :"app_user";
\endif
