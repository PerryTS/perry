#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

NAME="effect-basic"

if [[ "${1:-}" == "--__did-skip-marker" ]]; then
    exit 1
fi

if [[ "${PERRY_EFFECT_BASIC_ADVISORY:-0}" != "1" ]]; then
    fixture_skip "$NAME" "advisory #802 signal; set PERRY_EFFECT_BASIC_ADVISORY=1 to compile/run"
fi

fixture_setup "$NAME" || exit 1
fixture_compile_run_diff "$NAME"
