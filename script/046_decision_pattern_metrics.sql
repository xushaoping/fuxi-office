BEGIN;

ALTER TABLE decision_patterns
    ADD COLUMN IF NOT EXISTS success_rate NUMERIC(5, 2),
    ADD COLUMN IF NOT EXISTS usage_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS last_used TIMESTAMPTZ;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_patterns_success_rate_check'
    ) THEN
        ALTER TABLE decision_patterns
            ADD CONSTRAINT decision_patterns_success_rate_check
            CHECK (success_rate IS NULL OR (success_rate >= 0 AND success_rate <= 100));
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_patterns_usage_count_check'
    ) THEN
        ALTER TABLE decision_patterns
            ADD CONSTRAINT decision_patterns_usage_count_check
            CHECK (usage_count >= 0);
    END IF;
END $$;

COMMIT;
