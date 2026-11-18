BEGIN;

ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS attempt_no INTEGER NOT NULL DEFAULT 1;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_steps_attempt_no_check'
          AND conrelid = 'agent_steps'::regclass
    ) THEN
        ALTER TABLE agent_steps
            ADD CONSTRAINT agent_steps_attempt_no_check
            CHECK (attempt_no > 0);
    END IF;
END;
$$;

ALTER TABLE agent_steps
    DROP CONSTRAINT IF EXISTS agent_steps_orchestration_id_step_no_key;
CREATE UNIQUE INDEX IF NOT EXISTS agent_steps_orchestration_step_attempt_idx
    ON agent_steps (orchestration_id, step_no, attempt_no);

COMMIT;
