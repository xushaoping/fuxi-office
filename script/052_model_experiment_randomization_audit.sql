BEGIN;

ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_treatment_percent SMALLINT
        CHECK (experiment_treatment_percent BETWEEN 0 AND 100);
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_bucket SMALLINT
        CHECK (experiment_bucket BETWEEN 0 AND 99);

DO $$
BEGIN
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
        SELECT 1
        FROM pg_constraint
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

COMMIT;
