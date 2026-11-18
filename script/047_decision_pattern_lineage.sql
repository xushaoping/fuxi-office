BEGIN;

ALTER TABLE decision_patterns
    ADD COLUMN IF NOT EXISTS supersedes_pattern_id BIGINT,
    ADD COLUMN IF NOT EXISTS evolution_number INTEGER NOT NULL DEFAULT 1;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_patterns_supersedes_fk'
    ) THEN
        ALTER TABLE decision_patterns
            ADD CONSTRAINT decision_patterns_supersedes_fk
            FOREIGN KEY (supersedes_pattern_id)
            REFERENCES decision_patterns(id)
            ON DELETE SET NULL;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_patterns_evolution_number_check'
    ) THEN
        ALTER TABLE decision_patterns
            ADD CONSTRAINT decision_patterns_evolution_number_check
            CHECK (evolution_number > 0);
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS decision_patterns_lineage_idx
    ON decision_patterns (source_project_id, evolution_number DESC, id DESC);

COMMIT;
