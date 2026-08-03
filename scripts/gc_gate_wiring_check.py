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
    (
        ".github/workflows/gc-native-roots.yml",
        "gc-native-roots-complete",
        "the native-frame root arms (PERRY_STATEPOINTS / PERRY_RS4GC / "
        "PERRY_GC_SAFEPOINT_ONLY / PERRY_STACKMAP_WALKER) — the fan-in that "
        "makes one context speak for all four, so adding an arm later never "
        "needs a branch-protection edit",
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
        return " ".join(ln.strip() for ln in rest.splitlines() if ln.strip())
    return first


def job_steps(body: str) -> list[tuple[str, str, bool]]:
    """[(if-expression, run-script, opted-out-of-gating)] for each step.

    Both step spellings are parsed. `- name: x` / `run: …` puts `run` on its
    own line at indent 8; `- run: …` puts it on the dash line. Missing the
    second form would silently skip a step, which in a checker about
    unfailable gates would be its own joke.
    """
    steps = []
    for chunk in re.split(r"^      - ", body, flags=re.M)[1:]:
        chunk = "        " + chunk  # normalise the dash line to a plain key
        coe = bool(re.search(r"^        continue-on-error\s*:\s*true\s*$", chunk, re.M))
        steps.append((scalar(chunk, "if", 8), _block(chunk, "run", 8), coe))
    return steps


def event_admitted(expr: str, event: str) -> bool:
    """Does an `if:` expression let `event` through?

    Only `github.event_name` comparisons are interpreted. An `if:` that gates
    on something else (`github.ref`, a label, an input) is treated as
    admitting the event: this check exists to catch an event that is
    provably excluded, and guessing at the rest would produce false reds on
    correctly-wired gates.

    `!=` is handled explicitly. `github.event_name != 'pull_request'` — the
    spelling `gc-stress`'s own full-arm step uses — admits `schedule`, and
    reading it as an enumeration that omits `schedule` would have condemned
    a gate that works.
    """
    if not expr or "github.event_name" not in expr:
        return True
    neq = set(re.findall(r"github\.event_name\s*!=\s*'([a-z_]+)'", expr))
    eq = set(re.findall(r"github\.event_name\s*==\s*'([a-z_]+)'", expr))
    if event in neq:
        return False
    if eq:
        return event in eq
    return True


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
    steps = job_steps(body)
    # A step that runs a command and has not opted out of gating. `uses:`
    # setup steps are not gates and a green one proves nothing.
    gating = [(sif, run) for sif, run, coe in steps if run.strip() and not coe]

    # --- 1. main-line reachability -----------------------------------------
    # Job existence is not reachability, and neither is the job-level `if:`.
    # The subject is the gating STEP, and a step-level `if:` can exclude the
    # main-line event while the job around it reports success -- CLAUDE.md
    # hazard 4, the one this script is named for. `gc-stress` is the live
    # example: both of its matrix invocations carry their own `if:`.
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
        if not event_admitted(job_if, ev):
            continue
        if gating and not any(event_admitted(sif, ev) for sif, _ in gating):
            continue
        reachable.append(ev)
    if not reachable:
        have = ", ".join(sorted(triggers)) or "(none)"
        problems.append(
            f"{wf_name}: `{job_id}` never runs its gating command on main-line code. "
            f"Workflow triggers: {have}; job if: {job_if or '(none)'}. A gate that "
            f"only runs pre-merge and at tags cannot say which merge broke it — add "
            f"`schedule` (or a `push: branches: [main]`), and make sure both the "
            f"job's `if:` and at least one gating step's `if:` admit it."
        )

    # --- 2. job-level continue-on-error ------------------------------------
    if re.search(r"^    continue-on-error\s*:\s*true\s*$", body, re.M):
        problems.append(
            f"{wf_name}: `{job_id}` has job-level `continue-on-error: true` — it "
            f"reports failure without blocking anything."
        )

    # --- 3. gating steps must not swallow their exit status -----------------
    # Steps carrying their own `continue-on-error: true` are opted out on
    # purpose (informational stress runs) and are skipped by `gating`.
    for _sif, run in gating:
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
    # Workflow level and job level both cancel, so both are read.
    for where, conc, ind in (
        ("workflow", _block(text, "concurrency", 0), 2),
        (f"job `{job_id}`", _block(body, "concurrency", 4), 6),
    ):
        if not conc:
            continue
        cancel = scalar(conc, "cancel-in-progress", ind)
        # `true` and `${{ true }}` are the same instruction wearing two hats.
        if re.fullmatch(r"(true|\$\{\{\s*true\s*\}\})", cancel):
            problems.append(
                f"{wf_name}: {where} `concurrency.cancel-in-progress` is "
                f"unconditionally true — on a deep runner queue every new merge "
                f"cancels the previous main run before it reaches a runner "
                f"(#7205). Scope it to pull_request."
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
    cases = 0

    def expect(name: str, text: str, want_substr: str | None):
        nonlocal cases
        cases += 1
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
        "never runs its gating command on main-line code",
    )

    # hazard: workflow only pushes on tags, and has no schedule
    expect(
        "tags-only push",
        CLEAN.replace("  schedule:\n    - cron: '0 4 * * *'", "  push:\n    tags: ['v*']").replace(
            "      github.event_name == 'pull_request' ||\n      github.event_name == 'schedule'",
            "      github.event_name == 'pull_request' ||\n      github.event_name == 'push'",
        ),
        "never runs its gating command on main-line code",
    )

    # hazard: a gating step swallowing its exit status
    expect(
        "|| true in a gating step",
        CLEAN.replace("        run: ./scripts/thing.sh", "        run: ./scripts/thing.sh || true"),
        "|| true",
    )

    # the same, written in the inline `- run:` spelling. A parser that only
    # understood `- name:` steps would report this fixture clean.
    expect(
        "|| true in an inline step",
        CLEAN.replace("      - name: run it\n        run: ./scripts/thing.sh", "      - run: ./scripts/thing.sh || true"),
        "|| true",
    )

    # ★ hazard 4: the job is reachable on `schedule` and its gating step is
    # not. This is the shape `gc-stress` actually has -- both matrix
    # invocations carry their own `if:` -- so a checker that stopped at the
    # job-level `if:` would bless a job whose subject never runs.
    expect(
        "step-level if excludes the main-line event",
        CLEAN.replace(
            "      - name: run it\n        run: ./scripts/thing.sh",
            "      - name: run it\n        if: github.event_name == 'pull_request'\n        run: ./scripts/thing.sh",
        ),
        "never runs its gating command on main-line code",
    )

    # ★ false-positive guard: `!= 'pull_request'` ADMITS schedule. Reading it
    # as an enumeration that omits `schedule` would condemn a working gate --
    # and this is the exact spelling of gc-stress's full-arm step.
    expect(
        "negated event condition is not an exclusion",
        CLEAN.replace(
            "      - name: run it\n        run: ./scripts/thing.sh",
            "      - name: run it\n        if: github.event_name != 'pull_request'\n        run: ./scripts/thing.sh",
        ),
        None,
    )

    # job-level concurrency cancels just as hard as workflow-level
    expect(
        "job-level cancel-in-progress",
        CLEAN.replace(
            "    runs-on: ubuntu-latest",
            "    concurrency:\n      group: g\n      cancel-in-progress: ${{ true }}\n    runs-on: ubuntu-latest",
            1,
        ),
        "cancel-in-progress",
    )

    # a missing job is a hard error, not a silent pass
    cases += 1
    got = check_gate(CLEAN, "nope", "fixture.yml")
    if not got or "not found" not in got[0]:
        failures.append(f"missing job: expected a not-found problem, got {got}")

    if failures:
        for f in failures:
            print(f"SELF-TEST FAIL: {f}", file=sys.stderr)
        return 1
    print(f"gc_gate_wiring_check self-test: OK ({cases} cases)")
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
