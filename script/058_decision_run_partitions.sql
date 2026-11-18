BEGIN;

CREATE TABLE IF NOT EXISTS decision_runs_partitioned (
    id BIGINT NOT NULL,
    project_id BIGINT NOT NULL,
    version_id BIGINT NOT NULL,
    service_id VARCHAR(255) NOT NULL,
    service_name VARCHAR(255) NOT NULL,
    input_json JSONB NOT NULL,
    output_json JSONB NOT NULL,
    execution_trace_json JSONB NOT NULL,
    duration_ms BIGINT NOT NULL CHECK (duration_ms >= 0),
    status VARCHAR(16) NOT NULL CHECK (status IN ('succeeded', 'failed')),
    executed_by VARCHAR(255) NOT NULL,
    executed_at TIMESTAMPTZ NOT NULL,
    business_reference VARCHAR(255) NOT NULL DEFAULT '',
    actual_outcome_json JSONB,
    outcome_assessment VARCHAR(16)
        CHECK (outcome_assessment IS NULL OR outcome_assessment IN ('confirmed', 'mismatch', 'inconclusive')),
    outcome_note TEXT NOT NULL DEFAULT '',
    outcome_occurred_at TIMESTAMPTZ,
    outcome_reported_by VARCHAR(255),
    outcome_reported_at TIMESTAMPTZ,
    PRIMARY KEY (id, executed_at)
) PARTITION BY RANGE (executed_at);

CREATE INDEX IF NOT EXISTS decision_runs_partitioned_project_executed_idx
    ON decision_runs_partitioned (project_id, executed_at DESC);
CREATE INDEX IF NOT EXISTS decision_runs_partitioned_project_version_executed_idx
    ON decision_runs_partitioned (project_id, version_id, executed_at, id);
CREATE INDEX IF NOT EXISTS decision_runs_partitioned_project_outcome_idx
    ON decision_runs_partitioned (project_id, outcome_assessment, executed_at DESC);

DO $$
DECLARE
    first_month DATE;
    last_month DATE;
    month_start DATE;
    partition_name TEXT;
BEGIN
    SELECT date_trunc(
               'month',
               COALESCE(MIN(executed_at) AT TIME ZONE 'UTC', CURRENT_TIMESTAMP AT TIME ZONE 'UTC')
           )::DATE
      INTO first_month
      FROM decision_runs;
    last_month := date_trunc(
        'month', (CURRENT_TIMESTAMP AT TIME ZONE 'UTC') + INTERVAL '13 months'
    )::DATE;
    month_start := first_month;
    WHILE month_start <= last_month LOOP
        partition_name := format(
            'decision_runs_partitioned_%s', to_char(month_start, 'YYYY_MM')
        );
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF decision_runs_partitioned FOR VALUES FROM (%L) TO (%L)',
            partition_name,
            month_start::TIMESTAMP AT TIME ZONE 'UTC',
            (month_start + INTERVAL '1 month')::TIMESTAMP AT TIME ZONE 'UTC'
        );
        month_start := (month_start + INTERVAL '1 month')::DATE;
    END LOOP;
END;
$$;

CREATE TABLE IF NOT EXISTS decision_runs_partitioned_default
    PARTITION OF decision_runs_partitioned DEFAULT;

CREATE OR REPLACE FUNCTION mirror_decision_run_partitioned()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        DELETE FROM decision_runs_partitioned
        WHERE id = OLD.id AND executed_at = OLD.executed_at;
        RETURN OLD;
    END IF;

    IF TG_OP = 'UPDATE' AND
       (OLD.id IS DISTINCT FROM NEW.id OR OLD.executed_at IS DISTINCT FROM NEW.executed_at) THEN
        DELETE FROM decision_runs_partitioned
        WHERE id = OLD.id AND executed_at = OLD.executed_at;
    END IF;

    INSERT INTO decision_runs_partitioned (
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
    )
    ON CONFLICT (id, executed_at) DO UPDATE SET
        project_id = EXCLUDED.project_id,
        version_id = EXCLUDED.version_id,
        service_id = EXCLUDED.service_id,
        service_name = EXCLUDED.service_name,
        input_json = EXCLUDED.input_json,
        output_json = EXCLUDED.output_json,
        execution_trace_json = EXCLUDED.execution_trace_json,
        duration_ms = EXCLUDED.duration_ms,
        status = EXCLUDED.status,
        executed_by = EXCLUDED.executed_by,
        business_reference = EXCLUDED.business_reference,
        actual_outcome_json = EXCLUDED.actual_outcome_json,
        outcome_assessment = EXCLUDED.outcome_assessment,
        outcome_note = EXCLUDED.outcome_note,
        outcome_occurred_at = EXCLUDED.outcome_occurred_at,
        outcome_reported_by = EXCLUDED.outcome_reported_by,
        outcome_reported_at = EXCLUDED.outcome_reported_at;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS decision_runs_partitioned_mirror ON decision_runs;
CREATE TRIGGER decision_runs_partitioned_mirror
AFTER INSERT OR UPDATE OR DELETE ON decision_runs
FOR EACH ROW EXECUTE FUNCTION mirror_decision_run_partitioned();

INSERT INTO decision_runs_partitioned (
    id, project_id, version_id, service_id, service_name, input_json, output_json,
    execution_trace_json, duration_ms, status, executed_by, executed_at,
    business_reference, actual_outcome_json, outcome_assessment, outcome_note,
    outcome_occurred_at, outcome_reported_by, outcome_reported_at
)
SELECT id, project_id, version_id, service_id, service_name, input_json, output_json,
       execution_trace_json, duration_ms, status, executed_by, executed_at,
       business_reference, actual_outcome_json, outcome_assessment, outcome_note,
       outcome_occurred_at, outcome_reported_by, outcome_reported_at
FROM decision_runs
ON CONFLICT (id, executed_at) DO NOTHING;

CREATE OR REPLACE FUNCTION ensure_decision_run_month_partition(p_month DATE)
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    month_start DATE := date_trunc('month', p_month::TIMESTAMP)::DATE;
    month_start_utc TIMESTAMPTZ := date_trunc('month', p_month::TIMESTAMP) AT TIME ZONE 'UTC';
    next_month_utc TIMESTAMPTZ := (date_trunc('month', p_month::TIMESTAMP) + INTERVAL '1 month') AT TIME ZONE 'UTC';
    partition_name TEXT;
    has_default_rows BOOLEAN;
BEGIN
    partition_name := format(
        'decision_runs_partitioned_%s', to_char(month_start, 'YYYY_MM')
    );
    IF to_regclass(format('public.%I', partition_name)) IS NOT NULL THEN
        RETURN;
    END IF;

    PERFORM pg_advisory_xact_lock(hashtext(partition_name)::BIGINT);
    IF to_regclass(format('public.%I', partition_name)) IS NOT NULL THEN
        RETURN;
    END IF;

    LOCK TABLE decision_runs_partitioned_default IN ACCESS EXCLUSIVE MODE;
    IF to_regclass(format('public.%I', partition_name)) IS NOT NULL THEN
        RETURN;
    END IF;
    EXECUTE format(
        'SELECT EXISTS (SELECT 1 FROM decision_runs_partitioned_default WHERE executed_at >= %L AND executed_at < %L)',
        month_start_utc,
        next_month_utc
    ) INTO has_default_rows;
    IF has_default_rows THEN
        DROP TABLE IF EXISTS pg_temp.decision_run_partition_stage;
        EXECUTE format(
            'CREATE TEMP TABLE decision_run_partition_stage ON COMMIT DROP AS SELECT * FROM decision_runs_partitioned_default WHERE executed_at >= %L AND executed_at < %L',
            month_start_utc,
            next_month_utc
        );
        EXECUTE format(
            'DELETE FROM decision_runs_partitioned_default WHERE executed_at >= %L AND executed_at < %L',
            month_start_utc,
            next_month_utc
        );
    END IF;

    EXECUTE format(
        'CREATE TABLE %I PARTITION OF decision_runs_partitioned FOR VALUES FROM (%L) TO (%L)',
        partition_name,
        month_start_utc,
        next_month_utc
    );

    IF has_default_rows THEN
        INSERT INTO decision_runs_partitioned
        SELECT * FROM pg_temp.decision_run_partition_stage;
        DROP TABLE pg_temp.decision_run_partition_stage;
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION maintain_decision_run_partitions(months_ahead INTEGER DEFAULT 13)
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    month_offset INTEGER;
    current_month DATE := date_trunc(
        'month', CURRENT_TIMESTAMP AT TIME ZONE 'UTC'
    )::DATE;
BEGIN
    IF months_ahead < 1 OR months_ahead > 36 THEN
        RAISE EXCEPTION 'months_ahead must be between 1 and 36';
    END IF;
    FOR month_offset IN 0..months_ahead LOOP
        PERFORM ensure_decision_run_month_partition(
            (current_month + (month_offset || ' months')::INTERVAL)::DATE
        );
    END LOOP;
END;
$$;

REVOKE ALL ON FUNCTION mirror_decision_run_partitioned() FROM PUBLIC;
REVOKE ALL ON FUNCTION ensure_decision_run_month_partition(DATE) FROM PUBLIC;
REVOKE ALL ON FUNCTION maintain_decision_run_partitions(INTEGER) FROM PUBLIC;

SELECT maintain_decision_run_partitions(13);

\if :{?app_user}
GRANT SELECT ON TABLE decision_runs_partitioned TO :"app_user";
GRANT EXECUTE ON FUNCTION ensure_decision_run_month_partition(DATE) TO :"app_user";
GRANT EXECUTE ON FUNCTION maintain_decision_run_partitions(INTEGER) TO :"app_user";
\endif

COMMIT;
