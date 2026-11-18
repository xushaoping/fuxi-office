BEGIN;

ALTER TABLE decision_drift_automation_rules
    ADD COLUMN IF NOT EXISTS webhook_url TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_drift_webhook_url_check'
          AND conrelid = 'decision_drift_automation_rules'::regclass
    ) THEN
        ALTER TABLE decision_drift_automation_rules
            ADD CONSTRAINT decision_drift_webhook_url_check
            CHECK (webhook_url IS NULL OR
                   (length(webhook_url) <= 2048 AND webhook_url ~ '^https://[^[:space:]]+$'));
    END IF;
END;
$$;

CREATE TABLE IF NOT EXISTS decision_drift_webhook_deliveries (
    id BIGSERIAL PRIMARY KEY,
    event_id BIGINT NOT NULL UNIQUE
        REFERENCES decision_drift_events(id) ON DELETE CASCADE,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    endpoint_url TEXT NOT NULL CHECK (length(endpoint_url) <= 2048),
    payload_json JSONB NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'sending', 'delivered', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    locked_at TIMESTAMPTZ,
    last_http_status SMALLINT,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ,
    CHECK ((status = 'sending') = (locked_at IS NOT NULL))
);

CREATE INDEX IF NOT EXISTS decision_drift_webhook_pending_idx
    ON decision_drift_webhook_deliveries (next_attempt_at, id)
    WHERE status IN ('pending', 'sending');

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
    decision_drift_webhook_deliveries TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE
    decision_drift_webhook_deliveries_id_seq TO :"app_user";
\endif
