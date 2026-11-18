BEGIN;

ALTER TABLE agent_orchestrations
    ADD COLUMN IF NOT EXISTS heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

CREATE INDEX IF NOT EXISTS agent_orchestrations_heartbeat_idx
    ON agent_orchestrations (heartbeat_at)
    WHERE status IN ('pending', 'running');

COMMIT;
