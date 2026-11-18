BEGIN;

CREATE INDEX IF NOT EXISTS decision_patterns_owner_created_idx
    ON decision_patterns (owner_username, created_at DESC, id DESC);

COMMIT;
