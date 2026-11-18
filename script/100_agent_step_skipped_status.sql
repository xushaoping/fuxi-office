BEGIN;

ALTER TABLE agent_steps
    DROP CONSTRAINT IF EXISTS agent_steps_status_check;
ALTER TABLE agent_steps
    ADD CONSTRAINT agent_steps_status_check
    CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled', 'skipped'));

COMMIT;
