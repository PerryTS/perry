#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

NAME="zod-4-6"

if [[ "${1:-}" == "--__did-skip-marker" ]]; then
    exit 1
fi

# zod 4.6.5 has a private `validateAsync` in core/schemas.ts next to the exported
# one in core/parse.ts; both reach core/index.ts through `export *`. This
# fixture pins it to guard the export-star private-name collision fix.

fixture_setup "$NAME" || exit 1
fixture_compile_run_diff "$NAME"
