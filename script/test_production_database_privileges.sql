-- Connect directly as the restricted application role after migration 099.
-- Fixtures and partition maintenance are rolled back together.
\set ON_ERROR_STOP on
BEGIN;
DO $$
DECLARE
    fixture_user TEXT := 'privilege-regression-' || txid_current();
    project_key BIGINT;
    version_key BIGINT;
    run_key BIGINT;
    source_key BIGINT;
    audit_key BIGINT;
    knowledge_audit_key BIGINT;
    forbidden_sql TEXT;
    rejected BOOLEAN;
    month_offset INTEGER;
    future_time TIMESTAMPTZ;
BEGIN
    IF (SELECT rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls
        FROM pg_roles WHERE rolname = current_user)
       OR has_schema_privilege(current_user, 'public', 'CREATE') THEN
        RAISE EXCEPTION 'test must connect as a restricted non-owner application role';
    END IF;
    INSERT INTO public.users(username, password_hash) VALUES (fixture_user, 'fixture') RETURNING id INTO source_key;
    INSERT INTO public.decision_projects(owner_username, name) VALUES (fixture_user, 'privilege regression') RETURNING id INTO project_key;
    INSERT INTO public.decision_versions(project_id, version_number, created_by) VALUES (project_key, 1, fixture_user) RETURNING id INTO version_key;
    INSERT INTO public.audit_events(project_id, version_id, event_type, actor_username, summary)
        VALUES (project_key, version_key, 'created', fixture_user, 'retain original references') RETURNING id INTO audit_key;
    INSERT INTO public.knowledge_sources(owner_username, source_type, name, content)
        VALUES (fixture_user, 'term', 'privilege regression', 'fixture') RETURNING id INTO source_key;
    INSERT INTO public.knowledge_audit_events(knowledge_source_id, owner_username, event_type, summary)
        VALUES (source_key, fixture_user, 'created', 'retain original references') RETURNING id INTO knowledge_audit_key;
    INSERT INTO public.decision_runs(project_id, version_id, service_id, service_name, input_json, output_json,
        execution_trace_json, duration_ms, status, executed_by)
        VALUES (project_key, version_key, 'fixture', 'fixture', '{}', '{}', '{}', 1, 'succeeded', fixture_user)
        RETURNING id INTO run_key;
    UPDATE public.decision_runs SET duration_ms = 2 WHERE id = run_key;
    IF NOT EXISTS (SELECT 1 FROM public.decision_runs_partitioned WHERE id = run_key AND duration_ms = 2) THEN
        RAISE EXCEPTION 'restricted runtime CRUD did not mirror into partitions';
    END IF;
    PERFORM public.maintain_decision_run_partitions(13);
    PERFORM public.refresh_decision_run_daily_stats();
    IF NOT EXISTS (SELECT 1 FROM public.decision_run_daily_stats WHERE project_id = project_key) THEN
        RAISE EXCEPTION 'restricted runtime could not refresh aggregate';
    END IF;

    -- Force late/default rows through the owner-only temporary staging path.
    SELECT offset_no INTO month_offset FROM generate_series(14, 35) offset_no
    WHERE to_regclass(format('public.decision_runs_partitioned_%s',
        to_char(date_trunc('month', CURRENT_TIMESTAMP AT TIME ZONE 'UTC') + make_interval(months => offset_no), 'YYYY_MM'))) IS NULL
    ORDER BY offset_no LIMIT 1;
    IF month_offset IS NULL THEN
        RAISE EXCEPTION 'fresh regression database required: no uncreated month in maintenance window';
    END IF;
    future_time := (date_trunc('month', CURRENT_TIMESTAMP AT TIME ZONE 'UTC') + make_interval(months => month_offset)) AT TIME ZONE 'UTC';
    UPDATE public.decision_runs SET executed_at = future_time WHERE id = run_key;
    IF NOT EXISTS (SELECT 1 FROM public.decision_runs_partitioned WHERE id = run_key
                   AND tableoid = 'public.decision_runs_partitioned_default'::regclass) THEN
        RAISE EXCEPTION 'future fixture did not land in default partition';
    END IF;
    PERFORM public.maintain_decision_run_partitions(month_offset);
    IF NOT EXISTS (SELECT 1 FROM public.decision_runs_partitioned WHERE id = run_key
                   AND tableoid <> 'public.decision_runs_partitioned_default'::regclass) THEN
        RAISE EXCEPTION 'controlled owner maintenance failed to relocate default rows';
    END IF;

    FOREACH forbidden_sql IN ARRAY ARRAY[
        'CREATE TABLE public.forbidden_runtime_table(id integer)',
        'CREATE SCHEMA forbidden_runtime_schema',
        'CREATE TEMP TABLE forbidden_runtime_temp(id integer)',
        'CREATE ROLE forbidden_runtime_role',
        'ALTER TABLE public.audit_events DISABLE TRIGGER ALL',
        'ALTER TABLE public.users ADD COLUMN forbidden_runtime_column integer',
        'UPDATE public.audit_events SET summary = summary',
        'DELETE FROM public.audit_events',
        'TRUNCATE public.audit_events CASCADE',
        'UPDATE public.knowledge_audit_events SET summary = summary',
        'DELETE FROM public.knowledge_audit_events',
        'TRUNCATE public.knowledge_audit_events CASCADE',
        'UPDATE public.agent_orchestration_events SET payload_json = payload_json',
        'SELECT setval(''public.audit_events_id_seq'', 1)',
        'SELECT public.ensure_decision_run_month_partition(CURRENT_DATE)',
        'SELECT public.mirror_decision_run_partitioned()',
        'INSERT INTO public.decision_runs_partitioned SELECT * FROM public.decision_runs WHERE false'
    ] LOOP
        rejected := FALSE;
        BEGIN
            EXECUTE forbidden_sql;
        EXCEPTION WHEN insufficient_privilege THEN
            rejected := TRUE;
        END;
        IF NOT rejected THEN RAISE EXCEPTION 'restricted runtime unexpectedly allowed: %', forbidden_sql; END IF;
    END LOOP;
    FOREACH forbidden_sql IN ARRAY ARRAY[
        'SELECT public.maintain_decision_run_partitions(NULL)',
        'SELECT public.maintain_decision_run_partitions(0)',
        'SELECT public.maintain_decision_run_partitions(37)'
    ] LOOP
        rejected := FALSE;
        BEGIN
            EXECUTE forbidden_sql;
        EXCEPTION WHEN raise_exception THEN
            rejected := TRUE;
        END;
        IF NOT rejected THEN RAISE EXCEPTION 'invalid maintenance argument accepted: %', forbidden_sql; END IF;
    END LOOP;

    DELETE FROM public.decision_runs WHERE id = run_key;
    IF EXISTS (SELECT 1 FROM public.decision_runs_partitioned WHERE id = run_key) THEN
        RAISE EXCEPTION 'runtime run deletion failed to remove mirrored row';
    END IF;
    DELETE FROM public.decision_projects WHERE id = project_key;
    DELETE FROM public.knowledge_sources WHERE id = source_key;
    DELETE FROM public.users WHERE username = fixture_user;
    IF NOT EXISTS (SELECT 1 FROM public.audit_events WHERE id = audit_key AND project_id = project_key
                   AND version_id = version_key AND actor_username = fixture_user)
       OR NOT EXISTS (SELECT 1 FROM public.knowledge_audit_events WHERE id = knowledge_audit_key
                      AND knowledge_source_id = source_key AND owner_username = fixture_user) THEN
        RAISE EXCEPTION 'lifecycle deletion removed or changed immutable audit references';
    END IF;
    RAISE NOTICE 'restricted CRUD, mirror, maintenance, append-only event/audit logs and lifecycle retention passed';
END;
$$;
ROLLBACK;
