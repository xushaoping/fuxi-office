BEGIN;

ALTER TABLE optimization_simulations
    ADD COLUMN IF NOT EXISTS passed BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE optimization_simulations
    ADD COLUMN IF NOT EXISTS candidate_artifact_json JSONB NOT NULL DEFAULT '{}'::JSONB;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'optimization_simulations_passed_check'
          AND conrelid = 'optimization_simulations'::regclass
    ) THEN
        ALTER TABLE optimization_simulations
            ADD CONSTRAINT optimization_simulations_passed_check
            CHECK (NOT passed OR (test_set_size > 0 AND simulated_accuracy >= baseline_accuracy));
    END IF;
END;
$$;

COMMIT;
