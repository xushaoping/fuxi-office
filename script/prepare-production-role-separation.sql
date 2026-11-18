\set ON_ERROR_STOP on
-- Review-only by default. Run using an independent administrator, never the
-- application's old superuser session. Take and verify a backup first.
\if :{?app_user}
\else
\echo 'error: pass -v app_user=<legacy application role>'
DO $$ BEGIN RAISE EXCEPTION 'migration precondition failed; see the preceding error'; END; $$;
\endif
\if :{?owner_user}
\else
\set owner_user fuxi_owner
\endif
\if :{?apply_role_separation}
\else
\set apply_role_separation 0
\endif
SELECT :'apply_role_separation' IN ('0', '1') AS apply_option_valid,
       :'apply_role_separation' = '1' AS apply_role_changes \gset
\if :apply_option_valid
\else
\echo 'error: apply_role_separation must be 0 or 1'
DO $$ BEGIN RAISE EXCEPTION 'migration precondition failed; see the preceding error'; END; $$;
\endif

SELECT set_config('fuxi.legacy_app_user', :'app_user', FALSE);
SELECT set_config('fuxi.legacy_owner_user', :'owner_user', FALSE);
DO $$
DECLARE
    app_oid OID;
BEGIN
    IF current_user = current_setting('fuxi.legacy_app_user')
       OR session_user = current_setting('fuxi.legacy_app_user')
       OR NOT (SELECT rolsuper FROM pg_roles WHERE rolname = current_user) THEN
        RAISE EXCEPTION 'use a distinct superuser administrator; never downgrade the connection used for this migration';
    END IF;
    SELECT oid INTO app_oid FROM pg_roles WHERE rolname = current_setting('fuxi.legacy_app_user');
    IF app_oid IS NULL OR current_setting('fuxi.legacy_owner_user') IN (current_user, current_setting('fuxi.legacy_app_user')) THEN
        RAISE EXCEPTION 'application role must exist and owner, app and administrator must be distinct';
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = current_setting('fuxi.legacy_owner_user')
               AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)) THEN
        RAISE EXCEPTION 'target owner must be a non-login restricted ownership role';
    END IF;
    IF EXISTS (SELECT 1 FROM pg_shdepend
               WHERE refclassid = 'pg_authid'::regclass AND refobjid = app_oid AND deptype = 'o'
                 AND dbid NOT IN (0, (SELECT oid FROM pg_database WHERE datname = current_database()))) THEN
        RAISE EXCEPTION 'legacy app owns objects in other databases; migrate to a dedicated cluster or review all databases before role separation';
    END IF;
END;
$$;

\echo 'REVIEW: ownership transfer covers this database and shared objects owned by the legacy role.'
\echo 'REVIEW: the following databases currently owned by that role will transfer too:'
SELECT datname FROM pg_database WHERE datdba = (SELECT oid FROM pg_roles WHERE rolname = :'app_user') ORDER BY datname;
\echo 'REVIEW: all application role memberships will be revoked:'
SELECT parent.rolname FROM pg_auth_members membership
JOIN pg_roles parent ON parent.oid = membership.roleid
WHERE membership.member = (SELECT oid FROM pg_roles WHERE rolname = :'app_user') ORDER BY parent.rolname;
\echo 'REVIEW: planned SQL (not executed unless explicitly confirmed):'
SELECT format('CREATE ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;', :'owner_user')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'owner_user');
SELECT format('REASSIGN OWNED BY %I TO %I;', :'app_user', :'owner_user');
SELECT format('GRANT ALL ON ALL TABLES IN SCHEMA public TO %I;', :'owner_user');
SELECT format('GRANT ALL ON ALL SEQUENCES IN SCHEMA public TO %I;', :'owner_user');
SELECT format('REVOKE %I FROM %I;', parent.rolname, :'app_user')
FROM pg_auth_members membership JOIN pg_roles parent ON parent.oid = membership.roleid
WHERE membership.member = (SELECT oid FROM pg_roles WHERE rolname = :'app_user');
SELECT format('ALTER ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS NOINHERIT;', :'app_user');
\echo 'REVIEW: 099_production_database_privileges.sql follows in the same transaction.'

\if :apply_role_changes
\if :{?confirm_role_separation}
SELECT :'confirm_role_separation' = :'app_user' AS confirmation_matches \gset
\else
\set confirmation_matches false
\endif
\if :confirmation_matches
\else
\echo 'error: applying requires -v confirm_role_separation=<exact legacy app role name>'
DO $$ BEGIN RAISE EXCEPTION 'migration precondition failed; see the preceding error'; END; $$;
\endif
BEGIN;
SELECT format('CREATE ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS', :'owner_user')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'owner_user') \gexec
REASSIGN OWNED BY :"app_user" TO :"owner_user";
-- Legacy init may have revoked UPDATE/DELETE from the application while it
-- still owned append-only tables. REASSIGN preserves those self-revocations;
-- restore the trusted owner's privileges so RI checks / owner maintenance
-- still work. Runtime audit restrictions are applied independently by 099.
GRANT ALL ON ALL TABLES IN SCHEMA public TO :"owner_user";
GRANT ALL ON ALL SEQUENCES IN SCHEMA public TO :"owner_user";
SELECT format('REVOKE %I FROM %I', parent.rolname, :'app_user')
FROM pg_auth_members membership JOIN pg_roles parent ON parent.oid = membership.roleid
WHERE membership.member = (SELECT oid FROM pg_roles WHERE rolname = :'app_user') \gexec
ALTER ROLE :"app_user" LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS NOINHERIT;
\set privilege_transaction_open 1
\ir 099_production_database_privileges.sql
COMMIT;
\echo 'role separation applied; run the application-role deployment baseline before restarting the application'
\else
\echo 'review-only: no ownership, role attributes or privileges were changed'
\endif
