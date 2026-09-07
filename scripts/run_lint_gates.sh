#!/usr/bin/env bash
# Run every gate the CI `lint` job runs, locally, in one command.
#
# WHY THIS EXISTS
#
# `lint` invokes dozens of separate gate commands. Reviewers (human and agent) reach
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
# TWO TIERS. The script tier mirrors the `lint` job's locally executable
# `run:` commands. The
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
#   scripts/run_lint_gates.sh --self-test  # prove extraction succeeds and fails loudly
#   BASE_SHA=origin/main scripts/run_lint_gates.sh
#
# Not a substitute for `cargo test` — this is the lint tier only.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

: "${BASE_SHA:=origin/main}"
export BASE_SHA

if [[ "${1:-}" == "--self-test" ]]; then
    if ! _self_ok="$(RUN_LINT_GATES_FIXTURE=comment-led bash "$0" --list 2>&1)"; then
        echo "run_lint_gates self-test FAILED: comment-led run block was rejected" >&2
        printf '%s\n' "$_self_ok" >&2
        exit 1
    fi
    for _expected in \
        "PYTHONPATH=. python3 tests/test_public_baseline.py" \
        "python3 benchmarks/ci_public_baseline_check.py"; do
        if [[ "$_self_ok" != *"$_expected"* ]]; then
            echo "run_lint_gates self-test FAILED: comment-led run block lost: $_expected" >&2
            exit 1
        fi
    done

    if _self_bad="$(RUN_LINT_GATES_FIXTURE=empty bash "$0" --list 2>&1)"; then
        echo "run_lint_gates self-test FAILED: empty run step exited zero" >&2
        exit 1
    fi
    if [[ "$_self_bad" != *"Synthetic empty run step"* ]]; then
        echo "run_lint_gates self-test FAILED: empty-step error omitted its step name" >&2
        printf '%s\n' "$_self_bad" >&2
        exit 1
    fi

    echo "run_lint_gates self-test: OK (comment-led commands derived; empty step rejected by name)"
    exit 0
fi

# bash 3.2 (macOS) has no `mapfile`; read the list portably.
CMDS=()
CMD_STEPS=()
SKIP_REASONS=()
RUN_STEPS=0
if ! _extracted="$(python3 - <<'PY'
import re
import shlex
import sys
import os

try:
    import yaml
except ImportError:  # pragma: no cover - keeps the script usable without pyyaml
    sys.stderr.write("run_lint_gates: pyyaml is required to read the workflow\n")
    sys.exit(3)

fixture = os.environ.get("RUN_LINT_GATES_FIXTURE")
if fixture == "comment-led":
    workflow = {"jobs": {"lint": {"steps": [{
        "name": "Synthetic comment-led run step",
        "run": """# Leading comments must not hide the commands below.
PYTHONPATH=. python3 tests/test_public_baseline.py
# Another comment between commands.
python3 benchmarks/ci_public_baseline_check.py
""",
    }]}}}
elif fixture == "empty":
    workflow = {"jobs": {"lint": {"steps": [{
        "name": "Synthetic empty run step",
        "run": "# A run block with no derivable command must be fatal.\n",
    }]}}}
elif fixture:
    sys.stderr.write(f"run_lint_gates: unknown self-test fixture: {fixture}\n")
    sys.exit(3)
else:
    with open(".github/workflows/test.yml", encoding="utf-8") as workflow_file:
        workflow = yaml.safe_load(workflow_file)

steps = workflow["jobs"]["lint"].get("steps") or []

# These are the only lint commands that require values supplied by GitHub.
# Keep the step name and command signature explicit: a new expression cannot
# silently become a third skip, and a stale skip entry fails extraction.
ci_only = {
    "changeset": {
        "step": "Require a changelog.d/ fragment for crates/ changes",
        "needles": ("check_changeset_fragment.sh", "github.repository", "github.event.pull_request.number"),
        "reason": "needs GitHub API repository/PR context",
    },
    "shard-count": {
        "step": "CI plan policy self-test + docs table freshness",
        "needles": ("ci_cargo_test_shard.py", "--validate", "needs.plan.outputs.plan"),
        "reason": "needs the CI plan's shard count",
    },
}
matched_skips = set()
records = []
errors = []
run_steps = 0
command_names = {"python3", "cargo", "rustup", "node", "bash"}

def is_gate_command(line):
    """Recognize a top-level executable line, allowing leading env assignments."""
    if not line or line.startswith("#") or "<<" in line:
        return False
    # Scratch-file producers are inputs to a later assertion, not standalone
    # gates. Preserve the existing exception for self-tests/checks.
    if ">" in line and "--self-test" not in line and "--check" not in line:
        return False
    try:
        words = shlex.split(line)
    except ValueError:
        return False
    while words and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", words[0]):
        words.pop(0)
    if not words:
        return False
    executable = words[0]
    return (
        executable in command_names
        or executable.startswith("./scripts/")
        or executable.startswith("./tests/")
    )

for index, step in enumerate(steps, start=1):
    run = step.get("run")
    if run is None:
        continue
    run_steps += 1
    step_name = step.get("name") or f"<unnamed run step {index}>"
    # Join backslash continuations FIRST. Without this a multi-line gate is
    # extracted as its first line only and then RUN that way -- truncated, with
    # a trailing backslash -- which is #8929: the ci_cargo_test_shard.py step
    # failed for everyone. Joining is safe for the ratchet steps, whose
    # "git cat-file ... || git fetch ..." prelude is a SEPARATE logical command
    # from the "python3 scripts/..." gate on the line below it.
    joined = re.sub(r"\\\n[ \t]*", " ", run)
    step_records = []
    for line in joined.split("\n"):
        line = line.strip()
        if not is_gate_command(line):
            continue
        # A GitHub Actions expression is substituted in CI and never locally,
        # so only the two commands named above may be skipped locally.
        #
        # Built by concatenation on purpose, and NOT written literally: this
        # heredoc sits inside a process substitution, and bash 3.2 (macOS)
        # parses the body far enough to treat a literal dollar-brace-brace as
        # an unterminated parameter expansion -- the whole script then dies
        # with "unexpected EOF while looking for matching quote".
        gha_expr = "$" + "{" + "{"
        if gha_expr in line:
            matches = [
                key for key, rule in ci_only.items()
                if step_name == rule["step"] and all(needle in line for needle in rule["needles"])
            ]
            if len(matches) != 1:
                errors.append(f"step '{step_name}' has an unapproved CI-only command: {line}")
                continue
            key = matches[0]
            matched_skips.add(key)
            step_records.append(("skip", step_name, line, ci_only[key]["reason"]))
        else:
            step_records.append(("run", step_name, line, ""))

    if not step_records:
        errors.append(f"step '{step_name}' has a run: block but yielded zero commands")
    records.extend(step_records)

if not fixture:
    for key, rule in ci_only.items():
        if key not in matched_skips:
            errors.append(f"explicit CI-only skip '{rule['step']}' no longer matches the workflow")

if errors:
    for error in errors:
        sys.stderr.write(f"run_lint_gates: extraction error: {error}\n")
    sys.exit(4)

print(f"meta\t{run_steps}\t\t")
for kind, step_name, command, reason in records:
    print("\t".join((kind, step_name, command, reason)))
PY
)"; then
    exit 4
fi

while IFS=$'\t' read -r _kind _step _command _reason; do
    case "$_kind" in
        meta)
            RUN_STEPS="$_step"
            ;;
        run)
            CMDS+=("$_command")
            CMD_STEPS+=("$_step")
            SKIP_REASONS+=("")
            ;;
        skip)
            CMDS+=("#skip# $_command")
            CMD_STEPS+=("$_step")
            SKIP_REASONS+=("$_reason")
            ;;
    esac
done <<< "$_extracted"

if [[ "${1:-}" == "--list" ]]; then
    for _i in "${!CMDS[@]}"; do
        _c="${CMDS[$_i]}"
        _step="${CMD_STEPS[$_i]}"
        if [[ "$_c" == "#skip# "* ]]; then
            printf '[%s]\n  SKIP (CI-only: %s): %s\n' \
                "$_step" "${SKIP_REASONS[$_i]}" "${_c#\#skip\# }"
        else
            printf '[%s]\n  %s\n' "$_step" "$_c"
        fi
    done
    echo "(${#CMDS[@]} gate commands from ${RUN_STEPS} run steps, derived from .github/workflows/test.yml)"
    exit 0
fi

echo "run_lint_gates: ${#CMDS[@]} gate commands derived from ${RUN_STEPS} lint run steps"
echo

failed=()
skipped=0
for _i in "${!CMDS[@]}"; do
    cmd="${CMDS[$_i]}"
    step="${CMD_STEPS[$_i]}"
    if [[ "$cmd" == "#skip# "* ]]; then
        skipped=$((skipped + 1))
        printf '  skip  [%s] %s\n' "$step" "${cmd#\#skip\# }"
        printf '        (%s)\n' "${SKIP_REASONS[$_i]}"
        continue
    fi
    if out="$(eval "$cmd" 2>&1)"; then
        printf '  ok    [%s] %s\n' "$step" "$cmd"
    else
        printf '  FAIL  [%s] %s\n' "$step" "$cmd"
        printf '%s\n' "$out" | tail -6 | sed 's/^/          /'
        failed+=("[$step] $cmd")
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

total=$(( ${#CMDS[@]} - skipped ))
((compile_ran)) && total=$((total + 2))
suffix=""
((compile_ran)) || suffix=" (compile tier SKIPPED)"
((skipped)) && suffix="${suffix}; ${skipped} CI-only skipped"

echo
if ((${#failed[@]})); then
    echo "run_lint_gates: ${#failed[@]} of ${total} FAILED${suffix}"
    printf '  %s\n' "${failed[@]}"
    exit 1
fi
echo "run_lint_gates: all ${total} gates passed${suffix}"
