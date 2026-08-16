#!/usr/bin/env python3
"""Fail when a post-merge gate has not produced a successful `main` run recently.

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
recent SUCCESSFUL run on the default branch, and fails when that run is older than the
gate's budget.

Two things it deliberately does NOT count as evidence of health:

  * **pull_request runs.** PR runs supersede each other and therefore drain even when
    the queue is saturated. Counting them is exactly what made `gc-root-dominance`
    look healthy in #7856 while its `main` arm had been dark for two days.
  * **runs that merely exist.** `queued`, `in_progress` and `cancelled` runs are not
    results. A gate with 22 queued `main` runs and no successful one is a dark gate,
    which is the whole point.

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


def _utcnow() -> _dt.datetime:
    return _dt.datetime.now(_dt.timezone.utc)


def _parse_ts(value: str) -> _dt.datetime:
    return _dt.datetime.fromisoformat(value.replace("Z", "+00:00"))


@dataclass(frozen=True)
class Gate:
    workflow: str
    max_age_hours: float
    why: str


@dataclass(frozen=True)
class Verdict:
    gate: Gate
    age_hours: float | None  # None => no qualifying successful run at all
    last_success: str | None
    last_sha: str | None

    @property
    def stale(self) -> bool:
        return self.age_hours is None or self.age_hours > self.gate.max_age_hours

    @property
    def detail(self) -> str:
        if self.age_hours is None:
            return "NO successful post-merge run in the sampled window"
        return f"last success {self.age_hours:.1f}h ago (budget {self.gate.max_age_hours:g}h)"


def load_gates(manifest: Path = MANIFEST) -> tuple[list[Gate], str]:
    data = json.loads(manifest.read_text())
    gates = [
        Gate(
            workflow=g["workflow"],
            max_age_hours=float(g["max_age_hours"]),
            why=g.get("why", ""),
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


def newest_post_merge_success(
    repo: str, workflow: str, branch: str, fetch=_gh_json
) -> tuple[str, str] | None:
    """Return (created_at, head_sha) of the newest successful post-merge run, or None.

    Asks for successful runs on the branch and then filters by event. The API's
    `?event=` parameter takes a single value, so filtering client-side is what lets one
    request cover push + schedule + workflow_dispatch.
    """
    path = (
        f"repos/{repo}/actions/workflows/{workflow}/runs"
        f"?branch={branch}&status=success&per_page=50"
    )
    payload = fetch(path)
    for run in payload.get("workflow_runs", []):
        if run.get("event") in POST_MERGE_EVENTS:
            return run["created_at"], run.get("head_sha", "")
    return None


def evaluate(
    gates: Sequence[Gate], repo: str, branch: str, now: _dt.datetime, fetch=_gh_json
) -> list[Verdict]:
    verdicts: list[Verdict] = []
    for gate in gates:
        try:
            found = newest_post_merge_success(repo, gate.workflow, branch, fetch=fetch)
        except RuntimeError as exc:
            # A workflow file that no longer exists, or an API failure, must not be
            # silently treated as "fresh". Report it as stale with no timestamp.
            print(f"::warning::{gate.workflow}: {exc}", file=sys.stderr)
            found = None
        if found is None:
            verdicts.append(Verdict(gate, None, None, None))
            continue
        created, sha = found
        age = (now - _parse_ts(created)).total_seconds() / 3600.0
        verdicts.append(Verdict(gate, age, created, sha))
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
        f"The following post-merge gates have not produced a successful `{branch}` run "
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

    title = f"{ISSUE_MARKER}: {len(stale)} gate(s) have no recent successful `{branch}` run"
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

    fixtures = {
        # fresh: 2h old scheduled success
        "fresh.yml": [{"event": "schedule", "created_at": stamp(2), "head_sha": "aaa"}],
        # stale: last success 3 days ago -- the #7856 shape
        "stale.yml": [{"event": "push", "created_at": stamp(72), "head_sha": "bbb"}],
        # no successful post-merge run at all
        "never.yml": [],
        # THE TRAP: plenty of recent successes, but all of them pull_request runs.
        # This is what made gc-root-dominance look healthy while its main arm was dark.
        "pronly.yml": [
            {"event": "pull_request", "created_at": stamp(0.5), "head_sha": "ccc"},
            {"event": "pull_request", "created_at": stamp(1.0), "head_sha": "ddd"},
        ],
        # boundary: exactly at budget is NOT stale; just past it is.
        "boundary.yml": [{"event": "schedule", "created_at": stamp(12), "head_sha": "eee"}],
    }

    def fake_fetch(path: str) -> dict:
        wf = path.split("/actions/workflows/")[1].split("/runs")[0]
        return {"workflow_runs": fixtures[wf]}

    gates = [Gate(w, 12, "self-test") for w in fixtures]
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
    expect("stale.yml", True, "a 3-day-old success is stale")
    expect("never.yml", True, "no successful post-merge run at all is stale")
    expect("pronly.yml", True, "pull_request runs alone do NOT count as fresh")
    expect("boundary.yml", False, "exactly at budget is not yet stale")

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
            f"\n{len(stale)} of {len(verdicts)} gates have no recent successful "
            f"`{branch}` run. See docs/src/testing/ci-gate-scheduling.md",
            file=sys.stderr,
        )
    return _exit_code(verdicts)


if __name__ == "__main__":
    sys.exit(main())
