\if :{?start_at}
\else
\echo 'start_at is required (UTC timestamp)'
\quit 2
\endif
\if :{?end_at}
\else
\echo 'end_at is required (UTC timestamp)'
\quit 2
\endif

BEGIN READ ONLY;
SET LOCAL TIME ZONE 'UTC';
SET LOCAL fuxi_reconcile.start_at = :'start_at';
SET LOCAL fuxi_reconcile.end_at = :'end_at';

DO $$
DECLARE
    window_start TIMESTAMPTZ := current_setting('fuxi_reconcile.start_at')::TIMESTAMPTZ;
    window_end TIMESTAMPTZ := current_setting('fuxi_reconcile.end_at')::TIMESTAMPTZ;
    row_differences BIGINT;
    aggregate_differences BIGINT;
BEGIN
    IF window_start >= window_end THEN
        RAISE EXCEPTION 'start_at must be earlier than end_at';
    END IF;
    IF window_start::TIME <> TIME '00:00:00'
       OR window_end::TIME <> TIME '00:00:00' THEN
        RAISE EXCEPTION 'start_at and end_at must be UTC day boundaries for daily aggregate comparison';
    END IF;

    SELECT COUNT(*)
      INTO row_differences
      FROM decision_runs source
      FULL OUTER JOIN decision_runs_partitioned partitioned
        ON partitioned.id = source.id
       AND partitioned.executed_at = source.executed_at
     WHERE COALESCE(source.executed_at, partitioned.executed_at) >= window_start
       AND COALESCE(source.executed_at, partitioned.executed_at) < window_end
       AND (
           source.id IS NULL OR partitioned.id IS NULL OR
           ROW(source.project_id, source.version_id, source.service_id,
               source.service_name, source.input_json, source.output_json,
               source.execution_trace_json, source.duration_ms, source.status,
               source.executed_by, source.business_reference, source.actual_outcome_json,
               source.outcome_assessment, source.outcome_note, source.outcome_occurred_at,
               source.outcome_reported_by, source.outcome_reported_at)
           IS DISTINCT FROM
           ROW(partitioned.project_id, partitioned.version_id, partitioned.service_id,
               partitioned.service_name, partitioned.input_json, partitioned.output_json,
               partitioned.execution_trace_json, partitioned.duration_ms, partitioned.status,
               partitioned.executed_by, partitioned.business_reference,
               partitioned.actual_outcome_json, partitioned.outcome_assessment,
               partitioned.outcome_note, partitioned.outcome_occurred_at,
               partitioned.outcome_reported_by, partitioned.outcome_reported_at)
       );

    WITH source_daily AS (
        SELECT project_id, version_id, (executed_at AT TIME ZONE 'UTC')::DATE AS run_day,
               COUNT(*)::BIGINT AS total_runs,
               COUNT(*) FILTER (WHERE status = 'succeeded')::BIGINT AS succeeded_runs,
               AVG(duration_ms)::DOUBLE PRECISION AS average_duration_ms,
               percentile_disc(0.50) WITHIN GROUP (ORDER BY duration_ms)::BIGINT AS p50_ms,
               percentile_disc(0.95) WITHIN GROUP (ORDER BY duration_ms)::BIGINT AS p95_ms
          FROM decision_runs
         WHERE executed_at >= window_start AND executed_at < window_end
         GROUP BY project_id, version_id, (executed_at AT TIME ZONE 'UTC')::DATE
    ), source_rule_hits AS (
        SELECT run.project_id, run.version_id,
               (run.executed_at AT TIME ZONE 'UTC')::DATE AS run_day,
               matched.rule_id, COUNT(*)::BIGINT AS hit_count
          FROM decision_runs run
          CROSS JOIN LATERAL jsonb_array_elements(
              CASE WHEN jsonb_typeof(run.execution_trace_json->'decisions') = 'array'
                   THEN run.execution_trace_json->'decisions' ELSE '[]'::JSONB END
          ) AS decision(value)
          CROSS JOIN LATERAL jsonb_array_elements_text(
              CASE WHEN jsonb_typeof(decision.value->'matchedRuleIds') = 'array'
                   THEN decision.value->'matchedRuleIds' ELSE '[]'::JSONB END
          ) AS matched(rule_id)
         WHERE run.executed_at >= window_start AND run.executed_at < window_end
         GROUP BY run.project_id, run.version_id,
                  (run.executed_at AT TIME ZONE 'UTC')::DATE, matched.rule_id
    ), source_rule_distributions AS (
        SELECT project_id, version_id, run_day,
               jsonb_object_agg(rule_id, hit_count ORDER BY rule_id) AS rule_hit_distribution
          FROM source_rule_hits
         GROUP BY project_id, version_id, run_day
    ), source_with_rules AS (
        SELECT source_daily.*,
               COALESCE(source_rule_distributions.rule_hit_distribution, '{}'::JSONB)
                   AS rule_hit_distribution
          FROM source_daily
          LEFT JOIN source_rule_distributions USING (project_id, version_id, run_day)
    ), view_daily AS (
        SELECT *
          FROM decision_run_daily_stats
         WHERE run_day >= (window_start AT TIME ZONE 'UTC')::DATE
           AND run_day <= ((window_end - INTERVAL '1 microsecond') AT TIME ZONE 'UTC')::DATE
    )
    SELECT COUNT(*)
      INTO aggregate_differences
      FROM source_with_rules source
      FULL OUTER JOIN view_daily daily USING (project_id, version_id, run_day)
     WHERE source.project_id IS NULL OR daily.project_id IS NULL
        OR ROW(source.total_runs, source.succeeded_runs, source.average_duration_ms,
               source.p50_ms, source.p95_ms, source.rule_hit_distribution)
           IS DISTINCT FROM
           ROW(daily.total_runs, daily.succeeded_runs, daily.average_duration_ms,
               daily.p50_ms, daily.p95_ms, daily.rule_hit_distribution);

    IF row_differences <> 0 OR aggregate_differences <> 0 THEN
        RAISE EXCEPTION 'partition reconciliation failed: % row differences, % daily aggregate differences',
            row_differences, aggregate_differences;
    END IF;
    RAISE NOTICE 'partition reconciliation passed: % to %, no row or daily aggregate differences',
        window_start, window_end;
END;
$$;

COMMIT;
