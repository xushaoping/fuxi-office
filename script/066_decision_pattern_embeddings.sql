BEGIN;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_type type
        JOIN pg_namespace namespace ON namespace.oid = type.typnamespace
        WHERE type.typname = 'vector'
          AND namespace.nspname = ANY(current_schemas(true))
    ) THEN
        EXECUTE 'ALTER TABLE decision_patterns ADD COLUMN IF NOT EXISTS embedding vector(1024)';
        ALTER TABLE decision_patterns ADD COLUMN IF NOT EXISTS embedding_model TEXT;
        EXECUTE 'CREATE INDEX IF NOT EXISTS decision_patterns_embedding_idx ON decision_patterns USING hnsw (embedding vector_cosine_ops)';
    END IF;
END;
$$;

COMMIT;
