#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
database_url="${DATABASE_URL:-}"
app_user="${APP_DB_USER:-}"

if [[ -z "$database_url" ]]; then
    printf 'error: DATABASE_URL must point to the target database\n' >&2
    exit 2
fi
if [[ -z "$app_user" ]]; then
    printf 'error: APP_DB_USER must name the application database role\n' >&2
    exit 2
fi

for migration in \
    086_prompt_content_digests.sql \
    087_prompt_registry.sql \
    088_prompt_evaluation_jobs.sql \
    089_prompt_evaluation_job_cancellation.sql \
    090_prompt_evaluation_job_heartbeat.sql \
    095_prompt_registry_tester_judge.sql; do
    printf 'applying %s\n' "$migration"
    psql "$database_url" -X -v ON_ERROR_STOP=1 -v "app_user=$app_user" \
        -f "$repo_root/script/$migration"
done

printf 'prompt registry migrations 086-091, 093, and 095 are ready; restart the application\n'
