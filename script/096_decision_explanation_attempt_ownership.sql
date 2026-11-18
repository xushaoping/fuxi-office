BEGIN;

ALTER TABLE decision_run_explanations
    ADD COLUMN IF NOT EXISTS agent_run_id BIGINT
        REFERENCES agent_runs(id) ON DELETE SET NULL;

-- Existing caches belong to the most recent explanation attempt for that run.
-- Compare JSON as text so malformed legacy input cannot abort the migration.
UPDATE decision_run_explanations explanation
SET agent_run_id = (
    SELECT run.id
    FROM agent_runs run
    JOIN decision_runs decision ON decision.id = explanation.decision_run_id
    WHERE run.project_id = decision.project_id
      AND run.stage = 'decision_explanation'
      AND run.input_json->>'decisionRunId' = explanation.decision_run_id::TEXT
    ORDER BY run.id DESC
    LIMIT 1
)
WHERE explanation.agent_run_id IS NULL;

COMMENT ON COLUMN decision_run_explanations.agent_run_id IS
    'Owning generation attempt; old attempts must never update a newer cache.';

COMMIT;
