BEGIN;

ALTER TABLE decision_optimization_proposals
    ADD COLUMN IF NOT EXISTS effectiveness_result_json JSONB,
    ADD COLUMN IF NOT EXISTS effectiveness_tracked_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS decision_versions_project_base_lineage_idx
    ON decision_versions (project_id, base_version_id, id);

CREATE INDEX IF NOT EXISTS decision_optimization_proposals_effectiveness_pending_idx
    ON decision_optimization_proposals (resulting_version_id)
    WHERE status = 'confirmed' AND effectiveness_tracked_at IS NULL;

COMMIT;
