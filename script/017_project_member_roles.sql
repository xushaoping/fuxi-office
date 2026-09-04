-- Allow one collaborator to hold more than one project role.
-- Existing rows are preserved; the old username-only key is replaced with a
-- role-aware key so adding a second role no longer overwrites the first one.
BEGIN;

ALTER TABLE project_members
    DROP CONSTRAINT IF EXISTS project_members_pkey;

ALTER TABLE project_members
    ADD CONSTRAINT project_members_pkey PRIMARY KEY (project_id, username, role);

COMMIT;

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE project_members TO :"app_user";
\endif
