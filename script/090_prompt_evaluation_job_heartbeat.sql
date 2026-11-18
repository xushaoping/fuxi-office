BEGIN;

ALTER TABLE prompt_evaluation_jobs
    ADD COLUMN IF NOT EXISTS heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

CREATE INDEX IF NOT EXISTS prompt_evaluation_jobs_stale_idx
    ON prompt_evaluation_jobs (heartbeat_at)
    WHERE status IN ('pending', 'running');

GRANT SELECT, INSERT, UPDATE ON TABLE prompt_evaluation_jobs TO :"app_user";

COMMIT;
