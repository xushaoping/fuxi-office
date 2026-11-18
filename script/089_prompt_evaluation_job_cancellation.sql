BEGIN;

ALTER TABLE prompt_evaluation_jobs
    ADD COLUMN IF NOT EXISTS cancel_requested BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE prompt_evaluation_jobs
    DROP CONSTRAINT IF EXISTS prompt_evaluation_jobs_status_check;
ALTER TABLE prompt_evaluation_jobs
    ADD CONSTRAINT prompt_evaluation_jobs_status_check
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled'));

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'prompt_evaluation_jobs_cancelled_finished_check'
          AND conrelid = 'prompt_evaluation_jobs'::regclass
    ) THEN
        ALTER TABLE prompt_evaluation_jobs
            ADD CONSTRAINT prompt_evaluation_jobs_cancelled_finished_check
                CHECK ((status = 'cancelled' AND finished_at IS NOT NULL) OR status <> 'cancelled');
    END IF;
END;
$$;

COMMIT;
