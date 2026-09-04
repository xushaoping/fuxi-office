BEGIN;

ALTER TABLE decision_artifacts
    ADD COLUMN IF NOT EXISTS change_review_json JSONB;

COMMIT;
