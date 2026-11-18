BEGIN;

ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS prompt_digest CHAR(64);

ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS prompt_digest CHAR(64);

COMMENT ON COLUMN agent_runs.prompt_digest IS
    'SHA-256 of the exact system prompt used by the run; NULL for legacy or non-LLM runs.';
COMMENT ON COLUMN agent_steps.prompt_digest IS
    'SHA-256 of the exact system prompt or deterministic validator identity used by the step.';

GRANT SELECT, INSERT, UPDATE ON TABLE agent_runs, agent_steps TO :"app_user";

COMMIT;
