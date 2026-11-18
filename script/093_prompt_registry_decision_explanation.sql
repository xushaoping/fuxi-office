BEGIN;

ALTER TABLE prompt_assets
    DROP CONSTRAINT IF EXISTS prompt_assets_prompt_key_check;
ALTER TABLE prompt_assets
    ADD CONSTRAINT prompt_assets_prompt_key_check
    CHECK (prompt_key IN (
        'requirement_analysis',
        'decision_planning',
        'dsl_generation',
        'change_intent',
        'optimization_proposal',
        'decision_explanation'
    ));

COMMIT;
