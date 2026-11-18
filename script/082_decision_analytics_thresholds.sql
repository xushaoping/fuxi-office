BEGIN;

CREATE TABLE IF NOT EXISTS decision_analytics_thresholds (
    project_id BIGINT PRIMARY KEY
        REFERENCES decision_projects(id) ON DELETE CASCADE,
    failure_warning_rate DOUBLE PRECISION NOT NULL DEFAULT 0.10
        CHECK (failure_warning_rate > 0 AND failure_warning_rate < 1),
    failure_high_rate DOUBLE PRECISION NOT NULL DEFAULT 0.20
        CHECK (failure_high_rate > failure_warning_rate AND failure_high_rate <= 1),
    mismatch_warning_rate DOUBLE PRECISION NOT NULL DEFAULT 0.20
        CHECK (mismatch_warning_rate > 0 AND mismatch_warning_rate < 1),
    mismatch_high_rate DOUBLE PRECISION NOT NULL DEFAULT 0.30
        CHECK (mismatch_high_rate > mismatch_warning_rate AND mismatch_high_rate <= 1),
    updated_by VARCHAR(255) REFERENCES users(username) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_analytics_thresholds TO :"app_user";
\endif
