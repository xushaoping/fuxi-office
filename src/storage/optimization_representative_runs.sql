SELECT run.id, run.executed_at, run.status, run.outcome_assessment,
       COALESCE((
           SELECT jsonb_agg(input_key.key ORDER BY input_key.key)
           FROM jsonb_object_keys(
               CASE WHEN jsonb_typeof(run.input_json) = 'object'
                    THEN run.input_json ELSE '{}'::JSONB END
           ) AS input_key(key)
       ), '[]'::JSONB) AS input_fields,
       COALESCE((
           SELECT jsonb_agg(output_key.key ORDER BY output_key.key)
           FROM jsonb_object_keys(
               CASE WHEN jsonb_typeof(run.output_json) = 'object'
                    THEN run.output_json ELSE '{}'::JSONB END
           ) AS output_key(key)
       ), '[]'::JSONB) AS output_fields
FROM decision_runs_partitioned run
CROSS JOIN LATERAL (
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1
        FROM generate_series(0, cardinality($4::TEXT[]) - 1) ancestor(depth)
        WHERE jsonb_typeof(run.input_json #> ($4::TEXT[])[1:ancestor.depth])
              IS DISTINCT FROM 'object'
    ) THEN run.input_json #> $4::TEXT[] END AS numeric_value
) field
WHERE run.project_id = $1 AND run.version_id = $2
  AND run.executed_at >= NOW() - INTERVAL '30 days'
  AND run.executed_at <= NOW()
  AND (
      ($3 = 'HIGH_FAILURE_RATE' AND run.status = 'failed')
      OR ($3 IN ('HIGH_MISMATCH_RATE', 'RULE_FEEDBACK_MISMATCH')
          AND run.outcome_assessment = 'mismatch')
      OR ($3 IN ('INPUT_DISTRIBUTION_DRIFT', 'INPUT_FIELD_MISSINGNESS_DRIFT')
          AND cardinality($4::TEXT[]) > 0
          AND run.status = 'succeeded'
          AND run.outcome_assessment IN ('confirmed', 'mismatch')
          AND BTRIM(run.business_reference) <> ''
          AND (
              ($3 = 'INPUT_DISTRIBUTION_DRIFT'
                  AND jsonb_typeof(field.numeric_value) = 'number')
              OR ($3 = 'INPUT_FIELD_MISSINGNESS_DRIFT'
                  AND jsonb_typeof(field.numeric_value) IS DISTINCT FROM 'number')
          ))
  )
ORDER BY run.executed_at DESC, run.id DESC
LIMIT 3;
