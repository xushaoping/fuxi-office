BEGIN;

CREATE TABLE IF NOT EXISTS optimization_simulations (
    id BIGSERIAL PRIMARY KEY,
    suggestion_id BIGINT NOT NULL
        REFERENCES decision_optimization_proposals(id) ON DELETE CASCADE,
    baseline_version_id BIGINT NOT NULL
        REFERENCES decision_versions(id) ON DELETE CASCADE,
    version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    test_set_size INTEGER NOT NULL CHECK (test_set_size >= 0),
    baseline_accuracy NUMERIC(5, 2) NOT NULL
        CHECK (baseline_accuracy >= 0 AND baseline_accuracy <= 100),
    simulated_accuracy NUMERIC(5, 2) NOT NULL
        CHECK (simulated_accuracy >= 0 AND simulated_accuracy <= 100),
    accuracy_improvement NUMERIC(6, 2) NOT NULL,
    bias_rate_baseline NUMERIC(5, 2) NOT NULL
        CHECK (bias_rate_baseline >= 0 AND bias_rate_baseline <= 100),
    bias_rate_simulated NUMERIC(5, 2) NOT NULL
        CHECK (bias_rate_simulated >= 0 AND bias_rate_simulated <= 100),
    estimated_business_impact TEXT NOT NULL,
    test_set_source TEXT NOT NULL
        CHECK (test_set_source IN ('approved_business_tests', 'successful_history_runs')),
    created_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    passed BOOLEAN NOT NULL DEFAULT FALSE,
    candidate_artifact_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (baseline_version_id <> version_id),
    CONSTRAINT optimization_simulations_passed_check
        CHECK (NOT passed OR (test_set_size > 0 AND simulated_accuracy >= baseline_accuracy))
);

ALTER TABLE optimization_simulations
    ADD COLUMN IF NOT EXISTS passed BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE optimization_simulations
    ADD COLUMN IF NOT EXISTS candidate_artifact_json JSONB NOT NULL DEFAULT '{}'::JSONB;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'optimization_simulations_passed_check'
          AND conrelid = 'optimization_simulations'::regclass
    ) THEN
        ALTER TABLE optimization_simulations
            ADD CONSTRAINT optimization_simulations_passed_check
            CHECK (NOT passed OR (test_set_size > 0 AND simulated_accuracy >= baseline_accuracy));
    END IF;
END;
$$;

CREATE INDEX IF NOT EXISTS optimization_simulations_suggestion_idx
    ON optimization_simulations(suggestion_id, created_at DESC);

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE optimization_simulations TO :"app_user";
GRANT USAGE, SELECT, UPDATE
    ON SEQUENCE optimization_simulations_id_seq TO :"app_user";
\endif

COMMIT;
