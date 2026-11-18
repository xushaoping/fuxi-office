BEGIN;

ALTER TABLE decision_workflows
    ADD COLUMN IF NOT EXISTS tester_report_json JSONB;

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_workflows TO :"app_user";
\endif
