#!/usr/bin/env bash
# tests/release/packages/_harness.sh — fixture iterator for tier 3 of the
# release sweep. Walks every <fixture>/fixture.sh in this directory, runs
# it, accumulates pass/fail/skip totals, and writes a flat JSON summary
# to PERRY_TEST_SUMMARY_OUT (when set) so the orchestrator can read it.
#
# Each fixture.sh must:
#   - exit 0 with "PASS <name>"  on success
#   - exit 1 with "FAIL <name>"  on byte-mismatch / compile / run failure
#   - exit 0 with "SKIP <name> reason"  if a required backend isn't reachable
#     (the harness counts SKIP separately from PASS/FAIL — they're not
#     gate-blocking under --gate-0.6.0 if --allow-skip covers them at the
#     tier level)
#
# Usage:
#   tests/release/packages/_harness.sh                       # all fixtures
#   tests/release/packages/_harness.sh --filter hono         # one fixture
#   tests/release/packages/_harness.sh --filter hono,redis   # subset
#
# Env:
#   PERRY_BIN                 path to perry (default: target/release/perry)
#   PERRY_TEST_SUMMARY_OUT    write JSON summary here (no-op if unset)
#   PERRY_RELEASE_SWEEP_QUICK if "1", fixtures may shorten their workload

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
cd "$SCRIPT_DIR"

filter=""
for arg in "$@"; do
    case "$arg" in
        --filter=*) filter="${arg#--filter=}" ;;
        --filter)   shift; filter="${1:-}" ;;
        *)          ;;
    esac
done

# Build a CSV match-set if a filter was passed
filter_match() {
    local name="$1"
    [[ -z "$filter" ]] && return 0
    local IFS=','
    local f
    for f in $filter; do
        if [[ "$name" == "$f" ]]; then
            return 0
        fi
    done
    return 1
}

# Resolve perry binary if the env var didn't already
if [[ -z "${PERRY_BIN:-}" ]]; then
    if [[ -x "$REPO_ROOT/target/release/perry" ]]; then
        PERRY_BIN="$REPO_ROOT/target/release/perry"
    else
        echo "_harness: perry binary not found at $REPO_ROOT/target/release/perry" >&2
        echo "_harness: build with 'cargo build --release -p perry' first" >&2
        exit 2
    fi
fi
export PERRY_BIN

pass=0
fail=0
skip=0
fail_names=()
skip_names=()

for fix_dir in "$SCRIPT_DIR"/*/; do
    [[ -d "$fix_dir" ]] || continue
    name="$(basename "$fix_dir")"
    [[ "$name" == "_"* ]] && continue   # skip _shared/ etc.
    if ! filter_match "$name"; then
        continue
    fi
    if [[ ! -x "$fix_dir/fixture.sh" ]]; then
        echo "FAIL $name — no executable fixture.sh"
        fail=$((fail + 1))
        fail_names+=("$name")
        continue
    fi
    echo
    echo "=== fixture: $name ==="
    set +e
    "$fix_dir/fixture.sh"
    rc=$?
    set -e
    if [[ "$rc" -eq 0 ]]; then
        # PASS or SKIP — distinguish by whether the fixture's last printed
        # line started with "SKIP".
        if "$fix_dir/fixture.sh" --__did-skip-marker 2>/dev/null; then
            : # never reached; we don't actually rerun
        fi
        # Cheaper: re-read the fixture's last invocation output. We don't
        # capture it above (the user wants to see it inline), so we trust
        # the exit code — but a fixture can opt into SKIP by leaving a
        # sentinel file at "$fix_dir/.last-skip".
        if [[ -f "$fix_dir/.last-skip" ]]; then
            skip=$((skip + 1))
            skip_names+=("$name")
            rm -f "$fix_dir/.last-skip"
        else
            pass=$((pass + 1))
        fi
    else
        fail=$((fail + 1))
        fail_names+=("$name")
    fi
done

echo
echo "=== _harness summary ==="
echo "  passed: $pass"
echo "  failed: $fail"
echo "  skipped: $skip"
# `${arr[@]+"${arr[@]}"}` is the safe empty-array expansion under `set -u`
# (unset / zero-length arrays would otherwise error with "unbound variable").
if [[ $fail -gt 0 ]]; then echo "  failed fixtures: ${fail_names[@]+"${fail_names[@]}"}"; fi
if [[ $skip -gt 0 ]]; then echo "  skipped fixtures: ${skip_names[@]+"${skip_names[@]}"}"; fi

# Emit flat JSON summary for the orchestrator (release_sweep tier 3).
# `printf '"%s",' "${arr[@]}"` always prints once even when the array is
# empty (printf reuses the format on an empty arg list), which would yield
# `[""]` instead of `[]`. Guard with a non-empty length check.
if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    fail_csv=""
    skip_csv=""
    if [[ ${#fail_names[@]} -gt 0 ]]; then
        fail_csv="$(printf '"%s",' "${fail_names[@]}" | sed 's/,$//')"
    fi
    if [[ ${#skip_names[@]} -gt 0 ]]; then
        skip_csv="$(printf '"%s",' "${skip_names[@]}" | sed 's/,$//')"
    fi
    cat > "$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "_harness.sh", "passed": $pass, "failed": $fail, "skipped": $skip, "failed_fixtures": [${fail_csv}], "skipped_fixtures": [${skip_csv}]}
EOF
fi

[[ $fail -eq 0 ]]
