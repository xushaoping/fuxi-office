BEGIN;

ALTER TABLE decision_drift_events
    ADD COLUMN IF NOT EXISTS action_result_json JSONB;

COMMIT;
