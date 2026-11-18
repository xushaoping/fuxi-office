BEGIN;

ALTER TABLE decision_drift_events
    ADD COLUMN IF NOT EXISTS review_label TEXT,
    ADD COLUMN IF NOT EXISTS reviewed_by TEXT,
    ADD COLUMN IF NOT EXISTS reviewed_at TIMESTAMPTZ;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_drift_event_review_check'
          AND conrelid = 'decision_drift_events'::regclass
    ) THEN
        ALTER TABLE decision_drift_events
            ADD CONSTRAINT decision_drift_event_review_check
            CHECK (
                (review_label IS NULL OR review_label IN ('confirmed_drift', 'false_positive', 'inconclusive'))
                AND (review_label IS NULL) = (reviewed_by IS NULL)
                AND (review_label IS NULL) = (reviewed_at IS NULL)
            );
    END IF;
END;
$$;

COMMIT;

\if :{?app_user}
GRANT SELECT, UPDATE ON TABLE decision_drift_events TO :"app_user";
\endif
