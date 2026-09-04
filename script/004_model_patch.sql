BEGIN;

ALTER TABLE decision_artifacts
    ADD COLUMN IF NOT EXISTS model_patch_json JSONB;

COMMIT;
