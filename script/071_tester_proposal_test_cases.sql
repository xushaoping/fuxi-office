BEGIN;

ALTER TABLE decision_test_cases
    ADD COLUMN IF NOT EXISTS tester_proposal_id VARCHAR(64);

ALTER TABLE decision_test_cases
    DROP CONSTRAINT IF EXISTS decision_test_cases_source_check;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_test_cases_source_allowed_check'
          AND conrelid = 'decision_test_cases'::regclass
    ) THEN
        ALTER TABLE decision_test_cases
            ADD CONSTRAINT decision_test_cases_source_allowed_check
            CHECK (source IN ('manual', 'generated', 'tester_proposal'));
    END IF;
END;
$$;

CREATE UNIQUE INDEX IF NOT EXISTS decision_test_cases_tester_proposal_idx
    ON decision_test_cases (version_id, tester_proposal_id)
    WHERE tester_proposal_id IS NOT NULL;

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE ON TABLE decision_test_cases TO :"app_user";
\endif
