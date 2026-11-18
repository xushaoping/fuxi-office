BEGIN;

ALTER TABLE decision_run_explanations
    ADD COLUMN IF NOT EXISTS generation_revision BIGINT NOT NULL DEFAULT 1
        CHECK (generation_revision > 0);
ALTER TABLE decision_run_explanation_reviews
    ADD COLUMN IF NOT EXISTS explanation_revision BIGINT
        CHECK (explanation_revision > 0);

-- Legacy labels may refer to content that has since been overwritten. Keep them
-- append-only, with unknown revision; never infer a version from today's cache.
CREATE UNIQUE INDEX IF NOT EXISTS decision_run_explanation_reviews_revision_reviewer_idx
    ON decision_run_explanation_reviews (explanation_id, explanation_revision, reviewer_username)
    WHERE explanation_revision IS NOT NULL;

CREATE OR REPLACE FUNCTION validate_decision_explanation_review_revision()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    current_revision BIGINT;
    current_status TEXT;
BEGIN
    SELECT generation_revision, status INTO current_revision, current_status
    FROM decision_run_explanations WHERE id = NEW.explanation_id FOR UPDATE;
    IF NEW.explanation_revision IS NULL
       OR current_status IS DISTINCT FROM 'succeeded'
       OR NEW.explanation_revision IS DISTINCT FROM current_revision THEN
        RAISE EXCEPTION 'explanation changed or is not ready for review'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
DROP TRIGGER IF EXISTS decision_run_explanation_review_revision_guard
    ON decision_run_explanation_reviews;
CREATE TRIGGER decision_run_explanation_review_revision_guard
    BEFORE INSERT ON decision_run_explanation_reviews
    FOR EACH ROW EXECUTE FUNCTION validate_decision_explanation_review_revision();

COMMENT ON COLUMN decision_run_explanations.generation_revision IS
    'Monotonic generation attempt revision; increments whenever cache content is replaced.';
COMMENT ON COLUMN decision_run_explanation_reviews.explanation_revision IS
    'Reviewed generation revision; NULL denotes legacy labels with unprovable content attribution.';

COMMIT;
