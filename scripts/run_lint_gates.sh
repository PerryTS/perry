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
# `run:` commands. The COMPILE tier derives every gate command from the
# separate `warnings` and `check` jobs: product and host-compatible check /
# clippy scopes, followed by the API-docs regeneration and drift assertion.
# Host exclusions come from scripts/workspace_architecture.py, as they do in CI.
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

    if ! _self_compile="$(bash "$0" --list 2>&1)"; then
        echo "run_lint_gates self-test FAILED: real workflow extraction failed" >&2
        printf '%s\n' "$_self_compile" >&2
        exit 1
    fi
    _product_warnings='RUSTFLAGS="-D warnings" cargo check -p perry --bins'
    if [[ "$_self_compile" != *"$_product_warnings"* ]]; then
        echo "run_lint_gates self-test FAILED: compile tier lost the warnings product check" >&2
        exit 1
    fi

    if _self_missing="$(RUN_LINT_GATES_FIXTURE=warnings-missing-product bash "$0" --list 2>&1)"; then
        echo "run_lint_gates self-test FAILED: missing warnings product check exited zero" >&2
        exit 1
    fi
    if [[ "$_self_missing" != *"warnings product command"* ]]; then
        echo "run_lint_gates self-test FAILED: missing-product error omitted its subject" >&2
        printf '%s\n' "$_self_missing" >&2
        exit 1
    fi

    if ! _self_extra="$(RUN_LINT_GATES_FIXTURE=warnings-extra-command bash "$0" --list 2>&1)"; then
        echo "run_lint_gates self-test FAILED: extra warnings command was rejected" >&2
        printf '%s\n' "$_self_extra" >&2
        exit 1
    fi
    _extra_warnings='RUSTFLAGS="-D warnings" cargo check -p perry-runtime --lib'
    if [[ "$_self_extra" != *"$_extra_warnings"* ]]; then
        echo "run_lint_gates self-test FAILED: compile tier omitted an added warnings command" >&2
        exit 1
    fi

    echo "run_lint_gates self-test: OK (lint + compile commands derived; empty/missing steps rejected; added warnings command replayed)"
    exit 0
fi

# bash 3.2 (macOS) has no `mapfile`; read the list portably.
CMDS=()
CMD_STEPS=()
SKIP_REASONS=()
RUN_STEPS=0
COMPILE_CMDS=()
COMPILE_STEPS=()
COMPILE_HOST_SCOPE=()
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

with open(".github/workflows/test.yml", encoding="utf-8") as workflow_file:
    workflow = yaml.safe_load(workflow_file)

fixture = os.environ.get("RUN_LINT_GATES_FIXTURE")
if fixture == "comment-led":
    workflow["jobs"]["lint"]["steps"] = [{
        "name": "Synthetic comment-led run step",
        "run": """# Leading comments must not hide the commands below.
PYTHONPATH=. python3 tests/test_public_baseline.py
# Another comment between commands.
python3 benchmarks/ci_public_baseline_check.py
""",
    }]
elif fixture == "empty":
    workflow["jobs"]["lint"]["steps"] = [{
        "name": "Synthetic empty run step",
        "run": "# A run block with no derivable command must be fatal.\n",
    }]
elif fixture == "warnings-missing-product":
    workflow["jobs"]["warnings"]["steps"] = [
        step for step in workflow["jobs"]["warnings"]["steps"]
        if step.get("name") != "rustc warnings (product)"
    ]
elif fixture == "warnings-extra-command":
    for step in workflow["jobs"]["warnings"]["steps"]:
        if step.get("name") == "rustc warnings (host-compatible, all targets)":
            step["run"] += "\ncargo check -p perry-runtime --lib\n"
            break
elif fixture:
    sys.stderr.write(f"run_lint_gates: unknown self-test fixture: {fixture}\n")
    sys.exit(3)

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

compile_records = []
for job_name in ("warnings", "check"):
    job = workflow.get("jobs", {}).get(job_name)
    if not job:
        errors.append(f"compile job '{job_name}' is missing from the workflow")
        continue

    rustflags = ""
    if job_name == "warnings":
        rustflags = str((job.get("env") or {}).get("RUSTFLAGS") or "")
        if not rustflags:
            errors.append("compile job 'warnings' has no RUSTFLAGS value")

    job_record_count = 0
    for index, step in enumerate(job.get("steps") or [], start=1):
        run = step.get("run")
        if run is None:
            continue
        step_name = step.get("name") or f"<unnamed run step {index}>"
        joined = re.sub(r"\\\n[ \t]*", " ", run)

        # Toolchain installation is job setup, not one of the gates the local
        # compile tier replays. Keep this skip exact and loud if the step grows.
        if step_name == "Install Rust toolchain":
            setup_lines = [
                line.strip() for line in joined.split("\n")
                if line.strip() and not line.strip().startswith("#")
            ]
            if len(setup_lines) != 1 or not setup_lines[0].startswith("rustup toolchain install "):
                errors.append(
                    f"compile setup step '{job_name} / {step_name}' no longer contains only rustup install"
                )
            continue

        step_commands = []
        if "--print-excluded-scope host-compatible" in joined:
            # CI constructs an argv array because its runner has Bash 5. The
            # local driver supports macOS Bash 3.2, so derive the same argv and
            # append the same architecture-produced exclusions portably.
            assignments = re.findall(r"(?:^|\n)\s*cargo_args=\(([^)]*)\)", joined)
            invocations = re.findall(
                r"(?:^|\n)\s*cargo\s+(check|clippy)\s+\"\$\{cargo_args\[@\]\}\"\s*(?=\n|$)",
                joined,
            )
            architecture_calls = joined.count("--print-excluded-scope host-compatible")
            if len(assignments) != 1 or len(invocations) != 1 or architecture_calls != 1:
                errors.append(
                    f"compile step '{job_name} / {step_name}' has an unrecognized host-scope command shape"
                )
            else:
                try:
                    cargo_args = shlex.split(assignments[0])
                except ValueError as error:
                    errors.append(
                        f"compile step '{job_name} / {step_name}' has invalid cargo_args: {error}"
                    )
                else:
                    step_commands.append((
                        "cargo " + invocations[0] + " " + shlex.join(cargo_args),
                        "1",
                    ))
            dynamic_invocation = re.compile(
                r"cargo\s+(?:check|clippy)\s+\"\$\{cargo_args\[@\]\}\""
            )
            for raw_line in joined.split("\n"):
                line = raw_line.strip()
                if dynamic_invocation.fullmatch(line):
                    continue
                if is_gate_command(line):
                    step_commands.append((line, "0"))
        else:
            for raw_line in joined.split("\n"):
                line = raw_line.strip()
                drift = re.fullmatch(r"if ! (git diff --quiet -- .+); then", line)
                if drift:
                    step_commands.append((drift.group(1), "0"))
                    continue
                if is_gate_command(line):
                    step_commands.append((line, "0"))

        if not step_commands:
            errors.append(
                f"compile step '{job_name} / {step_name}' yielded zero commands"
            )
            continue

        for command, host_scope in step_commands:
            if rustflags:
                escaped_flags = rustflags.replace("\\", "\\\\").replace('"', '\\"')
                command = f'RUSTFLAGS="{escaped_flags}" {command}'
            compile_records.append((job_name, step_name, command, host_scope))
            job_record_count += 1

    if job_record_count == 0:
        errors.append(f"compile job '{job_name}' yielded zero commands")

warnings_product = 'RUSTFLAGS="-D warnings" cargo check -p perry --bins'
if not any(job == "warnings" and command == warnings_product
           for job, _step, command, _host_scope in compile_records):
    errors.append(
        "warnings product command is missing: " + warnings_product
    )

if errors:
    for error in errors:
        sys.stderr.write(f"run_lint_gates: extraction error: {error}\n")
    sys.exit(4)

print(f"meta\t{run_steps}\t\t")
for kind, step_name, command, reason in records:
    print("\t".join((kind, step_name, command, reason)))
for job_name, step_name, command, host_scope in compile_records:
    print("\t".join(("compile", f"{job_name}: {step_name}", command, host_scope)))
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
        compile)
            COMPILE_CMDS+=("$_command")
            COMPILE_STEPS+=("$_step")
            COMPILE_HOST_SCOPE+=("$_reason")
            ;;
    esac
done <<< "$_extracted"

EXCLUDES=()
if [[ "${1:-}" == "--list" || "${SKIP_COMPILE_GATES:-0}" != "1" ]]; then
    if ! _excluded_packages="$(python3 scripts/workspace_architecture.py \
        --print-excluded-scope host-compatible)"; then
        echo "run_lint_gates: failed to derive host-compatible exclusions" >&2
        exit 4
    fi
    while IFS= read -r _pkg; do
        [ -n "$_pkg" ] && EXCLUDES+=(--exclude "$_pkg")
    done <<< "$_excluded_packages"
fi

compile_command() {
    local _command="$1"
    local _host_scope="$2"
    local _arg
    local _quoted
    if [[ "$_host_scope" == "1" ]]; then
        for _arg in "${EXCLUDES[@]}"; do
            printf -v _quoted '%q' "$_arg"
            _command="$_command $_quoted"
        done
    fi
    printf '%s' "$_command"
}

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
    for _i in "${!COMPILE_CMDS[@]}"; do
        _c="$(compile_command "${COMPILE_CMDS[$_i]}" "${COMPILE_HOST_SCOPE[$_i]}")"
        printf '[compile / %s]\n  %s\n' "${COMPILE_STEPS[$_i]}" "$_c"
    done
    echo "(${#CMDS[@]} lint commands from ${RUN_STEPS} run steps + ${#COMPILE_CMDS[@]} compile commands, derived from .github/workflows/test.yml)"
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
    echo
    echo "run_lint_gates: ${#COMPILE_CMDS[@]} compile commands derived from warnings + check (${#EXCLUDES[@]} exclude args)"
    for _i in "${!COMPILE_CMDS[@]}"; do
        cmd="$(compile_command "${COMPILE_CMDS[$_i]}" "${COMPILE_HOST_SCOPE[$_i]}")"
        step="${COMPILE_STEPS[$_i]}"
        if out="$(eval "$cmd" 2>&1)"; then
            printf '  ok    [%s] %s\n' "$step" "$cmd"
        else
            printf '  FAIL  [%s] %s\n' "$step" "$cmd"
            printf '%s\n' "$out" | tail -6 | sed 's/^/          /'
            failed+=("[$step] $cmd")
        fi
    done
fi

total=$(( ${#CMDS[@]} - skipped ))
((compile_ran)) && total=$((total + ${#COMPILE_CMDS[@]}))
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
