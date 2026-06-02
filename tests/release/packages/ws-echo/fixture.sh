#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

fixture_setup "ws-echo" || exit 1
fixture_compile_run_diff "ws-echo"
