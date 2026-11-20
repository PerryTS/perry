#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

# Real-MySQL fixture (#9356): needs a running mysqld on 127.0.0.1:3306
# reachable as root without a password (override with
# PERRY_FIXTURE_MYSQL_USER / PERRY_FIXTURE_MYSQL_PASSWORD), with a
# `perry_drizzle_test` database. Skip (rather than fail) if MySQL isn't
# available — CI wiring is tracked in #804.
MYSQL_USER="${PERRY_FIXTURE_MYSQL_USER:-root}"
MYSQL_ARGS=(-h 127.0.0.1 -u "$MYSQL_USER")
if [[ -n "${PERRY_FIXTURE_MYSQL_PASSWORD:-}" ]]; then
    MYSQL_ARGS+=(-p"$PERRY_FIXTURE_MYSQL_PASSWORD")
fi
if ! command -v mysql >/dev/null 2>&1 || ! mysql "${MYSQL_ARGS[@]}" -e "SELECT 1" >/dev/null 2>&1; then
    fixture_skip "drizzle-mysql2-tx" "no MySQL on 127.0.0.1:3306 (see #804 for CI wiring)"
fi
mysql "${MYSQL_ARGS[@]}" -e "CREATE DATABASE IF NOT EXISTS perry_drizzle_test" >/dev/null 2>&1 || \
    fixture_skip "drizzle-mysql2-tx" "cannot create perry_drizzle_test database"

# A 1 MB nursery makes the copying minor fire every couple of transactions,
# so a promise that is not safe to hold across a young collection (the
# #9356 defect) fails within the first few iterations instead of at ~195.
export PERRY_GC_SCAVENGE_NURSERY_MB=1

fixture_setup "drizzle-mysql2-tx" || exit 1
fixture_compile_run_diff "drizzle-mysql2-tx"
