\set ON_ERROR_STOP on

BEGIN;
CREATE TEMP TABLE decision_runs_partitioned (
    id BIGINT, project_id BIGINT, version_id BIGINT,
    input_json JSONB, output_json JSONB,
    executed_at TIMESTAMPTZ, status TEXT,
    outcome_assessment TEXT, business_reference TEXT
);
INSERT INTO decision_runs_partitioned
SELECT id, 10, 20, input, '{"decision":"sensitive output"}'::JSONB,
       NOW() - INTERVAL '1 hour', 'succeeded', 'confirmed', 'case-' || id
FROM (VALUES
    (1, '{"applicant":{"age":42},"applicant.age":99}'::JSONB),
    (2, '{"applicant.age":101}'::JSONB),
    (3, '{"applicant":{"age":null}}'::JSONB),
    (4, '{"applicant":{"age":"sensitive input"}}'::JSONB),
    (5, '{}'::JSONB), (6, '{}'::JSONB),
    (7, '{"applicant":{"age":42}}'::JSONB),
    (8, '{"applicant":{"age":42}}'::JSONB),
    (9, '{"applicant":{"age":42}}'::JSONB),
    (10, '{"applicant":{"age":42}}'::JSONB),
    (11, '{}'::JSONB), (12, '{}'::JSONB)
) fixture(id, input);
UPDATE decision_runs_partitioned SET status = 'failed' WHERE id = 6;
UPDATE decision_runs_partitioned SET project_id = 11 WHERE id = 7;
UPDATE decision_runs_partitioned SET version_id = 21 WHERE id = 8;
UPDATE decision_runs_partitioned SET executed_at = NOW() - INTERVAL '31 days' WHERE id = 9;
UPDATE decision_runs_partitioned SET executed_at = NOW() + INTERVAL '1 day' WHERE id = 10;
UPDATE decision_runs_partitioned SET outcome_assessment = NULL WHERE id = 11;
UPDATE decision_runs_partitioned SET business_reference = ' ' WHERE id = 12;

-- Execute the exact query embedded in the application, with bound text-array paths.
\set representative_query `cat src/storage/optimization_representative_runs.sql`
PREPARE representative_runs (BIGINT, BIGINT, TEXT, TEXT[]) AS :representative_query

CREATE TEMP TABLE nested_evidence AS
EXECUTE representative_runs(10, 20, 'INPUT_DISTRIBUTION_DRIFT', ARRAY['applicant','age']);
CREATE TEMP TABLE literal_evidence AS
EXECUTE representative_runs(10, 20, 'INPUT_DISTRIBUTION_DRIFT', ARRAY['applicant.age']);
CREATE TEMP TABLE missing_evidence AS
EXECUTE representative_runs(10, 20, 'INPUT_FIELD_MISSINGNESS_DRIFT', ARRAY['applicant','age']);
CREATE TEMP TABLE empty_path_evidence AS
EXECUTE representative_runs(10, 20, 'INPUT_FIELD_MISSINGNESS_DRIFT', ARRAY[]::TEXT[]);
DO $$
BEGIN
    IF (SELECT ARRAY_AGG(id ORDER BY id) FROM nested_evidence) IS DISTINCT FROM ARRAY[1]::BIGINT[]
       OR (SELECT ARRAY_AGG(id ORDER BY id) FROM literal_evidence) IS DISTINCT FROM ARRAY[1,2]::BIGINT[]
       OR (SELECT ARRAY_AGG(id ORDER BY id) FROM missing_evidence) IS DISTINCT FROM ARRAY[3,4,5]::BIGINT[]
       OR EXISTS (SELECT 1 FROM empty_path_evidence) THEN
        RAISE EXCEPTION 'field evidence selected the wrong scope, path or numeric availability';
    END IF;
    IF EXISTS (SELECT 1 FROM missing_evidence row
               WHERE row_to_json(row)::TEXT LIKE '%sensitive%') THEN
        RAISE EXCEPTION 'representative evidence exposed input or output values';
    END IF;
END;
$$;

INSERT INTO decision_runs_partitioned VALUES
    (20, 10, 20, '{"applicant":{"0":88}}', '{}', NOW(), 'succeeded', 'confirmed', 'case-20'),
    (21, 10, 20, '{"applicant":[88]}', '{}', NOW(), 'succeeded', 'confirmed', 'case-21');
CREATE TEMP TABLE numeric_key_evidence AS
EXECUTE representative_runs(10, 20, 'INPUT_DISTRIBUTION_DRIFT', ARRAY['applicant','0']);
DO $$
BEGIN
    IF (SELECT ARRAY_AGG(id) FROM numeric_key_evidence) IS DISTINCT FROM ARRAY[20]::BIGINT[] THEN
        RAISE EXCEPTION 'numeric object keys must not traverse array indices';
    END IF;
END;
$$;
DEALLOCATE representative_runs;
ROLLBACK;
