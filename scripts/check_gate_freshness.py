#!/usr/bin/env python3
"""Fail when a post-merge gate has not produced a completed `main` result recently.

#7856: every heavy gate in this repo produced zero results on `main` for over two
days. They were not failing and not cancelled -- they never reached a runner, because
fourteen workflows enqueued ~29 jobs on every one of 58 daily merges against a repo
that runs ~9 jobs at a time. Five collector-touching PRs merged inside that window and
#7843's seven genuinely-red rows landed unseen.

The reason it lasted two days is the part worth engineering against: **the failure is
silent by construction.** An empty result set looks exactly like a healthy one that
nobody has looked at. Rescheduling the gates
(docs/src/testing/ci-gate-scheduling.md) does not fix that -- a cron that quietly
stops firing fails in precisely the same way. This script is the detector for both.

For each gate in scripts/gate_freshness.json it asks the Actions API for the most
recent COMPLETED run on the default branch, and fails when that result is older than
the gate's budget. The age starts when the run completed, not when it entered the
queue: a run that waited twelve hours and finished now is fresh evidence that the gate
reached a runner.

Two things it deliberately does NOT count as evidence of health:

  * **pull_request runs.** PR runs supersede each other and therefore drain even when
    the queue is saturated. Counting them is exactly what made `gc-root-dominance`
    look healthy in #7856 while its `main` arm had been dark for two days.
  * **runs that merely exist.** `queued`, `in_progress`, `cancelled` and `skipped`
    runs are not results. A gate with 22 queued `main` runs and no completed one is a
    dark gate, which is the whole point.

A completed failure DOES count as fresh. Its own red workflow run is the failure
signal; classifying it as starvation too conflates two independent diagnoses. This is
especially important for advisory workflows such as npm publish freshness, whose live
check deliberately fails and maintains its own issue while the registry is behind.

Usage:
    python3 scripts/check_gate_freshness.py --self-test   # prove it can still fail
    python3 scripts/check_gate_freshness.py --dry-run     # real API, no issue writes
    python3 scripts/check_gate_freshness.py               # CI gate
"""

from __future__ import annotations

import argparse
import datetime as _dt
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Sequence

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "scripts" / "gate_freshness.json"

# Marker in the sticky issue title so we update one issue forever instead of opening a
# new one every hour. Changing this string orphans the existing issue.
ISSUE_MARKER = "CI gate freshness alert"

# Runs from these events count as evidence that the post-merge arm produced a result.
# `pull_request` is excluded on purpose -- see the module docstring.
POST_MERGE_EVENTS = frozenset({"push", "schedule", "workflow_dispatch"})

# These terminal conclusions do not prove that a gate produced a verdict. Everything
# else returned with status=completed did reach a terminal result; in particular,
# failure and timed_out are visible gate failures rather than freshness failures.
NON_RESULTS = frozenset({None, "action_required", "cancelled", "skipped", "stale"})


def _utcnow() -> _dt.datetime:
    return _dt.datetime.now(_dt.timezone.utc)


def _parse_ts(value: str) -> _dt.datetime:
    return _dt.datetime.fromisoformat(value.replace("Z", "+00:00"))


@dataclass(frozen=True)
class Gate:
    workflow: str
    max_age_hours: float
    why: str
    source_workflow: str | None = None
    job_names: tuple[str, ...] = ()

    @property
    def api_workflow(self) -> str:
        return self.source_workflow or self.workflow


@dataclass(frozen=True)
class Verdict:
    gate: Gate
    age_hours: float | None  # None => no qualifying completed result at all
    last_result: str | None
    last_sha: str | None
    conclusion: str | None

    @property
    def stale(self) -> bool:
        return self.age_hours is None or self.age_hours > self.gate.max_age_hours

    @property
    def detail(self) -> str:
        if self.age_hours is None:
            return "NO completed post-merge result in the sampled window"
        return (
            f"last {self.conclusion} result {self.age_hours:.1f}h ago "
            f"(budget {self.gate.max_age_hours:g}h)"
        )


def load_gates(manifest: Path = MANIFEST) -> tuple[list[Gate], str]:
    data = json.loads(manifest.read_text())
    gates = [
        Gate(
            workflow=g["workflow"],
            max_age_hours=float(g["max_age_hours"]),
            why=g.get("why", ""),
            source_workflow=g.get("source_workflow"),
            job_names=tuple(g.get("job_names", [])),
        )
        for g in data["gates"]
    ]
    if not gates:
        raise SystemExit("gate_freshness.json lists no gates; refusing to pass vacuously")
    return gates, data.get("default_branch", "main")


def _gh_json(path: str) -> dict:
    proc = subprocess.run(
        ["gh", "api", "-H", "Accept: application/vnd.github+json", path],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"gh api {path} failed: {proc.stderr.strip()}")
    return json.loads(proc.stdout)


def newest_post_merge_result(
    repo: str, gate: Gate, branch: str, fetch=_gh_json
) -> tuple[str, str, str] | None:
    """Return (completed_at, head_sha, conclusion) for the newest result, or None.

    Asks for completed runs on the branch and then filters by event and conclusion.
    The API's `?event=` parameter takes a single value, so filtering client-side is
    what lets one request cover push + schedule + workflow_dispatch.

    The API orders runs by creation time, not completion time. Deep queues can invert
    those orders, so inspect the whole sampled page and choose the newest completion.
    """
    path = (
        f"repos/{repo}/actions/workflows/{gate.api_workflow}/runs"
        f"?branch={branch}&status=completed&per_page=50"
    )
    payload = fetch(path)
    candidates: list[tuple[str, str, str]] = []
    for run in payload.get("workflow_runs", []):
        if run.get("event") not in POST_MERGE_EVENTS:
            continue
        if run.get("status") != "completed":
            continue
        if gate.job_names:
            jobs = fetch(f"repos/{repo}/actions/runs/{run['id']}/jobs?per_page=100").get(
                "jobs", []
            )
            by_name = {job.get("name"): job for job in jobs}
            selected = [by_name.get(name) for name in gate.job_names]
            if any(job is None for job in selected):
                continue
            if any(
                job.get("status") != "completed"
                or job.get("conclusion") in NON_RESULTS
                or not job.get("completed_at")
                for job in selected
                if job is not None
            ):
                continue
            completed = max(
                (job["completed_at"] for job in selected if job is not None),
                key=_parse_ts,
            )
            conclusion = (
                "success"
                if all(job.get("conclusion") == "success" for job in selected if job is not None)
                else "failure"
            )
        else:
            if run.get("conclusion") in NON_RESULTS:
                continue
            completed = run.get("updated_at")
            conclusion = run.get("conclusion")
            if not completed or not conclusion:
                continue
        candidates.append((completed, run.get("head_sha", ""), conclusion))
    return max(candidates, key=lambda row: _parse_ts(row[0])) if candidates else None


def evaluate(
    gates: Sequence[Gate], repo: str, branch: str, now: _dt.datetime, fetch=_gh_json
) -> list[Verdict]:
    verdicts: list[Verdict] = []
    for gate in gates:
        try:
            found = newest_post_merge_result(repo, gate, branch, fetch=fetch)
        except RuntimeError as exc:
            # A workflow file that no longer exists, or an API failure, must not be
            # silently treated as "fresh". Report it as stale with no timestamp.
            print(f"::warning::{gate.workflow}: {exc}", file=sys.stderr)
            found = None
        if found is None:
            verdicts.append(Verdict(gate, None, None, None, None))
            continue
        completed, sha, conclusion = found
        age = (now - _parse_ts(completed)).total_seconds() / 3600.0
        verdicts.append(Verdict(gate, age, completed, sha, conclusion))
    return verdicts


def render(verdicts: Iterable[Verdict]) -> str:
    rows = list(verdicts)
    width = max((len(v.gate.workflow) for v in rows), default=10)
    lines = [f"{'workflow'.ljust(width)}  status   detail", f"{'-' * width}  -------  ------"]
    for v in rows:
        status = "STALE" if v.stale else "ok"
        lines.append(f"{v.gate.workflow.ljust(width)}  {status:<7}  {v.detail}")
    return "\n".join(lines)


def issue_body(verdicts: Sequence[Verdict], branch: str) -> str:
    stale = [v for v in verdicts if v.stale]
    out = [
        f"The following post-merge gates have not produced a completed `{branch}` result "
        "within their freshness budget.",
        "",
        "**This usually means they are starved, not broken** — queued behind a merge "
        "cadence the runner pool cannot drain (#7856). Check the Actions queue depth "
        "before assuming a code failure:",
        "",
        "```bash",
        'gh api "repos/PerryTS/perry/actions/runs?status=queued&per_page=100" \\',
        "  -q '.workflow_runs | length'",
        "```",
        "",
        "| gate | detail | why it matters |",
        "|---|---|---|",
    ]
    for v in stale:
        out.append(f"| `{v.gate.workflow}` | {v.detail} | {v.gate.why} |")
    out += [
        "",
        "Scheduling rationale and the cost it trades away: "
        "`docs/src/testing/ci-gate-scheduling.md`.",
        "",
        "_This issue is maintained automatically by `gate-freshness.yml`. It is updated "
        "in place, never duplicated, and closes itself once every gate is fresh again._",
    ]
    return "\n".join(out)


# --------------------------------------------------------------------------- sticky issue


def _gh(args: list[str]) -> str:
    proc = subprocess.run(["gh", *args], capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout


def sync_issue(repo: str, verdicts: Sequence[Verdict], branch: str) -> None:
    """Open, update, or close the single sticky alert issue. Never duplicates."""
    found = json.loads(
        _gh(
            [
                "issue",
                "list",
                "--repo",
                repo,
                "--state",
                "open",
                "--search",
                f'"{ISSUE_MARKER}" in:title',
                "--json",
                "number,title",
                "--limit",
                "10",
            ]
        )
    )
    existing = next((i["number"] for i in found if ISSUE_MARKER in i["title"]), None)
    stale = [v for v in verdicts if v.stale]

    if not stale:
        if existing is not None:
            _gh(
                [
                    "issue",
                    "close",
                    str(existing),
                    "--repo",
                    repo,
                    "--comment",
                    "All post-merge gates are fresh again; closing automatically.",
                ]
            )
            print(f"closed sticky issue #{existing} (all gates fresh)")
        return

    title = f"{ISSUE_MARKER}: {len(stale)} gate(s) have no recent completed `{branch}` result"
    body = issue_body(verdicts, branch)
    if existing is None:
        url = _gh(
            ["issue", "create", "--repo", repo, "--title", title, "--body", body]
        ).strip()
        print(f"opened sticky issue {url}")
    else:
        _gh(
            ["issue", "edit", str(existing), "--repo", repo, "--title", title, "--body", body]
        )
        print(f"updated sticky issue #{existing}")


# --------------------------------------------------------------------------- self-test


def self_test() -> int:
    """Prove the checker can still fail, and on which shapes.

    CLAUDE.md: a gate must assert its subject was live, not merely that nothing threw.
    So this plants each failure shape one at a time and asserts the verdict, rather
    than running the happy path and calling it green.
    """
    now = _dt.datetime(2026, 8, 11, 15, 0, tzinfo=_dt.timezone.utc)

    def stamp(hours_ago: float) -> str:
        return (now - _dt.timedelta(hours=hours_ago)).strftime("%Y-%m-%dT%H:%M:%SZ")

    def run(
        event: str,
        status: str,
        conclusion: str | None,
        created_hours_ago: float,
        completed_hours_ago: float | None,
        sha: str,
    ) -> dict:
        return {
            "event": event,
            "status": status,
            "conclusion": conclusion,
            "created_at": stamp(created_hours_ago),
            "updated_at": stamp(completed_hours_ago) if completed_hours_ago is not None else None,
            "head_sha": sha,
        }

    fixtures = {
        # Fresh: a scheduled success completed 2h ago.
        "fresh.yml": [run("schedule", "completed", "success", 3, 2, "aaa")],
        # A failure is still a fresh RESULT. It has its own red workflow signal.
        # Its 72h-old creation time also proves age is measured from completion.
        "failed.yml": [run("schedule", "completed", "failure", 72, 1, "aab")],
        # Stale: last completed result was 3 days ago -- the #7856 shape.
        "stale.yml": [run("push", "completed", "success", 73, 72, "bbb")],
        # No completed post-merge result at all.
        "never.yml": [],
        # Merely queued or in-progress main-line runs are not results.
        "pending.yml": [
            run("schedule", "queued", None, 1, None, "bbc"),
            run("push", "in_progress", None, 2, None, "bbd"),
        ],
        # A cancelled recent run is not a result; the older completion remains stale.
        "cancelled.yml": [
            run("schedule", "completed", "cancelled", 1, 0.5, "bbe"),
            run("schedule", "completed", "success", 73, 72, "bbf"),
        ],
        # THE TRAP: plenty of recent results, but all of them pull_request runs.
        # This is what made gc-root-dominance look healthy while its main arm was dark.
        "pronly.yml": [
            run("pull_request", "completed", "success", 1, 0.5, "ccc"),
            run("pull_request", "completed", "failure", 2, 1, "ddd"),
        ],
        # boundary: exactly at budget is NOT stale; just past it is.
        "boundary.yml": [run("schedule", "completed", "success", 13, 12, "eee")],
        # Reusable workflows appear as jobs on the caller run, not as their own run.
        # The caller may fail elsewhere; all configured child jobs still form a result.
        "test.yml": [
            {
                **run("push", "completed", "failure", 3, 1, "fff"),
                "id": 101,
            }
        ],
        # A partial child-job group is not a result. Fall back to the older complete
        # group, which is stale.
        "partial.yml": [
            {
                **run("push", "completed", "failure", 2, 1, "ggg"),
                "id": 102,
            },
            {
                **run("push", "completed", "success", 73, 72, "hhh"),
                "id": 103,
            },
        ],
    }

    job_fixtures = {
        101: [
            {"name": "audit / first", "status": "completed", "conclusion": "success", "completed_at": stamp(1.5)},
            {"name": "audit / second", "status": "completed", "conclusion": "success", "completed_at": stamp(1)},
        ],
        102: [
            {"name": "audit / first", "status": "completed", "conclusion": "success", "completed_at": stamp(1)},
        ],
        103: [
            {"name": "audit / first", "status": "completed", "conclusion": "success", "completed_at": stamp(72.5)},
            {"name": "audit / second", "status": "completed", "conclusion": "failure", "completed_at": stamp(72)},
        ],
    }

    def fake_fetch(path: str) -> dict:
        if "/actions/runs/" in path:
            run_id = int(path.split("/actions/runs/")[1].split("/jobs")[0])
            return {"jobs": job_fixtures[run_id]}
        wf = path.split("/actions/workflows/")[1].split("/runs")[0]
        return {"workflow_runs": fixtures[wf]}

    direct_workflows = [w for w in fixtures if w not in {"test.yml", "partial.yml"}]
    gates = [Gate(w, 12, "self-test") for w in direct_workflows]
    gates += [
        Gate(
            "nested.yml",
            12,
            "self-test",
            source_workflow="test.yml",
            job_names=("audit / first", "audit / second"),
        ),
        Gate(
            "nested-partial.yml",
            12,
            "self-test",
            source_workflow="partial.yml",
            job_names=("audit / first", "audit / second"),
        ),
    ]
    verdicts = {v.gate.workflow: v for v in evaluate(gates, "o/r", "main", now, fetch=fake_fetch)}

    failures: list[str] = []

    def expect(name: str, want_stale: bool, label: str) -> None:
        got = verdicts[name].stale
        if got != want_stale:
            failures.append(
                f"{label}: expected stale={want_stale} for {name}, got stale={got} "
                f"({verdicts[name].detail})"
            )

    expect("fresh.yml", False, "a recent scheduled success is fresh")
    expect("failed.yml", False, "a recent completed failure is fresh execution evidence")
    expect("stale.yml", True, "a 3-day-old completed result is stale")
    expect("never.yml", True, "no completed post-merge result at all is stale")
    expect("pending.yml", True, "queued and in-progress runs do NOT count as results")
    expect("cancelled.yml", True, "cancelled runs do NOT count as results")
    expect("pronly.yml", True, "pull_request results alone do NOT count as fresh")
    expect("boundary.yml", False, "exactly at budget is not yet stale")
    expect("nested.yml", False, "all reusable-workflow jobs form a fresh result")
    expect("nested-partial.yml", True, "a partial reusable-workflow job group is not fresh")

    # The renderer must actually say STALE, or a red verdict could print as green.
    table = render(verdicts.values())
    if "STALE" not in table:
        failures.append("render() produced no STALE marker for a stale set")

    # And the exit path must be non-zero when anything is stale.
    if _exit_code(list(verdicts.values())) == 0:
        failures.append("exit code was 0 despite stale gates -- the gate cannot fail")

    # Sanity: an all-fresh set must exit 0, or the gate would be permanently red and
    # would get muted, which is how gates die.
    only_fresh = [verdicts["fresh.yml"], verdicts["boundary.yml"]]
    if _exit_code(only_fresh) != 0:
        failures.append("exit code was non-zero for an all-fresh set")

    print(table)
    if failures:
        print("\nSELF-TEST FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print(f"\nself-test OK: {len(fixtures)} planted shapes, all verdicts as expected")
    return 0


def _exit_code(verdicts: Sequence[Verdict]) -> int:
    return 1 if any(v.stale for v in verdicts) else 0


# --------------------------------------------------------------------------- main


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--self-test", action="store_true", help="prove the checker can fail")
    ap.add_argument("--dry-run", action="store_true", help="query the API but do not touch issues")
    ap.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "PerryTS/perry"))
    args = ap.parse_args(argv)

    if args.self_test:
        return self_test()

    gates, branch = load_gates()
    verdicts = evaluate(gates, args.repo, branch, _utcnow())

    table = render(verdicts)
    print(table)

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as fh:
            fh.write(f"## Gate freshness (`{branch}`)\n\n```\n{table}\n```\n")

    stale = [v for v in verdicts if v.stale]
    for v in stale:
        print(f"::error::{v.gate.workflow} is stale: {v.detail}. {v.gate.why}")

    if not args.dry_run:
        try:
            sync_issue(args.repo, verdicts, branch)
        except RuntimeError as exc:
            # Failing to file the alert must not mask the alert itself.
            print(f"::warning::could not sync the sticky issue: {exc}", file=sys.stderr)

    if stale:
        print(
            f"\n{len(stale)} of {len(verdicts)} gates have no recent completed "
            f"`{branch}` result. See docs/src/testing/ci-gate-scheduling.md",
            file=sys.stderr,
        )
    return _exit_code(verdicts)


if __name__ == "__main__":
    sys.exit(main())
