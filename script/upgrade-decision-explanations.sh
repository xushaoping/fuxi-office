#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
database_url="${DATABASE_URL:-}"
app_user="${APP_DB_USER:-}"
db_user="${POSTGRES_USER:-fuxi}"
db_name="${POSTGRES_DB:-fuxi_dmn}"
compose_mode=0
dry_run=0
check_only=0
env_file=""

usage() {
    cat <<'HELP'
Usage: upgrade-decision-explanations.sh [--docker-compose [--env-file PATH]] [--dry-run | --check]

Default: use local psql with DATABASE_URL and APP_DB_USER.
Docker:  use the repository's docker-compose.yml postgres service; POSTGRES_USER
         and POSTGRES_DB default to fuxi / fuxi_dmn. APP_DB_USER defaults to
         POSTGRES_USER. --env-file defaults to the repository's .env.

--dry-run  Print the migration plan without connecting to any database.
--check    Read-only check of prerequisites and installed explanation columns.

Prerequisites: application base schema (including Agent runs), Prompt Registry
087/088 and its foundation. Upgrade Prompt Registry first if it is absent.
Applies 092, 094, cumulative prompt constraint 095, 096, and 097; does not replay
the narrower 091/093 prompt constraints. Run with a database migration role.
The application must be restarted after a successful upgrade.
HELP
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 2
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --docker-compose)
            [[ "$compose_mode" == 0 ]] || fail 'duplicate --docker-compose'
            compose_mode=1
            shift
            ;;
        --env-file)
            [[ -z "$env_file" ]] || fail 'duplicate --env-file'
            [[ $# -ge 2 && -n "$2" && "$2" != --* ]] || fail '--env-file requires a path'
            env_file="$2"
            shift 2
            ;;
        --dry-run)
            [[ "$dry_run" == 0 ]] || fail 'duplicate --dry-run'
            dry_run=1
            shift
            ;;
        --check)
            [[ "$check_only" == 0 ]] || fail 'duplicate --check'
            check_only=1
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *) fail 'unknown argument; use --help' ;;
    esac
done
[[ "$dry_run" == 0 || "$check_only" == 0 ]] || fail '--dry-run and --check are mutually exclusive'
[[ "$compose_mode" == 1 || -z "$env_file" ]] || fail '--env-file requires --docker-compose'

migrations=(
    092_decision_run_explanations.sql
    094_decision_run_explanation_reviews.sql
    095_prompt_registry_tester_judge.sql
    096_decision_explanation_attempt_ownership.sql
    097_decision_explanation_review_revisions.sql
)
for migration in "${migrations[@]}"; do
    [[ -f "$repo_root/script/$migration" ]] || fail "missing migration file: $migration"
done

if [[ "$dry_run" == 1 ]]; then
    printf 'dry-run: no connection or migration will be made\n'
    printf 'prerequisites: application base schema and Prompt Registry 087/088\n'
    printf 'planned migration: %s\n' "${migrations[@]}"
    exit 0
fi

if [[ "$compose_mode" == 1 ]]; then
    env_file="${env_file:-$repo_root/.env}"
    [[ -f "$env_file" ]] || fail 'Docker Compose env file does not exist'
    app_user="${app_user:-$db_user}"
    command -v docker >/dev/null 2>&1 || fail 'docker is not available'
else
    [[ -n "$database_url" ]] || fail 'DATABASE_URL must point to the target database'
    command -v psql >/dev/null 2>&1 || fail 'psql is not available'
fi
[[ -n "$app_user" ]] || fail 'APP_DB_USER must name the application database role'

run_psql() {
    if [[ "$compose_mode" == 1 ]]; then
        docker compose --env-file "$env_file" -f "$repo_root/docker-compose.yml" \
            exec -T postgres psql -U "$db_user" -d "$db_name" \
            -X -v ON_ERROR_STOP=1 -v "app_user=$app_user" "$@"
    else
        # libpq parses URI options only from an explicit connection argument;
        # PGDATABASE alone does not reliably apply URI host/port parameters.
        # Follow the existing upgrade helper convention without printing the URI.
        psql --dbname="$database_url" -X -v ON_ERROR_STOP=1 \
            -v "app_user=$app_user" "$@"
    fi
}

check_schema() {
    local installed="$1"
    local missing
    if ! missing="$(run_psql -qAt -v "check_installed=$installed" -f - <<'SQL'
BEGIN READ ONLY;
WITH required_tables(name) AS (VALUES
    ('users'), ('decision_runs'), ('agent_runs'),
    ('prompt_assets'), ('prompt_asset_events'), ('prompt_evaluation_jobs')
), required_columns(table_name, column_name) AS (VALUES
    ('users', 'username'),
    ('decision_runs', 'id'), ('decision_runs', 'project_id'),
    ('agent_runs', 'id'), ('agent_runs', 'project_id'),
    ('agent_runs', 'stage'), ('agent_runs', 'input_json'),
    ('prompt_assets', 'prompt_key')
), installed_columns(table_name, column_name) AS (VALUES
    ('decision_run_explanations', 'decision_run_id'),
    ('decision_run_explanations', 'status'),
    ('decision_run_explanations', 'explanation_json'),
    ('decision_run_explanations', 'agent_run_id'),
    ('decision_run_explanations', 'generation_revision'),
    ('decision_run_explanation_reviews', 'explanation_id'),
    ('decision_run_explanation_reviews', 'explanation_revision')
)
SELECT 'missing prerequisite table: ' || name
FROM required_tables WHERE to_regclass(format('public.%I', name)) IS NULL
UNION ALL
SELECT 'missing prerequisite column: ' || table_name || '.' || column_name
FROM required_columns
WHERE to_regclass(format('public.%I', table_name)) IS NOT NULL
  AND NOT EXISTS (
    SELECT 1 FROM pg_attribute
    WHERE attrelid = to_regclass(format('public.%I', table_name))
      AND attname = column_name AND NOT attisdropped)
UNION ALL
SELECT 'missing application database role'
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'app_user')
UNION ALL
SELECT 'missing installed column: ' || table_name || '.' || column_name
FROM installed_columns
WHERE :'check_installed' = '1' AND NOT EXISTS (
    SELECT 1 FROM pg_attribute
    WHERE attrelid = to_regclass(format('public.%I', table_name))
      AND attname = column_name AND NOT attisdropped);
COMMIT;
SQL
    )"; then
        printf 'error: database schema check failed\n' >&2
        return 1
    fi
    if [[ -n "$missing" ]]; then
        printf '%s\n' "$missing" >&2
        printf 'error: prerequisites or installed schema are incomplete; for missing Prompt Registry, run script/upgrade-prompt-registry.sh first\n' >&2
        return 1
    fi
}

if [[ "$check_only" == 1 ]]; then
    check_schema 1
    printf 'decision explanation schema check passed (read-only)\n'
    exit 0
fi

# Check all prerequisites before the first migration, avoiding a partial upgrade
# caused by discovering a missing Prompt Registry only at migration 095.
check_schema 0
for migration in "${migrations[@]}"; do
    printf 'applying %s\n' "$migration"
    # Read on the host: Docker mode does not require mounting the repository.
    run_psql -f - < "$repo_root/script/$migration"
done
check_schema 1
printf 'decision explanation migrations 092, 094-097 are ready; restart the application\n'
