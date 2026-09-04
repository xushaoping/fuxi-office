-- Project collaboration and separation of modelling, review, and release duties.
CREATE TABLE IF NOT EXISTS project_members (
    project_id BIGINT NOT NULL REFERENCES decision_projects(id) ON DELETE CASCADE,
    username VARCHAR(255) NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    role VARCHAR(16) NOT NULL CHECK (role IN ('modeler', 'reviewer', 'approver', 'viewer')),
    added_by VARCHAR(255) NOT NULL REFERENCES users(username),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, username, role)
);

CREATE INDEX IF NOT EXISTS project_members_username_idx
    ON project_members (username, project_id);

\if :{?app_user}
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE project_members TO :"app_user";
\endif
