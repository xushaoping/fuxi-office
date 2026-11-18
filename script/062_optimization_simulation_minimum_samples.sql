BEGIN;

UPDATE optimization_simulations
SET passed = FALSE
WHERE passed AND test_set_size < 10;

ALTER TABLE optimization_simulations
    DROP CONSTRAINT IF EXISTS optimization_simulations_passed_check;
ALTER TABLE optimization_simulations
    ADD CONSTRAINT optimization_simulations_passed_check
    CHECK (NOT passed OR (test_set_size >= 10 AND simulated_accuracy >= baseline_accuracy));

COMMIT;
