BEGIN;

CREATE TABLE IF NOT EXISTS decision_run_outcome_receipts (
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    service_id VARCHAR(255) NOT NULL,
    idempotency_key VARCHAR(128) NOT NULL,
    run_id BIGINT NOT NULL REFERENCES decision_runs(id) ON DELETE CASCADE,
    payload_digest CHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, service_id, idempotency_key)
);

CREATE INDEX IF NOT EXISTS decision_run_outcome_receipts_run_idx
    ON decision_run_outcome_receipts (project_id, run_id);

CREATE TABLE IF NOT EXISTS deviation_analyses (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    decision_run_id BIGINT NOT NULL REFERENCES decision_runs(id) ON DELETE CASCADE,
    status VARCHAR(32) NOT NULL
        CHECK (status IN ('candidate', 'insufficient_evidence')),
    attributed_rule TEXT,
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0
        CHECK (confidence >= 0 AND confidence <= 1),
    evidence_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (decision_run_id)
);

CREATE INDEX IF NOT EXISTS deviation_analyses_project_created_idx
    ON deviation_analyses (project_id, created_at DESC, id DESC);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
    decision_run_outcome_receipts, deviation_analyses TO :"app_user";
GRANT USAGE, SELECT ON SEQUENCE deviation_analyses_id_seq TO :"app_user";
\endif
