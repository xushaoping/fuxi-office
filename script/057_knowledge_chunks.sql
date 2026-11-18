BEGIN;

DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS vector;
EXCEPTION
    WHEN insufficient_privilege OR undefined_file OR feature_not_supported THEN
        RAISE NOTICE 'pgvector is unavailable; knowledge search will use keyword retrieval';
END;
$$;

CREATE TABLE IF NOT EXISTS knowledge_chunks (
    id BIGSERIAL PRIMARY KEY,
    knowledge_source_id BIGINT NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (knowledge_source_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS knowledge_chunks_source_idx
    ON knowledge_chunks (knowledge_source_id, chunk_index);
CREATE INDEX IF NOT EXISTS knowledge_chunks_content_idx
    ON knowledge_chunks USING GIN (to_tsvector('simple', content));

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_type type
        JOIN pg_namespace namespace ON namespace.oid = type.typnamespace
        WHERE type.typname = 'vector'
          AND namespace.nspname = ANY(current_schemas(true))
    ) THEN
        EXECUTE 'ALTER TABLE knowledge_chunks ADD COLUMN IF NOT EXISTS embedding vector(1024)';
        ALTER TABLE knowledge_chunks ADD COLUMN IF NOT EXISTS embedding_model TEXT;
        EXECUTE 'CREATE INDEX IF NOT EXISTS knowledge_chunks_embedding_idx ON knowledge_chunks USING hnsw (embedding vector_cosine_ops)';
    END IF;
END;
$$;

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE knowledge_chunks TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE knowledge_chunks_id_seq TO :"app_user";
\endif
