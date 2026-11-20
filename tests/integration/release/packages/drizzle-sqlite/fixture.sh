#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

fixture_setup "drizzle-sqlite" || exit 1
fixture_compile_run_diff "drizzle-sqlite"
