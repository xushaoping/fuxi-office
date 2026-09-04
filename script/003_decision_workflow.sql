-- Add requirement analysis, clarification and decision planning persistence.
-- Run while connected to fuxi_office, optionally with: -v app_user=fuxi

BEGIN;

CREATE TABLE IF NOT EXISTS decision_workflows (
    project_id BIGINT PRIMARY KEY REFERENCES decision_projects(id) ON DELETE CASCADE,
    status VARCHAR(24) NOT NULL
        CHECK (status IN ('analyzed', 'clarification', 'planned', 'confirmed')),
    requirement_text TEXT NOT NULL,
    decision_brief_json JSONB NOT NULL,
    clarifications_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    answers_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    decision_plan_json JSONB,
    confirmed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE decision_workflows TO :"app_user";
\endif
