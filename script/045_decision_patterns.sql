BEGIN;

CREATE TABLE IF NOT EXISTS decision_patterns (
    id BIGSERIAL PRIMARY KEY,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    source_project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    source_version_id BIGINT NOT NULL UNIQUE REFERENCES decision_versions(id) ON DELETE CASCADE,
    pattern_type TEXT NOT NULL CHECK (pattern_type IN ('tree', 'matrix', 'workflow', 'threshold', 'general')),
    category TEXT NOT NULL DEFAULT '',
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    keywords TEXT[] NOT NULL DEFAULT '{}',
    dsl_snippet TEXT NOT NULL,
    supersedes_pattern_id BIGINT REFERENCES decision_patterns(id) ON DELETE SET NULL,
    evolution_number INTEGER NOT NULL DEFAULT 1 CHECK (evolution_number > 0),
    success_rate NUMERIC(5, 2)
        CHECK (success_rate IS NULL OR (success_rate >= 0 AND success_rate <= 100)),
    usage_count INTEGER NOT NULL DEFAULT 0 CHECK (usage_count >= 0),
    last_used TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS decision_patterns_owner_type_idx
    ON decision_patterns (owner_username, pattern_type, created_at DESC);
CREATE INDEX IF NOT EXISTS decision_patterns_owner_created_idx
    ON decision_patterns (owner_username, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS decision_patterns_keywords_idx
    ON decision_patterns USING GIN (keywords);

CREATE TABLE IF NOT EXISTS pattern_recommendations (
    id BIGSERIAL PRIMARY KEY,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    project_id BIGINT REFERENCES decision_projects(id) ON DELETE CASCADE,
    recommended_pattern_id BIGINT NOT NULL REFERENCES decision_patterns(id) ON DELETE CASCADE,
    match_score NUMERIC(5, 2) NOT NULL CHECK (match_score >= 0 AND match_score <= 100),
    recommendation_reason TEXT NOT NULL,
    user_accepted BOOLEAN,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS pattern_recommendations_owner_created_idx
    ON pattern_recommendations (owner_username, created_at DESC);

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_patterns TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE decision_patterns_id_seq TO :"app_user";
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE pattern_recommendations TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE pattern_recommendations_id_seq TO :"app_user";
\endif

COMMIT;
