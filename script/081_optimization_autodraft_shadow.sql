BEGIN;

CREATE TABLE IF NOT EXISTS optimization_autodraft_shadow_evaluations (
    id BIGSERIAL PRIMARY KEY,
    simulation_id BIGINT NOT NULL UNIQUE
        REFERENCES optimization_simulations(id) ON DELETE CASCADE,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    proposal_id BIGINT NOT NULL REFERENCES decision_optimization_proposals(id) ON DELETE CASCADE,
    source_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    candidate_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    outcome VARCHAR(40) NOT NULL CHECK (outcome IN (
        'eligible_shadow_candidate',
        'blocked_project_risk',
        'blocked_finding_code',
        'blocked_unlabeled_samples',
        'blocked_insufficient_samples',
        'blocked_simulation',
        'blocked_change_risk'
    )),
    finding_code VARCHAR(64) NOT NULL,
    sample_source VARCHAR(32) NOT NULL
        CHECK (sample_source IN ('approved_business_tests', 'successful_history_runs')),
    test_set_size INTEGER NOT NULL CHECK (test_set_size >= 0),
    baseline_accuracy NUMERIC(5, 2) NOT NULL
        CHECK (baseline_accuracy BETWEEN 0 AND 100),
    simulated_accuracy NUMERIC(5, 2) NOT NULL
        CHECK (simulated_accuracy BETWEEN 0 AND 100),
    evaluated_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS optimization_autodraft_shadow_project_idx
    ON optimization_autodraft_shadow_evaluations (project_id, created_at DESC, id DESC);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE optimization_autodraft_shadow_evaluations TO :"app_user";
GRANT USAGE, SELECT, UPDATE
    ON SEQUENCE optimization_autodraft_shadow_evaluations_id_seq TO :"app_user";
\endif
