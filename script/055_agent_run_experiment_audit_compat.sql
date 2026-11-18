BEGIN;

-- Compatibility migration for databases upgraded before the experiment audit
-- migration was applied. Keep every audit column here so partial upgrades do
-- not fail one column at a time when a new agent run is recorded.
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_name TEXT;
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_variant TEXT;
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_treatment_percent SMALLINT;
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_bucket SMALLINT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_runs_experiment_variant_allowed_check'
          AND conrelid = 'agent_runs'::regclass
    ) THEN
        ALTER TABLE agent_runs
            ADD CONSTRAINT agent_runs_experiment_variant_allowed_check
            CHECK (experiment_variant IS NULL OR experiment_variant IN ('control', 'treatment'));
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname IN (
            'agent_runs_experiment_treatment_percent_range_check',
            'agent_runs_experiment_treatment_percent_check'
        )
          AND conrelid = 'agent_runs'::regclass
    ) THEN
        ALTER TABLE agent_runs
            ADD CONSTRAINT agent_runs_experiment_treatment_percent_range_check
            CHECK (
                experiment_treatment_percent IS NULL
                OR experiment_treatment_percent BETWEEN 0 AND 100
            );
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname IN (
            'agent_runs_experiment_bucket_range_check',
            'agent_runs_experiment_bucket_check'
        )
          AND conrelid = 'agent_runs'::regclass
    ) THEN
        ALTER TABLE agent_runs
            ADD CONSTRAINT agent_runs_experiment_bucket_range_check
            CHECK (experiment_bucket IS NULL OR experiment_bucket BETWEEN 0 AND 99);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_runs_experiment_audit_check'
          AND conrelid = 'agent_runs'::regclass
    ) THEN
        ALTER TABLE agent_runs
            ADD CONSTRAINT agent_runs_experiment_audit_check
            CHECK (
                (experiment_treatment_percent IS NULL) = (experiment_bucket IS NULL)
                AND (
                    experiment_bucket IS NULL
                    OR (
                        experiment_variant IS NOT NULL
                        AND ((experiment_bucket < experiment_treatment_percent)
                             = (experiment_variant = 'treatment'))
                    )
                )
            );
    END IF;
END;
$$;

CREATE INDEX IF NOT EXISTS agent_runs_experiment_idx
    ON agent_runs (experiment_name, experiment_variant, started_at DESC)
    WHERE experiment_name IS NOT NULL;

COMMIT;
