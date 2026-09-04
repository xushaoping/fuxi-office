-- Add system roles to users. Run while connected to fuxi_office.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS role VARCHAR(16) NOT NULL DEFAULT 'user';
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_role_check;
ALTER TABLE users
    ADD CONSTRAINT users_role_check CHECK (role IN ('admin', 'user'));

-- The application bootstraps the admin password from ADMIN_PASSWORD, so this
-- migration deliberately does not insert a password hash.
