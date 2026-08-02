#!/usr/bin/env python3
"""Assert the moving-GC gates are wired so that they CAN fail on `main`.

WHY THIS EXISTS
---------------
CLAUDE.md enumerates four ways a gate can be unable to fail. Every one of them
has bitten this repo, and each time the Actions page looked fine. This script
mechanises the check for the jobs that carry the moving-collector gates, so a
wiring regression is caught by `lint` (a REQUIRED context) instead of by
someone re-deriving it during an incident.

The specific miss it was written for (#7194): `gc-stress` carries
`scripts/gc_repsel_matrix.sh`, the only CI execution of the `requires=move`
allocation-point arms over the representation corpus. It lives in `test.yml`,
whose `push:` trigger is **tags only** ("Direct pushes to main do NOT trigger
tests"), and its own `if:` listed `push`, `pull_request` and
`workflow_dispatch` — but NOT `schedule`. So on the nightly `main` run, which
that same file calls "the only backstop for integration-suite regressions a
scoped PR run can't see", the job was **skipped**: twelve consecutive nightly
runs, `skipped` every time. Between tags, nothing ran the matrix on `main`.
`test_gap_repsel_p4a3_ptr_numarray` was consequently red on ten arms for over a
week with no CI event to say so.

WHAT "MAIN-LINE" MEANS HERE, AND WHY TAGS DO NOT COUNT
------------------------------------------------------
A gate is main-line-reachable when it runs on a `push` to `main` or on a
`schedule`. A tag push does not count. Tags fire at release time, which is
after every merge the gate was supposed to adjudicate; a gate that only speaks
at tags cannot tell you which merge broke it, and it stayed silent for the
whole week this issue was open. The point of the check is drift detection
between releases.

This script does NOT check branch protection — a required-context list is
server-side state, not a file in the tree. That gap is real (hazard 2) and is
reported by `--list` for a human to act on; it cannot be enforced from here.

Usage:
    python3 scripts/gc_gate_wiring_check.py            # check the repo
    python3 scripts/gc_gate_wiring_check.py --self-test # check the checker
    python3 scripts/gc_gate_wiring_check.py --list      # describe the gates
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# (workflow file, job id, what the job gates)
GATES = [
    (
        ".github/workflows/test.yml",
        "gc-stress",
        "scripts/gc_repsel_matrix.sh — the GC x representation-selection matrix, "
        "and the only CI execution of the requires=move allocation-point arms "
        "over the representation corpus",
    ),
    (
        ".github/workflows/gc-moving-witnesses.yml",
        "gc-moving-witnesses",
        "the test_gap_gc_* stale-root reproducers, run under "
        "PERRY_GC_MOVING_LOOP_POLLS=1 (the only arm in which they can fail)",
    ),
    (
        ".github/workflows/gc-root-dominance.yml",
        "gc-root-dominance",
        "scripts/gc_root_dominance_check.py — the static shadow-slot dominance "
        "pass over emitted LLVM IR",
    ),
    (
        ".github/workflows/gc-ratchet.yml",
        "gc-ratchet",
        "the pinned GC counter ratchet",
    ),
]

MAIN_LINE_EVENTS = ("push", "schedule")


# ---------------------------------------------------------------------------
# A deliberately small YAML reader.
#
# No PyYAML: nothing else in scripts/ imports it, and the lint runner's
# python3 is the stock image's. Workflow files are 2-space-indented and
# machine-written; the three shapes below are all this check needs.
# ---------------------------------------------------------------------------
def _block(text: str, header: str, indent: int) -> str:
    """The lines under `header` at `indent`, up to the next key at that indent."""
    pat = re.compile(rf"^{' ' * indent}{re.escape(header)}\s*:(.*)$", re.M)
    m = pat.search(text)
    if not m:
        return ""
    out = [m.group(1)]
    for line in text[m.end():].splitlines():
        if line.strip() and not line.startswith(" " * (indent + 1)):
            break
        out.append(line)
    return "\n".join(out)


def workflow_triggers(text: str) -> dict[str, str]:
    """Top-level `on:` keys -> their (possibly empty) sub-block."""
    on = _block(text, "on", 0)
    triggers: dict[str, str] = {}
    for m in re.finditer(r"^  ([a-z_]+)\s*:(.*)$", on, re.M):
        triggers[m.group(1)] = _block(on, m.group(1), 2)
    return triggers


def job_body(text: str, job_id: str) -> str:
    jobs = _block(text, "jobs", 0)
    return _block(jobs, job_id, 2)


def scalar(block: str, key: str, indent: int) -> str:
    """A scalar value, folding `>-` / `|` block scalars onto one line."""
    raw = _block(block, key, indent)
    if not raw:
        return ""
    first, _, rest = raw.partition("\n")
    first = first.strip()
    if first in (">-", ">", "|", "|-", ""):
        return " ".join(l.strip() for l in rest.splitlines() if l.strip())
    return first


# ---------------------------------------------------------------------------
# The four checks.
# ---------------------------------------------------------------------------
def check_gate(text: str, job_id: str, wf_name: str) -> list[str]:
    problems: list[str] = []
    body = job_body(text, job_id)
    if not body:
        return [f"{wf_name}: job `{job_id}` not found — the gate was renamed or deleted"]

    triggers = workflow_triggers(text)
    job_if = scalar(body, "if", 4)

    # --- 1. main-line reachability -----------------------------------------
    reachable = []
    for ev in MAIN_LINE_EVENTS:
        if ev not in triggers:
            continue
        if ev == "push":
            sub = triggers["push"]
            # tags-only push is not main-line (see the module docstring)
            if "branches" not in sub and "tags" in sub:
                continue
            if "branches" in sub and "main" not in sub:
                continue
        # a job-level `if:` that enumerates events must include this one
        if job_if and "github.event_name" in job_if and f"'{ev}'" not in job_if:
            continue
        reachable.append(ev)
    if not reachable:
        have = ", ".join(sorted(triggers)) or "(none)"
        problems.append(
            f"{wf_name}: `{job_id}` never runs on main-line code. Workflow triggers: "
            f"{have}; job if: {job_if or '(none)'}. A gate that only runs pre-merge "
            f"and at tags cannot say which merge broke it — add `schedule` (or a "
            f"`push: branches: [main]`) and list it in the job's `if:`."
        )

    # --- 2. job-level continue-on-error ------------------------------------
    if re.search(r"^    continue-on-error\s*:\s*true\s*$", body, re.M):
        problems.append(
            f"{wf_name}: `{job_id}` has job-level `continue-on-error: true` — it "
            f"reports failure without blocking anything."
        )

    # --- 3. gating steps must not swallow their exit status -----------------
    # Steps carrying their own `continue-on-error: true` are opted out on
    # purpose (informational stress runs) and are skipped.
    for step in re.split(r"^      - ", body, flags=re.M)[1:]:
        if re.search(r"^        continue-on-error\s*:\s*true\s*$", step, re.M):
            continue
        run = _block(step, "run", 8)
        for line in run.splitlines():
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if "|| true" in stripped:
                problems.append(
                    f"{wf_name}: `{job_id}` step swallows a failure with `|| true`: "
                    f"{stripped[:90]}"
                )
            # `checker | tee log` reports tee's status, not the checker's
            if re.search(r"\|\s*(tee|grep|head|tail)\b", stripped) and "set -o pipefail" not in run:
                problems.append(
                    f"{wf_name}: `{job_id}` pipes a gating command without "
                    f"`set -o pipefail`, so the shell reports the last stage's "
                    f"status: {stripped[:90]}"
                )

    # --- 4. concurrency must not cancel main-line runs ----------------------
    conc = _block(text, "concurrency", 0)
    if conc:
        cancel = scalar(conc, "cancel-in-progress", 2)
        if cancel == "true":
            problems.append(
                f"{wf_name}: `concurrency.cancel-in-progress` is unconditionally "
                f"true — on a deep runner queue every new merge cancels the "
                f"previous main run before it reaches a runner (#7205). Scope it "
                f"to pull_request."
            )
    return problems


# ---------------------------------------------------------------------------
# Self-test: the checker must be able to fail, too.
# ---------------------------------------------------------------------------
CLEAN = """\
name: X
on:
  pull_request:
  schedule:
    - cron: '0 4 * * *'
concurrency:
  group: x-${{ github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
jobs:
  gate:
    if: >-
      github.event_name == 'pull_request' ||
      github.event_name == 'schedule'
    runs-on: ubuntu-latest
    steps:
      - name: run it
        run: ./scripts/thing.sh
      - name: informational
        continue-on-error: true
        run: ./scripts/flaky.sh || true
"""


def _self_test() -> int:
    failures = []

    def expect(name: str, text: str, want_substr: str | None):
        got = check_gate(text, "gate", "fixture.yml")
        if want_substr is None:
            if got:
                failures.append(f"{name}: expected clean, got {got}")
        else:
            if not any(want_substr in p for p in got):
                failures.append(f"{name}: expected a problem matching {want_substr!r}, got {got}")

    expect("clean fixture", CLEAN, None)

    # hazard 1: job-level continue-on-error
    expect(
        "continue-on-error",
        CLEAN.replace("    runs-on: ubuntu-latest", "    continue-on-error: true\n    runs-on: ubuntu-latest", 1),
        "continue-on-error: true",
    )

    # hazard 3: unconditional cancel-in-progress
    expect(
        "cancel-in-progress",
        CLEAN.replace("cancel-in-progress: ${{ github.event_name == 'pull_request' }}", "cancel-in-progress: true"),
        "cancel-in-progress",
    )

    # hazard: the job's `if:` drops the only main-line event (the #7194 shape)
    expect(
        "if drops schedule",
        CLEAN.replace(
            "      github.event_name == 'pull_request' ||\n      github.event_name == 'schedule'",
            "      github.event_name == 'pull_request'",
        ),
        "never runs on main-line code",
    )

    # hazard: workflow only pushes on tags, and has no schedule
    expect(
        "tags-only push",
        CLEAN.replace("  schedule:\n    - cron: '0 4 * * *'", "  push:\n    tags: ['v*']").replace(
            "      github.event_name == 'pull_request' ||\n      github.event_name == 'schedule'",
            "      github.event_name == 'pull_request' ||\n      github.event_name == 'push'",
        ),
        "never runs on main-line code",
    )

    # hazard: a gating step swallowing its exit status
    expect(
        "|| true in a gating step",
        CLEAN.replace("        run: ./scripts/thing.sh", "        run: ./scripts/thing.sh || true"),
        "|| true",
    )

    # a missing job is a hard error, not a silent pass
    got = check_gate(CLEAN, "nope", "fixture.yml")
    if not got or "not found" not in got[0]:
        failures.append(f"missing job: expected a not-found problem, got {got}")

    if failures:
        for f in failures:
            print(f"SELF-TEST FAIL: {f}", file=sys.stderr)
        return 1
    print("gc_gate_wiring_check self-test: OK (7 cases)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test", action="store_true", help="check the checker, then exit")
    ap.add_argument("--list", action="store_true", help="describe the gates and exit")
    args = ap.parse_args()

    if args.self_test:
        return _self_test()

    if args.list:
        print("moving-GC gates checked for main-line reachability:\n")
        for wf, job, what in GATES:
            print(f"  {job:<22} {wf}\n{' ' * 26}{what}\n")
        print(
            "NOT checkable from the tree: branch protection's required-context\n"
            "list. None of these jobs is currently required, so a red or still-\n"
            "queued result does not block a merge (CLAUDE.md hazard 2)."
        )
        return 0

    problems: list[str] = []
    for wf, job, _ in GATES:
        path = REPO_ROOT / wf
        if not path.exists():
            problems.append(f"{wf}: missing — a GC gate workflow was deleted")
            continue
        problems.extend(check_gate(path.read_text(), job, wf))

    if problems:
        print("GC GATE WIRING: one or more gates cannot fail where it matters.\n", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        print(
            "\nSee CLAUDE.md, 'Four ways a gate can be unable to fail'.",
            file=sys.stderr,
        )
        return 1

    print(f"GC gate wiring OK ({len(GATES)} gates main-line-reachable and able to fail)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
