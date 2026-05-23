#!/usr/bin/env bash
# run_module_parity.sh — run the `node-suite` parity harness for a
# configurable list of modules and print a combined summary.
#
# Defaults to the modules we authored (process, perf_hooks); pass module
# names on the command line to override.
#
# Usage:
#   ./run_module_parity.sh                       # default: process + perf_hooks
#   ./run_module_parity.sh process               # single module
#   ./run_module_parity.sh process perf_hooks os # arbitrary list
#
# Each module is run via `./run_parity_tests.sh --suite node-suite
# --module <m>`. Per-module Parity Pass / Fail / Compile-Fail counts are
# parsed from the harness output and aggregated into a final table.

set -euo pipefail
cd "$(dirname "$0")"

modules=("$@")
if (( ${#modules[@]} == 0 )); then
    modules=(process perf_hooks)
fi

# Strip ANSI color escapes from harness output so we can grep cleanly.
strip_ansi() {
    sed -E $'s/\x1B\\[[0-9;]*[a-zA-Z]//g'
}

# Parse a single "Parity Pass: N" / "Parity Fail: N" / "Compile Fail: N"
# line into a bare integer; default to 0 when absent.
extract_count() {
    local label="$1" text="$2"
    awk -v L="$label" -F': *' '
        $1 ~ L {
            n = $2 + 0
            print n
            found = 1
            exit
        }
        END { if (!found) print 0 }
    ' <<<"$text"
}

declare -a rows
total_pass=0
total_fail=0
total_cf=0
have_failures=0

for m in "${modules[@]}"; do
    if [[ ! -d "test-parity/node-suite/$m" ]]; then
        rows+=("$(printf '  %-14s %s' "$m" "(not present on this branch — skipped)")")
        continue
    fi

    echo "================================================================"
    echo "  node-suite/$m"
    echo "================================================================"

    # The harness exits non-zero whenever any test fails (even ones in
    # known_failures), so we must tolerate that here — the per-module
    # counts already convey pass/fail.
    out=$(./run_parity_tests.sh --suite node-suite --module "$m" 2>&1) || true
    cleaned=$(printf '%s' "$out" | strip_ansi)

    # Replay the harness's own summary block for context.
    printf '%s\n' "$cleaned" | sed -n '/Parity Test Summary/,/Report saved/p'

    p=$(extract_count "Parity Pass"  "$cleaned")
    f=$(extract_count "Parity Fail"  "$cleaned")
    cf=$(extract_count "Compile Fail" "$cleaned")

    rows+=("$(printf '  %-14s %4d pass   %4d fail   %4d compile-fail' \
        "$m" "$p" "$f" "$cf")")
    total_pass=$((total_pass + p))
    total_fail=$((total_fail + f))
    total_cf=$((total_cf + cf))
    (( f > 0 || cf > 0 )) && have_failures=1
done

echo
echo "================================================================"
echo "  COMBINED SUMMARY"
echo "================================================================"
printf '%s\n' "${rows[@]}"
echo '  --------------------------------------------------------------'
printf '  %-14s %4d pass   %4d fail   %4d compile-fail\n' \
    "TOTAL" "$total_pass" "$total_fail" "$total_cf"

total=$((total_pass + total_fail + total_cf))
if (( total > 0 )); then
    pct=$(awk -v p="$total_pass" -v t="$total" 'BEGIN { printf "%.1f", (p/t)*100 }')
    echo "  Parity rate: $pct%"
fi

# Informational only: every `Parity Fail` should be a known_failures entry
# (the harness already breaks CI on untracked failures), so we don't gate
# on `have_failures` here.
exit 0
