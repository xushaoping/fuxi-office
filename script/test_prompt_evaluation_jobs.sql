BEGIN;

DO $$
DECLARE
    asset_id BIGINT;
    job_id BIGINT;
    cancelled_job_id BIGINT;
    pending_asset_id BIGINT;
    running_asset_id BIGINT;
    listing_asset_id BIGINT;
    pending_job_id BIGINT;
    running_job_id BIGINT;
    terminal_job_ids BIGINT[] := ARRAY[]::BIGINT[];
    listed_job_ids BIGINT[];
    i INTEGER;
BEGIN
    INSERT INTO prompt_assets (prompt_key, version, content, content_digest, created_by, status)
    VALUES ('tester_judge', 'judge-registry-test-v1', 'judge prompt body', repeat('e', 64), 'admin', 'active');
    BEGIN
        INSERT INTO prompt_assets (prompt_key, version, content, content_digest, created_by, status)
        VALUES ('tester_judge', 'judge-registry-test-v2', 'other judge body', repeat('f', 64), 'admin', 'active');
        RAISE EXCEPTION 'duplicate active tester_judge prompt was accepted';
    EXCEPTION WHEN unique_violation THEN
        NULL;
    END;
    BEGIN
        INSERT INTO prompt_assets (prompt_key, version, content, content_digest, created_by)
        VALUES ('unknown_judge', 'judge-unknown-v1', 'unknown prompt body', repeat('e', 64), 'admin');
        RAISE EXCEPTION 'unknown prompt category was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;

    INSERT INTO prompt_assets (prompt_key, version, content, content_digest, created_by)
    VALUES ('dsl_generation', 'job-test-v1', 'prompt body', repeat('a', 64), 'admin')
    RETURNING id INTO asset_id;

    INSERT INTO prompt_evaluation_jobs (prompt_asset_id, created_by, total_cases)
    VALUES (asset_id, 'admin', 50)
    RETURNING id INTO job_id;

    UPDATE prompt_evaluation_jobs
    SET status = 'running', started_at = NOW(), completed_cases = 3
    WHERE id = job_id;
    IF (SELECT completed_cases FROM prompt_evaluation_jobs WHERE id = job_id) <> 3 THEN
        RAISE EXCEPTION 'prompt evaluation progress was not persisted';
    END IF;

    BEGIN
        UPDATE prompt_evaluation_jobs SET completed_cases = 51 WHERE id = job_id;
        RAISE EXCEPTION 'progress greater than total was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;

    BEGIN
        INSERT INTO prompt_evaluation_jobs (prompt_asset_id, created_by, total_cases)
        VALUES (asset_id, 'admin', 50);
        RAISE EXCEPTION 'duplicate active prompt evaluation job was accepted';
    EXCEPTION WHEN unique_violation THEN
        NULL;
    END;

    BEGIN
        UPDATE prompt_evaluation_jobs
        SET status = 'succeeded', finished_at = NOW()
        WHERE id = job_id;
        RAISE EXCEPTION 'successful job without a result was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;

    BEGIN
        UPDATE prompt_evaluation_jobs
        SET status = 'failed', error_message = NULL, finished_at = NOW()
        WHERE id = job_id;
        RAISE EXCEPTION 'failed job without an error was accepted';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;

    UPDATE prompt_evaluation_jobs
    SET status = 'succeeded', result_json = '{"passed":false}'::JSONB, finished_at = NOW()
    WHERE id = job_id;

    INSERT INTO prompt_evaluation_jobs (prompt_asset_id, created_by, total_cases)
    VALUES (asset_id, 'admin', 20)
    RETURNING id INTO cancelled_job_id;
    UPDATE prompt_evaluation_jobs
    SET status = 'running', started_at = NOW(), cancel_requested = TRUE
    WHERE id = cancelled_job_id;
    UPDATE prompt_evaluation_jobs
    SET status = 'cancelled', finished_at = NOW()
    WHERE id = cancelled_job_id;
    IF (SELECT status FROM prompt_evaluation_jobs WHERE id = cancelled_job_id) <> 'cancelled' THEN
        RAISE EXCEPTION 'cancelled prompt evaluation did not reach its terminal state';
    END IF;

    UPDATE prompt_evaluation_jobs
    SET created_at = NOW() - INTERVAL '1 year'
    WHERE id IN (job_id, cancelled_job_id);

    INSERT INTO prompt_assets (prompt_key, version, content, content_digest, created_by)
    VALUES ('dsl_generation', 'job-list-pending-v1', 'prompt body', repeat('b', 64), 'admin')
    RETURNING id INTO pending_asset_id;
    INSERT INTO prompt_assets (prompt_key, version, content, content_digest, created_by)
    VALUES ('dsl_generation', 'job-list-running-v1', 'prompt body', repeat('c', 64), 'admin')
    RETURNING id INTO running_asset_id;
    INSERT INTO prompt_assets (prompt_key, version, content, content_digest, created_by)
    VALUES ('dsl_generation', 'job-list-terminal-v1', 'prompt body', repeat('d', 64), 'admin')
    RETURNING id INTO listing_asset_id;

    INSERT INTO prompt_evaluation_jobs (prompt_asset_id, created_by, total_cases, created_at)
    VALUES (pending_asset_id, 'admin', 1, NOW() - INTERVAL '1 second')
    RETURNING id INTO pending_job_id;
    INSERT INTO prompt_evaluation_jobs (prompt_asset_id, created_by, status, total_cases, created_at, started_at)
    VALUES (running_asset_id, 'admin', 'running', 1, NOW() - INTERVAL '2 seconds', NOW())
    RETURNING id INTO running_job_id;

    FOR i IN 1..21 LOOP
        INSERT INTO prompt_evaluation_jobs (
            prompt_asset_id, created_by, status, completed_cases, total_cases,
            result_json, created_at, finished_at
        )
        VALUES (
            listing_asset_id, 'admin', 'succeeded', 1, 1,
            '{"passed":true}'::JSONB,
            NOW() - make_interval(mins => CASE WHEN i = 19 THEN 18 ELSE i END), NOW()
        )
        RETURNING id INTO job_id;
        terminal_job_ids := array_append(terminal_job_ids, job_id);
    END LOOP;

    WITH listed AS (
        SELECT id, row_number() OVER (
            ORDER BY CASE WHEN status IN ('pending', 'running') THEN 0 ELSE 1 END,
                     created_at DESC, id DESC
        ) AS position
        FROM prompt_evaluation_jobs
        ORDER BY CASE WHEN status IN ('pending', 'running') THEN 0 ELSE 1 END,
                 created_at DESC, id DESC
        LIMIT 20
    )
    SELECT array_agg(id ORDER BY position) INTO listed_job_ids FROM listed;

    IF cardinality(listed_job_ids) <> 20 THEN
        RAISE EXCEPTION 'recent prompt evaluation job query returned %, expected 20', cardinality(listed_job_ids);
    END IF;
    IF listed_job_ids[1] <> pending_job_id OR listed_job_ids[2] <> running_job_id THEN
        RAISE EXCEPTION 'pending and running jobs were not prioritized by creation time';
    END IF;
    IF listed_job_ids[3] <> terminal_job_ids[1] OR listed_job_ids[20] <> terminal_job_ids[19] THEN
        RAISE EXCEPTION 'terminal jobs were not returned newest-first with descending ID tie-break within the 20-job limit';
    END IF;
END;
$$;

ROLLBACK;
