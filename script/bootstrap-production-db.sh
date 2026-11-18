#!/usr/bin/env bash
# Fresh-volume bootstrap only. Never silently downgrade an existing app/owner.
set -euo pipefail

script_dir="${FUXI_DB_SCRIPT_DIR:-$(cd "$(dirname "$0")" && pwd)}"
admin_user="${POSTGRES_USER:-postgres}"
database_name="${POSTGRES_DB:-fuxi_dmn}"
application_user="${APP_DB_USER:-fuxi}"
migration_owner="${APP_DB_OWNER:-fuxi_owner}"

fail() { printf 'error: %s\n' "$1" >&2; exit 2; }
for identifier in "$admin_user" "$database_name" "$application_user" "$migration_owner"; do
    [[ "$identifier" =~ ^[a-z_][a-z0-9_]{0,62}$ ]] || fail 'database/role names must be lowercase PostgreSQL identifiers'
done
[[ "$admin_user" != "$application_user" && "$migration_owner" != "$application_user" && "$migration_owner" != "$admin_user" ]] \
    || fail 'administrator, migration owner and application role must be distinct'
[[ -n "${DB_PASSWORD:-}" ]] || fail 'DB_PASSWORD must contain the application database password'
[[ "${#DB_PASSWORD}" -ge 16 ]] || fail 'DB_PASSWORD must contain at least 16 characters'
command -v psql >/dev/null 2>&1 || fail 'psql is required'
for file in init.sql 058_decision_run_partitions.sql 059_decision_run_daily_stats.sql 099_production_database_privileges.sql; do
    [[ -r "$script_dir/$file" ]] || fail "missing bootstrap SQL: $file"
done

# The official postgres entrypoint already creates POSTGRES_DB. SQL values use
# psql identifier/literal quoting; the password is read from env, never argv.
psql -X -q --username "$admin_user" --dbname "$database_name" -v ON_ERROR_STOP=1 \
    -v "app_user=$application_user" -v "owner_user=$migration_owner" <<'SQL'
SELECT set_config('fuxi.bootstrap_app_user', :'app_user', FALSE);
SELECT set_config('fuxi.bootstrap_owner_user', :'owner_user', FALSE);
DO $$
BEGIN
    IF NOT (SELECT rolsuper FROM pg_roles WHERE rolname = current_user) THEN
        RAISE EXCEPTION 'fresh bootstrap requires an independent database administrator';
    END IF;
    IF to_regclass('public.users') IS NOT NULL OR to_regclass('public.decision_projects') IS NOT NULL THEN
        RAISE EXCEPTION 'existing application schema detected; fresh bootstrap will not reinitialize it; review the legacy role-separation plan';
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname IN (
        current_setting('fuxi.bootstrap_app_user'), current_setting('fuxi.bootstrap_owner_user'))) THEN
        RAISE EXCEPTION 'application/owner role already exists; fresh bootstrap never silently reuses or downgrades existing roles';
    END IF;
END;
$$;
\getenv app_password DB_PASSWORD
SELECT format('CREATE ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS', :'owner_user') \gexec
SELECT format('CREATE ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L', :'app_user', :'app_password') \gexec
SELECT format('ALTER DATABASE %I OWNER TO %I', current_database(), :'owner_user') \gexec
ALTER SCHEMA public OWNER TO :"owner_user";
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS vector;
EXCEPTION WHEN undefined_file OR feature_not_supported THEN
    RAISE NOTICE 'pgvector unavailable; keyword retrieval remains supported';
END;
$$;
SQL

psql -X -q --username "$admin_user" --dbname "$database_name" -v ON_ERROR_STOP=1 \
    -v "app_user=$application_user" -v "owner_user=$migration_owner" \
    -v skip_database_bootstrap=1 -v "init_sql=$script_dir/init.sql" \
    -v "partition_sql=$script_dir/058_decision_run_partitions.sql" \
    -v "stats_sql=$script_dir/059_decision_run_daily_stats.sql" \
    -v "privileges_sql=$script_dir/099_production_database_privileges.sql" <<'SQL'
SET ROLE :"owner_user";
\i :init_sql
\i :partition_sql
\i :stats_sql
\i :privileges_sql
RESET ROLE;
SQL
printf 'database initialized with separate administrator, non-login owner and restricted application role\n'
