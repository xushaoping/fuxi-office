BEGIN;

-- Bound both the project filter and the ID cursor, including MAX(id).
CREATE INDEX IF NOT EXISTS deviation_analyses_project_id_idx
    ON deviation_analyses (project_id, id);

COMMIT;
