BEGIN;

CREATE TABLE IF NOT EXISTS decision_drift_scan_windows (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    source_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    scan_day DATE NOT NULL,
    evidence_json JSONB NOT NULL,
    review_label TEXT,
    reviewed_by TEXT,
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (project_id, source_version_id, scan_day),
    CONSTRAINT decision_drift_scan_window_review_check CHECK (
        (review_label IS NULL OR review_label IN ('drift', 'normal', 'inconclusive'))
        AND (review_label IS NULL) = (reviewed_by IS NULL)
        AND (review_label IS NULL) = (reviewed_at IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS decision_drift_scan_windows_review_idx
    ON decision_drift_scan_windows (project_id, id)
    WHERE review_label IS NULL;

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_drift_scan_windows TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON SEQUENCE decision_drift_scan_windows_id_seq TO :"app_user";
\endif
