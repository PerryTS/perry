#!/usr/bin/env bash
# parity_status.sh — print Perry's overall parity rate against
# `node --experimental-strip-types` across every node-suite module.
#
# Discovers all directories under test-parity/node-suite/ and runs the
# parity harness per module (via run_module_parity.sh which is itself a
# thin wrapper over run_parity_tests.sh). Prints a per-module breakdown
# plus a grand total + percentage.
#
# Usage:
#   ./parity_status.sh             # all node-suite modules
#   ./parity_status.sh os url      # arbitrary list (override discovery)
#
# A clean run is the source of truth for "how close is Perry to Node
# parity?" — every failing test is a known issue (the harness's own gate
# breaks CI on untracked failures, so the failure count reflects real
# remaining gaps).

set -uo pipefail
cd "$(dirname "$0")"

modules=("$@")
if (( ${#modules[@]} == 0 )); then
    while IFS= read -r d; do
        [ -d "$d" ] || continue
        m=$(basename "$d")
        # Only directories that actually contain .ts cases.
        if compgen -G "$d"'**/*.ts' > /dev/null 2>&1 || compgen -G "$d"'*.ts' > /dev/null 2>&1; then
            modules+=("$m")
        fi
    done < <(printf '%s\n' test-parity/node-suite/*/)
fi

if (( ${#modules[@]} == 0 )); then
    echo "No node-suite modules with tests found under test-parity/node-suite/." >&2
    exit 1
fi

# Defer to run_module_parity.sh which already prints a per-module +
# total table; it's the canonical front-end for batching the harness
# across modules.
exec ./run_module_parity.sh "${modules[@]}"
