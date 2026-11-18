BEGIN;

ALTER TABLE decision_optimization_proposals
    DROP CONSTRAINT IF EXISTS decision_optimization_proposals_status_check;

ALTER TABLE decision_optimization_proposals
    ADD CONSTRAINT decision_optimization_proposals_status_check
    CHECK (status IN ('open', 'confirmed', 'dismissed', 'withdrawn'));

COMMIT;
