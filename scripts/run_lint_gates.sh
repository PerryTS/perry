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

echo
if ((${#failed[@]})); then
    echo "run_lint_gates: ${#failed[@]} of ${#CMDS[@]} FAILED"
    printf '  %s\n' "${failed[@]}"
    exit 1
fi
echo "run_lint_gates: all ${#CMDS[@]} gates passed"
