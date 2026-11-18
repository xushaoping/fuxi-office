BEGIN;

ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_name TEXT;
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_variant TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname IN (
            'agent_runs_experiment_variant_allowed_check',
            'agent_runs_experiment_variant_check'
        )
          AND conrelid = 'agent_runs'::regclass
    ) THEN
        ALTER TABLE agent_runs
            ADD CONSTRAINT agent_runs_experiment_variant_allowed_check
            CHECK (experiment_variant IS NULL OR experiment_variant IN ('control', 'treatment'));
    END IF;
END;
$$;

CREATE INDEX IF NOT EXISTS agent_runs_experiment_idx
    ON agent_runs (experiment_name, experiment_variant, started_at DESC)
    WHERE experiment_name IS NOT NULL;

COMMIT;
