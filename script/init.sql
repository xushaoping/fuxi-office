-- Run this file while connected to the default `postgres` database.
-- `\gexec` creates the application database only when it does not exist.
-- Production bootstrap connects as an independent owner first and sets this
-- variable so this foundation script cannot switch back to the admin session.
\if :{?skip_database_bootstrap}
\else
SELECT 'CREATE DATABASE fuxi_dmn'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'fuxi_dmn')\gexec
\connect fuxi_dmn
\endif

DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS vector;
EXCEPTION
    WHEN insufficient_privilege OR undefined_file OR feature_not_supported THEN
        RAISE NOTICE 'pgvector is unavailable; knowledge search will use keyword retrieval';
END;
$$;

CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    username VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    role VARCHAR(16) NOT NULL DEFAULT 'user'
        CHECK (role IN ('admin', 'user')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS role VARCHAR(16) NOT NULL DEFAULT 'user';
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_role_check;
ALTER TABLE users
    ADD CONSTRAINT users_role_check CHECK (role IN ('admin', 'user'));

CREATE TABLE IF NOT EXISTS decision_projects (
    id BIGSERIAL PRIMARY KEY,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    name VARCHAR(120) NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    risk_level VARCHAR(16) NOT NULL DEFAULT 'medium'
        CHECK (risk_level IN ('low', 'medium', 'high')),
    current_draft_version_id BIGINT,
    published_version_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS decision_analytics_thresholds (
    project_id BIGINT PRIMARY KEY
        REFERENCES decision_projects(id) ON DELETE CASCADE,
    failure_warning_rate DOUBLE PRECISION NOT NULL DEFAULT 0.10
        CHECK (failure_warning_rate > 0 AND failure_warning_rate < 1),
    failure_high_rate DOUBLE PRECISION NOT NULL DEFAULT 0.20
        CHECK (failure_high_rate > failure_warning_rate AND failure_high_rate <= 1),
    failure_min_runs INTEGER NOT NULL DEFAULT 10
        CHECK (failure_min_runs BETWEEN 1 AND 100000),
    mismatch_warning_rate DOUBLE PRECISION NOT NULL DEFAULT 0.20
        CHECK (mismatch_warning_rate > 0 AND mismatch_warning_rate < 1),
    mismatch_high_rate DOUBLE PRECISION NOT NULL DEFAULT 0.30
        CHECK (mismatch_high_rate > mismatch_warning_rate AND mismatch_high_rate <= 1),
    mismatch_min_samples INTEGER NOT NULL DEFAULT 5
        CHECK (mismatch_min_samples BETWEEN 1 AND 100000),
    updated_by VARCHAR(255) REFERENCES users(username) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS decision_projects_owner_updated_idx
    ON decision_projects (owner_username, updated_at DESC);

CREATE TABLE IF NOT EXISTS project_members (
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    role VARCHAR(16) NOT NULL CHECK (role IN ('modeler', 'reviewer', 'approver', 'viewer')),
    added_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, username, role)
);

CREATE INDEX IF NOT EXISTS project_members_username_idx
    ON project_members (username, project_id);

CREATE TABLE IF NOT EXISTS decision_versions (
    id BIGSERIAL PRIMARY KEY,
    revision_id VARCHAR(64) NOT NULL DEFAULT md5(random()::text || clock_timestamp()::text),
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    version_number INTEGER NOT NULL CHECK (version_number > 0),
    status VARCHAR(16) NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'validated', 'reviewed', 'published', 'retired')),
    base_version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    rollback_source_version_id BIGINT,
    rollback_reason TEXT,
    change_summary JSONB,
    created_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_by VARCHAR(255) REFERENCES users(username),
    reviewed_at TIMESTAMPTZ,
    UNIQUE (revision_id),
    UNIQUE (project_id, version_number)
);

ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS revision_id VARCHAR(64);
UPDATE decision_versions
SET revision_id = md5(random()::text || clock_timestamp()::text)
WHERE revision_id IS NULL;
ALTER TABLE decision_versions
    ALTER COLUMN revision_id SET DEFAULT md5(random()::text || clock_timestamp()::text),
    ALTER COLUMN revision_id SET NOT NULL;
ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS parent_revision_id VARCHAR(64);
CREATE UNIQUE INDEX IF NOT EXISTS decision_versions_revision_id_idx
    ON decision_versions(revision_id);

CREATE INDEX IF NOT EXISTS decision_versions_project_created_idx
    ON decision_versions (project_id, created_at DESC);

ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS rollback_source_version_id BIGINT;
ALTER TABLE decision_versions
    ADD COLUMN IF NOT EXISTS rollback_reason TEXT;

CREATE TABLE IF NOT EXISTS decision_artifacts (
    id BIGSERIAL PRIMARY KEY,
    version_id BIGINT NOT NULL UNIQUE REFERENCES decision_versions(id) ON DELETE CASCADE,
    decision_brief_json JSONB,
    decision_plan_json JSONB,
    model_patch_json JSONB,
    change_review_json JSONB,
    dsl_yaml TEXT NOT NULL,
    dmn_xml TEXT NOT NULL,
    verification_report_json JSONB,
    content_hash VARCHAR(128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE decision_artifacts
    ADD COLUMN IF NOT EXISTS model_patch_json JSONB;
ALTER TABLE decision_artifacts
    ADD COLUMN IF NOT EXISTS change_review_json JSONB;

CREATE TABLE IF NOT EXISTS agent_runs (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    stage VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    model VARCHAR(255),
    prompt_version VARCHAR(64),
    prompt_digest CHAR(64),
    input_json JSONB,
    output_json JSONB,
    usage_json JSONB,
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    experiment_name TEXT,
    experiment_variant TEXT,
    experiment_treatment_percent SMALLINT,
    experiment_bucket SMALLINT,
    error_json JSONB,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    CONSTRAINT agent_runs_experiment_treatment_percent_range_check
        CHECK (
            experiment_treatment_percent IS NULL
            OR experiment_treatment_percent BETWEEN 0 AND 100
        ),
    CONSTRAINT agent_runs_experiment_bucket_range_check
        CHECK (experiment_bucket IS NULL OR experiment_bucket BETWEEN 0 AND 99),
    CONSTRAINT agent_runs_experiment_variant_allowed_check
        CHECK (experiment_variant IS NULL OR experiment_variant IN ('control', 'treatment')),
    CONSTRAINT agent_runs_experiment_audit_check
        CHECK (
            (experiment_treatment_percent IS NULL) = (experiment_bucket IS NULL)
            AND (
                experiment_bucket IS NULL
                OR (
                    experiment_variant IS NOT NULL
                    AND ((experiment_bucket < experiment_treatment_percent)
                         = (experiment_variant = 'treatment'))
                )
            )
        )
);

ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS usage_json JSONB;
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_name TEXT;
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_variant TEXT;
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_treatment_percent SMALLINT;
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS experiment_bucket SMALLINT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_runs_experiment_variant_allowed_check'
          AND conrelid = 'agent_runs'::regclass
    ) THEN
        ALTER TABLE agent_runs
            ADD CONSTRAINT agent_runs_experiment_variant_allowed_check
            CHECK (experiment_variant IS NULL OR experiment_variant IN ('control', 'treatment'));
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_runs_experiment_treatment_percent_range_check'
          AND conrelid = 'agent_runs'::regclass
    ) THEN
        ALTER TABLE agent_runs
            ADD CONSTRAINT agent_runs_experiment_treatment_percent_range_check
            CHECK (
                experiment_treatment_percent IS NULL
                OR experiment_treatment_percent BETWEEN 0 AND 100
            );
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_runs_experiment_bucket_range_check'
          AND conrelid = 'agent_runs'::regclass
    ) THEN
        ALTER TABLE agent_runs
            ADD CONSTRAINT agent_runs_experiment_bucket_range_check
            CHECK (experiment_bucket IS NULL OR experiment_bucket BETWEEN 0 AND 99);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_runs_experiment_audit_check'
          AND conrelid = 'agent_runs'::regclass
    ) THEN
        ALTER TABLE agent_runs
            ADD CONSTRAINT agent_runs_experiment_audit_check
            CHECK (
                (experiment_treatment_percent IS NULL) = (experiment_bucket IS NULL)
                AND (
                    experiment_bucket IS NULL
                    OR (
                        experiment_variant IS NOT NULL
                        AND ((experiment_bucket < experiment_treatment_percent)
                             = (experiment_variant = 'treatment'))
                    )
                )
            );
    END IF;
END;
$$;

CREATE INDEX IF NOT EXISTS agent_runs_project_started_idx
    ON agent_runs (project_id, started_at DESC);

CREATE TABLE IF NOT EXISTS agent_orchestrations (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    trigger_kind VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'running'
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    current_stage VARCHAR(64) NOT NULL,
    parent_orchestration_id BIGINT REFERENCES agent_orchestrations(id) ON DELETE SET NULL,
    retry_of_orchestration_id BIGINT REFERENCES agent_orchestrations(id) ON DELETE SET NULL,
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    CHECK ((status IN ('pending', 'running')) = (finished_at IS NULL)),
    CHECK (parent_orchestration_id IS NULL OR parent_orchestration_id <> id)
);
ALTER TABLE agent_orchestrations
    ADD COLUMN IF NOT EXISTS heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE agent_orchestrations
    ADD COLUMN IF NOT EXISTS parent_orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;
ALTER TABLE agent_orchestrations
    ADD COLUMN IF NOT EXISTS retry_of_orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS agent_orchestrations_project_created_idx
    ON agent_orchestrations (project_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS agent_orchestrations_status_created_idx
    ON agent_orchestrations (status, created_at)
    WHERE status IN ('pending', 'running');
CREATE INDEX IF NOT EXISTS agent_orchestrations_heartbeat_idx
    ON agent_orchestrations (heartbeat_at)
    WHERE status IN ('pending', 'running');
CREATE INDEX IF NOT EXISTS agent_orchestrations_parent_idx
    ON agent_orchestrations (parent_orchestration_id)
    WHERE parent_orchestration_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS agent_orchestrations_retry_of_idx
    ON agent_orchestrations (retry_of_orchestration_id)
    WHERE retry_of_orchestration_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS agent_orchestrations_one_active_retry_idx
    ON agent_orchestrations (retry_of_orchestration_id)
    WHERE retry_of_orchestration_id IS NOT NULL
      AND status IN ('pending', 'running');
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS prompt_digest CHAR(64);
CREATE TABLE IF NOT EXISTS agent_steps (
    id BIGSERIAL PRIMARY KEY,
    orchestration_id BIGINT NOT NULL REFERENCES agent_orchestrations(id) ON DELETE CASCADE,
    agent_run_id BIGINT REFERENCES agent_runs(id) ON DELETE CASCADE,
    step_no INTEGER NOT NULL CHECK (step_no > 0),
    attempt_no INTEGER NOT NULL DEFAULT 1 CHECK (attempt_no > 0),
    agent_role VARCHAR(64) NOT NULL,
    task_kind VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'running'
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled', 'skipped')),
    model VARCHAR(255),
    prompt_version VARCHAR(64),
    prompt_digest CHAR(64),
    artifact_kind VARCHAR(64),
    artifact_schema_version SMALLINT NOT NULL DEFAULT 1 CHECK (artifact_schema_version > 0),
    artifact_digest CHAR(64),
    usage_json JSONB,
    error_json JSONB,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    UNIQUE (orchestration_id, step_no, attempt_no),
    UNIQUE (orchestration_id, id),
    CHECK ((status IN ('pending', 'running')) = (finished_at IS NULL))
);
ALTER TABLE agent_steps ALTER COLUMN agent_run_id DROP NOT NULL;
ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS prompt_digest CHAR(64);
ALTER TABLE agent_steps DROP CONSTRAINT IF EXISTS agent_steps_agent_run_id_key;
ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS attempt_no INTEGER NOT NULL DEFAULT 1;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_steps_attempt_no_check'
          AND conrelid = 'agent_steps'::regclass
    ) THEN
        ALTER TABLE agent_steps
            ADD CONSTRAINT agent_steps_attempt_no_check
            CHECK (attempt_no > 0);
    END IF;
END;
$$;
ALTER TABLE agent_steps
    DROP CONSTRAINT IF EXISTS agent_steps_orchestration_id_step_no_key;
CREATE UNIQUE INDEX IF NOT EXISTS agent_steps_orchestration_step_attempt_idx
    ON agent_steps (orchestration_id, step_no, attempt_no);
ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS artifact_kind VARCHAR(64);
ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS artifact_schema_version SMALLINT NOT NULL DEFAULT 1;
ALTER TABLE agent_steps
    ADD COLUMN IF NOT EXISTS artifact_digest CHAR(64);
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'agent_steps_artifact_schema_version_check'
          AND conrelid = 'agent_steps'::regclass
    ) THEN
        ALTER TABLE agent_steps
            ADD CONSTRAINT agent_steps_artifact_schema_version_check
            CHECK (artifact_schema_version > 0);
    END IF;
END;
$$;
CREATE UNIQUE INDEX IF NOT EXISTS agent_steps_agent_run_idx
    ON agent_steps (agent_run_id) WHERE agent_run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS agent_steps_orchestration_idx
    ON agent_steps (orchestration_id, step_no);
CREATE TABLE IF NOT EXISTS agent_handoffs (
    id BIGSERIAL PRIMARY KEY,
    orchestration_id BIGINT NOT NULL REFERENCES agent_orchestrations(id) ON DELETE CASCADE,
    from_step_id BIGINT NOT NULL,
    to_step_id BIGINT NOT NULL,
    context_digest CHAR(64) NOT NULL,
    context_summary_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (from_step_id <> to_step_id),
    FOREIGN KEY (orchestration_id, from_step_id)
        REFERENCES agent_steps(orchestration_id, id) ON DELETE CASCADE,
    FOREIGN KEY (orchestration_id, to_step_id)
        REFERENCES agent_steps(orchestration_id, id) ON DELETE CASCADE,
    UNIQUE (from_step_id, to_step_id)
);
CREATE TABLE IF NOT EXISTS agent_orchestration_events (
    id BIGSERIAL PRIMARY KEY,
    orchestration_id BIGINT NOT NULL REFERENCES agent_orchestrations(id) ON DELETE CASCADE,
    event_name VARCHAR(32) NOT NULL,
    payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '24 hours'),
    CHECK (event_name IN ('orchestration', 'agent', 'artifact', 'handoff', 'stage', 'step', 'delta', 'preview', 'result', 'workflow_result', 'error'))
);
CREATE INDEX IF NOT EXISTS agent_orchestration_events_replay_idx
    ON agent_orchestration_events (orchestration_id, id);
CREATE INDEX IF NOT EXISTS agent_orchestration_events_expiry_idx
    ON agent_orchestration_events (expires_at);

CREATE INDEX IF NOT EXISTS agent_runs_experiment_idx
    ON agent_runs (experiment_name, experiment_variant, started_at DESC)
    WHERE experiment_name IS NOT NULL;

CREATE TABLE IF NOT EXISTS model_performance_metrics (
    id BIGSERIAL PRIMARY KEY,
    model_name TEXT NOT NULL,
    task_type TEXT NOT NULL,
    sample_count BIGINT NOT NULL DEFAULT 0 CHECK (sample_count >= 0),
    success_count BIGINT NOT NULL DEFAULT 0 CHECK (success_count >= 0),
    success_rate NUMERIC(5, 2) NOT NULL DEFAULT 0,
    avg_latency_ms INTEGER NOT NULL DEFAULT 0 CHECK (avg_latency_ms >= 0),
    estimated_cost_usd NUMERIC(12, 6),
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (model_name, task_type),
    CHECK (success_count <= sample_count),
    CHECK (success_rate >= 0 AND success_rate <= 100)
);

CREATE OR REPLACE FUNCTION record_model_performance_metric()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    normalized_task TEXT;
    normalized_model TEXT;
    run_latency_ms INTEGER;
    run_succeeded BIGINT;
BEGIN
    IF OLD.status <> 'running'
        OR NEW.status NOT IN ('succeeded', 'failed')
        OR NEW.model IS NULL
        OR NEW.finished_at IS NULL THEN
        RETURN NEW;
    END IF;

    IF NEW.usage_json ? 'response_models' THEN
        IF jsonb_typeof(NEW.usage_json->'response_models') IS DISTINCT FROM 'array' THEN
            RETURN NEW;
        END IF;
        IF jsonb_array_length(NEW.usage_json->'response_models') <> 1 THEN
            RETURN NEW;
        END IF;
        normalized_model := NULLIF(BTRIM(NEW.usage_json->'response_models'->>0), '');
        IF normalized_model IS NULL THEN
            RETURN NEW;
        END IF;
    ELSE
        normalized_model := NEW.model;
    END IF;

    normalized_task := CASE NEW.stage
        WHEN 'requirement_analysis' THEN 'analyze'
        WHEN 'decision_planning' THEN 'plan'
        WHEN 'dsl_generation' THEN 'dsl'
        WHEN 'change_intent' THEN 'change_intent'
        ELSE NEW.stage
    END;
    run_latency_ms := LEAST(
        2147483647,
        GREATEST(0, (EXTRACT(EPOCH FROM (NEW.finished_at - OLD.started_at)) * 1000)::BIGINT)
    )::INTEGER;
    run_succeeded := CASE WHEN NEW.status = 'succeeded' THEN 1 ELSE 0 END;

    INSERT INTO model_performance_metrics (
        model_name, task_type, sample_count, success_count, success_rate,
        avg_latency_ms, last_updated
    ) VALUES (
        normalized_model, normalized_task, 1, run_succeeded, run_succeeded * 100,
        run_latency_ms, NEW.finished_at
    )
    ON CONFLICT (model_name, task_type) DO UPDATE SET
        sample_count = model_performance_metrics.sample_count + 1,
        success_count = model_performance_metrics.success_count + EXCLUDED.success_count,
        success_rate = ROUND(
            (model_performance_metrics.success_count + EXCLUDED.success_count)::NUMERIC
            * 100 / (model_performance_metrics.sample_count + 1),
            2
        ),
        avg_latency_ms = (
            (model_performance_metrics.avg_latency_ms::BIGINT
                * model_performance_metrics.sample_count + EXCLUDED.avg_latency_ms)
            / (model_performance_metrics.sample_count + 1)
        )::INTEGER,
        last_updated = EXCLUDED.last_updated;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS agent_runs_record_model_performance ON agent_runs;
CREATE TRIGGER agent_runs_record_model_performance
    AFTER UPDATE OF status ON agent_runs
    FOR EACH ROW
    WHEN (OLD.status = 'running' AND NEW.status IN ('succeeded', 'failed'))
    EXECUTE FUNCTION record_model_performance_metric();

CREATE TABLE IF NOT EXISTS agent_model_votes (
    id BIGSERIAL PRIMARY KEY,
    run_id BIGINT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    task_type TEXT NOT NULL,
    models_used TEXT[] NOT NULL DEFAULT '{}',
    consensus_result JSONB NOT NULL,
    divergence_detail JSONB NOT NULL,
    final_decision JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (run_id, task_type)
);

CREATE INDEX IF NOT EXISTS agent_model_votes_created_at_idx
    ON agent_model_votes (created_at DESC);

CREATE TABLE IF NOT EXISTS business_logic_patterns (
    id BIGSERIAL PRIMARY KEY,
    pattern_type TEXT NOT NULL UNIQUE
        CHECK (pattern_type IN ('tree', 'matrix', 'workflow', 'threshold')),
    description TEXT NOT NULL,
    keywords TEXT[] NOT NULL DEFAULT '{}',
    typical_structures JSONB NOT NULL DEFAULT '{}'::JSONB,
    sample_clarifications JSONB NOT NULL DEFAULT '[]'::JSONB,
    success_rate NUMERIC(5, 2)
        CHECK (success_rate IS NULL OR (success_rate >= 0 AND success_rate <= 100)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO business_logic_patterns (
    pattern_type, description, keywords, typical_structures, sample_clarifications
) VALUES
    ('tree', '多条件分支、分级或互斥规则', ARRAY['如果', '若', '否则', '条件', '分级'], '{"shape":"conditions -> branches -> outcome"}'::JSONB, '["多个条件同时满足时如何处理？", "未匹配任何条件时的默认结果是什么？"]'::JSONB),
    ('matrix', '多个维度、指标或权重共同计算评分', ARRAY['评分', '打分', '权重', '维度', '综合得分'], '{"shape":"dimensions -> weights -> score -> category"}'::JSONB, '["各维度的权重或评分范围是什么？", "总分边界如何映射到结果？"]'::JSONB),
    ('workflow', '按顺序执行的校验、审批或处理步骤', ARRAY['流程', '步骤', '首先', '然后', '审批'], '{"shape":"ordered steps -> gates -> terminal outcome"}'::JSONB, '["步骤失败后是否继续后续流程？", "哪些步骤允许并行处理？"]'::JSONB),
    ('threshold', '基于边界、区间或门槛判定结果', ARRAY['阈值', '超过', '低于', '至少', '区间'], '{"shape":"variable + boundaries -> outcome"}'::JSONB, '["边界值属于哪一侧？", "阈值之间的空档如何处理？"]'::JSONB)
ON CONFLICT (pattern_type) DO NOTHING;

CREATE TABLE IF NOT EXISTS decision_test_cases (
    id BIGSERIAL PRIMARY KEY,
    version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    name VARCHAR(160) NOT NULL,
    category VARCHAR(24) NOT NULL DEFAULT 'business'
        CHECK (category IN ('business', 'boundary', 'regression')),
    input_json JSONB NOT NULL,
    expected_output_json JSONB NOT NULL,
    actual_output_json JSONB,
    execution_trace_json JSONB,
    status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'passed', 'failed', 'error')),
    source VARCHAR(16) NOT NULL DEFAULT 'manual'
        CHECK (source IN ('manual', 'generated', 'tester_proposal')),
    tester_proposal_id VARCHAR(64),
    approved_by VARCHAR(255) REFERENCES users(username) ON DELETE SET NULL,
    approved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE decision_test_cases
    ADD COLUMN IF NOT EXISTS execution_trace_json JSONB;
ALTER TABLE decision_test_cases
    ADD COLUMN IF NOT EXISTS tester_proposal_id VARCHAR(64);
ALTER TABLE decision_test_cases
    DROP CONSTRAINT IF EXISTS decision_test_cases_source_check;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_test_cases_source_allowed_check'
          AND conrelid = 'decision_test_cases'::regclass
    ) THEN
        ALTER TABLE decision_test_cases
            ADD CONSTRAINT decision_test_cases_source_allowed_check
            CHECK (source IN ('manual', 'generated', 'tester_proposal'));
    END IF;
END;
$$;

CREATE INDEX IF NOT EXISTS decision_test_cases_version_created_idx
    ON decision_test_cases (version_id, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS decision_test_cases_tester_proposal_idx
    ON decision_test_cases (version_id, tester_proposal_id)
    WHERE tester_proposal_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS decision_runs (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE RESTRICT,
    service_id VARCHAR(255) NOT NULL,
    service_name VARCHAR(255) NOT NULL,
    input_json JSONB NOT NULL,
    output_json JSONB NOT NULL,
    execution_trace_json JSONB NOT NULL,
    duration_ms BIGINT NOT NULL CHECK (duration_ms >= 0),
    status VARCHAR(16) NOT NULL CHECK (status IN ('succeeded', 'failed')),
    executed_by VARCHAR(255) NOT NULL REFERENCES users(username),
    executed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    business_reference VARCHAR(255) NOT NULL DEFAULT '',
    actual_outcome_json JSONB,
    outcome_assessment VARCHAR(16)
        CHECK (outcome_assessment IS NULL OR outcome_assessment IN ('confirmed', 'mismatch', 'inconclusive')),
    outcome_note TEXT NOT NULL DEFAULT '',
    outcome_occurred_at TIMESTAMPTZ,
    outcome_reported_by VARCHAR(255) REFERENCES users(username),
    outcome_reported_at TIMESTAMPTZ
);

ALTER TABLE decision_runs
    ADD COLUMN IF NOT EXISTS business_reference VARCHAR(255) NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS actual_outcome_json JSONB,
    ADD COLUMN IF NOT EXISTS outcome_assessment VARCHAR(16),
    ADD COLUMN IF NOT EXISTS outcome_note TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS outcome_occurred_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS outcome_reported_by VARCHAR(255) REFERENCES users(username),
    ADD COLUMN IF NOT EXISTS outcome_reported_at TIMESTAMPTZ;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_runs_outcome_assessment_check'
    ) THEN
        ALTER TABLE decision_runs
            ADD CONSTRAINT decision_runs_outcome_assessment_check
            CHECK (outcome_assessment IS NULL OR outcome_assessment IN ('confirmed', 'mismatch', 'inconclusive'));
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS decision_runs_project_executed_idx
    ON decision_runs (project_id, executed_at DESC);
CREATE INDEX IF NOT EXISTS decision_runs_project_version_executed_idx
    ON decision_runs (project_id, version_id, executed_at, id);
CREATE INDEX IF NOT EXISTS decision_runs_project_outcome_idx
    ON decision_runs (project_id, outcome_assessment, executed_at DESC);

CREATE TABLE IF NOT EXISTS decision_run_outcome_receipts (
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    service_id VARCHAR(255) NOT NULL,
    idempotency_key VARCHAR(128) NOT NULL,
    run_id BIGINT NOT NULL REFERENCES decision_runs(id) ON DELETE CASCADE,
    payload_digest CHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, service_id, idempotency_key)
);

CREATE INDEX IF NOT EXISTS decision_run_outcome_receipts_run_idx
    ON decision_run_outcome_receipts (project_id, run_id);

CREATE TABLE IF NOT EXISTS decision_run_explanations (
    id BIGSERIAL PRIMARY KEY,
    decision_run_id BIGINT NOT NULL UNIQUE
        REFERENCES decision_runs(id) ON DELETE CASCADE,
    agent_run_id BIGINT REFERENCES agent_runs(id) ON DELETE SET NULL,
    generation_revision BIGINT NOT NULL DEFAULT 1 CHECK (generation_revision > 0),
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
    explanation_json JSONB,
    model TEXT,
    prompt_version TEXT NOT NULL,
    prompt_digest CHAR(64) NOT NULL CHECK (prompt_digest ~ '^[0-9a-f]{64}$'),
    trace_digest CHAR(64) NOT NULL CHECK (trace_digest ~ '^[0-9a-f]{64}$'),
    error_message TEXT,
    created_by TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    CHECK (
        (status = 'succeeded' AND explanation_json IS NOT NULL AND completed_at IS NOT NULL)
        OR (status <> 'succeeded' AND explanation_json IS NULL)
    )
);
ALTER TABLE decision_run_explanations
    ADD COLUMN IF NOT EXISTS agent_run_id BIGINT
        REFERENCES agent_runs(id) ON DELETE SET NULL;
ALTER TABLE decision_run_explanations
    ADD COLUMN IF NOT EXISTS generation_revision BIGINT NOT NULL DEFAULT 1
        CHECK (generation_revision > 0);
CREATE INDEX IF NOT EXISTS decision_run_explanations_status_idx
    ON decision_run_explanations (status, updated_at)
    WHERE status = 'running';

CREATE TABLE IF NOT EXISTS decision_run_explanation_reviews (
    id BIGSERIAL PRIMARY KEY,
    explanation_id BIGINT NOT NULL
        REFERENCES decision_run_explanations(id) ON DELETE CASCADE,
    explanation_revision BIGINT CHECK (explanation_revision > 0),
    reviewer_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    review_label VARCHAR(24) NOT NULL
        CHECK (review_label IN ('readable', 'needs_revision', 'inconclusive')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS decision_run_explanation_reviews_history_idx
    ON decision_run_explanation_reviews (explanation_id, created_at DESC, id DESC);
ALTER TABLE decision_run_explanation_reviews
    ADD COLUMN IF NOT EXISTS explanation_revision BIGINT
        CHECK (explanation_revision > 0);
-- Legacy labels may refer to content that has since been overwritten. Keep them
-- append-only, with unknown revision; never infer a version from today's cache.
CREATE UNIQUE INDEX IF NOT EXISTS decision_run_explanation_reviews_revision_reviewer_idx
    ON decision_run_explanation_reviews (explanation_id, explanation_revision, reviewer_username)
    WHERE explanation_revision IS NOT NULL;

CREATE OR REPLACE FUNCTION validate_decision_explanation_review_revision()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    current_revision BIGINT;
    current_status TEXT;
BEGIN
    SELECT generation_revision, status INTO current_revision, current_status
    FROM decision_run_explanations WHERE id = NEW.explanation_id FOR UPDATE;
    IF NEW.explanation_revision IS NULL
       OR current_status IS DISTINCT FROM 'succeeded'
       OR NEW.explanation_revision IS DISTINCT FROM current_revision THEN
        RAISE EXCEPTION 'explanation changed or is not ready for review'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
DROP TRIGGER IF EXISTS decision_run_explanation_review_revision_guard
    ON decision_run_explanation_reviews;
CREATE TRIGGER decision_run_explanation_review_revision_guard
    BEFORE INSERT ON decision_run_explanation_reviews
    FOR EACH ROW EXECUTE FUNCTION validate_decision_explanation_review_revision();

REVOKE UPDATE, DELETE ON TABLE decision_run_explanation_reviews FROM PUBLIC;

CREATE TABLE IF NOT EXISTS deviation_analyses (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    decision_run_id BIGINT NOT NULL REFERENCES decision_runs(id) ON DELETE CASCADE,
    status VARCHAR(32) NOT NULL CHECK (status IN ('candidate', 'insufficient_evidence')),
    attributed_rule TEXT,
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0 CHECK (confidence >= 0 AND confidence <= 1),
    evidence_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    review_label VARCHAR(32) CHECK (review_label IN ('correct', 'incorrect', 'inconclusive')),
    reviewed_by TEXT,
    reviewed_at TIMESTAMPTZ,
    CHECK (
        (review_label IS NULL AND reviewed_by IS NULL AND reviewed_at IS NULL)
        OR (review_label IS NOT NULL AND reviewed_by IS NOT NULL AND reviewed_at IS NOT NULL)
    ),
    UNIQUE (decision_run_id)
);

CREATE INDEX IF NOT EXISTS deviation_analyses_project_created_idx
    ON deviation_analyses (project_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS deviation_analyses_project_id_idx
    ON deviation_analyses (project_id, id);

CREATE TABLE IF NOT EXISTS version_behavior_diff (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    baseline_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    candidate_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    baseline_run_id BIGINT REFERENCES decision_runs(id) ON DELETE SET NULL,
    candidate_run_id BIGINT REFERENCES decision_runs(id) ON DELETE SET NULL,
    test_case_id BIGINT REFERENCES decision_test_cases(id) ON DELETE SET NULL,
    baseline_output JSONB NOT NULL,
    candidate_output JSONB NOT NULL,
    diff_json JSONB NOT NULL,
    risk_level TEXT NOT NULL CHECK (risk_level IN ('low', 'medium', 'high')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (baseline_version_id <> candidate_version_id),
    UNIQUE (baseline_run_id, candidate_run_id)
);

CREATE INDEX IF NOT EXISTS version_behavior_diff_candidate_idx
    ON version_behavior_diff(candidate_version_id, created_at DESC);
CREATE INDEX IF NOT EXISTS version_behavior_diff_project_idx
    ON version_behavior_diff(project_id, created_at DESC);

CREATE TABLE IF NOT EXISTS regression_test_sources (
    id BIGSERIAL PRIMARY KEY,
    candidate_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    source_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    source_run_id BIGINT NOT NULL REFERENCES decision_runs(id) ON DELETE CASCADE,
    test_input JSONB NOT NULL,
    expected_output JSONB NOT NULL,
    actual_output JSONB,
    execution_trace_json JSONB,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'passed', 'failed', 'error')),
    risk_level TEXT NOT NULL DEFAULT 'low'
        CHECK (risk_level IN ('low', 'medium', 'high')),
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (candidate_version_id, source_run_id)
);

CREATE INDEX IF NOT EXISTS regression_test_sources_candidate_idx
    ON regression_test_sources(candidate_version_id, status);

-- Public, login-free invocation credentials. Only a keyed hash is persisted;
-- the secret is shown once when a credential is created.
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

CREATE TABLE IF NOT EXISTS decision_optimization_proposals (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    source_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    finding_code VARCHAR(64) NOT NULL,
    finding_level VARCHAR(16) NOT NULL,
    element VARCHAR(255) NOT NULL DEFAULT '',
    title VARCHAR(240) NOT NULL,
    summary TEXT NOT NULL,
    proposed_actions_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    evidence_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    analysis_snapshot_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    status VARCHAR(16) NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'confirmed', 'dismissed', 'withdrawn')),
    generated_by VARCHAR(255) NOT NULL REFERENCES users(username),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_by VARCHAR(255) REFERENCES users(username),
    resolved_at TIMESTAMPTZ,
    resolution_note TEXT NOT NULL DEFAULT '',
    resulting_version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    effectiveness_result_json JSONB,
    effectiveness_tracked_at TIMESTAMPTZ
);

ALTER TABLE decision_optimization_proposals
    ADD COLUMN IF NOT EXISTS effectiveness_result_json JSONB,
    ADD COLUMN IF NOT EXISTS effectiveness_tracked_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS decision_optimization_proposals_project_generated_idx
    ON decision_optimization_proposals (project_id, generated_at DESC, id DESC);
CREATE UNIQUE INDEX IF NOT EXISTS decision_optimization_proposals_open_source_finding_idx
    ON decision_optimization_proposals (project_id, source_version_id, finding_code, element)
    WHERE status = 'open';
CREATE INDEX IF NOT EXISTS decision_optimization_proposals_effectiveness_pending_idx
    ON decision_optimization_proposals (resulting_version_id)
    WHERE status = 'confirmed' AND effectiveness_tracked_at IS NULL;
CREATE INDEX IF NOT EXISTS decision_versions_project_base_lineage_idx
    ON decision_versions (project_id, base_version_id, id);

CREATE TABLE IF NOT EXISTS decision_drift_automation_rules (
    project_id BIGINT PRIMARY KEY REFERENCES decision_projects(id) ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    finding_codes TEXT[] NOT NULL DEFAULT '{}',
    cooldown_hours INTEGER NOT NULL DEFAULT 24
        CHECK (cooldown_hours BETWEEN 1 AND 720),
    webhook_url TEXT,
    next_eligible_at TIMESTAMPTZ,
    updated_by VARCHAR(255) NOT NULL REFERENCES users(username),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (cardinality(finding_codes) <= 20)
);
ALTER TABLE decision_drift_automation_rules
    ADD COLUMN IF NOT EXISTS webhook_url TEXT;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_drift_webhook_url_check'
          AND conrelid = 'decision_drift_automation_rules'::regclass
    ) THEN
        ALTER TABLE decision_drift_automation_rules
            ADD CONSTRAINT decision_drift_webhook_url_check
            CHECK (webhook_url IS NULL OR
                   (length(webhook_url) <= 2048 AND webhook_url ~ '^https://[^[:space:]]+$'));
    END IF;
END;
$$;
CREATE INDEX IF NOT EXISTS decision_drift_rules_enabled_idx
    ON decision_drift_automation_rules (project_id)
    WHERE enabled;

CREATE TABLE IF NOT EXISTS decision_drift_events (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    source_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    dedupe_key TEXT NOT NULL,
    finding_codes TEXT[] NOT NULL,
    evidence_json JSONB NOT NULL,
    action_result_json JSONB,
    review_label TEXT,
    reviewed_by TEXT,
    reviewed_at TIMESTAMPTZ,
    status VARCHAR(16) NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'dismissed', 'proposal_created')),
    observed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ,
    UNIQUE (project_id, dedupe_key)
);
ALTER TABLE decision_drift_events
    ADD COLUMN IF NOT EXISTS review_label TEXT,
    ADD COLUMN IF NOT EXISTS reviewed_by TEXT,
    ADD COLUMN IF NOT EXISTS reviewed_at TIMESTAMPTZ;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'decision_drift_event_review_check'
          AND conrelid = 'decision_drift_events'::regclass
    ) THEN
        ALTER TABLE decision_drift_events
            ADD CONSTRAINT decision_drift_event_review_check
            CHECK (
                (review_label IS NULL OR review_label IN ('confirmed_drift', 'false_positive', 'inconclusive'))
                AND (review_label IS NULL) = (reviewed_by IS NULL)
                AND (review_label IS NULL) = (reviewed_at IS NULL)
            );
    END IF;
END;
$$;
CREATE INDEX IF NOT EXISTS decision_drift_events_project_observed_idx
    ON decision_drift_events (project_id, observed_at DESC, id DESC);

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

CREATE TABLE IF NOT EXISTS decision_drift_webhook_deliveries (
    id BIGSERIAL PRIMARY KEY,
    event_id BIGINT NOT NULL UNIQUE
        REFERENCES decision_drift_events(id) ON DELETE CASCADE,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    endpoint_url TEXT NOT NULL CHECK (length(endpoint_url) <= 2048),
    payload_json JSONB NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'sending', 'delivered', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    locked_at TIMESTAMPTZ,
    last_http_status SMALLINT,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ,
    CHECK ((status = 'sending') = (locked_at IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS decision_drift_webhook_pending_idx
    ON decision_drift_webhook_deliveries (next_attempt_at, id)
    WHERE status IN ('pending', 'sending');

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
        CHECK (NOT passed OR (test_set_size >= 10 AND simulated_accuracy >= baseline_accuracy))
);

ALTER TABLE optimization_simulations
    ADD COLUMN IF NOT EXISTS passed BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE optimization_simulations
    ADD COLUMN IF NOT EXISTS candidate_artifact_json JSONB NOT NULL DEFAULT '{}'::JSONB;
UPDATE optimization_simulations SET passed = FALSE WHERE passed AND test_set_size < 10;
ALTER TABLE optimization_simulations
    DROP CONSTRAINT IF EXISTS optimization_simulations_passed_check;
ALTER TABLE optimization_simulations
    ADD CONSTRAINT optimization_simulations_passed_check
    CHECK (NOT passed OR (test_set_size >= 10 AND simulated_accuracy >= baseline_accuracy));
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

CREATE TABLE IF NOT EXISTS audit_events (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    event_type VARCHAR(64) NOT NULL,
    actor_username VARCHAR(255) NOT NULL,
    summary VARCHAR(240) NOT NULL,
    details_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS audit_events_project_created_idx
    ON audit_events (project_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS decision_templates (
    id BIGSERIAL PRIMARY KEY,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    name VARCHAR(120) NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    industry VARCHAR(80) NOT NULL DEFAULT '',
    risk_level VARCHAR(16) NOT NULL DEFAULT 'medium'
        CHECK (risk_level IN ('low', 'medium', 'high')),
    source_project_id BIGINT REFERENCES decision_projects(id) ON DELETE SET NULL,
    source_version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    requirement_text TEXT,
    decision_brief_json JSONB,
    decision_plan_json JSONB,
    dsl_yaml TEXT NOT NULL,
    dmn_xml TEXT NOT NULL,
    test_cases_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS decision_templates_owner_updated_idx
    ON decision_templates (owner_username, updated_at DESC, id DESC);

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

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_type type
        JOIN pg_namespace namespace ON namespace.oid = type.typnamespace
        WHERE type.typname = 'vector'
          AND namespace.nspname = ANY(current_schemas(true))
    ) THEN
        EXECUTE 'ALTER TABLE decision_patterns ADD COLUMN IF NOT EXISTS embedding vector(1024)';
        ALTER TABLE decision_patterns ADD COLUMN IF NOT EXISTS embedding_model TEXT;
        EXECUTE 'CREATE INDEX IF NOT EXISTS decision_patterns_embedding_idx ON decision_patterns USING hnsw (embedding vector_cosine_ops)';
    END IF;
END;
$$;

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

CREATE TABLE IF NOT EXISTS knowledge_sources (
    id BIGSERIAL PRIMARY KEY,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    source_type VARCHAR(16) NOT NULL CHECK (source_type IN ('term', 'field', 'policy')),
    name VARCHAR(160) NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL,
    version_label VARCHAR(80) NOT NULL DEFAULT '',
    source_uri TEXT NOT NULL DEFAULT '',
    valid_from DATE,
    valid_until DATE,
    status VARCHAR(16) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (valid_until IS NULL OR valid_from IS NULL OR valid_until >= valid_from)
);

CREATE INDEX IF NOT EXISTS knowledge_sources_owner_updated_idx
    ON knowledge_sources (owner_username, updated_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS knowledge_chunks (
    id BIGSERIAL PRIMARY KEY,
    knowledge_source_id BIGINT NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (knowledge_source_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS knowledge_chunks_source_idx
    ON knowledge_chunks (knowledge_source_id, chunk_index);
CREATE INDEX IF NOT EXISTS knowledge_chunks_content_idx
    ON knowledge_chunks USING GIN (to_tsvector('simple', content));

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_type type
        JOIN pg_namespace namespace ON namespace.oid = type.typnamespace
        WHERE type.typname = 'vector'
          AND namespace.nspname = ANY(current_schemas(true))
    ) THEN
        EXECUTE 'ALTER TABLE knowledge_chunks ADD COLUMN IF NOT EXISTS embedding vector(1024)';
        ALTER TABLE knowledge_chunks ADD COLUMN IF NOT EXISTS embedding_model TEXT;
        EXECUTE 'CREATE INDEX IF NOT EXISTS knowledge_chunks_embedding_idx ON knowledge_chunks USING hnsw (embedding vector_cosine_ops)';
    END IF;
END;
$$;

CREATE TABLE IF NOT EXISTS repair_memories (
    id BIGSERIAL PRIMARY KEY,
    error_signature CHAR(64) NOT NULL,
    error_category VARCHAR(2) NOT NULL CHECK (error_category IN ('E1', 'E2', 'E3', 'E4', 'E5', 'E6')),
    context_digest CHAR(64),
    repair_summary TEXT NOT NULL,
    repair_diff JSONB,
    outcome VARCHAR(16) NOT NULL CHECK (outcome IN ('success', 'partial', 'failed')),
    model VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS repair_memories_signature_idx
    ON repair_memories (error_signature, error_category, outcome, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS project_knowledge_sources (
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    knowledge_source_id BIGINT NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    attached_by VARCHAR(255) NOT NULL REFERENCES users(username),
    attached_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, knowledge_source_id)
);

CREATE INDEX IF NOT EXISTS project_knowledge_sources_source_idx
    ON project_knowledge_sources (knowledge_source_id, project_id);

CREATE TABLE IF NOT EXISTS knowledge_audit_events (
    id BIGSERIAL PRIMARY KEY,
    knowledge_source_id BIGINT REFERENCES knowledge_sources(id) ON DELETE SET NULL,
    owner_username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    event_type VARCHAR(64) NOT NULL,
    summary VARCHAR(240) NOT NULL,
    details_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS knowledge_audit_events_owner_created_idx
    ON knowledge_audit_events (owner_username, created_at DESC, id DESC);

ALTER TABLE decision_artifacts
    ADD COLUMN IF NOT EXISTS knowledge_citations_json JSONB NOT NULL DEFAULT '[]'::JSONB;
ALTER TABLE decision_templates
    ADD COLUMN IF NOT EXISTS knowledge_citations_json JSONB NOT NULL DEFAULT '[]'::JSONB;

CREATE TABLE IF NOT EXISTS decision_workflows (
    project_id BIGINT PRIMARY KEY REFERENCES decision_projects(id) ON DELETE CASCADE,
    base_revision BIGINT,
    original_requirement_text TEXT,
    status VARCHAR(24) NOT NULL
        CHECK (status IN ('analyzed', 'clarification', 'planned', 'confirmed')),
    requirement_text TEXT NOT NULL,
    decision_brief_json JSONB NOT NULL,
    clarifications_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    answers_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    decision_plan_json JSONB,
    confirmed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
ALTER TABLE decision_workflows
    ADD COLUMN IF NOT EXISTS tester_report_json JSONB;
ALTER TABLE decision_workflows
    ADD COLUMN IF NOT EXISTS analysis_orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS decision_workflows_analysis_orchestration_idx
    ON decision_workflows (analysis_orchestration_id)
    WHERE analysis_orchestration_id IS NOT NULL;
ALTER TABLE decision_workflows
    ADD COLUMN IF NOT EXISTS plan_orchestration_id BIGINT
        REFERENCES agent_orchestrations(id) ON DELETE SET NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_projects_current_draft_fk'
    ) THEN
        ALTER TABLE decision_projects
            ADD CONSTRAINT decision_projects_current_draft_fk
            FOREIGN KEY (current_draft_version_id)
            REFERENCES decision_versions(id) ON DELETE SET NULL;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_projects_published_fk'
    ) THEN
        ALTER TABLE decision_projects
            ADD CONSTRAINT decision_projects_published_fk
            FOREIGN KEY (published_version_id)
            REFERENCES decision_versions(id) ON DELETE SET NULL;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'decision_versions_rollback_source_fk'
    ) THEN
        ALTER TABLE decision_versions
            ADD CONSTRAINT decision_versions_rollback_source_fk
            FOREIGN KEY (rollback_source_version_id)
            REFERENCES decision_versions(id) ON DELETE SET NULL;
    END IF;
END $$;

-- Persisted model-change preview candidates (safe preview/commit workflow).
CREATE INDEX IF NOT EXISTS decision_artifacts_content_hash_idx
    ON decision_artifacts(version_id, content_hash);

CREATE TABLE IF NOT EXISTS model_change_candidates (
    id BIGSERIAL PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    base_version_id BIGINT NOT NULL REFERENCES decision_versions(id) ON DELETE CASCADE,
    base_revision_id VARCHAR(64) NOT NULL,
    base_hash VARCHAR(128) NOT NULL,
    dsl_yaml TEXT NOT NULL,
    dmn_xml TEXT NOT NULL,
    change_json JSONB NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'committing', 'committed', 'expired')),
    created_by VARCHAR(255) NOT NULL REFERENCES users(username),
    resulting_version_id BIGINT REFERENCES decision_versions(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS prompt_assets (
    id BIGSERIAL PRIMARY KEY,
    prompt_key TEXT NOT NULL CHECK (prompt_key IN ('requirement_analysis', 'decision_planning', 'dsl_generation', 'change_intent', 'optimization_proposal', 'decision_explanation', 'tester_judge')),
    version TEXT NOT NULL CHECK (version ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$'),
    content TEXT NOT NULL CHECK (length(content) BETWEEN 1 AND 100000),
    content_digest CHAR(64) NOT NULL CHECK (content_digest ~ '^[0-9a-f]{64}$'),
    status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'evaluated', 'active', 'retired')),
    evaluation_json JSONB,
    created_by TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    evaluated_at TIMESTAMPTZ,
    activated_at TIMESTAMPTZ,
    retired_at TIMESTAMPTZ,
    UNIQUE (prompt_key, version)
);
ALTER TABLE prompt_assets
    DROP CONSTRAINT IF EXISTS prompt_assets_prompt_key_check;
ALTER TABLE prompt_assets
    ADD CONSTRAINT prompt_assets_prompt_key_check
    CHECK (prompt_key IN (
        'requirement_analysis',
        'decision_planning',
        'dsl_generation',
        'change_intent',
        'optimization_proposal',
        'decision_explanation',
        'tester_judge'
    ));
CREATE UNIQUE INDEX IF NOT EXISTS prompt_assets_one_active_idx
    ON prompt_assets (prompt_key) WHERE status = 'active';
CREATE INDEX IF NOT EXISTS prompt_assets_history_idx
    ON prompt_assets (prompt_key, created_at DESC, id DESC);
CREATE TABLE IF NOT EXISTS prompt_asset_events (
    id BIGSERIAL PRIMARY KEY,
    prompt_asset_id BIGINT NOT NULL REFERENCES prompt_assets(id) ON DELETE RESTRICT,
    event_type TEXT NOT NULL CHECK (event_type IN ('draft_created', 'evaluated', 'activated', 'rollback')),
    actor_username TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    details_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS prompt_asset_events_history_idx
    ON prompt_asset_events (prompt_asset_id, created_at DESC, id DESC);
CREATE TABLE IF NOT EXISTS prompt_evaluation_jobs (
    id BIGSERIAL PRIMARY KEY,
    prompt_asset_id BIGINT NOT NULL REFERENCES prompt_assets(id) ON DELETE RESTRICT,
    created_by TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    completed_cases INTEGER NOT NULL DEFAULT 0 CHECK (completed_cases >= 0),
    total_cases INTEGER NOT NULL CHECK (total_cases > 0),
    result_json JSONB,
    error_message TEXT,
    cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    CHECK (completed_cases <= total_cases),
    CHECK ((status = 'succeeded' AND result_json IS NOT NULL AND finished_at IS NOT NULL)
        OR status <> 'succeeded'),
    CHECK ((status = 'failed' AND error_message IS NOT NULL AND finished_at IS NOT NULL)
        OR status <> 'failed'),
    CONSTRAINT prompt_evaluation_jobs_cancelled_finished_check
        CHECK ((status = 'cancelled' AND finished_at IS NOT NULL) OR status <> 'cancelled')
);
CREATE UNIQUE INDEX IF NOT EXISTS prompt_evaluation_jobs_one_active_per_asset_idx
    ON prompt_evaluation_jobs (prompt_asset_id)
    WHERE status IN ('pending', 'running');
CREATE INDEX IF NOT EXISTS prompt_evaluation_jobs_history_idx
    ON prompt_evaluation_jobs (prompt_asset_id, created_at DESC, id DESC);
CREATE OR REPLACE FUNCTION prevent_prompt_asset_content_mutation()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'DELETE'
       OR NEW.prompt_key IS DISTINCT FROM OLD.prompt_key
       OR NEW.version IS DISTINCT FROM OLD.version
       OR NEW.content IS DISTINCT FROM OLD.content
       OR NEW.content_digest IS DISTINCT FROM OLD.content_digest
       OR NEW.created_by IS DISTINCT FROM OLD.created_by
       OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'prompt asset content versions are immutable';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE OR REPLACE FUNCTION prevent_prompt_asset_event_mutation()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'prompt asset events are append-only';
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS prompt_assets_immutable ON prompt_assets;
CREATE TRIGGER prompt_assets_immutable
    BEFORE UPDATE OR DELETE ON prompt_assets
    FOR EACH ROW EXECUTE FUNCTION prevent_prompt_asset_content_mutation();
DROP TRIGGER IF EXISTS prompt_asset_events_append_only ON prompt_asset_events;
CREATE TRIGGER prompt_asset_events_append_only
    BEFORE UPDATE OR DELETE ON prompt_asset_events
    FOR EACH ROW EXECUTE FUNCTION prevent_prompt_asset_event_mutation();

ALTER TABLE model_change_candidates
    ADD COLUMN IF NOT EXISTS base_revision_id VARCHAR(64);
UPDATE model_change_candidates candidate
SET base_revision_id = version.revision_id
FROM decision_versions version
WHERE candidate.base_version_id = version.id
  AND candidate.base_revision_id IS NULL;
ALTER TABLE model_change_candidates
    ALTER COLUMN base_revision_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS model_change_candidates_project_idx
    ON model_change_candidates(project_id, created_at DESC);

-- Optional application-role grants. Invoke with `-v app_user=fuxi` when the
-- tables are created by a different administrator role.
\if :{?app_user}
GRANT USAGE ON SCHEMA public TO :"app_user";
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
    users,
    decision_projects,
    decision_analytics_thresholds,
    project_members,
    decision_versions,
    decision_artifacts,
    decision_test_cases,
    decision_runs,
    decision_optimization_proposals,
    optimization_simulations,
    optimization_autodraft_shadow_evaluations,
    decision_templates,
    decision_patterns,
    agent_orchestrations,
    agent_steps,
    agent_handoffs,
    agent_orchestration_events,
    pattern_recommendations,
    knowledge_sources,
    knowledge_chunks,
    repair_memories,
    project_knowledge_sources,
    agent_runs,
    model_performance_metrics,
    agent_model_votes,
    business_logic_patterns,
    version_behavior_diff,
    regression_test_sources,
    decision_workflows,
    model_change_candidates,
    decision_service_api_keys,
    decision_run_outcome_receipts,
    decision_run_explanations,
    decision_run_explanation_reviews,
    deviation_analyses,
    decision_drift_automation_rules,
    decision_drift_events,
    decision_drift_scan_windows,
    decision_drift_webhook_deliveries,
    prompt_assets,
    prompt_evaluation_jobs
TO :"app_user";
GRANT SELECT, INSERT ON TABLE prompt_asset_events TO :"app_user";
GRANT SELECT, INSERT ON TABLE audit_events, knowledge_audit_events TO :"app_user";
REVOKE UPDATE, DELETE, TRUNCATE, TRIGGER ON TABLE
    audit_events, knowledge_audit_events FROM :"app_user";
GRANT SELECT, INSERT, UPDATE ON TABLE prompt_evaluation_jobs TO :"app_user";
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO :"app_user";
GRANT SELECT, INSERT ON TABLE decision_run_explanation_reviews TO :"app_user";
REVOKE UPDATE, DELETE ON TABLE decision_run_explanation_reviews FROM :"app_user";
REVOKE UPDATE, TRUNCATE, TRIGGER ON TABLE agent_orchestration_events FROM :"app_user";
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    REVOKE ALL ON TABLES FROM :"app_user";
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    REVOKE ALL ON SEQUENCES FROM :"app_user";
\endif

-- The application creates the admin account using the required ADMIN_PASSWORD.
-- Never seed accounts with a shared password in the database bootstrap.
