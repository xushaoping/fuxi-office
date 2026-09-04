-- Login-free, scoped credentials for published decision service execution.
-- Secrets are Argon2 hashes and are never recoverable from the database.
CREATE TABLE IF NOT EXISTS decision_service_api_keys (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    service_id VARCHAR(255) NOT NULL,
    name VARCHAR(120) NOT NULL,
    key_prefix VARCHAR(24) NOT NULL UNIQUE,
    key_hash VARCHAR(255) NOT NULL,
    created_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS decision_service_api_keys_scope_idx
    ON decision_service_api_keys (project_id, service_id, revoked_at);

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_service_api_keys TO :"app_user";
GRANT USAGE, SELECT ON SEQUENCE decision_service_api_keys_id_seq TO :"app_user";
\endif
