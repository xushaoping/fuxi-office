BEGIN;

CREATE MATERIALIZED VIEW IF NOT EXISTS decision_run_daily_stats AS
WITH run_stats AS (
    SELECT project_id,
           version_id,
           (executed_at AT TIME ZONE 'UTC')::DATE AS run_day,
           COUNT(*)::BIGINT AS total_runs,
           COUNT(*) FILTER (WHERE status = 'succeeded')::BIGINT AS succeeded_runs,
           AVG(duration_ms)::DOUBLE PRECISION AS average_duration_ms,
           percentile_disc(0.50) WITHIN GROUP (ORDER BY duration_ms)::BIGINT AS p50_ms,
           percentile_disc(0.95) WITHIN GROUP (ORDER BY duration_ms)::BIGINT AS p95_ms
    FROM decision_runs_partitioned
    GROUP BY project_id, version_id, (executed_at AT TIME ZONE 'UTC')::DATE
), rule_hits AS (
    SELECT run.project_id,
           run.version_id,
           (run.executed_at AT TIME ZONE 'UTC')::DATE AS run_day,
           matched.rule_id,
           COUNT(*)::BIGINT AS hit_count
    FROM decision_runs_partitioned run
    CROSS JOIN LATERAL jsonb_array_elements(
        CASE WHEN jsonb_typeof(run.execution_trace_json->'decisions') = 'array'
             THEN run.execution_trace_json->'decisions' ELSE '[]'::JSONB END
    ) AS decision(value)
    CROSS JOIN LATERAL jsonb_array_elements_text(
        CASE WHEN jsonb_typeof(decision.value->'matchedRuleIds') = 'array'
             THEN decision.value->'matchedRuleIds' ELSE '[]'::JSONB END
    ) AS matched(rule_id)
    GROUP BY run.project_id, run.version_id,
             (run.executed_at AT TIME ZONE 'UTC')::DATE, matched.rule_id
), rule_distributions AS (
    SELECT project_id, version_id, run_day,
           jsonb_object_agg(rule_id, hit_count ORDER BY rule_id) AS rule_hit_distribution
    FROM rule_hits
    GROUP BY project_id, version_id, run_day
)
SELECT run_stats.project_id,
       run_stats.version_id,
       run_stats.run_day,
       run_stats.total_runs,
       run_stats.succeeded_runs,
       run_stats.total_runs - run_stats.succeeded_runs AS failed_runs,
       run_stats.succeeded_runs::DOUBLE PRECISION / NULLIF(run_stats.total_runs, 0) AS success_rate,
       run_stats.average_duration_ms,
       run_stats.p50_ms,
       run_stats.p95_ms,
       COALESCE(rule_distributions.rule_hit_distribution, '{}'::JSONB) AS rule_hit_distribution,
       NOW() AS refreshed_at
FROM run_stats
LEFT JOIN rule_distributions USING (project_id, version_id, run_day)
WITH DATA;

CREATE UNIQUE INDEX IF NOT EXISTS decision_run_daily_stats_key_idx
    ON decision_run_daily_stats (project_id, version_id, run_day);

CREATE OR REPLACE FUNCTION refresh_decision_run_daily_stats()
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
BEGIN
    REFRESH MATERIALIZED VIEW decision_run_daily_stats;
END;
$$;

REVOKE ALL ON FUNCTION refresh_decision_run_daily_stats() FROM PUBLIC;

\if :{?app_user}
GRANT SELECT ON TABLE decision_run_daily_stats TO :"app_user";
GRANT EXECUTE ON FUNCTION refresh_decision_run_daily_stats() TO :"app_user";
\endif

COMMIT;
