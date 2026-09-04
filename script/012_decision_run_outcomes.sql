-- Actual business outcomes and governed feedback for published decision runs.

BEGIN;

ALTER TABLE decision_runs
    ADD COLUMN IF NOT EXISTS business_reference VARCHAR(255) NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS actual_outcome_json JSONB,
    ADD COLUMN IF NOT EXISTS outcome_assessment VARCHAR(16),
    ADD COLUMN IF NOT EXISTS outcome_note TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS outcome_occurred_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS outcome_reported_by VARCHAR(255) REFERENCES users(username),
    ADD COLUMN IF NOT EXISTS outcome_reported_at TIMESTAMPTZ;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_runs_outcome_assessment_check'
    ) THEN
        ALTER TABLE decision_runs
            ADD CONSTRAINT decision_runs_outcome_assessment_check
            CHECK (outcome_assessment IS NULL OR outcome_assessment IN ('confirmed', 'mismatch', 'inconclusive'));
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS decision_runs_project_outcome_idx
    ON decision_runs (project_id, outcome_assessment, executed_at DESC);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_runs TO :"app_user";
\endif
