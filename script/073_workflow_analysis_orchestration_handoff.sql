BEGIN;

ALTER TABLE decision_workflows
    ADD COLUMN IF NOT EXISTS analysis_orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS decision_workflows_analysis_orchestration_idx
    ON decision_workflows (analysis_orchestration_id)
    WHERE analysis_orchestration_id IS NOT NULL;

COMMIT;
