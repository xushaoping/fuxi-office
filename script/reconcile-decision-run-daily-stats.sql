-- Compare the daily materialized view with the partitioned source.
-- Usage: psql -X -v ON_ERROR_STOP=1 -v lookback_days=30 -f this-file
-- The window covers the preceding complete UTC days and never refreshes or writes data.

\if :{?lookback_days}
\else
\set lookback_days 30
\endif

SELECT CASE
         WHEN :'lookback_days' ~ '^[1-9][0-9]*$'
          AND :'lookback_days'::BIGINT <= 3660 THEN 'on'
         ELSE 'off'
       END AS lookback_valid
\gset

\if :lookback_valid
\else
\echo 'lookback_days must be an integer from 1 through 3660'
DO $$ BEGIN RAISE EXCEPTION 'invalid lookback_days'; END $$;
\endif

BEGIN READ ONLY;
SET LOCAL TIME ZONE 'UTC';
SET LOCAL fuxi.daily_stats_reconcile.lookback_days = :'lookback_days';

WITH bounds AS (
    SELECT (NOW() AT TIME ZONE 'UTC')::DATE
               - current_setting('fuxi.daily_stats_reconcile.lookback_days')::INTEGER AS start_day,
           (NOW() AT TIME ZONE 'UTC')::DATE AS end_day
), run_stats AS (
    SELECT run.project_id,
           run.version_id,
           (run.executed_at AT TIME ZONE 'UTC')::DATE AS run_day,
           COUNT(*)::BIGINT AS total_runs,
           COUNT(*) FILTER (WHERE run.status = 'succeeded')::BIGINT AS succeeded_runs,
           AVG(run.duration_ms)::DOUBLE PRECISION AS average_duration_ms,
           percentile_disc(0.50) WITHIN GROUP (ORDER BY run.duration_ms)::BIGINT AS p50_ms,
           percentile_disc(0.95) WITHIN GROUP (ORDER BY run.duration_ms)::BIGINT AS p95_ms
    FROM decision_runs_partitioned run
    CROSS JOIN bounds
    WHERE run.executed_at >= bounds.start_day::TIMESTAMP AT TIME ZONE 'UTC'
      AND run.executed_at < bounds.end_day::TIMESTAMP AT TIME ZONE 'UTC'
    GROUP BY run.project_id, run.version_id,
             (run.executed_at AT TIME ZONE 'UTC')::DATE
), rule_hits AS (
    SELECT run.project_id,
           run.version_id,
           (run.executed_at AT TIME ZONE 'UTC')::DATE AS run_day,
           matched.rule_id,
           COUNT(*)::BIGINT AS hit_count
    FROM decision_runs_partitioned run
    CROSS JOIN bounds
    CROSS JOIN LATERAL jsonb_array_elements(
        CASE WHEN jsonb_typeof(run.execution_trace_json->'decisions') = 'array'
             THEN run.execution_trace_json->'decisions' ELSE '[]'::JSONB END
    ) AS decision(value)
    CROSS JOIN LATERAL jsonb_array_elements_text(
        CASE WHEN jsonb_typeof(decision.value->'matchedRuleIds') = 'array'
             THEN decision.value->'matchedRuleIds' ELSE '[]'::JSONB END
    ) AS matched(rule_id)
    WHERE run.executed_at >= bounds.start_day::TIMESTAMP AT TIME ZONE 'UTC'
      AND run.executed_at < bounds.end_day::TIMESTAMP AT TIME ZONE 'UTC'
    GROUP BY run.project_id, run.version_id,
             (run.executed_at AT TIME ZONE 'UTC')::DATE, matched.rule_id
), rule_distributions AS (
    SELECT project_id,
           version_id,
           run_day,
           jsonb_object_agg(rule_id, hit_count ORDER BY rule_id) AS rule_hit_distribution
    FROM rule_hits
    GROUP BY project_id, version_id, run_day
), expected AS (
    SELECT stats.project_id,
           stats.version_id,
           stats.run_day,
           stats.total_runs,
           stats.succeeded_runs,
           stats.total_runs - stats.succeeded_runs AS failed_runs,
           stats.succeeded_runs::DOUBLE PRECISION / NULLIF(stats.total_runs, 0) AS success_rate,
           stats.average_duration_ms,
           stats.p50_ms,
           stats.p95_ms,
           COALESCE(distribution.rule_hit_distribution, '{}'::JSONB) AS rule_hit_distribution
    FROM run_stats stats
    LEFT JOIN rule_distributions distribution USING (project_id, version_id, run_day)
), actual AS (
    SELECT daily.project_id,
           daily.version_id,
           daily.run_day,
           daily.total_runs,
           daily.succeeded_runs,
           daily.failed_runs,
           daily.success_rate,
           daily.average_duration_ms,
           daily.p50_ms,
           daily.p95_ms,
           daily.rule_hit_distribution
    FROM decision_run_daily_stats daily
    CROSS JOIN bounds
    WHERE daily.run_day >= bounds.start_day
      AND daily.run_day < bounds.end_day
), differences AS (
    SELECT COALESCE(expected.project_id, actual.project_id) AS project_id,
           COALESCE(expected.version_id, actual.version_id) AS version_id,
           COALESCE(expected.run_day, actual.run_day) AS run_day,
           CASE
               WHEN expected.project_id IS NULL THEN 'orphan_materialized_row'
               WHEN actual.project_id IS NULL THEN 'missing_materialized_row'
               ELSE 'value_mismatch'
           END AS difference,
           CASE WHEN expected.project_id IS NULL THEN NULL ELSE
               jsonb_build_object(
                   'totalRuns', expected.total_runs,
                   'succeededRuns', expected.succeeded_runs,
                   'failedRuns', expected.failed_runs,
                   'successRate', expected.success_rate,
                   'averageDurationMs', expected.average_duration_ms,
                   'p50Ms', expected.p50_ms,
                   'p95Ms', expected.p95_ms,
                   'ruleHitDistribution', expected.rule_hit_distribution
               ) END AS expected,
           CASE WHEN actual.project_id IS NULL THEN NULL ELSE
               jsonb_build_object(
                   'totalRuns', actual.total_runs,
                   'succeededRuns', actual.succeeded_runs,
                   'failedRuns', actual.failed_runs,
                   'successRate', actual.success_rate,
                   'averageDurationMs', actual.average_duration_ms,
                   'p50Ms', actual.p50_ms,
                   'p95Ms', actual.p95_ms,
                   'ruleHitDistribution', actual.rule_hit_distribution
               ) END AS actual
    FROM expected
    FULL OUTER JOIN actual USING (project_id, version_id, run_day)
    WHERE expected.project_id IS NULL OR actual.project_id IS NULL
       OR ROW(expected.total_runs, expected.succeeded_runs, expected.failed_runs,
              expected.success_rate, expected.average_duration_ms, expected.p50_ms,
              expected.p95_ms, expected.rule_hit_distribution)
          IS DISTINCT FROM
          ROW(actual.total_runs, actual.succeeded_runs, actual.failed_runs,
              actual.success_rate, actual.average_duration_ms, actual.p50_ms,
              actual.p95_ms, actual.rule_hit_distribution)
), ranked_differences AS (
    SELECT differences.*,
           row_number() OVER (ORDER BY project_id, version_id, run_day) AS row_number
    FROM differences
)
SELECT CASE WHEN COUNT(*) > 0 THEN 'on' ELSE 'off' END AS has_differences,
       COUNT(*)::BIGINT AS difference_count,
       CASE WHEN COUNT(*) > 100 THEN 'on' ELSE 'off' END AS details_truncated,
       COALESCE(
           jsonb_agg(to_jsonb(ranked_differences) - 'row_number'
                     ORDER BY project_id, version_id, run_day)
               FILTER (WHERE row_number <= 100),
           '[]'::JSONB
       )::TEXT AS difference_details
FROM ranked_differences
\gset

\if :has_differences
\echo 'daily stats reconciliation differences: ' :difference_count
SELECT jsonb_pretty(:'difference_details'::JSONB);
\if :details_truncated
\echo 'Only the first 100 mismatches are shown.'
\endif
ROLLBACK;
DO $$ BEGIN RAISE EXCEPTION 'daily stats reconciliation failed'; END $$;
\else
\echo 'daily stats reconciliation passed: no differences in the preceding complete UTC days'
COMMIT;
\endif
