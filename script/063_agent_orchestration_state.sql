BEGIN;

CREATE TABLE IF NOT EXISTS agent_orchestrations (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    trigger_kind VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'running'
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    current_stage VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    CHECK ((status IN ('pending', 'running')) = (finished_at IS NULL))
);

CREATE INDEX IF NOT EXISTS agent_orchestrations_project_created_idx
    ON agent_orchestrations (project_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS agent_orchestrations_status_created_idx
    ON agent_orchestrations (status, created_at)
    WHERE status IN ('pending', 'running');

ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;

CREATE TABLE IF NOT EXISTS agent_steps (
    id BIGSERIAL PRIMARY KEY,
    orchestration_id BIGINT NOT NULL REFERENCES agent_orchestrations(id) ON DELETE CASCADE,
    agent_run_id BIGINT REFERENCES agent_runs(id) ON DELETE CASCADE,
    step_no INTEGER NOT NULL CHECK (step_no > 0),
    agent_role VARCHAR(64) NOT NULL,
    task_kind VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'running'
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    model VARCHAR(255),
    prompt_version VARCHAR(64),
    artifact_kind VARCHAR(64),
    artifact_schema_version SMALLINT NOT NULL DEFAULT 1 CHECK (artifact_schema_version > 0),
    artifact_digest CHAR(64),
    usage_json JSONB,
    error_json JSONB,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    UNIQUE (orchestration_id, step_no),
    UNIQUE (orchestration_id, id),
    CHECK ((status IN ('pending', 'running')) = (finished_at IS NULL))
);

ALTER TABLE agent_steps ALTER COLUMN agent_run_id DROP NOT NULL;
ALTER TABLE agent_steps DROP CONSTRAINT IF EXISTS agent_steps_agent_run_id_key;
CREATE UNIQUE INDEX IF NOT EXISTS agent_steps_agent_run_idx
    ON agent_steps (agent_run_id) WHERE agent_run_id IS NOT NULL;
ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS artifact_kind VARCHAR(64);
ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS artifact_schema_version SMALLINT NOT NULL DEFAULT 1;
ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS artifact_digest CHAR(64);
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_steps_artifact_schema_version_check'
          AND conrelid = 'agent_steps'::regclass
    ) THEN
        ALTER TABLE agent_steps
            ADD CONSTRAINT agent_steps_artifact_schema_version_check
            CHECK (artifact_schema_version > 0);
    END IF;
END;
$$;

CREATE INDEX IF NOT EXISTS agent_steps_orchestration_idx
    ON agent_steps (orchestration_id, step_no);

CREATE TABLE IF NOT EXISTS agent_handoffs (
    id BIGSERIAL PRIMARY KEY,
    orchestration_id BIGINT NOT NULL REFERENCES agent_orchestrations(id) ON DELETE CASCADE,
    from_step_id BIGINT NOT NULL,
    to_step_id BIGINT NOT NULL,
    context_digest CHAR(64) NOT NULL,
    context_summary_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (from_step_id <> to_step_id),
    FOREIGN KEY (orchestration_id, from_step_id)
        REFERENCES agent_steps(orchestration_id, id) ON DELETE CASCADE,
    FOREIGN KEY (orchestration_id, to_step_id)
        REFERENCES agent_steps(orchestration_id, id) ON DELETE CASCADE,
    UNIQUE (from_step_id, to_step_id)
);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
    agent_orchestrations, agent_steps, agent_handoffs TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE
    agent_orchestrations_id_seq, agent_steps_id_seq, agent_handoffs_id_seq TO :"app_user";
\endif
