#!/usr/bin/env bash
# Isolated-cluster integration test. Creates fixture databases/roles only.
set -euo pipefail
[[ "${FUXI_DATABASE_PRIVILEGE_TEST:-}" == 1 ]] || {
    printf 'error: use only an isolated cluster and set FUXI_DATABASE_PRIVILEGE_TEST=1\n' >&2
    exit 2
}
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
admin_user="${PRODUCTION_TEST_PG_USER:-postgres}"
psql_admin=(psql -X -q -v ON_ERROR_STOP=1 -U "$admin_user")

"${psql_admin[@]}" -d postgres -c 'CREATE DATABASE fuxi_privilege_fresh'
POSTGRES_USER="$admin_user" POSTGRES_DB=fuxi_privilege_fresh \
    APP_DB_USER=fuxi_fresh_app APP_DB_OWNER=fuxi_fresh_owner \
    DB_PASSWORD=isolated-fixture-password "$repo_root/script/bootstrap-production-db.sh"
psql -X -q -v ON_ERROR_STOP=1 -U fuxi_fresh_app -d fuxi_privilege_fresh \
    -f "$repo_root/script/test_production_database_privileges.sql"
"${psql_admin[@]}" -d fuxi_privilege_fresh -v app_user=fuxi_fresh_app \
    -f "$repo_root/script/099_production_database_privileges.sql"
if POSTGRES_USER="$admin_user" POSTGRES_DB=fuxi_privilege_fresh \
    APP_DB_USER=fuxi_fresh_app APP_DB_OWNER=fuxi_fresh_owner \
    DB_PASSWORD=isolated-fixture-password "$repo_root/script/bootstrap-production-db.sh"; then
    printf 'error: bootstrap accepted an existing schema/roles\n' >&2; exit 1
fi
if psql -X -q -v ON_ERROR_STOP=1 -U fuxi_fresh_app -d fuxi_privilege_fresh \
    -c 'CREATE DATABASE forbidden_runtime_database'; then
    printf 'error: runtime role created a database\n' >&2; exit 1
fi

"${psql_admin[@]}" -d postgres -c 'CREATE ROLE fuxi_legacy_app LOGIN SUPERUSER; CREATE ROLE fuxi_legacy_parent NOLOGIN; GRANT fuxi_legacy_parent TO fuxi_legacy_app; CREATE ROLE fuxi_rollback_app LOGIN SUPERUSER'
"${psql_admin[@]}" -d postgres -c 'CREATE DATABASE fuxi_privilege_legacy OWNER fuxi_legacy_app'
"${psql_admin[@]}" -d postgres -c 'CREATE DATABASE fuxi_privilege_rollback OWNER fuxi_rollback_app'
for sql in init.sql 058_decision_run_partitions.sql 059_decision_run_daily_stats.sql; do
    psql -X -q -v ON_ERROR_STOP=1 -U fuxi_legacy_app -d fuxi_privilege_legacy \
        -v skip_database_bootstrap=1 -v app_user=fuxi_legacy_app -f "$repo_root/script/$sql"
done
"${psql_admin[@]}" -d fuxi_privilege_legacy -v app_user=fuxi_legacy_app -v owner_user=fuxi_legacy_owner \
    -f "$repo_root/script/prepare-production-role-separation.sql"
"${psql_admin[@]}" -d fuxi_privilege_legacy -c "DO \$\$ BEGIN
    IF NOT (SELECT rolsuper FROM pg_roles WHERE rolname='fuxi_legacy_app')
       OR (SELECT datdba FROM pg_database WHERE datname=current_database()) <> 'fuxi_legacy_app'::regrole
       OR EXISTS (SELECT 1 FROM pg_roles WHERE rolname='fuxi_legacy_owner') THEN
        RAISE EXCEPTION 'review-only changed role attributes or ownership';
    END IF; END; \$\$;"
if "${psql_admin[@]}" -d fuxi_privilege_legacy -v app_user=fuxi_legacy_app -v owner_user=fuxi_legacy_owner \
    -v apply_role_separation=1 -f "$repo_root/script/prepare-production-role-separation.sql"; then
    printf 'error: legacy role separation accepted missing confirmation\n' >&2; exit 1
fi
"${psql_admin[@]}" -d fuxi_privilege_legacy -v app_user=fuxi_legacy_app -v owner_user=fuxi_legacy_owner \
    -v apply_role_separation=1 -v confirm_role_separation=fuxi_legacy_app \
    -f "$repo_root/script/prepare-production-role-separation.sql"
psql -X -q -v ON_ERROR_STOP=1 -U fuxi_legacy_app -d fuxi_privilege_legacy \
    -f "$repo_root/script/test_production_database_privileges.sql"

# Missing application tables force 099 to fail after REASSIGN/ALTER ROLE;
# the caller's transaction must restore both ownership and role privileges.
psql -X -q -v ON_ERROR_STOP=1 -U fuxi_rollback_app -d fuxi_privilege_rollback \
    -c 'CREATE TABLE public.users (id integer)'
if "${psql_admin[@]}" -d fuxi_privilege_rollback -v app_user=fuxi_rollback_app -v owner_user=fuxi_rollback_owner \
    -v apply_role_separation=1 -v confirm_role_separation=fuxi_rollback_app \
    -f "$repo_root/script/prepare-production-role-separation.sql"; then
    printf 'error: incomplete legacy schema unexpectedly migrated\n' >&2; exit 1
fi
"${psql_admin[@]}" -d fuxi_privilege_rollback -c "DO \$\$ BEGIN
    IF NOT (SELECT rolsuper FROM pg_roles WHERE rolname='fuxi_rollback_app')
       OR (SELECT relowner FROM pg_class WHERE oid='public.users'::regclass) <> 'fuxi_rollback_app'::regrole
       OR (SELECT datdba FROM pg_database WHERE datname=current_database()) <> 'fuxi_rollback_app'::regrole
       OR EXISTS (SELECT 1 FROM pg_roles WHERE rolname='fuxi_rollback_owner') THEN
        RAISE EXCEPTION 'failed privilege migration did not roll back role/ownership changes';
    END IF; END; \$\$;"

# Newly created tables do not inherit broad runtime write privileges.
for fixture in fresh legacy; do
    fixture_owner="fuxi_${fixture}_owner"
    fixture_app="fuxi_${fixture}_app"
    "${psql_admin[@]}" -d "fuxi_privilege_$fixture" -c "SET ROLE $fixture_owner; CREATE TABLE public.future_privilege_fixture(id integer)"
    psql -X -q -v ON_ERROR_STOP=1 -U "$fixture_app" -d "fuxi_privilege_$fixture" -c "DO \$\$ BEGIN
        IF has_table_privilege(current_user, 'public.future_privilege_fixture', 'SELECT,INSERT,UPDATE,DELETE') THEN
            RAISE EXCEPTION 'future table inherited application privileges';
        END IF; END; \$\$;"
    psql -X -q -v ON_ERROR_STOP=1 -U "$fixture_app" -d "fuxi_privilege_$fixture" -At -F '|' \
        -f "$repo_root/script/check-deployment-baseline.sql" | awk -F '|' '$3 == "missing" { print; failed=1 } END { exit failed }'
done
printf 'production bootstrap, legacy review/apply, rollback and future ACL regression passed\n'
