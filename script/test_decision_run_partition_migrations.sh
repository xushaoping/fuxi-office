#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$repo_root/target/test-results"
exec > >(tee "$repo_root/target/test-results/database-regression.log") 2>&1
pg_bin_dir="${PG_BIN_DIR:-$(pg_config --bindir)}"
test_root="$(mktemp -d /tmp/fuxi-dmn-partition-test.XXXXXX)"
data_dir="$test_root/data"
port=$((55000 + RANDOM % 10000))

cleanup() {
    "$pg_bin_dir/pg_ctl" -D "$data_dir" -m immediate stop >/dev/null 2>&1 || true
    rm -rf "$test_root"
}
trap cleanup EXIT

"$pg_bin_dir/initdb" -D "$data_dir" -A trust -U postgres --no-sync \
    -c shared_memory_type=mmap -c dynamic_shared_memory_type=mmap
"$pg_bin_dir/pg_ctl" -D "$data_dir" \
    -l "$test_root/postgres.log" \
    -o "-p $port -k /tmp -c listen_addresses='' -c shared_memory_type=mmap -c dynamic_shared_memory_type=mmap" \
    start

cd "$repo_root"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d postgres \
    -c "CREATE ROLE fuxi LOGIN; CREATE ROLE fuxi_migration_test LOGIN; GRANT CREATE ON SCHEMA public TO fuxi_migration_test;"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres -v app_user=fuxi -f script/test_decision_run_partition_migrations.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" -U fuxi -d postgres \
    -c "INSERT INTO decision_run_explanation_reviews (explanation_id, reviewer_username, review_label) SELECT id, 'reviewer-test', 'readable' FROM decision_run_explanations WHERE decision_run_id = 1"
if "$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" -U fuxi -d postgres \
    -c "UPDATE decision_run_explanation_reviews SET review_label = 'needs_revision'"; then
    echo "application role was able to mutate append-only explanation reviews" >&2
    exit 1
fi

"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres \
    -c "UPDATE decision_runs SET duration_ms = duration_ms + 1 WHERE id = 1"
if "$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v lookback_days=3660 \
    -h /tmp -p "$port" -U fuxi_migration_test -d postgres \
    -f script/reconcile-decision-run-daily-stats.sql; then
    echo "daily stats reconciliation accepted a stale materialized view" >&2
    exit 1
fi
if "$pg_bin_dir/psql" -v ON_ERROR_STOP=1 \
    -v start_at='2019-03-01 00:00:00+00' \
    -v end_at='2026-10-16 00:00:00+00' \
    -h /tmp -p "$port" -U fuxi_migration_test -d postgres \
    -f script/reconcile-decision-run-partitions.sql; then
    echo "partition reconciliation accepted a stale daily aggregate" >&2
    exit 1
fi

"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres \
    -c "REFRESH MATERIALIZED VIEW decision_run_daily_stats"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v lookback_days=3660 \
    -h /tmp -p "$port" -U fuxi_migration_test -d postgres \
    -f script/reconcile-decision-run-daily-stats.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres \
    -c "DELETE FROM decision_runs_partitioned WHERE id = 2"
if "$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v lookback_days=3660 \
    -h /tmp -p "$port" -U fuxi_migration_test -d postgres \
    -f script/reconcile-decision-run-daily-stats.sql; then
    echo "daily stats reconciliation accepted an orphan materialized row" >&2
    exit 1
fi
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres \
    -c "INSERT INTO decision_runs_partitioned SELECT * FROM decision_runs WHERE id = 2"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres \
    -c "INSERT INTO decision_runs (id, project_id, version_id, service_id, service_name, input_json, output_json, execution_trace_json, duration_ms, status, executed_by, executed_at) VALUES (6, 10, 20, 'svc', 'service', '{}'::JSONB, '{}'::JSONB, '{}'::JSONB, 2, 'succeeded', 'test', NOW() - INTERVAL '1 day')"
if "$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v lookback_days=30 \
    -h /tmp -p "$port" -U fuxi_migration_test -d postgres \
    -f script/reconcile-decision-run-daily-stats.sql; then
    echo "daily stats reconciliation accepted a missing materialized row" >&2
    exit 1
fi
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres \
    -c "REFRESH MATERIALIZED VIEW decision_run_daily_stats"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v lookback_days=30 \
    -h /tmp -p "$port" -U fuxi_migration_test -d postgres \
    -f script/reconcile-decision-run-daily-stats.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres \
    -c "DELETE FROM decision_runs WHERE id = 6"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres \
    -c "REFRESH MATERIALIZED VIEW decision_run_daily_stats"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U fuxi_migration_test -d postgres \
    -c "UPDATE decision_runs_partitioned SET output_json = '{}'::JSONB WHERE id = 1"
if "$pg_bin_dir/psql" -v ON_ERROR_STOP=1 \
    -v start_at='2019-03-01 00:00:00+00' \
    -v end_at='2026-10-16 00:00:00+00' \
    -h /tmp -p "$port" -U fuxi_migration_test -d postgres \
    -f script/reconcile-decision-run-partitions.sql; then
    echo "partition reconciliation accepted a deliberately corrupted copy" >&2
    exit 1
fi

"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d postgres \
    -f script/init.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/101_agent_orchestration_sse_events.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/101_agent_orchestration_sse_events.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -c "ALTER TABLE agent_runs DROP COLUMN prompt_digest; ALTER TABLE agent_steps DROP COLUMN prompt_digest"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/086_prompt_content_digests.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/087_prompt_registry.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/088_prompt_evaluation_jobs.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/089_prompt_evaluation_job_cancellation.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/089_prompt_evaluation_job_cancellation.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/090_prompt_evaluation_job_heartbeat.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/091_prompt_registry_optimization_proposal.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/093_prompt_registry_decision_explanation.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/095_prompt_registry_tester_judge.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/094_decision_run_explanation_reviews.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -c "ALTER TABLE decision_run_explanations DROP COLUMN agent_run_id"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/test_decision_explanation_attempt_ownership.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -c "DROP TRIGGER decision_run_explanation_review_revision_guard ON decision_run_explanation_reviews; ALTER TABLE decision_run_explanations DROP COLUMN generation_revision; ALTER TABLE decision_run_explanation_reviews DROP COLUMN explanation_revision"
if PATH="$pg_bin_dir:$PATH" DATABASE_URL="postgresql://postgres@localhost:$port/fuxi_dmn?host=/tmp" APP_DB_USER=fuxi \
    ./script/upgrade-decision-explanations.sh --check; then
    echo "explanation upgrade check accepted missing revision columns" >&2
    exit 1
fi
PATH="$pg_bin_dir:$PATH" DATABASE_URL="postgresql://postgres@localhost:$port/fuxi_dmn?host=/tmp" APP_DB_USER=fuxi \
    ./script/upgrade-decision-explanations.sh
PATH="$pg_bin_dir:$PATH" DATABASE_URL="postgresql://postgres@localhost:$port/fuxi_dmn?host=/tmp" APP_DB_USER=fuxi \
    ./script/upgrade-decision-explanations.sh
PATH="$pg_bin_dir:$PATH" DATABASE_URL="postgresql://postgres@localhost:$port/fuxi_dmn?host=/tmp" APP_DB_USER=fuxi \
    ./script/upgrade-decision-explanations.sh --check
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -f script/test_decision_explanation_review_revisions.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/094_decision_run_explanation_reviews.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/093_prompt_registry_decision_explanation.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/095_prompt_registry_tester_judge.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/090_prompt_evaluation_job_heartbeat.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -f script/test_prompt_evaluation_jobs.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/088_prompt_evaluation_jobs.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -c "INSERT INTO prompt_assets (prompt_key, version, content, content_digest, created_by) VALUES ('dsl_generation', 'registry-test-v1', 'prompt body', repeat('a', 64), 'admin'), ('optimization_proposal', 'registry-opt-v1', 'optimization prompt', repeat('b', 64), 'admin'), ('decision_explanation', 'registry-explanation-v1', 'explanation prompt', repeat('c', 64), 'admin'); INSERT INTO prompt_asset_events (prompt_asset_id, event_type, actor_username) SELECT id, 'draft_created', 'admin' FROM prompt_assets WHERE version IN ('registry-test-v1', 'registry-opt-v1', 'registry-explanation-v1')"
if "$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -c "UPDATE prompt_assets SET content = 'tampered' WHERE version = 'registry-test-v1'"; then
    echo "prompt registry allowed mutation of immutable prompt content" >&2
    exit 1
fi
if "$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -c "DELETE FROM prompt_asset_events WHERE event_type = 'draft_created'"; then
    echo "prompt registry allowed mutation of append-only audit events" >&2
    exit 1
fi
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/087_prompt_registry.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/086_prompt_content_digests.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/058_decision_run_partitions.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/059_decision_run_daily_stats.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/test_init_autodraft_shadow_permissions.sql
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -v app_user=fuxi \
    -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    -f script/099_production_database_privileges.sql
DATABASE_URL="postgresql://fuxi@localhost:$port/fuxi_dmn?host=/tmp" \
    ./script/check-deployment-baseline.sh
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -c "REVOKE UPDATE ON TABLE decision_drift_events FROM fuxi"
if DATABASE_URL="postgresql://fuxi@localhost:$port/fuxi_dmn?host=/tmp" \
    ./script/check-deployment-baseline.sh; then
    echo "deployment baseline did not detect missing drift event permission" >&2
    exit 1
fi
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" \
    -U postgres -d fuxi_dmn \
    -c "GRANT UPDATE ON TABLE decision_drift_events TO fuxi"
TEST_DATABASE_URL="postgresql://fuxi@localhost:$port/fuxi_dmn?host=/tmp" \
    cargo test --locked --bin fuxi-dmn prompt_registry::tests::stale_evaluation_recovery_marks_orphans_without_touching_live_jobs -- --ignored --nocapture
TEST_DATABASE_URL="postgresql://fuxi@localhost:$port/fuxi_dmn?host=/tmp" \
    cargo test --locked --bin fuxi-dmn tester_judge_postgres_heartbeat_and_terminal_state_guards -- --ignored --nocapture
TEST_DATABASE_URL="postgresql://fuxi@localhost:$port/fuxi_dmn?host=/tmp" \
    cargo test --locked --bin fuxi-dmn decision_explanation_postgres -- --ignored --nocapture
TEST_DATABASE_URL="postgresql://fuxi@localhost:$port/fuxi_dmn?host=/tmp" \
    cargo test --locked --bin fuxi-dmn deviation_inventory_postgres -- --ignored --nocapture

# The production gate is exercised through both the psql tool and the runtime
# query, under the same restricted role. Empty schemas must fail readiness.
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" -U postgres -d postgres \
    -c "CREATE DATABASE fuxi_incomplete_readiness"
HEALTH_TEST_DATABASE_URL="postgresql://fuxi@localhost:$port/fuxi_dmn?host=/tmp" \
    cargo test --locked --bin fuxi-dmn health::tests::deployment_baseline_accepts_migrated_runtime_database -- --ignored --nocapture
HEALTH_INCOMPLETE_TEST_DATABASE_URL="postgresql://postgres@localhost:$port/fuxi_incomplete_readiness?host=/tmp" \
    cargo test --locked --bin fuxi-dmn health::tests::deployment_baseline_rejects_incomplete_database -- --ignored --nocapture
cargo test --locked --bin fuxi-dmn startup_tests::graceful_shutdown -- --ignored --nocapture
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 \
    -h /tmp -p "$port" -U fuxi -d fuxi_dmn \
    -f script/test_production_database_privileges.sql
FUXI_DATABASE_PRIVILEGE_TEST=1 PGHOST=/tmp PGPORT="$port" PRODUCTION_TEST_PG_USER=postgres \
    PATH="$pg_bin_dir:$PATH" script/test_production_database_bootstrap.sh

# Restore actual fixture data, ownership and ACLs into a new disposable database.
# This records local restore behavior only, not a production RPO/RTO guarantee.
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" -U fuxi -d fuxi_dmn \
    -c "SELECT public.refresh_decision_run_daily_stats()"
"$pg_bin_dir/psql" -X -qAt -v ON_ERROR_STOP=1 -h /tmp -p "$port" -U fuxi -d fuxi_dmn \
    -f script/test_backup_fingerprint.sql > "$test_root/before-restore.tsv"
"$pg_bin_dir/pg_dump" -h /tmp -p "$port" -U postgres -d fuxi_dmn \
    --format=custom --file="$test_root/fixture-backup.dump"
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" -U postgres -d postgres \
    -c "CREATE DATABASE fuxi_restore_verification TEMPLATE template0"
# Restoring to a different database name does not replay the source DATABASE
# object's ACL. Establish that boundary explicitly before restoring its contents.
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 -h /tmp -p "$port" -U postgres -d postgres \
    -c "REVOKE ALL ON DATABASE fuxi_restore_verification FROM PUBLIC; GRANT CONNECT ON DATABASE fuxi_restore_verification TO fuxi"
restore_start=$SECONDS
"$pg_bin_dir/pg_restore" -h /tmp -p "$port" -U postgres -d fuxi_restore_verification \
    --exit-on-error --single-transaction "$test_root/fixture-backup.dump"
restore_seconds=$((SECONDS - restore_start))
"$pg_bin_dir/psql" -X -qAt -v ON_ERROR_STOP=1 -h /tmp -p "$port" -U fuxi -d fuxi_restore_verification \
    -f script/test_backup_fingerprint.sql > "$test_root/after-restore.tsv"
diff -u "$test_root/before-restore.tsv" "$test_root/after-restore.tsv"
DATABASE_URL="postgresql://fuxi@localhost:$port/fuxi_restore_verification?host=/tmp" \
    ./script/check-deployment-baseline.sh
"$pg_bin_dir/psql" -v ON_ERROR_STOP=1 \
    -h /tmp -p "$port" -U fuxi -d fuxi_restore_verification \
    -f script/test_production_database_privileges.sql
printf 'Isolated backup/restore passed: all fixture table counts/digests and runtime privileges match; restore %ss (local fixture only).\n' "$restore_seconds"
