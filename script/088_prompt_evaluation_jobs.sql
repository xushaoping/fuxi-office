BEGIN;

CREATE TABLE IF NOT EXISTS prompt_evaluation_jobs (
    id BIGSERIAL PRIMARY KEY,
    prompt_asset_id BIGINT NOT NULL REFERENCES prompt_assets(id) ON DELETE RESTRICT,
    created_by TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed')),
    completed_cases INTEGER NOT NULL DEFAULT 0 CHECK (completed_cases >= 0),
    total_cases INTEGER NOT NULL CHECK (total_cases > 0),
    result_json JSONB,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    CHECK (completed_cases <= total_cases),
    CHECK ((status = 'succeeded' AND result_json IS NOT NULL AND finished_at IS NOT NULL)
        OR status <> 'succeeded'),
    CHECK ((status = 'failed' AND error_message IS NOT NULL AND finished_at IS NOT NULL)
        OR status <> 'failed')
);

CREATE UNIQUE INDEX IF NOT EXISTS prompt_evaluation_jobs_one_active_per_asset_idx
    ON prompt_evaluation_jobs (prompt_asset_id)
    WHERE status IN ('pending', 'running');
CREATE INDEX IF NOT EXISTS prompt_evaluation_jobs_history_idx
    ON prompt_evaluation_jobs (prompt_asset_id, created_at DESC, id DESC);

GRANT SELECT, INSERT, UPDATE ON TABLE prompt_evaluation_jobs TO :"app_user";
GRANT USAGE, SELECT ON SEQUENCE prompt_evaluation_jobs_id_seq TO :"app_user";

COMMIT;
