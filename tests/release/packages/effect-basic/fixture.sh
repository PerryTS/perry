#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

NAME="effect-basic"

if [[ "${1:-}" == "--__did-skip-marker" ]]; then
    exit 1
fi

# #5890: this fixture now compiles and runs clean on `main`, so it runs by
# default like the other tier-3 package smokes (drizzle-mysql, ink-link)
# instead of skipping behind an opt-in flag. The CI job stays advisory
# (`continue-on-error: true`) per the tier-3 convention. The historical
# `PERRY_EFFECT_BASIC_ADVISORY=1` gate (added in #4391 when Effect reliably
# failed) is gone; the env var is now a harmless no-op.

fixture_setup "$NAME" || exit 1
fixture_compile_run_diff "$NAME"
