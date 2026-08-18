#!/usr/bin/env bash
# Run every gate the CI `lint` job runs, locally, in one command.
#
# WHY THIS EXISTS
#
# `lint` invokes ~48 separate gate commands. Reviewers (human and agent) reach
# for the handful that look topically relevant to the diff in front of them and
# merge on that, which is how five separate gates went red on `main` in a single
# day (2026-08-17): `gc_runtime_root_holders` after #8270, `-D warnings` after
# #8294, `api-docs-drift` after #8279, and `raw_handle_debt` twice, after #8269
# and #8299. Each break was found only when a LATER pull request tripped over
# it. A gate you did not run is indistinguishable from a gate that passed.
#
# The command list is DERIVED FROM .github/workflows/test.yml at run time, not
# copied, so it cannot drift from what CI actually does. If the workflow gains a
# gate, this picks it up on the next run.
#
# TWO TIERS. The script tier is the `lint` job's ~48 script/fmt commands. The
# COMPILE tier mirrors the separate `warnings` and `check` jobs -- `cargo check
# --workspace --all-targets` under `-D warnings`, and `cargo clippy --workspace`
# -- both over the same host-compatible package scope CI uses, derived from
# scripts/workspace_architecture.py rather than copied.
#
# The compile tier exists because deriving only from `lint` is not the same as
# "what CI runs": on 2026-08-18 #8333 left a test helper unused, `main` went red
# on the `warnings` job, and this script reported "all 48 gates passed" for
# every PR audited in between. A tier you do not run is a tier that did not
# pass -- the same argument this script was written to make.
#
# Set SKIP_COMPILE_GATES=1 to skip it while iterating. The summary line then
# SAYS the tier was skipped, so a fast run cannot be mistaken for a full one.
#
# Usage:
#   scripts/run_lint_gates.sh              # every gate; non-zero if any fails
#   scripts/run_lint_gates.sh --list       # print what would run, run nothing
#   BASE_SHA=origin/main scripts/run_lint_gates.sh
#
# Not a substitute for `cargo test` — this is the lint tier only.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

: "${BASE_SHA:=origin/main}"
export BASE_SHA

# bash 3.2 (macOS) has no `mapfile`; read the list portably.
CMDS=()
while IFS= read -r _line; do
    [ -n "$_line" ] && CMDS+=("$_line")
done < <(python3 - <<'PY'
import re
import sys

try:
    import yaml
except ImportError:  # pragma: no cover - keeps the script usable without pyyaml
    sys.stderr.write("run_lint_gates: pyyaml is required to read the workflow\n")
    sys.exit(3)

workflow = yaml.safe_load(open(".github/workflows/test.yml"))
steps = workflow["jobs"]["lint"].get("steps") or []
seen = set()
for step in steps:
    run = step.get("run")
    if not run:
        continue
    for line in run.split("\n"):
        line = line.strip()
        if not re.match(r"^(python3 scripts/|\./scripts/|cargo fmt)", line):
            continue
        # Steps that only redirect into a scratch file prove nothing locally.
        if ">" in line and "--self-test" not in line and "--check" not in line:
            continue
        if line in seen:
            continue
        seen.add(line)
        print(line)
PY
)

if [[ "${1:-}" == "--list" ]]; then
    printf '%s\n' "${CMDS[@]}"
    echo "(${#CMDS[@]} gate commands, derived from .github/workflows/test.yml)"
    exit 0
fi

echo "run_lint_gates: ${#CMDS[@]} gate commands derived from the lint job"
echo

failed=()
for cmd in "${CMDS[@]}"; do
    if out="$(eval "$cmd" 2>&1)"; then
        printf '  ok    %s\n' "$cmd"
    else
        printf '  FAIL  %s\n' "$cmd"
        printf '%s\n' "$out" | tail -6 | sed 's/^/          /'
        failed+=("$cmd")
    fi
done

# ---------------------------------------------------------------------------
# Compile tier: the `warnings` and `check` jobs.
compile_ran=0
if [[ "${SKIP_COMPILE_GATES:-0}" == "1" ]]; then
    echo
    echo "  skip  compile tier (SKIP_COMPILE_GATES=1)"
else
    compile_ran=1
    EXCLUDES=()
    while IFS= read -r _pkg; do
        [ -n "$_pkg" ] && EXCLUDES+=(--exclude "$_pkg")
    done < <(python3 scripts/workspace_architecture.py --print-excluded-scope host-compatible)

    echo
    echo "run_lint_gates: compile tier (${#EXCLUDES[@]} exclude args from workspace_architecture.py)"

    if out="$(RUSTFLAGS='-D warnings' cargo check --workspace --all-targets "${EXCLUDES[@]}" 2>&1)"; then
        printf '  ok    warnings: cargo check --workspace --all-targets (-D warnings)\n'
    else
        printf '  FAIL  warnings: cargo check --workspace --all-targets (-D warnings)\n'
        printf '%s\n' "$out" | grep -E '^(error|warning)' | head -6 | sed 's/^/          /'
        failed+=("warnings: cargo check --workspace --all-targets")
    fi

    if out="$(cargo clippy --workspace "${EXCLUDES[@]}" 2>&1)"; then
        printf '  ok    check: cargo clippy --workspace\n'
    else
        printf '  FAIL  check: cargo clippy --workspace\n'
        printf '%s\n' "$out" | grep -E '^(error|warning)' | head -6 | sed 's/^/          /'
        failed+=("check: cargo clippy --workspace")
    fi
fi

total=${#CMDS[@]}
((compile_ran)) && total=$((total + 2))
suffix=""
((compile_ran)) || suffix=" (compile tier SKIPPED)"

echo
if ((${#failed[@]})); then
    echo "run_lint_gates: ${#failed[@]} of ${total} FAILED${suffix}"
    printf '  %s\n' "${failed[@]}"
    exit 1
fi
echo "run_lint_gates: all ${total} gates passed${suffix}"
