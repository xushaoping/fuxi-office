BEGIN;

ALTER TABLE agent_orchestrations
    ADD COLUMN IF NOT EXISTS retry_of_orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS agent_orchestrations_retry_of_idx
    ON agent_orchestrations (retry_of_orchestration_id)
    WHERE retry_of_orchestration_id IS NOT NULL;

COMMIT;
