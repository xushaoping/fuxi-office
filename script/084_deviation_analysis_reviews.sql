BEGIN;

ALTER TABLE deviation_analyses
    ADD COLUMN IF NOT EXISTS review_label VARCHAR(32)
        CHECK (review_label IN ('correct', 'incorrect', 'inconclusive')),
    ADD COLUMN IF NOT EXISTS reviewed_by TEXT,
    ADD COLUMN IF NOT EXISTS reviewed_at TIMESTAMPTZ;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'deviation_analyses'::regclass
          AND conname = 'deviation_analyses_review_metadata_check'
    ) THEN
        ALTER TABLE deviation_analyses
            ADD CONSTRAINT deviation_analyses_review_metadata_check
            CHECK (
                (review_label IS NULL AND reviewed_by IS NULL AND reviewed_at IS NULL)
                OR (review_label IS NOT NULL AND reviewed_by IS NOT NULL AND reviewed_at IS NOT NULL)
            );
    END IF;
END;
$$;

COMMIT;
