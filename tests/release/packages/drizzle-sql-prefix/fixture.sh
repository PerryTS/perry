#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
source ../_fixture_lib.sh

# No database or mysql2 connection is needed. Keep the package version pinned
# to the production report; compiler/runtime linking remains the harness's job.
fixture_setup "drizzle-sql-prefix"
export PERRY_SQL_PREFIX_ITERATIONS=1000
fixture_compile_run_diff "drizzle-sql-prefix"
