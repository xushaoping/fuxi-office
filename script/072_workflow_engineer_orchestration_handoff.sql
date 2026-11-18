BEGIN;

ALTER TABLE agent_orchestrations
    ADD COLUMN IF NOT EXISTS parent_orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_orchestrations_parent_not_self_check'
          AND conrelid = 'agent_orchestrations'::regclass
    ) THEN
        ALTER TABLE agent_orchestrations
            ADD CONSTRAINT agent_orchestrations_parent_not_self_check
            CHECK (parent_orchestration_id IS NULL OR parent_orchestration_id <> id);
    END IF;
END;
$$;

CREATE INDEX IF NOT EXISTS agent_orchestrations_parent_idx
    ON agent_orchestrations (parent_orchestration_id)
    WHERE parent_orchestration_id IS NOT NULL;

ALTER TABLE decision_workflows
    ADD COLUMN IF NOT EXISTS plan_orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;

COMMIT;
