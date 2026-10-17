BEGIN;

ALTER TABLE decision_workflows
    ADD COLUMN IF NOT EXISTS base_revision BIGINT,
    ADD COLUMN IF NOT EXISTS original_requirement_text TEXT;

COMMIT;
