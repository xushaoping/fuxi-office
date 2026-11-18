BEGIN;

ALTER TABLE decision_analytics_thresholds
    ADD COLUMN IF NOT EXISTS failure_min_runs INTEGER NOT NULL DEFAULT 10
        CHECK (failure_min_runs BETWEEN 1 AND 100000),
    ADD COLUMN IF NOT EXISTS mismatch_min_samples INTEGER NOT NULL DEFAULT 5
        CHECK (mismatch_min_samples BETWEEN 1 AND 100000);

COMMIT;
