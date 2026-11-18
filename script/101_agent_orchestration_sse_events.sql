BEGIN;

CREATE TABLE IF NOT EXISTS agent_orchestration_events (
    id BIGSERIAL PRIMARY KEY,
    orchestration_id BIGINT NOT NULL REFERENCES agent_orchestrations(id) ON DELETE CASCADE,
    event_name VARCHAR(32) NOT NULL,
    payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '24 hours'),
    CHECK (event_name IN ('orchestration', 'agent', 'artifact', 'handoff', 'stage', 'step', 'delta', 'preview', 'result', 'workflow_result', 'error'))
);

CREATE INDEX IF NOT EXISTS agent_orchestration_events_replay_idx
    ON agent_orchestration_events (orchestration_id, id);
CREATE INDEX IF NOT EXISTS agent_orchestration_events_expiry_idx
    ON agent_orchestration_events (expires_at);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, DELETE ON TABLE agent_orchestration_events TO :"app_user";
GRANT USAGE, SELECT ON SEQUENCE agent_orchestration_events_id_seq TO :"app_user";
REVOKE UPDATE, TRUNCATE, TRIGGER ON TABLE agent_orchestration_events FROM :"app_user";
\endif
