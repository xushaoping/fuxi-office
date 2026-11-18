BEGIN;

CREATE TABLE IF NOT EXISTS decision_run_explanation_reviews (
    id BIGSERIAL PRIMARY KEY,
    explanation_id BIGINT NOT NULL
        REFERENCES decision_run_explanations(id) ON DELETE CASCADE,
    reviewer_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    review_label VARCHAR(24) NOT NULL
        CHECK (review_label IN ('readable', 'needs_revision', 'inconclusive')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS decision_run_explanation_reviews_history_idx
    ON decision_run_explanation_reviews (explanation_id, created_at DESC, id DESC);

COMMENT ON TABLE decision_run_explanation_reviews IS
    'Append-only reviewer labels for decision explanation readability; contains no business payload.';

\if :{?app_user}
GRANT SELECT, INSERT ON TABLE decision_run_explanation_reviews TO :"app_user";
REVOKE UPDATE, DELETE ON TABLE decision_run_explanation_reviews FROM :"app_user";
GRANT USAGE, SELECT ON SEQUENCE decision_run_explanation_reviews_id_seq TO :"app_user";
\endif

COMMIT;
