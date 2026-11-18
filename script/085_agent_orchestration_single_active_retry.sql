BEGIN;

DO $$
BEGIN
    IF EXISTS (
        SELECT retry_of_orchestration_id
        FROM agent_orchestrations
        WHERE retry_of_orchestration_id IS NOT NULL
          AND status IN ('pending', 'running')
        GROUP BY retry_of_orchestration_id
        HAVING COUNT(*) > 1
    ) THEN
        RAISE EXCEPTION 'cannot enforce one active orchestration retry: duplicate active retries already exist';
    END IF;
END;
$$;

CREATE UNIQUE INDEX IF NOT EXISTS agent_orchestrations_one_active_retry_idx
    ON agent_orchestrations (retry_of_orchestration_id)
    WHERE retry_of_orchestration_id IS NOT NULL
      AND status IN ('pending', 'running');

COMMIT;
