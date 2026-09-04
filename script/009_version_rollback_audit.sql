-- Immutable rollback provenance and project lifecycle audit events.

BEGIN;

ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS rollback_source_version_id BIGINT;
ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS rollback_reason TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_versions_rollback_source_fk'
    ) THEN
        ALTER TABLE decision_versions
            ADD CONSTRAINT decision_versions_rollback_source_fk
            FOREIGN KEY (rollback_source_version_id)
            REFERENCES decision_versions(id) ON DELETE SET NULL;
    END IF;
END $$;

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

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE audit_events TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE audit_events_id_seq TO :"app_user";
\endif
