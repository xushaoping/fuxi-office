-- Add immutable revision tokens for optimistic concurrency on model changes.
BEGIN;

ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS revision_id VARCHAR(64);

UPDATE decision_versions
SET revision_id = md5(random()::text || clock_timestamp()::text)
WHERE revision_id IS NULL;

ALTER TABLE decision_versions
    ALTER COLUMN revision_id SET DEFAULT md5(random()::text || clock_timestamp()::text),
    ALTER COLUMN revision_id SET NOT NULL;

ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS parent_revision_id VARCHAR(64);

CREATE UNIQUE INDEX IF NOT EXISTS decision_versions_revision_id_idx
    ON decision_versions(revision_id);

ALTER TABLE model_change_candidates
    ADD COLUMN IF NOT EXISTS base_revision_id VARCHAR(64);

UPDATE model_change_candidates candidate
SET base_revision_id = version.revision_id
FROM decision_versions version
WHERE candidate.base_version_id = version.id
  AND candidate.base_revision_id IS NULL;
ALTER TABLE model_change_candidates
    ALTER COLUMN base_revision_id SET NOT NULL;

-- Link existing versions to their parent revision where a base version exists.
UPDATE decision_versions child
SET parent_revision_id = parent.revision_id
FROM decision_versions parent
WHERE child.base_version_id = parent.id
  AND child.parent_revision_id IS NULL;

COMMIT;
