BEGIN TRANSACTION READ ONLY;

SELECT current_database() AS database_name, current_user AS connected_role;

WITH expected_objects(feature, relation_name, required_columns, optional) AS (
    VALUES
        ('修复记忆', 'repair_memories', ARRAY['error_signature', 'repair_summary', 'outcome']::TEXT[], FALSE),
        ('知识分块', 'knowledge_chunks', ARRAY['knowledge_source_id', 'chunk_index', 'content']::TEXT[], FALSE),
        ('分区运行记录', 'decision_runs_partitioned', ARRAY['project_id', 'version_id', 'executed_at']::TEXT[], FALSE),
        ('外部结果回传', 'decision_runs', ARRAY['business_reference', 'actual_outcome_json', 'outcome_assessment', 'outcome_reported_at']::TEXT[], FALSE),
        ('需求分析编排关联', 'decision_workflows', ARRAY['analysis_orchestration_id', 'plan_orchestration_id', 'tester_report_json']::TEXT[], FALSE),
        ('运行日统计', 'decision_run_daily_stats', ARRAY['project_id', 'version_id', 'run_day']::TEXT[], FALSE),
        ('分析阈值', 'decision_analytics_thresholds', ARRAY['failure_warning_rate', 'failure_high_rate', 'failure_min_runs', 'mismatch_warning_rate', 'mismatch_high_rate', 'mismatch_min_samples']::TEXT[], FALSE),
        ('偏差归因审阅', 'deviation_analyses', ARRAY['review_label', 'reviewed_by', 'reviewed_at']::TEXT[], FALSE),
        ('外部回传幂等回执', 'decision_run_outcome_receipts', ARRAY['service_id', 'idempotency_key', 'run_id', 'payload_digest']::TEXT[], FALSE),
        ('决策运行解释缓存', 'decision_run_explanations', ARRAY['decision_run_id', 'agent_run_id', 'generation_revision', 'status', 'explanation_json', 'prompt_digest', 'trace_digest']::TEXT[], FALSE),
        ('解释可读性审阅', 'decision_run_explanation_reviews', ARRAY['explanation_id', 'explanation_revision', 'reviewer_username', 'review_label', 'created_at']::TEXT[], FALSE),
        ('模拟候选快照', 'optimization_simulations', ARRAY['candidate_artifact_json', 'passed']::TEXT[], FALSE),
        ('持久化编排', 'agent_orchestrations', ARRAY['heartbeat_at', 'retry_of_orchestration_id']::TEXT[], FALSE),
        ('编排步骤尝试与产物', 'agent_steps', ARRAY['attempt_no', 'artifact_schema_version', 'artifact_digest']::TEXT[], FALSE),
        ('提示词内容审计', 'agent_runs', ARRAY['prompt_version', 'prompt_digest']::TEXT[], FALSE),
        ('步骤提示词内容审计', 'agent_steps', ARRAY['prompt_version', 'prompt_digest']::TEXT[], FALSE),
        ('提示词版本注册表', 'prompt_assets', ARRAY['prompt_key', 'version', 'content_digest', 'status', 'evaluation_json']::TEXT[], FALSE),
        ('提示词发布审计', 'prompt_asset_events', ARRAY['prompt_asset_id', 'event_type', 'actor_username', 'details_json']::TEXT[], FALSE),
        ('提示词在线评测作业', 'prompt_evaluation_jobs', ARRAY['prompt_asset_id', 'status', 'completed_cases', 'total_cases', 'result_json', 'cancel_requested', 'heartbeat_at']::TEXT[], FALSE),
        ('编排 handoff 摘要', 'agent_handoffs', ARRAY['context_digest', 'context_summary_json']::TEXT[], FALSE),
        ('编排 SSE 事件回放', 'agent_orchestration_events', ARRAY['orchestration_id', 'event_name', 'payload_json', 'expires_at']::TEXT[], FALSE),
        ('漂移事件审阅', 'decision_drift_events', ARRAY['action_result_json', 'review_label', 'reviewed_by', 'reviewed_at']::TEXT[], FALSE),
        ('漂移扫描窗口', 'decision_drift_scan_windows', ARRAY['scan_day', 'review_label']::TEXT[], FALSE),
        ('Webhook Outbox', 'decision_drift_webhook_deliveries', ARRAY['event_id', 'status', 'attempt_count']::TEXT[], FALSE),
        ('Shadow 评估历史', 'optimization_autodraft_shadow_evaluations', ARRAY['simulation_id', 'outcome', 'created_at']::TEXT[], FALSE)
), observed AS (
    SELECT
        feature,
        relation_name,
        optional,
        to_regclass(format('public.%I', relation_name)) AS relation_oid,
        ARRAY(
            SELECT required_column
            FROM unnest(required_columns) AS required_column
            WHERE NOT EXISTS (
                SELECT 1
                FROM pg_attribute
                WHERE attrelid = to_regclass(format('public.%I', relation_name))
                  AND attname = required_column
                  AND NOT attisdropped
            )
        ) AS missing_columns
    FROM expected_objects
)
SELECT
    feature,
    relation_name AS expected_object,
    CASE
        WHEN relation_oid IS NULL OR cardinality(missing_columns) > 0 THEN
            CASE WHEN optional THEN 'optional_missing' ELSE 'missing' END
        WHEN optional THEN 'optional_present'
        ELSE 'present'
    END AS status,
    CASE
        WHEN relation_oid IS NULL THEN 'relation absent'
        WHEN cardinality(missing_columns) > 0 THEN 'missing columns: ' || array_to_string(missing_columns, ', ')
        ELSE 'required relation and columns exist'
    END AS details
FROM observed
UNION ALL
SELECT
    '分析阈值表权限',
    'decision_analytics_thresholds',
    CASE
        WHEN thresholds.oid IS NULL THEN 'missing'
        WHEN has_table_privilege(current_user, thresholds.oid, 'SELECT')
         AND has_table_privilege(current_user, thresholds.oid, 'INSERT')
         AND has_table_privilege(current_user, thresholds.oid, 'UPDATE') THEN 'present'
        ELSE 'missing'
    END,
    'checked for the connected role: ' || current_user
FROM (SELECT to_regclass('public.decision_analytics_thresholds') AS oid) AS thresholds
UNION ALL
SELECT
    '向量检索',
    'extension:vector',
    CASE WHEN EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'vector') THEN 'optional_present' ELSE 'optional_missing' END,
    'pgvector is optional; keyword retrieval remains available without it'
UNION ALL
SELECT
    'Shadow 表权限',
    'optimization_autodraft_shadow_evaluations',
    CASE
        WHEN shadow.oid IS NULL THEN 'missing'
        WHEN has_table_privilege(current_user, shadow.oid, 'SELECT')
         AND has_table_privilege(current_user, shadow.oid, 'INSERT')
         AND has_table_privilege(current_user, shadow.oid, 'UPDATE')
         AND has_table_privilege(current_user, shadow.oid, 'DELETE') THEN 'present'
        ELSE 'missing'
    END,
    'checked for the connected role: ' || current_user
FROM (SELECT to_regclass('public.optimization_autodraft_shadow_evaluations') AS oid) AS shadow
UNION ALL
SELECT
    'Shadow 序列权限',
    'optimization_autodraft_shadow_evaluations_id_seq',
    CASE
        WHEN sequence.oid IS NULL THEN 'missing'
        WHEN has_sequence_privilege(current_user, sequence.oid, 'USAGE')
         AND has_sequence_privilege(current_user, sequence.oid, 'SELECT')
         AND NOT has_sequence_privilege(current_user, sequence.oid, 'UPDATE') THEN 'present'
        ELSE 'missing'
    END,
    'checked for the connected role: ' || current_user
FROM (SELECT to_regclass('public.optimization_autodraft_shadow_evaluations_id_seq') AS oid) AS sequence
UNION ALL
SELECT
    '回传回执表权限',
    'decision_run_outcome_receipts',
    CASE
        WHEN relation.oid IS NULL THEN 'missing'
        WHEN has_table_privilege(current_user, relation.oid, 'SELECT')
         AND has_table_privilege(current_user, relation.oid, 'INSERT')
         AND has_table_privilege(current_user, relation.oid, 'UPDATE') THEN 'present'
        ELSE 'missing'
    END,
    'checked for the connected role: ' || current_user
FROM (SELECT to_regclass('public.decision_run_outcome_receipts') AS oid) AS relation
UNION ALL
SELECT
    '解释审阅记录表权限',
    'decision_run_explanation_reviews',
    CASE
        WHEN relation.oid IS NULL THEN 'missing'
        WHEN has_table_privilege(current_user, relation.oid, 'SELECT')
         AND has_table_privilege(current_user, relation.oid, 'INSERT')
         AND NOT has_table_privilege(current_user, relation.oid, 'UPDATE')
         AND NOT has_table_privilege(current_user, relation.oid, 'DELETE') THEN 'present'
        ELSE 'missing'
    END,
    'append-only access checked for the connected role: ' || current_user
FROM (SELECT to_regclass('public.decision_run_explanation_reviews') AS oid) AS relation
UNION ALL
SELECT
    '解释审阅版本触发器',
    'decision_run_explanation_review_revision_guard',
    CASE WHEN EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgrelid = to_regclass('public.decision_run_explanation_reviews')
          AND tgname = 'decision_run_explanation_review_revision_guard'
          AND NOT tgisinternal AND tgenabled IN ('O', 'A')
    ) THEN 'present' ELSE 'missing' END,
    'new labels must bind the currently succeeded generation revision'
UNION ALL
SELECT
    '解释审阅版本唯一约束',
    'decision_run_explanation_reviews_revision_reviewer_idx',
    CASE WHEN EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('public.decision_run_explanation_reviews_revision_reviewer_idx')
          AND indisunique AND indisvalid
    ) THEN 'present' ELSE 'missing' END,
    'one vote per reviewer and explanation revision'
UNION ALL
SELECT
    'Agent 步骤表权限',
    'agent_steps',
    CASE
        WHEN relation.oid IS NULL THEN 'missing'
        WHEN has_table_privilege(current_user, relation.oid, 'SELECT')
         AND has_table_privilege(current_user, relation.oid, 'INSERT')
         AND has_table_privilege(current_user, relation.oid, 'UPDATE')
         AND has_table_privilege(current_user, relation.oid, 'DELETE') THEN 'present'
        ELSE 'missing'
    END,
    'checked for the connected role: ' || current_user
FROM (SELECT to_regclass('public.agent_steps') AS oid) AS relation
UNION ALL
SELECT
    '编排事件回放表权限',
    'agent_orchestration_events',
    CASE
        WHEN relation.oid IS NULL THEN 'missing'
        WHEN has_table_privilege(current_user, relation.oid, 'SELECT')
         AND has_table_privilege(current_user, relation.oid, 'INSERT')
         AND has_table_privilege(current_user, relation.oid, 'DELETE')
         AND NOT has_table_privilege(current_user, relation.oid, 'UPDATE') THEN 'present'
        ELSE 'missing'
    END,
    'append-only event payload access for connected role: ' || current_user
FROM (SELECT to_regclass('public.agent_orchestration_events') AS oid) AS relation
UNION ALL
SELECT
    'Tester 预算降级步骤状态',
    'agent_steps_status_check',
    CASE WHEN EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = to_regclass('public.agent_steps')
          AND conname = 'agent_steps_status_check'
          AND pg_get_constraintdef(oid) LIKE '%skipped%'
    ) THEN 'present' ELSE 'missing' END,
    'agent_steps can persist an optional Tester skipped by budget gating'
UNION ALL
SELECT
    '提示词版本不可变触发器',
    'prompt_assets_immutable',
    CASE WHEN EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'prompt_assets_immutable' AND NOT tgisinternal
    ) THEN 'present' ELSE 'missing' END,
    'prompt content identity fields cannot be changed or deleted'
UNION ALL
SELECT
    '提示词审计追加触发器',
    'prompt_asset_events_append_only',
    CASE WHEN EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'prompt_asset_events_append_only' AND NOT tgisinternal
    ) THEN 'present' ELSE 'missing' END,
    'prompt lifecycle events cannot be changed or deleted'
UNION ALL
SELECT
    '提示词资产权限',
    'prompt_assets',
    CASE
        WHEN relation.oid IS NOT NULL
         AND has_table_privilege(current_user, relation.oid, 'SELECT')
         AND has_table_privilege(current_user, relation.oid, 'INSERT')
         AND has_table_privilege(current_user, relation.oid, 'UPDATE')
         AND has_sequence_privilege(current_user, to_regclass('public.prompt_assets_id_seq'), 'USAGE') THEN 'present'
        ELSE 'missing'
    END,
    'checked for the connected role: ' || current_user
FROM (SELECT to_regclass('public.prompt_assets') AS oid) AS relation
UNION ALL
SELECT
    '提示词事件表权限',
    'prompt_asset_events',
    CASE
        WHEN relation.oid IS NOT NULL
         AND has_table_privilege(current_user, relation.oid, 'SELECT')
         AND has_table_privilege(current_user, relation.oid, 'INSERT') THEN 'present'
        ELSE 'missing'
    END,
    'checked for the connected role: ' || current_user
FROM (SELECT to_regclass('public.prompt_asset_events') AS oid) AS relation
UNION ALL
SELECT
    '提示词评测作业表权限',
    'prompt_evaluation_jobs',
    CASE
        WHEN relation.oid IS NOT NULL
         AND has_table_privilege(current_user, relation.oid, 'SELECT')
         AND has_table_privilege(current_user, relation.oid, 'INSERT')
         AND has_table_privilege(current_user, relation.oid, 'UPDATE')
         AND has_sequence_privilege(current_user, to_regclass('public.prompt_evaluation_jobs_id_seq'), 'USAGE') THEN 'present'
        ELSE 'missing'
    END,
    'checked for the connected role: ' || current_user
FROM (SELECT to_regclass('public.prompt_evaluation_jobs') AS oid) AS relation
UNION ALL
SELECT
    '归因清单游标索引',
    'deviation_analyses_project_id_idx',
    CASE WHEN EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('public.deviation_analyses_project_id_idx')
          AND indrelid = to_regclass('public.deviation_analyses') AND indisvalid
    ) THEN 'present' ELSE 'missing' END,
    'project-scoped ID pagination and maximum ID lookup'
UNION ALL
SELECT
    '单活跃重试约束',
    'agent_orchestrations_one_active_retry_idx',
    CASE WHEN EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'public'
          AND indexname = 'agent_orchestrations_one_active_retry_idx'
    ) THEN 'present' ELSE 'missing' END,
    'one pending/running retry per root orchestration'
UNION ALL
SELECT
    '漂移表权限',
    'decision_drift_*',
    CASE
        WHEN to_regclass('public.decision_drift_automation_rules') IS NOT NULL
         AND to_regclass('public.decision_drift_events') IS NOT NULL
         AND to_regclass('public.decision_drift_scan_windows') IS NOT NULL
         AND to_regclass('public.decision_drift_webhook_deliveries') IS NOT NULL
         AND has_table_privilege(current_user, to_regclass('public.decision_drift_automation_rules'), 'SELECT')
         AND has_table_privilege(current_user, to_regclass('public.decision_drift_automation_rules'), 'UPDATE')
         AND has_table_privilege(current_user, to_regclass('public.decision_drift_events'), 'SELECT')
         AND has_table_privilege(current_user, to_regclass('public.decision_drift_events'), 'UPDATE')
         AND has_table_privilege(current_user, to_regclass('public.decision_drift_scan_windows'), 'INSERT')
         AND has_table_privilege(current_user, to_regclass('public.decision_drift_webhook_deliveries'), 'INSERT') THEN 'present'
        ELSE 'missing'
    END,
    'checked for the connected role: ' || current_user
UNION ALL
SELECT
    '应用数据库角色最小权限',
    'role:' || current_user,
    CASE WHEN rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
              AND NOT rolreplication AND NOT rolbypassrls
              AND NOT EXISTS (SELECT 1 FROM pg_auth_members WHERE member = runtime_role.oid)
         THEN 'present' ELSE 'missing' END,
    'runtime role must be LOGIN, non-privileged and have no role memberships'
FROM pg_roles runtime_role WHERE rolname = current_user
UNION ALL
SELECT
    '应用数据库对象所有权隔离',
    'public/runtime-ownership',
    CASE WHEN NOT EXISTS (
        SELECT 1 FROM pg_class relation JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = 'public' AND relation.relowner = (SELECT oid FROM pg_roles WHERE rolname = current_user)
    ) AND NOT EXISTS (
        SELECT 1 FROM pg_proc routine JOIN pg_namespace namespace ON namespace.oid = routine.pronamespace
        WHERE namespace.nspname = 'public' AND routine.proowner = (SELECT oid FROM pg_roles WHERE rolname = current_user)
    ) AND NOT EXISTS (
        SELECT 1 FROM pg_database WHERE datname = current_database() AND datdba = (SELECT oid FROM pg_roles WHERE rolname = current_user)
    ) AND NOT has_schema_privilege(current_user, 'public', 'CREATE')
      AND NOT has_database_privilege(current_user, current_database(), 'CREATE')
      AND NOT has_database_privilege(current_user, current_database(), 'TEMP')
    THEN 'present' ELSE 'missing' END,
    'runtime cannot own/alter application objects or create schema/temp objects'
UNION ALL
SELECT
    '审计证据追加权限',
    'audit_events/knowledge_audit_events',
    CASE WHEN bool_and(
        relation.oid IS NOT NULL
        AND has_table_privilege(current_user, relation.oid, 'SELECT')
        AND has_table_privilege(current_user, relation.oid, 'INSERT')
        AND NOT has_table_privilege(current_user, relation.oid, 'UPDATE')
        AND NOT has_table_privilege(current_user, relation.oid, 'DELETE')
        AND NOT has_table_privilege(current_user, relation.oid, 'TRUNCATE')
        AND NOT has_table_privilege(current_user, relation.oid, 'TRIGGER')
        AND EXISTS (SELECT 1 FROM pg_trigger audit_trigger
                    WHERE audit_trigger.tgrelid = relation.oid AND NOT audit_trigger.tgisinternal
                      AND audit_trigger.tgname IN ('audit_events_append_only', 'knowledge_audit_events_append_only')
                      AND audit_trigger.tgenabled IN ('O', 'A'))
        AND NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid = relation.oid AND contype = 'f')
    ) THEN 'present' ELSE 'missing' END,
    'audit references survive lifecycle deletion; direct changes/truncation are forbidden'
FROM (VALUES (to_regclass('public.audit_events')), (to_regclass('public.knowledge_audit_events'))) relation(oid)
UNION ALL
SELECT
    '受控数据库维护入口',
    'partition/statistics-functions',
    CASE WHEN count(routine.oid) = 2 AND bool_and(
        routine.prosecdef
        AND routine.proowner <> (SELECT oid FROM pg_roles WHERE rolname = current_user)
        AND NOT pg_has_role(current_user, routine.proowner, 'MEMBER')
        AND routine.proconfig @> ARRAY['search_path=pg_catalog, public, pg_temp']::TEXT[]
        AND has_function_privilege(current_user, routine.oid, 'EXECUTE')
        AND NOT EXISTS (SELECT 1 FROM aclexplode(COALESCE(routine.proacl, acldefault('f', routine.proowner))) permission
                        WHERE permission.grantee = 0 AND permission.privilege_type = 'EXECUTE')
    ) AND NOT has_function_privilege(current_user, to_regprocedure('public.ensure_decision_run_month_partition(date)'), 'EXECUTE')
      AND NOT has_function_privilege(current_user, to_regprocedure('public.mirror_decision_run_partitioned()'), 'EXECUTE')
    THEN 'present' ELSE 'missing' END,
    'maintenance has a trusted owner, fixed search_path and only restricted runtime entrypoints'
FROM (VALUES (to_regprocedure('public.maintain_decision_run_partitions(integer)')),
             (to_regprocedure('public.refresh_decision_run_daily_stats()'))) expected(oid)
LEFT JOIN pg_proc routine ON routine.oid = expected.oid
ORDER BY feature;

ROLLBACK;
