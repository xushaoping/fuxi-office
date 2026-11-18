BEGIN;

CREATE TABLE IF NOT EXISTS decision_drift_automation_rules (
    project_id BIGINT PRIMARY KEY REFERENCES decision_projects(id) ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    finding_codes TEXT[] NOT NULL DEFAULT '{}',
    cooldown_hours INTEGER NOT NULL DEFAULT 24
        CHECK (cooldown_hours BETWEEN 1 AND 720),
    next_eligible_at TIMESTAMPTZ,
    updated_by VARCHAR(255) NOT NULL REFERENCES users(username),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (cardinality(finding_codes) <= 20)
);

CREATE TABLE IF NOT EXISTS decision_drift_events (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    source_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    dedupe_key TEXT NOT NULL,
    finding_codes TEXT[] NOT NULL,
    evidence_json JSONB NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'dismissed', 'proposal_created')),
    observed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ,
    UNIQUE (project_id, dedupe_key)
);

CREATE INDEX IF NOT EXISTS decision_drift_events_project_observed_idx
    ON decision_drift_events (project_id, observed_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS decision_drift_rules_enabled_idx
    ON decision_drift_automation_rules (project_id)
    WHERE enabled;

COMMIT;
