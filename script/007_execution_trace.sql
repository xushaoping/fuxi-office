-- Persist deterministic execution traces for business acceptance cases.

ALTER TABLE decision_test_cases
    ADD COLUMN IF NOT EXISTS execution_trace_json JSONB;

-- Optional application-role grant for installations with separate table owners.
\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_test_cases TO :"app_user";
\endif
