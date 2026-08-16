#!/usr/bin/env python3
"""Cancel QUEUED pull-request workflow runs whose pull request is already closed.

WHY THIS EXISTS
---------------
GitHub does not reliably cancel a workflow run that is still sitting in the
queue when its pull request merges and its branch auto-deletes. Perry squash-
merges and auto-deletes branches, and every PR fans out to ~11 workflows, so a
busy day leaves hundreds of runs queued against branches that no longer exist.
They cannot gate anything -- the PR they were measuring is already in `main` --
but they hold runner slots ahead of the scheduled `main` gates, which is how
those gates go dark.

Measured on 2026-08-12 (#7966): 1,529 queued runs, of which 794 were
`pull_request` runs spread over 63 head branches. Two of those branches still
existed. The other 61 were merged-and-deleted, accounting for roughly 790 runs
-- 51% of the entire queue -- pinned in front of ten six-hourly `main` gates
that had not completed a run in over 32 hours.

WHAT IT WILL AND WILL NOT TOUCH
-------------------------------
Cancelling runs is destructive and this script is deliberately timid:

  * Only `event == "pull_request"` runs. A `push`, `schedule`,
    `workflow_dispatch` or tag run is never a candidate -- those ARE the
    main-line gates this exists to protect.
  * Only `status == "queued"`. A run that already reached a runner is left
    alone: it has consumed the scarce thing (a slot) and killing it mid-flight
    just wastes the work.
  * Only when the head branch has no OPEN pull request. Keying on open PRs
    rather than on branch existence is what makes fork PRs safe -- a fork's
    head branch never appears in this repo's refs, but its PR does appear in
    the open-PR list, so it is never a candidate.
  * `--dry-run` is the DEFAULT. Cancelling requires an explicit `--apply`.
  * `--max` caps one invocation, so a bug cannot empty the queue in one go.

Usage:
    python3 scripts/reap_stale_ci_runs.py                # report only
    python3 scripts/reap_stale_ci_runs.py --apply        # actually cancel
    python3 scripts/reap_stale_ci_runs.py --self-test    # check the checker
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys

REPO = "PerryTS/perry"

# Events that carry a main-line verdict. A run with one of these events is
# never a reaping candidate, whatever its branch looks like.
MAIN_LINE_EVENTS = frozenset({"push", "schedule", "workflow_dispatch", "release"})


def should_reap(run: dict, open_pr_branches: set[str]) -> bool:
    """The whole policy, in one testable function.

    `run` needs `event`, `status` and `head_branch`.
    """
    if run.get("event") != "pull_request":
        return False
    if run.get("status") != "queued":
        return False
    branch = run.get("head_branch")
    if not branch:
        # No branch to reason about -> refuse. "Unknown" must never mean "cancel".
        return False
    return branch not in open_pr_branches


def _gh_json(args: list[str]) -> object:
    out = subprocess.run(
        ["gh", *args], capture_output=True, text=True, check=True
    ).stdout
    return json.loads(out) if out.strip() else []


def open_pr_branches() -> set[str]:
    rows = _gh_json(
        ["pr", "list", "--repo", REPO, "--state", "open", "--limit", "500",
         "--json", "headRefName"]
    )
    return {r["headRefName"] for r in rows}  # type: ignore[index]


def queued_runs() -> list[dict]:
    runs: list[dict] = []
    page = 1
    while True:
        batch = _gh_json(
            ["api", f"repos/{REPO}/actions/runs?status=queued&per_page=100&page={page}",
             "-q", ".workflow_runs"]
        )
        if not batch:
            break
        runs.extend(batch)  # type: ignore[arg-type]
        if len(batch) < 100:  # type: ignore[arg-type]
            break
        page += 1
        if page > 30:  # 3000 runs is far past any sane queue; stop rather than spin
            break
    return runs


def cancel(run_id: int) -> bool:
    r = subprocess.run(
        ["gh", "api", "-X", "POST", f"repos/{REPO}/actions/runs/{run_id}/cancel"],
        capture_output=True, text=True,
    )
    return r.returncode == 0


def _self_test() -> int:
    open_prs = {"feat/live-one", "fork-contributor-branch"}
    cases = [
        ("merged PR, branch gone",
         {"event": "pull_request", "status": "queued", "head_branch": "fix/7843-gc-ratchet"}, True),
        ("open PR is protected",
         {"event": "pull_request", "status": "queued", "head_branch": "feat/live-one"}, False),
        ("fork PR is protected by its OPEN pr, not by branch existence",
         {"event": "pull_request", "status": "queued", "head_branch": "fork-contributor-branch"}, False),
        ("a scheduled main gate is never reaped",
         {"event": "schedule", "status": "queued", "head_branch": "main"}, False),
        ("a push to main is never reaped",
         {"event": "push", "status": "queued", "head_branch": "main"}, False),
        ("a tag/dispatch run is never reaped",
         {"event": "workflow_dispatch", "status": "queued", "head_branch": "main"}, False),
        ("an already-running PR run is left alone",
         {"event": "pull_request", "status": "in_progress", "head_branch": "fix/7843-gc-ratchet"}, False),
        ("a completed run is not a candidate",
         {"event": "pull_request", "status": "completed", "head_branch": "fix/7843-gc-ratchet"}, False),
        ("missing branch means refuse, not cancel",
         {"event": "pull_request", "status": "queued", "head_branch": None}, False),
    ]
    failures = []
    for name, run, want in cases:
        got = should_reap(run, open_prs)
        if got != want:
            failures.append(f"{name}: want {want}, got {got}")

    # Sabotage: a policy that reaped everything would pass a test that only
    # checked the positive case. Assert the guard rails actually bind.
    if should_reap({"event": "schedule", "status": "queued", "head_branch": "gone"}, set()):
        failures.append("SABOTAGE: a schedule run was reaped with an empty open-PR set")
    if not should_reap(
        {"event": "pull_request", "status": "queued", "head_branch": "gone"}, set()
    ):
        failures.append("SABOTAGE: the detector cannot fire at all")

    if failures:
        print("reap_stale_ci_runs self-test FAILED", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print(f"reap_stale_ci_runs self-test: OK ({len(cases) + 2} cases)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--apply", action="store_true",
                    help="actually cancel (default is a dry run)")
    ap.add_argument("--max", type=int, default=400,
                    help="cap cancellations for one invocation (default 400)")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        return _self_test()

    protected = open_pr_branches()
    runs = queued_runs()
    victims = [r for r in runs if should_reap(r, protected)]

    print(f"queued runs:        {len(runs)}")
    print(f"open PR branches:   {len(protected)}")
    print(f"reapable (stale PR):{len(victims)}")

    by_branch: dict[str, int] = {}
    for r in victims:
        by_branch[r["head_branch"]] = by_branch.get(r["head_branch"], 0) + 1
    for br, n in sorted(by_branch.items(), key=lambda kv: -kv[1]):
        print(f"  {n:4d}  {br}")

    if not args.apply:
        print("\nDRY RUN — nothing cancelled. Re-run with --apply to cancel.")
        return 0

    done = 0
    for r in victims[: args.max]:
        if cancel(r["id"]):
            done += 1
    print(f"\ncancelled {done} of {len(victims)} stale queued runs")
    return 0


if __name__ == "__main__":
    sys.exit(main())
