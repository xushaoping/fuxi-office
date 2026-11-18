CREATE TABLE decision_runs (
    id BIGINT PRIMARY KEY,
    project_id BIGINT NOT NULL,
    version_id BIGINT NOT NULL,
    service_id VARCHAR(255) NOT NULL,
    service_name VARCHAR(255) NOT NULL,
    input_json JSONB NOT NULL,
    output_json JSONB NOT NULL,
    execution_trace_json JSONB NOT NULL,
    duration_ms BIGINT NOT NULL,
    status VARCHAR(16) NOT NULL,
    executed_by VARCHAR(255) NOT NULL,
    executed_at TIMESTAMPTZ NOT NULL,
    business_reference VARCHAR(255) NOT NULL DEFAULT '',
    actual_outcome_json JSONB,
    outcome_assessment VARCHAR(16),
    outcome_note TEXT NOT NULL DEFAULT '',
    outcome_occurred_at TIMESTAMPTZ,
    outcome_reported_by VARCHAR(255),
    outcome_reported_at TIMESTAMPTZ
);

INSERT INTO decision_runs VALUES
    (1, 10, 20, 'svc', 'service', '{"age": 25}', '{"eligible": true}',
     '{"decisions":[{"matchedRuleIds":["age-qualified"]}]}', 5, 'succeeded', 'test',
     '2026-10-15 12:00:00+00', '', NULL, 'confirmed', '', NULL, NULL, NULL),
    (2, 10, 20, 'svc', 'service', '{"age": 17}', '{"eligible": false}', '{}',
     8, 'failed', 'test', '2020-05-15 12:00:00+00', 'ref', NULL, 'mismatch',
     'note', NULL, NULL, NULL);

CREATE TABLE knowledge_sources (id BIGINT PRIMARY KEY);
INSERT INTO knowledge_sources VALUES (1);
CREATE TABLE decision_patterns (id BIGINT PRIMARY KEY);
CREATE TABLE decision_projects (id BIGINT PRIMARY KEY);
CREATE TABLE users (username VARCHAR(255) PRIMARY KEY);
CREATE TABLE decision_workflows (project_id BIGINT PRIMARY KEY);
CREATE TABLE decision_test_cases (
    id BIGSERIAL PRIMARY KEY,
    version_id BIGINT NOT NULL,
    source VARCHAR(16) NOT NULL DEFAULT 'manual'
        CHECK (source IN ('manual', 'generated'))
);
CREATE TABLE agent_runs (
    id BIGSERIAL PRIMARY KEY,
    status VARCHAR(16) NOT NULL DEFAULT 'running',
    finished_at TIMESTAMPTZ,
    usage_json JSONB
);
INSERT INTO decision_projects VALUES (10), (11);
INSERT INTO decision_workflows (project_id) VALUES (10);
\i script/082_decision_analytics_thresholds.sql
\i script/056_repair_memories.sql
\i script/057_knowledge_chunks.sql
\i script/066_decision_pattern_embeddings.sql
\i script/063_agent_orchestration_state.sql
\i script/067_agent_orchestration_heartbeat_recovery.sql
\i script/070_workflow_tester_report.sql
\i script/071_tester_proposal_test_cases.sql
\i script/072_workflow_engineer_orchestration_handoff.sql
\i script/073_workflow_analysis_orchestration_handoff.sql
\i script/074_agent_step_attempt_history.sql
\i script/075_agent_orchestration_retry_lineage.sql
\i script/100_agent_step_skipped_status.sql
\i script/100_agent_step_skipped_status.sql

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'agent_steps'::regclass
          AND conname = 'agent_steps_status_check'
          AND pg_get_constraintdef(oid) LIKE '%skipped%'
    ) THEN
        RAISE EXCEPTION 'agent_steps status constraint does not accept skipped';
    END IF;
END;
$$;

INSERT INTO users (username) VALUES ('orchestration-test'), ('reviewer-test');
\i script/092_decision_run_explanations.sql
\i script/092_decision_run_explanations.sql
\i script/094_decision_run_explanation_reviews.sql
\i script/094_decision_run_explanation_reviews.sql
INSERT INTO decision_run_explanations (
    decision_run_id, status, explanation_json, prompt_version, prompt_digest,
    trace_digest, created_by, completed_at
) VALUES (
    1, 'succeeded', '{"summary":"Decision explanation","decisions":[]}'::JSONB,
    'decision-explanation-v1', repeat('a', 64), repeat('b', 64),
    'orchestration-test', NOW()
);
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM decision_run_explanations
        WHERE decision_run_id = 1 AND status = 'succeeded'
          AND explanation_json->>'summary' = 'Decision explanation'
    ) THEN
        RAISE EXCEPTION 'decision run explanation cache insert failed';
    END IF;
    BEGIN
        INSERT INTO decision_run_explanations (
            decision_run_id, status, prompt_version, prompt_digest,
            trace_digest, created_by
        ) VALUES (
            1, 'running', 'decision-explanation-v1', repeat('a', 64),
            repeat('b', 64), 'orchestration-test'
        );
        RAISE EXCEPTION 'decision run explanation cache accepted duplicate run';
    EXCEPTION WHEN unique_violation THEN
        NULL;
    END;
END;
$$;
DO $$
BEGIN
    IF NOT has_table_privilege('fuxi', 'decision_run_explanation_reviews', 'SELECT')
       OR NOT has_table_privilege('fuxi', 'decision_run_explanation_reviews', 'INSERT')
       OR has_table_privilege('fuxi', 'decision_run_explanation_reviews', 'UPDATE')
       OR has_table_privilege('fuxi', 'decision_run_explanation_reviews', 'DELETE') THEN
        RAISE EXCEPTION 'application role explanation review privileges are not append-only';
    END IF;
END;
$$;
INSERT INTO agent_runs DEFAULT VALUES RETURNING id \gset orchestration_run_
INSERT INTO agent_orchestrations (project_id, username, trigger_kind, current_stage)
VALUES (10, 'orchestration-test', 'requirement_analysis', 'requirement_analysis')
RETURNING id \gset orchestration_
UPDATE agent_runs SET orchestration_id = :orchestration_id
WHERE id = :orchestration_run_id;
INSERT INTO agent_steps (
    orchestration_id, agent_run_id, step_no, agent_role, task_kind
) VALUES (
    :orchestration_id, :orchestration_run_id, 1,
    'requirement_analysis', 'requirement_analysis'
);
INSERT INTO agent_steps (
    orchestration_id, step_no, agent_role, task_kind, artifact_kind,
    artifact_digest
) VALUES (
    :orchestration_id, 2, 'architect', 'plan_decision_model',
    'decision_plan', repeat('a', 64)
) RETURNING id \gset orchestration_step_two_
INSERT INTO agent_handoffs (
    orchestration_id, from_step_id, to_step_id, context_digest, context_summary_json
) SELECT :orchestration_id, first_step.id, :orchestration_step_two_id,
         repeat('b', 64), '{"schemaVersion":1,"fieldNames":["brief"]}'::JSONB
FROM agent_steps first_step
WHERE first_step.agent_run_id = :orchestration_run_id;
UPDATE agent_steps SET status = 'succeeded', finished_at = NOW()
WHERE orchestration_id = :orchestration_id;
UPDATE agent_orchestrations SET status = 'succeeded', finished_at = NOW()
WHERE id = :orchestration_id;
UPDATE decision_workflows SET plan_orchestration_id = :orchestration_id
WHERE project_id = 10;
INSERT INTO agent_orchestrations (
    project_id, username, trigger_kind, current_stage, status, finished_at
) VALUES (
    10, 'orchestration-test', 'requirement_analysis', 'requirement_analysis',
    'succeeded', NOW()
) RETURNING id \gset analysis_orchestration_
UPDATE agent_orchestrations
SET parent_orchestration_id = :analysis_orchestration_id
WHERE id = :orchestration_id;
UPDATE decision_workflows SET analysis_orchestration_id = :analysis_orchestration_id
WHERE project_id = 10;
INSERT INTO agent_orchestrations (
    project_id, username, trigger_kind, current_stage, parent_orchestration_id
) VALUES (
    10, 'orchestration-test', 'dsl_generation', 'dsl_generation', :orchestration_id
) RETURNING id \gset engineer_orchestration_
INSERT INTO agent_runs DEFAULT VALUES RETURNING id \gset engineer_run_
UPDATE agent_runs SET orchestration_id = :engineer_orchestration_id WHERE id = :engineer_run_id;
INSERT INTO agent_steps (
    orchestration_id, agent_run_id, step_no, agent_role, task_kind, artifact_kind
) VALUES (
    :engineer_orchestration_id, :engineer_run_id, 1, 'engineer', 'generate_dmn_dsl', 'dmn_model'
);
UPDATE agent_steps SET status = 'succeeded', finished_at = NOW()
WHERE orchestration_id = :engineer_orchestration_id;
UPDATE agent_orchestrations SET status = 'succeeded', finished_at = NOW()
WHERE id = :engineer_orchestration_id;

INSERT INTO agent_runs DEFAULT VALUES RETURNING id \gset cancellation_run_
INSERT INTO agent_orchestrations (project_id, username, trigger_kind, current_stage)
VALUES (10, 'orchestration-test', 'decision_planning', 'plan_decision_model')
RETURNING id \gset cancellation_orchestration_
UPDATE agent_runs SET orchestration_id = :cancellation_orchestration_id
WHERE id = :cancellation_run_id;
INSERT INTO agent_steps (
    orchestration_id, agent_run_id, step_no, agent_role, task_kind
) VALUES (
    :cancellation_orchestration_id, :cancellation_run_id, 1,
    'architect', 'plan_decision_model'
);
UPDATE agent_orchestrations
SET status = 'cancelled', finished_at = NOW()
WHERE id = :cancellation_orchestration_id AND status = 'running';
UPDATE agent_runs SET status = 'cancelled', finished_at = NOW()
WHERE id = :cancellation_run_id AND status = 'running';
UPDATE agent_steps SET status = 'cancelled', finished_at = NOW()
WHERE orchestration_id = :cancellation_orchestration_id AND status = 'running';

INSERT INTO agent_runs DEFAULT VALUES RETURNING id \gset recovery_run_
INSERT INTO agent_orchestrations (project_id, username, trigger_kind, current_stage)
VALUES (10, 'orchestration-test', 'requirement_analysis', 'requirement_analysis')
RETURNING id \gset recovery_orchestration_
UPDATE agent_runs SET orchestration_id = :recovery_orchestration_id
WHERE id = :recovery_run_id;
INSERT INTO agent_steps (
    orchestration_id, agent_run_id, step_no, agent_role, task_kind
) VALUES (
    :recovery_orchestration_id, :recovery_run_id, 1,
    'analyst', 'requirement_analysis'
);
UPDATE agent_orchestrations
SET heartbeat_at = NOW() - INTERVAL '3 minutes'
WHERE id = :recovery_orchestration_id;
WITH stale AS (
    UPDATE agent_orchestrations
    SET status = 'failed', finished_at = NOW()
    WHERE status IN ('pending', 'running')
      AND heartbeat_at < NOW() - INTERVAL '2 minutes'
    RETURNING id
)
UPDATE agent_runs
SET status = 'failed', finished_at = NOW()
WHERE orchestration_id IN (SELECT id FROM stale) AND status = 'running';
UPDATE agent_steps
SET status = 'failed', finished_at = NOW()
WHERE orchestration_id = :recovery_orchestration_id AND status = 'running';
INSERT INTO agent_steps (
    orchestration_id, step_no, attempt_no, agent_role, task_kind,
    status, error_json, finished_at
) VALUES (
    :recovery_orchestration_id, 1, 2, 'analyst', 'requirement_analysis',
    'failed', '{"message":"transient upstream failure"}'::JSONB, NOW()
);
DO $$
DECLARE
    failed_orchestration_id BIGINT;
BEGIN
    SELECT id INTO failed_orchestration_id
    FROM agent_orchestrations WHERE status = 'failed';
    BEGIN
        INSERT INTO agent_steps (
            orchestration_id, step_no, attempt_no, agent_role, task_kind,
            status, finished_at
        ) VALUES (
            failed_orchestration_id, 1, 2, 'analyst', 'requirement_analysis',
            'failed', NOW()
        );
        RAISE EXCEPTION 'duplicate step attempt was accepted';
    EXCEPTION WHEN unique_violation THEN
        NULL;
    END;
    BEGIN
        INSERT INTO agent_steps (
            orchestration_id, step_no, attempt_no, agent_role, task_kind,
            status, finished_at
        ) VALUES (
            failed_orchestration_id, 1, 0, 'analyst', 'requirement_analysis',
            'failed', NOW()
        );
        RAISE EXCEPTION 'non-positive step attempt was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
END;
$$;

INSERT INTO agent_orchestrations (
    project_id, username, trigger_kind, current_stage, status, finished_at,
    retry_of_orchestration_id
) SELECT project_id, username, trigger_kind, current_stage, 'failed', NOW(), id
FROM agent_orchestrations WHERE status = 'failed'
RETURNING id \gset retry_orchestration_
UPDATE agent_runs SET usage_json = '{"total_tokens":0}'::JSONB;
INSERT INTO agent_runs (status, finished_at, usage_json, orchestration_id)
VALUES (
    'succeeded', NOW(), '{"total_tokens":42}'::JSONB, :retry_orchestration_id
);
DO $$
DECLARE
    observed_tokens BIGINT;
BEGIN
    WITH RECURSIVE tree(id) AS (
        SELECT id FROM agent_orchestrations
        WHERE status = 'failed' AND retry_of_orchestration_id IS NULL
        UNION
        SELECT child.id FROM agent_orchestrations child
        JOIN tree parent
          ON child.parent_orchestration_id = parent.id
          OR child.retry_of_orchestration_id = parent.id
    )
    SELECT COALESCE(SUM((run.usage_json->>'total_tokens')::BIGINT), 0)
    INTO observed_tokens
    FROM tree JOIN agent_runs run ON run.orchestration_id = tree.id;
    IF observed_tokens <> 42 THEN
        RAISE EXCEPTION 'retry lineage token aggregation returned %', observed_tokens;
    END IF;
END;
$$;

INSERT INTO decision_test_cases (version_id, source, tester_proposal_id)
VALUES (90001, 'tester_proposal', 'proposal-001');
DO $$
BEGIN
    BEGIN
        INSERT INTO decision_test_cases (version_id, source, tester_proposal_id)
        VALUES (90001, 'tester_proposal', 'proposal-001');
        RAISE EXCEPTION 'tester proposal generated a duplicate formal test case';
    EXCEPTION WHEN unique_violation THEN
        NULL;
    END;
END;
$$;

INSERT INTO knowledge_chunks (knowledge_source_id, chunk_index, content, token_count, metadata)
VALUES (1, 0, 'keyword fallback fixture', 3, '{}'::JSONB);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'knowledge_chunks' AND column_name = 'content'
    ) THEN
        RAISE EXCEPTION 'keyword knowledge chunk schema was not installed';
    END IF;
    IF EXISTS (
        SELECT 1 FROM pg_extension WHERE extname = 'vector'
    ) <> EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'knowledge_chunks' AND column_name = 'embedding'
    ) THEN
        RAISE EXCEPTION 'knowledge vector columns do not match pgvector availability';
    END IF;
    IF EXISTS (
        SELECT 1 FROM pg_extension WHERE extname = 'vector'
    ) <> EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'decision_patterns' AND column_name = 'embedding'
    ) THEN
        RAISE EXCEPTION 'pattern vector columns do not match pgvector availability';
    END IF;
END;
$$;

\i script/058_decision_run_partitions.sql
\i script/059_decision_run_daily_stats.sql

DO $$
BEGIN
    IF (SELECT COUNT(*) FROM decision_runs_partitioned) <> 2 THEN
        RAISE EXCEPTION 'historical backfill did not copy all rows';
    END IF;

    INSERT INTO decision_runs VALUES
        (3, 10, 20, 'svc', 'service', '{"age": 30}', '{"eligible": true}',
         '{"decisions":[{"matchedRuleIds":["new-rule"]}]}', 13, 'succeeded',
         'test', NOW(), '', NULL, NULL, '', NULL, NULL, NULL);
    IF NOT EXISTS (
        SELECT 1 FROM decision_runs_partitioned
        WHERE id = 3 AND output_json->>'eligible' = 'true'
    ) THEN
        RAISE EXCEPTION 'insert was not mirrored';
    END IF;

    UPDATE decision_runs
    SET output_json = '{"eligible": false}', outcome_assessment = 'mismatch'
    WHERE id = 3;
    IF NOT EXISTS (
        SELECT 1 FROM decision_runs_partitioned
        WHERE id = 3 AND output_json->>'eligible' = 'false'
          AND outcome_assessment = 'mismatch'
    ) THEN
        RAISE EXCEPTION 'update was not mirrored';
    END IF;

    DELETE FROM decision_runs WHERE id = 3;
    IF EXISTS (SELECT 1 FROM decision_runs_partitioned WHERE id = 3) THEN
        RAISE EXCEPTION 'delete was not mirrored';
    END IF;

    INSERT INTO decision_runs VALUES
        (4, 10, 20, 'svc', 'service', '{}', '{}', '{}', 2, 'succeeded', 'test',
         '2019-03-15 12:00:00+00', '', NULL, NULL, '', NULL, NULL, NULL);
    IF NOT EXISTS (
        SELECT 1 FROM decision_runs_partitioned_default WHERE id = 4
    ) THEN
        RAISE EXCEPTION 'unpartitioned late row did not land in the default partition';
    END IF;
    PERFORM ensure_decision_run_month_partition('2019-03-01');
    IF NOT EXISTS (
        SELECT 1 FROM decision_runs_partitioned_2019_03 WHERE id = 4
    ) OR EXISTS (
        SELECT 1 FROM decision_runs_partitioned_default WHERE id = 4
    ) THEN
        RAISE EXCEPTION 'late row was not moved from the default partition';
    END IF;

    INSERT INTO decision_runs (
        id, project_id, version_id, service_id, service_name,
        input_json, output_json, execution_trace_json, duration_ms,
        status, executed_by, executed_at, outcome_assessment
    ) VALUES (
        5, 10, 20, 'svc', 'service',
        '{"age": 42, "region": "north"}',
        '{"decision": "deny", "reason": "sensitive reason"}',
        '{"matchedRules": ["age-rule"]}', 3, 'succeeded', 'test', NOW(), 'mismatch'
    );
    IF NOT EXISTS (
        SELECT 1
        FROM decision_runs_partitioned run
        WHERE run.id = 5
          AND run.outcome_assessment = 'mismatch'
          AND run.input_json ? 'age'
          AND run.input_json ? 'region'
          AND run.output_json ? 'decision'
          AND run.output_json ? 'reason'
          AND NOT EXISTS (
              SELECT 1
              FROM jsonb_object_keys(run.input_json) AS input_key(key)
              WHERE input_key.key IN ('42', 'north')
          )
          AND NOT EXISTS (
              SELECT 1
              FROM jsonb_object_keys(run.output_json) AS output_key(key)
              WHERE output_key.key IN ('deny', 'sensitive reason')
          )
    ) THEN
        RAISE EXCEPTION 'representative run field summary did not preserve names without values';
    END IF;
    DELETE FROM decision_runs WHERE id = 5;

    PERFORM refresh_decision_run_daily_stats();
    IF NOT EXISTS (
        SELECT 1 FROM decision_run_daily_stats
        WHERE run_day = DATE '2026-10-15'
          AND total_runs = 1
          AND succeeded_runs = 1
          AND failed_runs = 0
          AND average_duration_ms = 5
          AND p50_ms = 5
          AND p95_ms = 5
          AND rule_hit_distribution->>'age-qualified' = '1'
    ) THEN
        RAISE EXCEPTION 'daily aggregate does not match the historical fixture';
    END IF;
END;
$$;

\set lookback_days 3660
\i script/reconcile-decision-run-daily-stats.sql

\set start_at '2019-03-01 00:00:00+00'
\set end_at '2026-10-16 00:00:00+00'
\i script/reconcile-decision-run-partitions.sql

CREATE TABLE decision_optimization_proposals (
    id BIGINT PRIMARY KEY,
    resulting_version_id BIGINT,
    status TEXT NOT NULL,
    project_id BIGINT,
    source_version_id BIGINT
);
CREATE TABLE optimization_simulations (
    id BIGINT PRIMARY KEY,
    baseline_accuracy NUMERIC(5, 2) NOT NULL,
    simulated_accuracy NUMERIC(5, 2) NOT NULL,
    test_set_size INTEGER NOT NULL,
    suggestion_id BIGINT,
    version_id BIGINT,
    passed BOOLEAN NOT NULL DEFAULT FALSE,
    bias_rate_baseline NUMERIC(5, 2) NOT NULL DEFAULT 20,
    bias_rate_simulated NUMERIC(5, 2) NOT NULL DEFAULT 10,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE TABLE decision_versions (
    id BIGINT PRIMARY KEY,
    project_id BIGINT NOT NULL,
    base_version_id BIGINT,
    status TEXT NOT NULL,
    change_summary JSONB NOT NULL DEFAULT '{}'::JSONB
);
CREATE TABLE audit_events (
    id BIGINT PRIMARY KEY,
    version_id BIGINT NOT NULL,
    event_type TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
INSERT INTO optimization_simulations (id, baseline_accuracy, simulated_accuracy, test_set_size)
VALUES (1, 80, 80, 10), (2, 80, 80, 10);
INSERT INTO decision_optimization_proposals VALUES (1, 12, 'confirmed', 7, 10);
UPDATE optimization_simulations
SET suggestion_id = 1, version_id = 30, passed = TRUE
WHERE id = 2;
INSERT INTO decision_versions VALUES
    (10, 7, NULL, 'retired', '{}'::JSONB),
    (12, 7, 10, 'draft', '{"candidateVersionId": 30}'::JSONB),
    (13, 7, 12, 'published', '{}'::JSONB),
    (30, 7, 10, 'draft', '{}'::JSONB);
INSERT INTO audit_events VALUES
    (1, 13, 'version.published', NOW() - INTERVAL '45 days');

\i script/060_optimization_simulation_candidate_snapshot.sql
\i script/061_optimization_proposal_effectiveness.sql
UPDATE optimization_simulations SET test_set_size = 9, passed = TRUE WHERE id = 1;
\i script/062_optimization_simulation_minimum_samples.sql
\i script/065_decision_run_outcomes_and_deviations.sql
\i script/098_deviation_analysis_inventory_index.sql
\i script/098_deviation_analysis_inventory_index.sql

INSERT INTO decision_run_outcome_receipts (
    project_id, service_id, idempotency_key, run_id, payload_digest
) VALUES (10, 'svc', 'outcome-1', 1, repeat('a', 64));
INSERT INTO deviation_analyses (
    project_id, decision_run_id, status, attributed_rule, confidence, evidence_json
) VALUES (10, 1, 'candidate', 'rule-1', 0.35, '{"source":"trace"}');

DO $$
BEGIN
    IF (SELECT passed FROM optimization_simulations WHERE id = 1) IS DISTINCT FROM FALSE
       OR (SELECT candidate_artifact_json FROM optimization_simulations WHERE id = 1)
            IS DISTINCT FROM '{}'::JSONB THEN
        RAISE EXCEPTION 'legacy simulations were not made ineligible for confirmation';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'decision_optimization_proposals'
          AND column_name = 'effectiveness_result_json'
    ) OR NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'decision_optimization_proposals'
          AND column_name = 'effectiveness_tracked_at'
    ) THEN
        RAISE EXCEPTION 'effectiveness tracking columns were not installed';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE indexname = 'decision_versions_project_base_lineage_idx'
    ) THEN
        RAISE EXCEPTION 'version descendant lookup index was not installed';
    END IF;

    IF NOT EXISTS (
        WITH RECURSIVE proposal_lineage AS (
            SELECT proposal.id AS proposal_id,
                   proposal.project_id,
                   proposal.source_version_id,
                   proposal.resulting_version_id,
                   proposal.resulting_version_id AS version_id
            FROM decision_optimization_proposals proposal
            WHERE proposal.status = 'confirmed'
              AND proposal.effectiveness_tracked_at IS NULL

            UNION

            SELECT lineage.proposal_id,
                   lineage.project_id,
                   lineage.source_version_id,
                   lineage.resulting_version_id,
                   child.id
            FROM proposal_lineage lineage
            JOIN decision_versions child
              ON child.base_version_id = lineage.version_id
             AND child.project_id = lineage.project_id
        ), eligible AS (
            SELECT proposal.id AS proposal_id,
                   proposal.resulting_version_id,
                   published.version_id AS published_version_id,
                   resulting.change_summary->>'candidateVersionId' AS candidate_version_id
            FROM decision_optimization_proposals proposal
            JOIN decision_versions resulting
              ON resulting.id = proposal.resulting_version_id
             AND resulting.project_id = proposal.project_id
            JOIN LATERAL (
                SELECT lineage.version_id, event.created_at
                FROM audit_events event
                JOIN proposal_lineage lineage
                  ON lineage.proposal_id = proposal.id
                 AND lineage.version_id = event.version_id
                JOIN decision_versions published_version
                  ON published_version.id = lineage.version_id
                 AND published_version.project_id = proposal.project_id
                 AND published_version.status IN ('published', 'retired')
                WHERE event.event_type = 'version.published'
                ORDER BY event.created_at, event.id
                LIMIT 1
            ) published ON TRUE
            JOIN LATERAL (
                SELECT version_id
                FROM optimization_simulations
                WHERE suggestion_id = proposal.id
                  AND version_id = NULLIF(resulting.change_summary->>'candidateVersionId', '')::BIGINT
                  AND passed
                ORDER BY created_at DESC, id DESC
                LIMIT 1
            ) simulation ON TRUE
            WHERE proposal.status = 'confirmed'
              AND proposal.effectiveness_tracked_at IS NULL
              AND resulting.change_summary ? 'candidateVersionId'
              AND published.created_at <= NOW() - INTERVAL '30 days'
        )
        SELECT 1
        FROM eligible
        WHERE proposal_id = 1
          AND resulting_version_id = 12
          AND published_version_id = 13
          AND candidate_version_id = '30'
    ) THEN
        RAISE EXCEPTION 'effectiveness query did not select the published descendant and candidate';
    END IF;

    BEGIN
        INSERT INTO optimization_simulations (
            id, baseline_accuracy, simulated_accuracy, test_set_size, passed
        ) VALUES (2, 80, 79, 10, TRUE);
        RAISE EXCEPTION 'passed simulation constraint accepted a regressed candidate';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
    BEGIN
        INSERT INTO optimization_simulations (
            id, baseline_accuracy, simulated_accuracy, test_set_size, passed
        ) VALUES (3, 90, 90, 9, TRUE);
        RAISE EXCEPTION 'passed simulation constraint accepted too few samples';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
    INSERT INTO optimization_simulations (
        id, baseline_accuracy, simulated_accuracy, test_set_size, passed
    ) VALUES (4, 90, 90, 10, TRUE);
END;
$$;

\i script/056_repair_memories.sql
\i script/057_knowledge_chunks.sql
\i script/066_decision_pattern_embeddings.sql
\i script/063_agent_orchestration_state.sql
\i script/067_agent_orchestration_heartbeat_recovery.sql
\i script/070_workflow_tester_report.sql
\i script/071_tester_proposal_test_cases.sql
\i script/072_workflow_engineer_orchestration_handoff.sql
\i script/073_workflow_analysis_orchestration_handoff.sql
\i script/074_agent_step_attempt_history.sql
\i script/075_agent_orchestration_retry_lineage.sql
\i script/100_agent_step_skipped_status.sql
\i script/076_decision_drift_automation.sql
\i script/076_decision_drift_automation.sql
\i script/077_decision_drift_event_actions.sql
\i script/077_decision_drift_event_actions.sql
\i script/078_decision_drift_webhook_outbox.sql
\i script/078_decision_drift_webhook_outbox.sql
\i script/079_decision_drift_event_reviews.sql
\i script/079_decision_drift_event_reviews.sql
\i script/080_decision_drift_scan_windows.sql
\i script/080_decision_drift_scan_windows.sql
\i script/081_optimization_autodraft_shadow.sql
\i script/081_optimization_autodraft_shadow.sql
\i script/082_decision_analytics_thresholds.sql
\i script/083_decision_analytics_sample_thresholds.sql
\i script/082_decision_analytics_thresholds.sql
\i script/083_decision_analytics_sample_thresholds.sql
\i script/084_deviation_analysis_reviews.sql
\i script/084_deviation_analysis_reviews.sql
\i script/085_agent_orchestration_single_active_retry.sql
\i script/085_agent_orchestration_single_active_retry.sql

BEGIN;
DO $$
DECLARE
    root_orchestration_id BIGINT;
    active_retry_id BIGINT;
BEGIN
    SELECT id INTO root_orchestration_id
    FROM agent_orchestrations
    WHERE status = 'failed' AND retry_of_orchestration_id IS NULL
    ORDER BY id
    LIMIT 1;
    IF root_orchestration_id IS NULL THEN
        RAISE EXCEPTION 'retry uniqueness test requires a failed root orchestration';
    END IF;

    INSERT INTO agent_orchestrations (
        project_id, username, trigger_kind, current_stage, status,
        retry_of_orchestration_id
    )
    SELECT project_id, username, trigger_kind, current_stage, 'pending', id
    FROM agent_orchestrations WHERE id = root_orchestration_id
    RETURNING id INTO active_retry_id;

    BEGIN
        INSERT INTO agent_orchestrations (
            project_id, username, trigger_kind, current_stage, status,
            retry_of_orchestration_id
        )
        SELECT project_id, username, trigger_kind, current_stage, 'running', id
        FROM agent_orchestrations WHERE id = root_orchestration_id;
        RAISE EXCEPTION 'parallel active orchestration retry was accepted';
    EXCEPTION WHEN unique_violation THEN
        NULL;
    END;

    UPDATE agent_orchestrations
    SET status = 'failed', finished_at = NOW()
    WHERE id = active_retry_id;
END;
$$;
ROLLBACK;

DO $$
DECLARE
    sample_run_id BIGINT;
    sample_project_id BIGINT;
BEGIN
    SELECT id, project_id INTO sample_run_id, sample_project_id
    FROM decision_runs
    ORDER BY id
    LIMIT 1;
    IF sample_run_id IS NULL THEN
        RAISE EXCEPTION 'missing run fixture for deviation review constraints';
    END IF;
    BEGIN
        INSERT INTO deviation_analyses (project_id, decision_run_id, status, review_label)
        VALUES (sample_project_id, sample_run_id, 'candidate', 'invalid');
        RAISE EXCEPTION 'invalid deviation review label was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
    BEGIN
        INSERT INTO deviation_analyses (project_id, decision_run_id, status, review_label)
        VALUES (sample_project_id, sample_run_id, 'candidate', 'correct');
        RAISE EXCEPTION 'incomplete deviation review metadata was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
END;
$$;

INSERT INTO decision_analytics_thresholds (
    project_id, failure_warning_rate, failure_high_rate,
    mismatch_warning_rate, mismatch_high_rate
) VALUES (10, 0.15, 0.25, 0.25, 0.45);
UPDATE decision_analytics_thresholds
SET failure_min_runs = 12, mismatch_min_samples = 8
WHERE project_id = 10;
DO $$
BEGIN
    BEGIN
        INSERT INTO decision_analytics_thresholds (
            project_id, failure_warning_rate, failure_high_rate,
            mismatch_warning_rate, mismatch_high_rate
        ) VALUES (11, 0.30, 0.20, 0.25, 0.45);
        RAISE EXCEPTION 'invalid analytics thresholds were accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
    BEGIN
        INSERT INTO decision_analytics_thresholds (
            project_id, failure_warning_rate, failure_high_rate,
            mismatch_warning_rate, mismatch_high_rate, failure_min_runs
        ) VALUES (12, 0.10, 0.20, 0.20, 0.30, 0);
        RAISE EXCEPTION 'zero failure sample threshold was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
    BEGIN
        INSERT INTO decision_analytics_thresholds (
            project_id, failure_warning_rate, failure_high_rate,
            mismatch_warning_rate, mismatch_high_rate, mismatch_min_samples
        ) VALUES (13, 0.10, 0.20, 0.20, 0.30, 100001);
        RAISE EXCEPTION 'oversized mismatch sample threshold was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
    IF NOT EXISTS (
        SELECT 1 FROM decision_analytics_thresholds
        WHERE project_id = 10
          AND failure_warning_rate = 0.15
          AND failure_high_rate = 0.25
          AND mismatch_warning_rate = 0.25
          AND mismatch_high_rate = 0.45
          AND failure_min_runs = 12
          AND mismatch_min_samples = 8
    ) THEN
        RAISE EXCEPTION 'project analytics thresholds were not persisted';
    END IF;
END;
$$;
\i script/058_decision_run_partitions.sql
\i script/059_decision_run_daily_stats.sql
\i script/060_optimization_simulation_candidate_snapshot.sql
\i script/061_optimization_proposal_effectiveness.sql
\i script/062_optimization_simulation_minimum_samples.sql
\i script/065_decision_run_outcomes_and_deviations.sql

UPDATE optimization_simulations
SET suggestion_id = 1, version_id = 30
WHERE id = 4;
INSERT INTO optimization_autodraft_shadow_evaluations (
    simulation_id, project_id, proposal_id, source_version_id, candidate_version_id,
    outcome, finding_code, sample_source, test_set_size, baseline_accuracy,
    simulated_accuracy, evaluated_by
) VALUES (
    4, 10, 1, 12, 30, 'blocked_change_risk', 'HIGH_FAILURE_RATE',
    'approved_business_tests', 10, 80, 80, 'orchestration-test'
) ON CONFLICT (simulation_id) DO NOTHING;
INSERT INTO optimization_autodraft_shadow_evaluations (
    simulation_id, project_id, proposal_id, source_version_id, candidate_version_id,
    outcome, finding_code, sample_source, test_set_size, baseline_accuracy,
    simulated_accuracy, evaluated_by
) VALUES (
    4, 10, 1, 12, 30, 'blocked_change_risk', 'HIGH_FAILURE_RATE',
    'approved_business_tests', 10, 80, 80, 'orchestration-test'
) ON CONFLICT (simulation_id) DO NOTHING;
DO $$
BEGIN
    IF (SELECT COUNT(*) FROM optimization_autodraft_shadow_evaluations
        WHERE simulation_id = 4) <> 1 THEN
        RAISE EXCEPTION 'shadow evaluation was not idempotent';
    END IF;
    BEGIN
        UPDATE optimization_autodraft_shadow_evaluations
        SET outcome = 'draft_created' WHERE simulation_id = 4;
        RAISE EXCEPTION 'unsupported shadow outcome was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
END;
$$;

INSERT INTO decision_versions (id, project_id, base_version_id, status)
VALUES (20, 10, NULL, 'published');
INSERT INTO decision_drift_scan_windows (
    project_id, source_version_id, scan_day, evidence_json
) VALUES (
    10, 20, CURRENT_DATE,
    '{"projectId":10,"versionId":20,"summary":{"totalRuns":30,"conclusiveOutcomeRuns":8,"failureRate":0.0,"mismatchRate":0.25},"findings":[{"code":"HIGH_MISMATCH_RATE"}]}'::JSONB
) ON CONFLICT (project_id, source_version_id, scan_day) DO NOTHING;
INSERT INTO decision_drift_scan_windows (
    project_id, source_version_id, scan_day, evidence_json
) VALUES (
    10, 20, CURRENT_DATE,
    '{"projectId":10,"versionId":20,"summary":{"totalRuns":30,"conclusiveOutcomeRuns":8,"failureRate":0.0,"mismatchRate":0.25},"findings":[{"code":"HIGH_MISMATCH_RATE"}]}'::JSONB
) ON CONFLICT (project_id, source_version_id, scan_day) DO NOTHING;
DO $$
BEGIN
    IF (SELECT COUNT(*) FROM decision_drift_scan_windows WHERE project_id = 10) <> 1 THEN
        RAISE EXCEPTION 'daily drift scan window was not idempotent';
    END IF;
END;
$$;
INSERT INTO decision_drift_automation_rules (
    project_id, enabled, finding_codes, cooldown_hours, webhook_url, updated_by
) VALUES (
    10, TRUE, ARRAY['RULE_HIT_DRIFT'], 24,
    'https://hooks.example.com/decision-drift', 'orchestration-test'
);
INSERT INTO decision_drift_events (
    project_id, source_version_id, dedupe_key, finding_codes, evidence_json
) VALUES (
    10, 20, 'v20:2026-11-12:RULE_HIT_DRIFT', ARRAY['RULE_HIT_DRIFT'],
    '{"findings":[{"code":"RULE_HIT_DRIFT","element":"eligibility/age-qualified"}]}'::JSONB
) ON CONFLICT (project_id, dedupe_key) DO NOTHING;
INSERT INTO decision_drift_webhook_deliveries (
    event_id, project_id, endpoint_url, payload_json
)
SELECT id, project_id, 'https://hooks.example.com/decision-drift',
       jsonb_build_object('schemaVersion', 1, 'eventId', id, 'projectId', project_id)
FROM decision_drift_events
WHERE project_id = 10 AND dedupe_key = 'v20:2026-11-12:RULE_HIT_DRIFT'
ON CONFLICT (event_id) DO NOTHING;
INSERT INTO decision_drift_events (
    project_id, source_version_id, dedupe_key, finding_codes, evidence_json
) VALUES (
    10, 20, 'v20:2026-11-12:RULE_HIT_DRIFT', ARRAY['RULE_HIT_DRIFT'],
    '{"findings":[{"code":"RULE_HIT_DRIFT"}]}'::JSONB
) ON CONFLICT (project_id, dedupe_key) DO NOTHING;
INSERT INTO decision_runs VALUES (
    99, 10, 20, 'svc', 'service', '{}', '{}',
    '{"decisions":[{"decisionId":"eligibility","decisionName":"Eligibility","matchedRuleIds":["age-qualified"]}]}',
    5, 'succeeded', 'test', NOW() - INTERVAL '1 day', '', NULL, 'confirmed', '', NULL, NULL, NULL
);
UPDATE decision_drift_events
SET status = 'proposal_created', action_result_json = '{"proposalIds":[31]}'::JSONB,
    resolved_at = NOW()
WHERE project_id = 10 AND dedupe_key = 'v20:2026-11-12:RULE_HIT_DRIFT';
DO $$
BEGIN
    IF (SELECT COUNT(*) FROM decision_drift_events WHERE project_id = 10) <> 1
       OR NOT EXISTS (
           SELECT 1 FROM decision_drift_events
           WHERE project_id = 10 AND status = 'proposal_created'
             AND action_result_json->'proposalIds'->>0 = '31'
       )
       OR NOT EXISTS (
           SELECT 1 FROM decision_drift_automation_rules
           WHERE project_id = 10 AND enabled AND cooldown_hours = 24
             AND webhook_url = 'https://hooks.example.com/decision-drift'
       )
       OR (SELECT COUNT(*) FROM decision_drift_webhook_deliveries
           WHERE project_id = 10 AND status = 'pending'
             AND payload_json->>'eventId' = (SELECT id::TEXT FROM decision_drift_events WHERE project_id = 10 LIMIT 1)) <> 1
       OR NOT EXISTS (
           SELECT 1 FROM information_schema.columns
           WHERE table_name = 'decision_drift_events'
             AND column_name = 'action_result_json'
       ) THEN
        RAISE EXCEPTION 'drift automation rule/event idempotency failed';
    END IF;
END;
$$;

UPDATE decision_drift_events
SET review_label = 'false_positive', reviewed_by = 'orchestration-test', reviewed_at = NOW()
WHERE project_id = 10 AND dedupe_key = 'v20:2026-11-12:RULE_HIT_DRIFT';
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM decision_drift_events
        WHERE project_id = 10 AND review_label = 'false_positive'
          AND reviewed_by = 'orchestration-test' AND reviewed_at IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'drift event review fields were not persisted';
    END IF;
    BEGIN
        UPDATE decision_drift_events SET review_label = 'wrong_label'
        WHERE project_id = 10;
        RAISE EXCEPTION 'invalid drift event review label was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
END;
$$;

UPDATE decision_drift_scan_windows
SET review_label = 'normal', reviewed_by = 'orchestration-test', reviewed_at = NOW()
WHERE project_id = 10;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM decision_drift_scan_windows
        WHERE project_id = 10 AND review_label = 'normal'
          AND reviewed_by = 'orchestration-test' AND reviewed_at IS NOT NULL
          AND evidence_json->'findings'->0->>'code' = 'HIGH_MISMATCH_RATE'
    ) THEN
        RAISE EXCEPTION 'drift scan window review was not persisted';
    END IF;
    BEGIN
        UPDATE decision_drift_scan_windows SET review_label = 'false_positive'
        WHERE project_id = 10;
        RAISE EXCEPTION 'invalid drift scan window review label was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
END;
$$;

UPDATE decision_drift_webhook_deliveries
SET status = 'failed', attempt_count = 8, last_error = 'test failure'
WHERE project_id = 10;
UPDATE decision_drift_webhook_deliveries
SET status = 'pending', attempt_count = 0, last_error = NULL, next_attempt_at = NOW()
WHERE project_id = 10 AND status = 'failed';
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM decision_drift_webhook_deliveries
        WHERE project_id = 10 AND status = 'pending' AND attempt_count = 0
          AND last_error IS NULL
    ) THEN
        RAISE EXCEPTION 'webhook outbox retry reset failed';
    END IF;
END;
$$;

DO $$
DECLARE
    observed_runs BIGINT;
    succeeded_runs BIGINT;
    feedback_runs BIGINT;
    mismatch_runs BIGINT;
    matched_rule_ids TEXT[];
BEGIN
    WITH targets AS (
        SELECT DISTINCT NULLIF(BTRIM(finding->>'element'), '') AS element,
               SPLIT_PART(BTRIM(finding->>'element'), '/', 1) AS decision_key,
               NULLIF(SPLIT_PART(BTRIM(finding->>'element'), '/', 2), '') AS rule_key
        FROM decision_drift_events event,
             jsonb_array_elements(COALESCE(event.evidence_json->'findings', '[]'::JSONB)) finding
        WHERE event.project_id = 10 AND event.source_version_id = 20
          AND event.dedupe_key = 'v20:2026-11-12:RULE_HIT_DRIFT'
          AND NULLIF(BTRIM(finding->>'element'), '') IS NOT NULL
    ), runs AS (
        SELECT id, status, outcome_assessment, execution_trace_json
        FROM decision_runs_partitioned
        WHERE project_id = 10 AND version_id = 20 AND executed_at >= NOW() - INTERVAL '30 days'
    )
    SELECT COUNT(DISTINCT run.id) FILTER (WHERE impact.matches)::BIGINT,
           COUNT(DISTINCT run.id) FILTER (WHERE impact.matches AND run.status = 'succeeded')::BIGINT,
           COUNT(DISTINCT run.id) FILTER (WHERE impact.matches AND run.outcome_assessment IN ('confirmed', 'mismatch'))::BIGINT,
           COUNT(DISTINCT run.id) FILTER (WHERE impact.matches AND run.outcome_assessment = 'mismatch')::BIGINT,
           COALESCE(ARRAY_AGG(DISTINCT rule.value) FILTER (WHERE impact.matches AND rule.value IS NOT NULL), ARRAY[]::TEXT[])
      INTO observed_runs, succeeded_runs, feedback_runs, mismatch_runs, matched_rule_ids
    FROM targets target
    LEFT JOIN runs run ON TRUE
    LEFT JOIN LATERAL jsonb_array_elements(COALESCE(run.execution_trace_json->'decisions', '[]'::JSONB)) step(value)
      ON step.value->>'decisionName' = target.decision_key OR step.value->>'decisionId' = target.decision_key
    LEFT JOIN LATERAL jsonb_array_elements_text(COALESCE(step.value->'matchedRuleIds', '[]'::JSONB)) rule(value)
      ON TRUE
    CROSS JOIN LATERAL (SELECT step.value IS NOT NULL
          AND (target.rule_key IS NULL OR step.value->'matchedRuleIds' ? target.rule_key) AS matches) impact;

    IF observed_runs <> 1 OR succeeded_runs <> 1 OR feedback_runs <> 1
       OR mismatch_runs <> 0 OR matched_rule_ids <> ARRAY['age-qualified']::TEXT[] THEN
        RAISE EXCEPTION 'unexpected drift impact aggregate: observed %, succeeded %, feedback %, mismatch %, rules %',
            observed_runs, succeeded_runs, feedback_runs, mismatch_runs, matched_rule_ids;
    END IF;
END;
$$;
DELETE FROM decision_runs WHERE id = 99;

DO $$
BEGIN
    IF (SELECT COUNT(*) FROM decision_runs_partitioned) <> 3 THEN
        RAISE EXCEPTION 'reapplying partition migration duplicated or removed rows';
    END IF;
    IF (SELECT COUNT(*) FROM knowledge_chunks) <> 1 THEN
        RAISE EXCEPTION 'reapplying knowledge migration changed chunk data';
    END IF;
    IF (SELECT COUNT(*) FROM decision_analytics_thresholds) <> 1
       OR (SELECT failure_min_runs FROM decision_analytics_thresholds WHERE project_id = 10) <> 12
       OR (SELECT mismatch_min_samples FROM decision_analytics_thresholds WHERE project_id = 10) <> 8 THEN
        RAISE EXCEPTION 'reapplying analytics-threshold migration changed settings';
    END IF;
    IF (SELECT COUNT(*) FROM agent_orchestrations) <> 6
       OR (SELECT COUNT(*) FROM agent_steps) <> 6
       OR (SELECT COUNT(*) FROM agent_handoffs) <> 1 THEN
        RAISE EXCEPTION 'reapplying orchestration migration changed persisted history';
    END IF;
    IF (SELECT COUNT(*) FROM agent_orchestrations WHERE status = 'cancelled') <> 1
       OR (SELECT COUNT(*) FROM agent_runs WHERE status = 'cancelled') <> 1
       OR (SELECT COUNT(*) FROM agent_steps WHERE status = 'cancelled') <> 1 THEN
        RAISE EXCEPTION 'cancelled orchestration did not retain its terminal status';
    END IF;
    IF (SELECT COUNT(*) FROM agent_orchestrations WHERE status = 'failed') <> 2
       OR (SELECT COUNT(*) FROM agent_runs WHERE status = 'failed') <> 1
       OR (SELECT COUNT(*) FROM agent_steps WHERE status = 'failed') <> 2 THEN
        RAISE EXCEPTION 'stale orchestration recovery did not retain failure status';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM agent_steps
        WHERE orchestration_id = (SELECT id FROM agent_orchestrations WHERE status = 'failed' AND retry_of_orchestration_id IS NULL)
          AND step_no = 1 AND attempt_no = 1 AND status = 'failed'
    ) OR NOT EXISTS (
        SELECT 1 FROM agent_steps
        WHERE orchestration_id = (SELECT id FROM agent_orchestrations WHERE status = 'failed' AND retry_of_orchestration_id IS NULL)
          AND step_no = 1 AND attempt_no = 2 AND status = 'failed'
    ) THEN
        RAISE EXCEPTION 'step attempt history was not preserved';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM agent_orchestrations engineer
        JOIN decision_workflows workflow
          ON workflow.plan_orchestration_id = engineer.parent_orchestration_id
        WHERE engineer.trigger_kind = 'dsl_generation'
    ) THEN
        RAISE EXCEPTION 'Engineer orchestration lost its confirmed planning parent';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'decision_workflows' AND column_name = 'tester_report_json'
    ) THEN
        RAISE EXCEPTION 'tester report workflow column was not installed';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'decision_workflows' AND column_name = 'analysis_orchestration_id'
    ) OR NOT EXISTS (
        SELECT 1 FROM decision_workflows workflow
        JOIN agent_orchestrations plan ON plan.id = workflow.plan_orchestration_id
        WHERE workflow.project_id = 10
          AND plan.parent_orchestration_id = workflow.analysis_orchestration_id
    ) THEN
        RAISE EXCEPTION 'analysis orchestration handoff was not persisted';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'agent_orchestrations'
          AND column_name = 'retry_of_orchestration_id'
    ) OR NOT EXISTS (
        SELECT 1 FROM agent_orchestrations retry
        JOIN agent_orchestrations original ON original.id = retry.retry_of_orchestration_id
        WHERE original.status = 'failed' AND retry.status = 'failed'
    ) THEN
        RAISE EXCEPTION 'orchestration retry lineage was not retained';
    END IF;
    IF (SELECT COUNT(*) FROM decision_test_cases
        WHERE source = 'tester_proposal' AND tester_proposal_id = 'proposal-001') <> 1 THEN
        RAISE EXCEPTION 'tester proposal did not retain one formal test case link';
    END IF;
    IF (SELECT COUNT(*) FROM decision_run_outcome_receipts) <> 1
       OR (SELECT COUNT(*) FROM deviation_analyses) <> 1 THEN
        RAISE EXCEPTION 'reapplying outcome/deviation migration changed existing data';
    END IF;
END;
$$;

WITH orchestration AS (
    INSERT INTO agent_orchestrations (
        project_id, username, trigger_kind, current_stage, status, finished_at
    ) VALUES (10, 'orchestration-test', 'migration-test', 'tester', 'succeeded', NOW())
    RETURNING id
)
INSERT INTO agent_steps (
    orchestration_id, step_no, agent_role, task_kind, status, error_json, finished_at
)
SELECT id, 1, 'tester', 'budget_skipped_test', 'skipped', '{"kind":"budget_skip"}', NOW()
FROM orchestration;
\i script/100_agent_step_skipped_status.sql
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM agent_steps
        WHERE task_kind = 'budget_skipped_test'
          AND status = 'skipped'
          AND finished_at IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'budget-skipped Tester step was not retained';
    END IF;
END;
$$;

\i script/test_optimization_representative_runs.sql

SELECT 'partition migration smoke test passed' AS result;
