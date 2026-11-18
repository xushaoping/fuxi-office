-- Run only against the isolated fresh-init fixture, before applying 096.
INSERT INTO users (username, password_hash, role)
VALUES ('admin', 'fixture-only-not-a-login-hash', 'admin')
ON CONFLICT (username) DO NOTHING;

INSERT INTO decision_projects (owner_username, name)
VALUES ('admin', 'Explanation ownership upgrade fixture') RETURNING id \gset ownership_project_
INSERT INTO decision_versions (project_id, version_number, created_by)
VALUES (:ownership_project_id, 1, 'admin') RETURNING id \gset ownership_version_
INSERT INTO decision_runs (
    project_id, version_id, service_id, service_name, input_json, output_json,
    execution_trace_json, duration_ms, status, executed_by
) VALUES (
    :ownership_project_id, :ownership_version_id, 'fixture', 'fixture', '{}', '{}',
    '{}', 1, 'succeeded', 'admin'
) RETURNING id \gset ownership_decision_
INSERT INTO agent_runs (project_id, stage, status, input_json)
VALUES (:ownership_project_id, 'decision_explanation', 'failed',
    jsonb_build_object('decisionRunId', :ownership_decision_id))
RETURNING id \gset ownership_old_
INSERT INTO agent_runs (project_id, stage, status, input_json)
VALUES (:ownership_project_id, 'decision_explanation', 'running',
    jsonb_build_object('decisionRunId', :ownership_decision_id))
RETURNING id \gset ownership_current_
-- Newer malformed or unrelated Agent input must not win the backfill.
INSERT INTO agent_runs (project_id, stage, status, input_json) VALUES
    (:ownership_project_id, 'decision_explanation', 'failed', '{"decisionRunId":"invalid-id"}'),
    (:ownership_project_id, 'tester_judge', 'running',
        jsonb_build_object('decisionRunId', :ownership_decision_id));
INSERT INTO decision_run_explanations (
    decision_run_id, status, prompt_version, prompt_digest, trace_digest, created_by
) VALUES (:ownership_decision_id, 'running', 'fixture-v1', repeat('a',64), repeat('b',64), 'admin')
RETURNING id \gset ownership_cache_

CREATE TEMP TABLE expected_explanation_ownership AS
SELECT :ownership_cache_id::BIGINT AS cache_id, :ownership_current_id::BIGINT AS expected_owner,
       :ownership_old_id::BIGINT AS prior_owner;

\i script/096_decision_explanation_attempt_ownership.sql
\i script/096_decision_explanation_attempt_ownership.sql
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM decision_run_explanations cache, expected_explanation_ownership expected
        WHERE cache.id = expected.cache_id AND cache.agent_run_id = expected.expected_owner
    ) THEN
        RAISE EXCEPTION '096 did not backfill the most recent explanation attempt';
    END IF;
    BEGIN
        UPDATE decision_run_explanations SET agent_run_id = -1
        WHERE id = (SELECT cache_id FROM expected_explanation_ownership);
        RAISE EXCEPTION '096 accepted nonexistent owning Agent run';
    EXCEPTION WHEN foreign_key_violation THEN NULL;
    END;
END;
$$;

-- Re-applying the migration must not overwrite an explicit owner.
UPDATE decision_run_explanations cache SET agent_run_id = expected.prior_owner
FROM expected_explanation_ownership expected WHERE cache.id = expected.cache_id;
\i script/096_decision_explanation_attempt_ownership.sql
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM decision_run_explanations cache, expected_explanation_ownership expected
        WHERE cache.id = expected.cache_id AND cache.agent_run_id = expected.prior_owner
    ) THEN
        RAISE EXCEPTION '096 overwrote an explicit owner on re-application';
    END IF;
END;
$$;
DELETE FROM decision_runs WHERE id = :ownership_decision_id;
DELETE FROM decision_projects WHERE id = :ownership_project_id;
DROP TABLE expected_explanation_ownership;
