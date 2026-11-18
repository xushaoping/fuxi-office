BEGIN;

CREATE TABLE IF NOT EXISTS repair_memories (
    id BIGSERIAL PRIMARY KEY,
    error_signature CHAR(64) NOT NULL,
    error_category VARCHAR(2) NOT NULL
        CHECK (error_category IN ('E1', 'E2', 'E3', 'E4', 'E5', 'E6')),
    context_digest CHAR(64),
    repair_summary TEXT NOT NULL,
    repair_diff JSONB,
    outcome VARCHAR(16) NOT NULL
        CHECK (outcome IN ('success', 'partial', 'failed')),
    model VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS repair_memories_signature_idx
    ON repair_memories (error_signature, error_category, outcome, created_at DESC, id DESC);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT ON TABLE repair_memories TO :"app_user";
GRANT USAGE, SELECT ON SEQUENCE repair_memories_id_seq TO :"app_user";
\endif
