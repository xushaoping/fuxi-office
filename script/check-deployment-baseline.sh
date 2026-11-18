#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
database_url="${DATABASE_URL:-}"

if [[ -z "$database_url" ]]; then
    printf 'error: DATABASE_URL must point to the target database\n' >&2
    exit 2
fi

psql "$database_url" -X -q -v ON_ERROR_STOP=1 -At -F '|' \
    -f "$repo_root/script/check-deployment-baseline.sql" \
    | awk -F '|' '
        NF < 3 { next }
        $3 == "missing" {
            printf "missing: %s (%s) - %s\n", $1, $2, $4
            failed = 1
        }
        $3 != "missing" {
            printf "%s: %s (%s)\n", $3, $1, $2
        }
        END {
            if (failed) {
                print "deployment baseline check failed" > "/dev/stderr"
                exit 1
            }
            print "deployment baseline check passed"
        }
    '
