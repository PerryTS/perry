#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

# Real-MySQL fixture: needs a running mysqld on 127.0.0.1:3306 reachable
# as root without a password, with a `perry_drizzle_test` database. Skip
# (rather than fail) if MySQL isn't available — CI wiring is tracked in
# #804.
if ! command -v mysql >/dev/null 2>&1 || ! mysql -h 127.0.0.1 -u root -e "SELECT 1" >/dev/null 2>&1; then
    fixture_skip "drizzle-mysql" "no MySQL on 127.0.0.1:3306 (see #804 for CI wiring)"
fi
mysql -h 127.0.0.1 -u root -e "CREATE DATABASE IF NOT EXISTS perry_drizzle_test" >/dev/null 2>&1 || \
    fixture_skip "drizzle-mysql" "cannot create perry_drizzle_test database"

fixture_setup "drizzle-mysql" || exit 1
fixture_compile_run_diff "drizzle-mysql"
