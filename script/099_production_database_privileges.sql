\set ON_ERROR_STOP on
-- Run as the schema owner or an independent migration administrator.
-- This migration intentionally refuses privileged/owning application roles.
\if :{?app_user}
\else
\echo 'error: pass -v app_user=<restricted application role>'
DO $$ BEGIN RAISE EXCEPTION 'migration precondition failed; see the preceding error'; END; $$;
\endif

\if :{?privilege_transaction_open}
\else
BEGIN;
\endif
SELECT set_config('fuxi.migration_app_user', :'app_user', TRUE);
DO $$
DECLARE
    app_oid OID;
BEGIN
    SELECT oid INTO app_oid FROM pg_roles
    WHERE rolname = current_setting('fuxi.migration_app_user')
      AND rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
      AND NOT rolreplication AND NOT rolbypassrls;
    IF app_oid IS NULL THEN
        RAISE EXCEPTION 'application role is absent or privileged; use an independent administrator and review the legacy role-separation plan first';
    END IF;
    IF app_oid = (SELECT datdba FROM pg_database WHERE datname = current_database())
       OR EXISTS (SELECT 1 FROM pg_auth_members WHERE member = app_oid)
       OR EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                  WHERE n.nspname = 'public' AND c.relowner = app_oid)
       OR EXISTS (SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
                  WHERE n.nspname = 'public' AND p.proowner = app_oid)
       OR EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'public' AND nspowner = app_oid) THEN
        RAISE EXCEPTION 'application role owns database objects or has role memberships; migration does not automatically transfer ownership or downgrade roles';
    END IF;
END;
$$;

REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM :"app_user";
GRANT USAGE ON SCHEMA public TO :"app_user";
SELECT format('REVOKE ALL ON DATABASE %I FROM PUBLIC', current_database()) \gexec
SELECT format('REVOKE ALL ON DATABASE %I FROM %I', current_database(), :'app_user') \gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO %I', current_database(), :'app_user') \gexec

-- No implicit write access on future tables; each migration must declare grants.
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    REVOKE ALL ON TABLES FROM :"app_user";
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    REVOKE ALL ON SEQUENCES FROM :"app_user";

-- Remove legacy broad grants made by any earlier object owner, including global
-- default ACLs. Such owners must be controlled by this migration administrator.
DO $$
DECLARE
    acl RECORD;
    schema_clause TEXT;
    object_kind TEXT;
BEGIN
    FOR acl IN
        SELECT DISTINCT d.defaclrole, d.defaclnamespace, d.defaclobjtype
        FROM pg_default_acl d, LATERAL aclexplode(d.defaclacl) privilege
        WHERE privilege.grantee = (SELECT oid FROM pg_roles WHERE rolname = current_setting('fuxi.migration_app_user'))
          AND (d.defaclnamespace = 0 OR d.defaclnamespace = 'public'::regnamespace)
          AND d.defaclobjtype IN ('r', 'S')
    LOOP
        schema_clause := CASE WHEN acl.defaclnamespace = 0 THEN '' ELSE ' IN SCHEMA public' END;
        object_kind := CASE WHEN acl.defaclobjtype = 'r' THEN 'TABLES' ELSE 'SEQUENCES' END;
        EXECUTE format('ALTER DEFAULT PRIVILEGES FOR ROLE %I%s REVOKE ALL ON %s FROM %I',
            pg_get_userbyid(acl.defaclrole), schema_clause, object_kind,
            current_setting('fuxi.migration_app_user'));
    END LOOP;
END;
$$;

-- Preserve append-only audit references after legitimate lifecycle deletion.
-- Leaving CASCADE/SET NULL foreign keys would erase or mutate audit evidence.
DO $$
DECLARE
    relation_name TEXT;
    constraint_name TEXT;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY['audit_events', 'knowledge_audit_events'] LOOP
        FOR constraint_name IN
            SELECT conname FROM pg_constraint
            WHERE conrelid = to_regclass(format('public.%I', relation_name)) AND contype = 'f'
        LOOP
            EXECUTE format('ALTER TABLE public.%I DROP CONSTRAINT %I', relation_name, constraint_name);
        END LOOP;
    END LOOP;
END;
$$;

-- Reassigned legacy owners need explicit runtime grants after losing ownership.
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
    public.users, public.decision_projects, public.decision_analytics_thresholds,
    public.project_members, public.decision_versions, public.decision_artifacts,
    public.decision_test_cases, public.decision_runs, public.decision_optimization_proposals,
    public.optimization_simulations, public.optimization_autodraft_shadow_evaluations,
    public.decision_templates, public.decision_patterns, public.agent_orchestrations,
    public.agent_steps, public.agent_handoffs, public.agent_orchestration_events,
    public.pattern_recommendations,
    public.knowledge_sources, public.knowledge_chunks, public.repair_memories,
    public.project_knowledge_sources, public.agent_runs, public.model_performance_metrics,
    public.agent_model_votes, public.business_logic_patterns, public.version_behavior_diff,
    public.regression_test_sources, public.decision_workflows, public.model_change_candidates,
    public.decision_service_api_keys, public.decision_run_outcome_receipts,
    public.decision_run_explanations, public.deviation_analyses,
    public.decision_drift_automation_rules, public.decision_drift_events,
    public.decision_drift_scan_windows, public.decision_drift_webhook_deliveries,
    public.prompt_assets, public.prompt_evaluation_jobs TO :"app_user";
REVOKE UPDATE, TRUNCATE, TRIGGER ON TABLE public.agent_orchestration_events FROM :"app_user";

CREATE OR REPLACE FUNCTION public.reject_audit_evidence_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    RAISE EXCEPTION 'audit evidence is append-only';
END;
$$;
REVOKE ALL ON FUNCTION public.reject_audit_evidence_mutation() FROM PUBLIC, :"app_user";

DROP TRIGGER IF EXISTS audit_events_append_only ON public.audit_events;
CREATE TRIGGER audit_events_append_only
    BEFORE UPDATE OR DELETE OR TRUNCATE ON public.audit_events
    FOR EACH STATEMENT EXECUTE FUNCTION public.reject_audit_evidence_mutation();
DROP TRIGGER IF EXISTS knowledge_audit_events_append_only ON public.knowledge_audit_events;
CREATE TRIGGER knowledge_audit_events_append_only
    BEFORE UPDATE OR DELETE OR TRUNCATE ON public.knowledge_audit_events
    FOR EACH STATEMENT EXECUTE FUNCTION public.reject_audit_evidence_mutation();

REVOKE ALL ON TABLE public.audit_events, public.knowledge_audit_events,
    public.prompt_asset_events, public.decision_run_explanation_reviews FROM PUBLIC, :"app_user";
GRANT SELECT, INSERT ON TABLE public.audit_events, public.knowledge_audit_events,
    public.prompt_asset_events, public.decision_run_explanation_reviews TO :"app_user";
REVOKE UPDATE ON ALL SEQUENCES IN SCHEMA public FROM :"app_user";
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO :"app_user";
REVOKE TRUNCATE, TRIGGER, REFERENCES ON ALL TABLES IN SCHEMA public FROM PUBLIC, :"app_user";

-- Partition rows are written only through the controlled mirror trigger.
REVOKE ALL ON TABLE public.decision_runs_partitioned FROM PUBLIC, :"app_user";
GRANT SELECT ON TABLE public.decision_runs_partitioned TO :"app_user";
DO $$
DECLARE
    partition_oid REGCLASS;
BEGIN
    FOR partition_oid IN
        SELECT inhrelid::regclass FROM pg_inherits WHERE inhparent = 'public.decision_runs_partitioned'::regclass
    LOOP
        EXECUTE format('REVOKE ALL ON TABLE %s FROM PUBLIC, %I', partition_oid, current_setting('fuxi.migration_app_user'));
    END LOOP;
END;
$$;

CREATE OR REPLACE FUNCTION public.mirror_decision_run_partitioned()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        DELETE FROM public.decision_runs_partitioned WHERE id = OLD.id AND executed_at = OLD.executed_at;
        RETURN OLD;
    END IF;
    IF TG_OP = 'UPDATE' AND
       (OLD.id IS DISTINCT FROM NEW.id OR OLD.executed_at IS DISTINCT FROM NEW.executed_at) THEN
        DELETE FROM public.decision_runs_partitioned WHERE id = OLD.id AND executed_at = OLD.executed_at;
    END IF;
    INSERT INTO public.decision_runs_partitioned (
        id, project_id, version_id, service_id, service_name, input_json, output_json,
        execution_trace_json, duration_ms, status, executed_by, executed_at,
        business_reference, actual_outcome_json, outcome_assessment, outcome_note,
        outcome_occurred_at, outcome_reported_by, outcome_reported_at
    ) VALUES (
        NEW.id, NEW.project_id, NEW.version_id, NEW.service_id, NEW.service_name,
        NEW.input_json, NEW.output_json, NEW.execution_trace_json, NEW.duration_ms,
        NEW.status, NEW.executed_by, NEW.executed_at, NEW.business_reference,
        NEW.actual_outcome_json, NEW.outcome_assessment, NEW.outcome_note,
        NEW.outcome_occurred_at, NEW.outcome_reported_by, NEW.outcome_reported_at
    ) ON CONFLICT (id, executed_at) DO UPDATE SET
        project_id = EXCLUDED.project_id, version_id = EXCLUDED.version_id,
        service_id = EXCLUDED.service_id, service_name = EXCLUDED.service_name,
        input_json = EXCLUDED.input_json, output_json = EXCLUDED.output_json,
        execution_trace_json = EXCLUDED.execution_trace_json, duration_ms = EXCLUDED.duration_ms,
        status = EXCLUDED.status, executed_by = EXCLUDED.executed_by,
        business_reference = EXCLUDED.business_reference, actual_outcome_json = EXCLUDED.actual_outcome_json,
        outcome_assessment = EXCLUDED.outcome_assessment, outcome_note = EXCLUDED.outcome_note,
        outcome_occurred_at = EXCLUDED.outcome_occurred_at,
        outcome_reported_by = EXCLUDED.outcome_reported_by, outcome_reported_at = EXCLUDED.outcome_reported_at;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION public.ensure_decision_run_month_partition(p_month DATE)
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
DECLARE
    month_start DATE;
    month_start_utc TIMESTAMPTZ;
    next_month_utc TIMESTAMPTZ;
    partition_name TEXT;
    has_default_rows BOOLEAN;
BEGIN
    IF p_month IS NULL OR NOT isfinite(p_month)
       OR p_month < DATE '1900-01-01'
       OR p_month > ((CURRENT_TIMESTAMP AT TIME ZONE 'UTC') + INTERVAL '36 months')::DATE THEN
        RAISE EXCEPTION 'partition month is outside the supported maintenance window';
    END IF;
    month_start := date_trunc('month', p_month::TIMESTAMP)::DATE;
    month_start_utc := month_start::TIMESTAMP AT TIME ZONE 'UTC';
    next_month_utc := (month_start + INTERVAL '1 month')::TIMESTAMP AT TIME ZONE 'UTC';
    partition_name := format('decision_runs_partitioned_%s', to_char(month_start, 'YYYY_MM'));
    IF to_regclass(format('public.%I', partition_name)) IS NOT NULL THEN RETURN; END IF;
    PERFORM pg_advisory_xact_lock(hashtext(partition_name)::BIGINT);
    IF to_regclass(format('public.%I', partition_name)) IS NOT NULL THEN RETURN; END IF;
    LOCK TABLE public.decision_runs_partitioned_default IN ACCESS EXCLUSIVE MODE;
    IF to_regclass(format('public.%I', partition_name)) IS NOT NULL THEN RETURN; END IF;
    SELECT EXISTS (SELECT 1 FROM public.decision_runs_partitioned_default
                   WHERE executed_at >= month_start_utc AND executed_at < next_month_utc)
        INTO has_default_rows;
    IF has_default_rows THEN
        DROP TABLE IF EXISTS pg_temp.decision_run_partition_stage;
        CREATE TEMP TABLE decision_run_partition_stage ON COMMIT DROP AS
            SELECT * FROM public.decision_runs_partitioned_default
            WHERE executed_at >= month_start_utc AND executed_at < next_month_utc;
        DELETE FROM public.decision_runs_partitioned_default
            WHERE executed_at >= month_start_utc AND executed_at < next_month_utc;
    END IF;
    EXECUTE format('CREATE TABLE public.%I PARTITION OF public.decision_runs_partitioned FOR VALUES FROM (%L) TO (%L)',
        partition_name, month_start_utc, next_month_utc);
    IF has_default_rows THEN
        INSERT INTO public.decision_runs_partitioned SELECT * FROM pg_temp.decision_run_partition_stage;
        DROP TABLE pg_temp.decision_run_partition_stage;
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION public.maintain_decision_run_partitions(months_ahead INTEGER DEFAULT 13)
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
DECLARE
    month_offset INTEGER;
    current_month DATE := date_trunc('month', CURRENT_TIMESTAMP AT TIME ZONE 'UTC')::DATE;
BEGIN
    IF months_ahead IS NULL OR months_ahead < 1 OR months_ahead > 36 THEN
        RAISE EXCEPTION 'months_ahead must be between 1 and 36';
    END IF;
    FOR month_offset IN 0..months_ahead LOOP
        PERFORM public.ensure_decision_run_month_partition(
            (current_month + (month_offset || ' months')::INTERVAL)::DATE);
    END LOOP;
END;
$$;

CREATE OR REPLACE FUNCTION public.refresh_decision_run_daily_stats()
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    REFRESH MATERIALIZED VIEW public.decision_run_daily_stats;
END;
$$;

REVOKE ALL ON FUNCTION public.mirror_decision_run_partitioned(),
    public.ensure_decision_run_month_partition(DATE),
    public.maintain_decision_run_partitions(INTEGER),
    public.refresh_decision_run_daily_stats() FROM PUBLIC, :"app_user";
GRANT EXECUTE ON FUNCTION public.maintain_decision_run_partitions(INTEGER),
    public.refresh_decision_run_daily_stats() TO :"app_user";
GRANT SELECT ON TABLE public.decision_run_daily_stats TO :"app_user";

COMMENT ON TABLE public.audit_events IS
    'Append-only audit evidence; original project/version references survive lifecycle deletion without foreign-key mutation.';
COMMENT ON TABLE public.knowledge_audit_events IS
    'Append-only audit evidence; original source/user references survive lifecycle deletion without foreign-key mutation.';
\if :{?privilege_transaction_open}
\else
COMMIT;
\endif
