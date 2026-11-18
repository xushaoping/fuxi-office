BEGIN;

CREATE TABLE IF NOT EXISTS agent_model_votes (
    id BIGSERIAL PRIMARY KEY,
    run_id BIGINT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    task_type TEXT NOT NULL,
    models_used TEXT[] NOT NULL DEFAULT '{}',
    consensus_result JSONB NOT NULL,
    divergence_detail JSONB NOT NULL,
    final_decision JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (run_id, task_type)
);

CREATE INDEX IF NOT EXISTS agent_model_votes_created_at_idx
    ON agent_model_votes (created_at DESC);

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE agent_model_votes TO :"app_user";
GRANT USAGE, SELECT, UPDATE
    ON SEQUENCE agent_model_votes_id_seq TO :"app_user";
\endif

COMMIT;
