-- Run as the isolated fixture owner (postgres) after 097. No business labels persist.
BEGIN;

INSERT INTO decision_projects (owner_username, name)
VALUES ('admin', 'Explanation revision regression') RETURNING id \gset review_project_
INSERT INTO decision_versions (project_id, version_number, created_by)
VALUES (:review_project_id, 1, 'admin') RETURNING id \gset review_version_
INSERT INTO decision_runs (
    project_id, version_id, service_id, service_name, input_json, output_json,
    execution_trace_json, duration_ms, status, executed_by
) VALUES (
    :review_project_id, :review_version_id, 'fixture', 'fixture', '{}', '{}',
    '{}', 1, 'succeeded', 'admin'
) RETURNING id \gset review_decision_
INSERT INTO decision_run_explanations (
    decision_run_id, status, explanation_json, prompt_version, prompt_digest,
    trace_digest, created_by, completed_at
) VALUES (
    :review_decision_id, 'succeeded', '{"summary":"fixture one","decisions":[]}',
    'fixture-v1', repeat('a',64), repeat('b',64), 'admin', NOW()
) RETURNING id \gset review_cache_

CREATE TEMP TABLE review_revision_fixture AS SELECT :review_cache_id::BIGINT AS cache_id;

-- Simulate a pre-097 legacy label without inventing a revision for it.
ALTER TABLE decision_run_explanation_reviews DISABLE TRIGGER decision_run_explanation_review_revision_guard;
INSERT INTO decision_run_explanation_reviews (explanation_id, reviewer_username, review_label)
VALUES (:review_cache_id, 'admin', 'readable');
ALTER TABLE decision_run_explanation_reviews ENABLE TRIGGER decision_run_explanation_review_revision_guard;
INSERT INTO decision_run_explanation_reviews (
    explanation_id, explanation_revision, reviewer_username, review_label
) VALUES (:review_cache_id, 1, 'admin', 'needs_revision');

DO $$
BEGIN
    BEGIN
        INSERT INTO decision_run_explanation_reviews (explanation_id, explanation_revision, reviewer_username, review_label)
        SELECT cache_id, 1, 'admin', 'readable' FROM review_revision_fixture;
        RAISE EXCEPTION 'duplicate current revision review was accepted';
    EXCEPTION WHEN unique_violation THEN NULL;
    END;
    BEGIN
        INSERT INTO decision_run_explanation_reviews (explanation_id, reviewer_username, review_label)
        SELECT cache_id, 'admin', 'readable' FROM review_revision_fixture;
        RAISE EXCEPTION 'new review without an explicit revision was accepted';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
END;
$$;

UPDATE decision_run_explanations
SET generation_revision = generation_revision + 1, status = 'running',
    explanation_json = NULL, created_at = NOW(), completed_at = NULL
WHERE id = :review_cache_id;
DO $$
BEGIN
    BEGIN
        INSERT INTO decision_run_explanation_reviews (explanation_id, explanation_revision, reviewer_username, review_label)
        SELECT cache_id, 1, 'admin', 'readable' FROM review_revision_fixture;
        RAISE EXCEPTION 'old page could submit a stale revision';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        INSERT INTO decision_run_explanation_reviews (explanation_id, explanation_revision, reviewer_username, review_label)
        SELECT cache_id, 2, 'admin', 'readable' FROM review_revision_fixture;
        RAISE EXCEPTION 'in-progress explanation accepted a review';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
END;
$$;
UPDATE decision_run_explanations
SET status = 'succeeded', explanation_json = '{"summary":"fixture two","decisions":[]}',
    completed_at = NOW() WHERE id = :review_cache_id;
INSERT INTO decision_run_explanation_reviews (
    explanation_id, explanation_revision, reviewer_username, review_label
) VALUES (:review_cache_id, 2, 'admin', 'readable');

DO $$
BEGIN
    IF (SELECT COUNT(*) FROM decision_run_explanation_reviews review
        JOIN review_revision_fixture fixture ON fixture.cache_id = review.explanation_id) <> 3 THEN
        RAISE EXCEPTION 'regeneration mutated or deleted append-only prior labels';
    END IF;
    IF (SELECT COUNT(*) FROM decision_run_explanation_reviews review
        JOIN decision_run_explanations cache ON cache.id = review.explanation_id
        JOIN review_revision_fixture fixture ON fixture.cache_id = cache.id
        WHERE review.explanation_revision = cache.generation_revision) <> 1 THEN
        RAISE EXCEPTION 'current revision inherited legacy or stale labels';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM decision_run_explanation_reviews review
        JOIN review_revision_fixture fixture ON fixture.cache_id = review.explanation_id
        WHERE review.explanation_revision IS NULL
    ) THEN
        RAISE EXCEPTION 'legacy unknown revision was relabelled';
    END IF;
END;
$$;

ROLLBACK;
