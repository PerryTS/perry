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
from urllib.parse import parse_qs, urlencode, urlsplit

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

# Bound API work even when a parent workflow has an unusually deep history.
# These are a sampled freshness search: a fresh inspected completion is enough
# evidence to pass; exhausting either bound without fresh evidence fails closed.
MAX_API_REQUESTS_PER_PARENT = 256
MAX_RUN_PAGES_PER_EVENT = 10
RUNS_PER_PAGE = 100
JOBS_PER_PAGE = 100


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
    search_error: str | None = None

    @property
    def stale(self) -> bool:
        return self.age_hours is None or self.age_hours > self.gate.max_age_hours

    @property
    def detail(self) -> str:
        if self.search_error and self.age_hours is not None:
            return (
                f"best inspected result {self.age_hours:.1f}h ago; "
                f"search incomplete: {self.search_error}"
            )
        if self.age_hours is None:
            if self.search_error:
                return f"INCOMPLETE SEARCH: {self.search_error}"
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


class _IncompleteSearch(RuntimeError):
    def __init__(
        self, message: str, best: tuple[str, str, str] | None = None
    ) -> None:
        super().__init__(message)
        self.best = best


class _SearchContext:
    """Request and response cache shared by gates using the same parent run."""

    def __init__(
        self,
        fetch,
        max_requests: int = MAX_API_REQUESTS_PER_PARENT,
        max_run_pages: int = MAX_RUN_PAGES_PER_EVENT,
    ) -> None:
        self.fetch = fetch
        self.max_requests = max_requests
        self.max_run_pages = max_run_pages
        self.requests: dict[tuple[str, str], int] = {}
        self.responses: dict[str, dict] = {}

    def get(self, repo: str, parent: str, path: str) -> dict:
        if path in self.responses:
            return self.responses[path]
        key = (repo, parent)
        used = self.requests.get(key, 0)
        if used >= self.max_requests:
            raise _IncompleteSearch(
                f"request cap {self.max_requests} reached for parent workflow {parent}"
            )
        self.requests[key] = used + 1
        try:
            payload = self.fetch(path)
        except (RuntimeError, ValueError, OSError) as exc:
            raise _IncompleteSearch(f"API request failed for {path}: {exc}") from exc
        if not isinstance(payload, dict):
            raise _IncompleteSearch(f"API response for {path} was not an object")
        self.responses[path] = payload
        return payload


def newest_post_merge_result(
    repo: str,
    gate: Gate,
    branch: str,
    now: _dt.datetime,
    context: _SearchContext,
) -> tuple[str, str, str] | None:
    """Return newest inspected completion, or fail closed on incomplete search.

    Event streams are partitioned because high-rate workflow_dispatch runs can
    crowd scheduled runs off a single creation-ordered page. Pages are inspected
    round-robin by event, and actual selected-job completion timestamps determine
    the winner. A fresh inspected result is sufficient evidence; this bounded
    search does not claim to find the globally newest historical completion.
    """
    best: tuple[str, str, str] | None = None
    exhausted = {event: False for event in ("schedule", "push", "workflow_dispatch")}
    prior_run_pages: dict[str, set[tuple[str, ...]]] = {
        event: set() for event in exhausted
    }

    def is_fresh(result: tuple[str, str, str] | None) -> bool:
        if result is None:
            return False
        age = (now - _parse_ts(result[0])).total_seconds() / 3600.0
        return age <= gate.max_age_hours

    def job_group_result(run: dict) -> tuple[str, str] | None:
        jobs: list[dict] = []
        prior_pages: set[tuple[str, ...]] = set()
        page = 1
        while True:
            path = (
                f"repos/{repo}/actions/runs/{run['id']}/jobs?"
                + urlencode({"per_page": JOBS_PER_PAGE, "page": page})
            )
            payload = context.get(repo, gate.api_workflow, path)
            if "jobs" not in payload:
                raise _IncompleteSearch(f"jobs response for run {run['id']} omitted jobs")
            rows = payload["jobs"]
            if not isinstance(rows, list):
                raise _IncompleteSearch(f"jobs response for run {run['id']} was not a list")
            signature = tuple(
                str(job.get("id") or f"{job.get('name')}:{job.get('started_at')}:{job.get('completed_at')}")
                for job in rows
            )
            if len(rows) == JOBS_PER_PAGE and signature in prior_pages:
                raise _IncompleteSearch(f"repeated full jobs page for run {run['id']}")
            if len(rows) == JOBS_PER_PAGE:
                prior_pages.add(signature)
            jobs.extend(rows)
            if len(rows) < JOBS_PER_PAGE:
                break
            page += 1
        selected = [
            next((job for job in jobs if job.get("name") == name), None)
            for name in gate.job_names
        ]
        if any(job is None for job in selected):
            return None
        if any(
            job.get("status") != "completed"
            or job.get("conclusion") in NON_RESULTS
            or not job.get("completed_at")
            for job in selected
            if job is not None
        ):
            return None
        completed = max(
            (job["completed_at"] for job in selected if job is not None),
            key=_parse_ts,
        )
        conclusion = (
            "success"
            if all(job.get("conclusion") == "success" for job in selected if job is not None)
            else "failure"
        )
        return completed, conclusion

    try:
        for page in range(1, context.max_run_pages + 1):
            for event in ("schedule", "push", "workflow_dispatch"):
                if exhausted[event]:
                    continue
                path = (
                    f"repos/{repo}/actions/workflows/{gate.api_workflow}/runs?"
                    + urlencode(
                        {
                            "branch": branch,
                            "status": "completed",
                            "event": event,
                            "per_page": RUNS_PER_PAGE,
                            "page": page,
                        }
                    )
                )
                payload = context.get(repo, gate.api_workflow, path)
                if "workflow_runs" not in payload:
                    raise _IncompleteSearch(
                        f"runs response for {event} page {page} omitted workflow_runs"
                    )
                rows = payload["workflow_runs"]
                if not isinstance(rows, list):
                    raise _IncompleteSearch(f"runs response for {event} page {page} was not a list")
                signature = tuple(str(run.get("id") or f"{run.get('created_at')}:{run.get('head_sha')}") for run in rows)
                if len(rows) == RUNS_PER_PAGE and signature in prior_run_pages[event]:
                    raise _IncompleteSearch(f"repeated full {event} run page {page}")
                if len(rows) == RUNS_PER_PAGE:
                    prior_run_pages[event].add(signature)
                else:
                    exhausted[event] = True

                for run in rows:
                    if run.get("event") != event or run.get("status") != "completed":
                        continue
                    if gate.job_names:
                        result = job_group_result(run)
                        if result is None:
                            continue
                        completed, conclusion = result
                    else:
                        if run.get("conclusion") in NON_RESULTS:
                            continue
                        completed = run.get("updated_at")
                        conclusion = run.get("conclusion")
                        if not completed or not conclusion:
                            continue
                    candidate = (completed, run.get("head_sha", ""), conclusion)
                    if best is None or _parse_ts(candidate[0]) > _parse_ts(best[0]):
                        best = candidate
                # Once this fetched page has been fully inspected, an in-budget
                # observation proves freshness even if older pages remain.
                if is_fresh(best):
                    return best

            if all(exhausted.values()):
                return best
        raise _IncompleteSearch(
            f"run-page cap {context.max_run_pages} reached for parent workflow {gate.api_workflow}"
        )
    except _IncompleteSearch as exc:
        if is_fresh(best):
            return best
        raise _IncompleteSearch(str(exc), best=best) from exc
    except (AttributeError, KeyError, TypeError, ValueError) as exc:
        message = f"invalid Actions response data for {gate.api_workflow}: {exc}"
        if is_fresh(best):
            return best
        raise _IncompleteSearch(message, best=best) from exc


def evaluate(
    gates: Sequence[Gate],
    repo: str,
    branch: str,
    now: _dt.datetime,
    fetch=_gh_json,
    *,
    max_api_requests: int = MAX_API_REQUESTS_PER_PARENT,
    max_run_pages: int = MAX_RUN_PAGES_PER_EVENT,
) -> list[Verdict]:
    verdicts: list[Verdict] = []
    context = _SearchContext(fetch, max_api_requests, max_run_pages)
    for gate in gates:
        try:
            found = newest_post_merge_result(repo, gate, branch, now, context)
        except _IncompleteSearch as exc:
            # An incomplete search without a fresh observation must remain a
            # stale, explicit error rather than a synthesized no-result verdict.
            print(f"::warning::{gate.workflow}: {exc}", file=sys.stderr)
            if exc.best is None:
                verdicts.append(Verdict(gate, None, None, None, None, str(exc)))
            else:
                completed, sha, conclusion = exc.best
                age = (now - _parse_ts(completed)).total_seconds() / 3600.0
                verdicts.append(
                    Verdict(gate, age, completed, sha, conclusion, str(exc))
                )
            continue
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


def _gh_api(method: str, path: str, fields: dict[str, str] | None = None) -> dict:
    """Call GitHub's REST API without depending on the GraphQL-backed `gh issue`."""
    args = [
        "gh",
        "api",
        "--method",
        method,
        "-H",
        "Accept: application/vnd.github+json",
    ]
    for key, value in (fields or {}).items():
        args.extend(["-f", f"{key}={value}"])
    args.append(path)
    proc = subprocess.run(args, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"gh api {method} {path} failed: {proc.stderr.strip()}")
    return json.loads(proc.stdout) if proc.stdout.strip() else {}


def sync_issue(
    repo: str,
    verdicts: Sequence[Verdict],
    branch: str,
    request=_gh_api,
) -> None:
    """Open, update, or close the single sticky alert issue. Never duplicates."""
    found = request(
        "GET",
        "search/issues",
        {
            "q": f'repo:{repo} is:issue in:title "{ISSUE_MARKER}"',
            "per_page": "100",
        },
    )
    matches = [i for i in found.get("items", []) if ISSUE_MARKER in i.get("title", "")]
    # Historical episodes may predate the current sticky issue (#7966 preceded
    # #8085). Reuse the newest match so a later recurrence returns to the issue that
    # the previous sweep most recently maintained.
    existing = max(matches, key=lambda i: i["number"], default=None)
    stale = [v for v in verdicts if v.stale]

    if not stale:
        if existing is not None and existing.get("state") == "open":
            number = existing["number"]
            request(
                "PATCH",
                f"repos/{repo}/issues/{number}",
                {"state": "closed", "state_reason": "completed"},
            )
            request(
                "POST",
                f"repos/{repo}/issues/{number}/comments",
                {"body": "All post-merge gates are fresh again; closing automatically."},
            )
            print(f"closed sticky issue #{number} (all gates fresh)")
        return

    title = f"{ISSUE_MARKER}: {len(stale)} gate(s) have no recent completed `{branch}` result"
    body = issue_body(verdicts, branch)
    if existing is None:
        created = request(
            "POST", f"repos/{repo}/issues", {"title": title, "body": body}
        )
        print(f"opened sticky issue {created.get('html_url', '')}".rstrip())
    else:
        number = existing["number"]
        request(
            "PATCH",
            f"repos/{repo}/issues/{number}",
            {"state": "open", "title": title, "body": body},
        )
        action = "reopened" if existing.get("state") != "open" else "updated"
        print(f"{action} sticky issue #{number}")


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
        run_id: int | None = None,
    ) -> dict:
        result = {
            "event": event,
            "status": status,
            "conclusion": conclusion,
            "created_at": stamp(created_hours_ago),
            "updated_at": stamp(completed_hours_ago) if completed_hours_ago is not None else None,
            "head_sha": sha,
        }
        if run_id is not None:
            result["id"] = run_id
        return result

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
                **run("push", "completed", "success", 1, 0.5, "ggx"),
                "id": 105,
            },
            {
                **run("push", "completed", "failure", 2, 1, "ggg"),
                "id": 102,
            },
            {
                **run("push", "completed", "success", 73, 72, "hhh"),
                "id": 103,
            },
        ],
        "paged.yml": [{**run("push", "completed", "failure", 3, 1, "iii"), "id": 104}],
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
        104: ([{"name": f"noise {i}", "status": "completed", "conclusion": "success", "completed_at": stamp(2)} for i in range(100)]
              + [{"name": "audit / required", "status": "completed", "conclusion": "failure", "completed_at": stamp(1)}]),
        105: [
            {"name": "audit / first", "status": "completed", "conclusion": "success", "completed_at": stamp(0.5)},
            {"name": "audit / second", "status": "completed", "conclusion": "skipped", "completed_at": stamp(0.5)},
        ],
    }

    fake_calls: list[str] = []

    def fake_fetch(path: str) -> dict:
        fake_calls.append(path)
        parsed = urlsplit(path)
        query = parse_qs(parsed.query)
        if "/actions/runs/" in parsed.path:
            run_id = int(parsed.path.split("/actions/runs/")[1].split("/jobs")[0])
            if "page" in query:
                page = int(query["page"][0])
                rows = job_fixtures[run_id]
                return {"jobs": rows[(page - 1) * 100 : page * 100]}
            return {"jobs": job_fixtures[run_id]}
        wf = parsed.path.split("/actions/workflows/")[1].split("/runs")[0]
        rows = fixtures[wf]
        if "event" in query:
            rows = [row for row in rows if row.get("event") == query["event"][0]]
        page = int(query.get("page", ["1"])[0])
        return {"workflow_runs": rows[(page - 1) * RUNS_PER_PAGE : page * RUNS_PER_PAGE]}

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
        Gate("paged-subject.yml", 12, "self-test", source_workflow="paged.yml", job_names=("audit / required",)),
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
    expect("paged-subject.yml", False, "a required job on page two is found")
    if not any("/actions/runs/104/jobs" in path and "page=2" in path for path in fake_calls):
        failures.append("nested selected-job pagination did not request jobs page two")

    def offline_fetch(run_map: dict[str, list[dict]], job_map: dict[int, list[dict]], calls: list[str]):
        def fetch(path: str) -> dict:
            calls.append(path)
            parsed = urlsplit(path)
            query = parse_qs(parsed.query)
            if "/actions/runs/" in parsed.path:
                run_id = int(parsed.path.split("/actions/runs/")[1].split("/jobs")[0])
                page = int(query.get("page", ["1"])[0])
                rows = job_map.get(run_id, [])
                return {"jobs": rows[(page - 1) * JOBS_PER_PAGE : page * JOBS_PER_PAGE]}
            workflow = parsed.path.split("/actions/workflows/")[1].split("/runs")[0]
            event = query["event"][0]
            rows = [row for row in run_map.get(workflow, []) if row.get("event") == event]
            page = int(query.get("page", ["1"])[0])
            start = (page - 1) * RUNS_PER_PAGE
            return {"workflow_runs": rows[start : start + RUNS_PER_PAGE]}

        return fetch

    # Fifty unrelated dispatches used to displace the daily scheduled result
    # from the single mixed page. Event partitioning exposes the failed run.
    displaced_runs = [
        {**run("workflow_dispatch", "completed", "success", 1, 0.5, f"noise-{i}"), "id": 500 + i}
        for i in range(50)
    ] + [{**run("schedule", "completed", "failure", 40, 25, "daily"), "id": 600}]
    displaced_jobs = {
        600: [{"name": "npm-publish-freshness / npm-publish-freshness", "status": "completed", "conclusion": "failure", "completed_at": stamp(25)}]
    }
    calls: list[str] = []
    displaced_gate = Gate(
        "npm-publish-freshness.yml",
        30,
        "self-test",
        source_workflow="maintenance.yml",
        job_names=("npm-publish-freshness / npm-publish-freshness",),
    )
    displaced = evaluate(
        [displaced_gate],
        "o/r",
        "main",
        now,
        fetch=offline_fetch({"maintenance.yml": displaced_runs}, displaced_jobs, calls),
    )[0]
    if displaced.stale or displaced.conclusion != "failure" or displaced.age_hours != 25:
        failures.append("event partitioning did not preserve the older in-budget failed schedule result")
    if not any("event=schedule" in path for path in calls):
        failures.append("freshness search did not request the scheduled event partition")

    # The observed 42-run scheduled queue costs 43 calls: one parent page and
    # one jobs page per run. Keep the configured request budget comfortably
    # above this known maintenance traffic while still bounding runaway search.
    batch_runs = [
        {**run("schedule", "completed", "failure", 40, 25, f"batch-{i}"), "id": 620 + i}
        for i in range(42)
    ]
    batch_jobs = {
        620 + i: ([{
            "name": "npm-publish-freshness / npm-publish-freshness",
            "status": "completed",
            "conclusion": "failure",
            "completed_at": stamp(25),
        }] if i == 41 else [])
        for i in range(42)
    }
    batch_calls: list[str] = []
    batch_result = evaluate(
        [displaced_gate],
        "o/r",
        "main",
        now,
        fetch=offline_fetch({"maintenance.yml": batch_runs}, batch_jobs, batch_calls),
    )[0]
    if batch_result.stale or batch_result.conclusion != "failure" or len(batch_calls) != 43:
        failures.append("42-run scheduled traffic did not complete within its calibrated 43-request search")

    # Round-robin pagination reaches page two even when another event also has
    # a full noise page. Creation order does not determine the winning result.
    page_runs = [
        run("schedule", "completed", "failure", 50, 40, f"old-{i}")
        for i in range(RUNS_PER_PAGE)
    ] + [run("schedule", "completed", "failure", 100, 2, "late-completion")] + [
        run("workflow_dispatch", "completed", "failure", 3, 40, f"dispatch-{i}")
        for i in range(RUNS_PER_PAGE)
    ]
    calls = []
    paged = evaluate(
        [Gate("page-two.yml", 12, "self-test")],
        "o/r",
        "main",
        now,
        fetch=offline_fetch({"page-two.yml": page_runs}, {}, calls),
    )[0]
    if paged.stale or paged.conclusion != "failure" or paged.last_sha != "late-completion":
        failures.append("round-robin bounded search did not find the fresh schedule result on run page two")
    if not any("event=schedule" in path and "page=2" in path for path in calls):
        failures.append("run page two was not requested")

    # The newer-created run completed earlier; select by actual completion.
    inverted_runs = [
        run("schedule", "completed", "failure", 8, 1, "created-older-completed-newer"),
        run("schedule", "completed", "success", 6, 5, "created-newer-completed-older"),
    ]
    inverted = evaluate(
        [Gate("inverted.yml", 12, "self-test")],
        "o/r",
        "main",
        now,
        fetch=offline_fetch({"inverted.yml": inverted_runs}, {}, []),
    )[0]
    if inverted.last_result != stamp(1) or inverted.last_sha != "created-older-completed-newer":
        failures.append("freshness selection used run creation order instead of completion time")

    nested_inverted_runs = [
        {**run("schedule", "completed", "failure", 20, 0.5, "parent-completed-newer"), "id": 901},
        {**run("schedule", "completed", "success", 10, 1, "parent-completed-older"), "id": 902},
    ]
    nested_inverted_jobs = {
        901: [{"name": "nested subject", "status": "completed", "conclusion": "failure", "completed_at": stamp(10)}],
        902: [{"name": "nested subject", "status": "completed", "conclusion": "success", "completed_at": stamp(2)}],
    }
    nested_inverted = evaluate(
        [Gate("nested-inverted.yml", 12, "self-test", source_workflow="nested-inverted-parent.yml", job_names=("nested subject",))],
        "o/r",
        "main",
        now,
        fetch=offline_fetch({"nested-inverted-parent.yml": nested_inverted_runs}, nested_inverted_jobs, []),
    )[0]
    if nested_inverted.last_result != stamp(2) or nested_inverted.last_sha != "parent-completed-older":
        failures.append("nested freshness selection did not use the latest selected-job completion")

    # No fresh observation before a request cap, API failure, repeated page, or
    # configured page limit must produce an explicit incomplete-search verdict.
    stale_full_page = [
        run("schedule", "completed", "success", 50, 49, f"stale-{i}")
        for i in range(RUNS_PER_PAGE)
    ]
    cap_runs = {"capped.yml": stale_full_page}
    cap_calls: list[str] = []
    cap = evaluate(
        [Gate("capped.yml", 12, "self-test")],
        "o/r",
        "main",
        now,
        fetch=offline_fetch(cap_runs, {}, cap_calls),
        max_api_requests=1,
    )[0]
    if not cap.stale or "search incomplete" not in cap.detail or "request cap 1" not in cap.detail:
        failures.append("request-cap exhaustion did not fail closed with an explicit reason")
    if len(cap_calls) != 1:
        failures.append(f"request cap 1 allowed {len(cap_calls)} API calls")

    # A realistic noisy nested parent can consume the bounded budget inspecting
    # page-one job groups before reaching later event pages. It must say that
    # search was incomplete, never claim the gate had no result historically.
    nested_noise_runs = [
        {**run("schedule", "completed", "failure", 50, 40, f"nested-noise-{i}"), "id": 1000 + i}
        for i in range(RUNS_PER_PAGE)
    ] + [
        {**run("push", "completed", "failure", 50, 40, f"nested-push-{i}"), "id": 2000 + i}
        for i in range(RUNS_PER_PAGE)
    ] + [
        {**run("workflow_dispatch", "completed", "failure", 50, 40, f"nested-dispatch-{i}"), "id": 3000 + i}
        for i in range(RUNS_PER_PAGE)
    ] + [
        {**run("schedule", "completed", "failure", 50, 2, "nested-page-two"), "id": 4000}
    ]
    nested_budget_calls: list[str] = []
    nested_budget = evaluate(
        [Gate("nested-budget.yml", 12, "self-test", source_workflow="nested-budget-parent.yml", job_names=("missing subject",))],
        "o/r",
        "main",
        now,
        fetch=offline_fetch({"nested-budget-parent.yml": nested_noise_runs}, {}, nested_budget_calls),
    )[0]
    if not nested_budget.stale or "request cap 256" not in nested_budget.detail or len(nested_budget_calls) != 256:
        failures.append(
            "nested job scans exceeded their request budget or hid incomplete-search status: "
            f"{nested_budget.detail}; calls={len(nested_budget_calls)}"
        )
    if any("event=schedule" in path and "page=2" in path for path in nested_budget_calls):
        failures.append("nested request-cap fixture unexpectedly advanced past its page-one job scans")

    page_cap = evaluate(
        [Gate("capped.yml", 12, "self-test")],
        "o/r",
        "main",
        now,
        fetch=offline_fetch(cap_runs, {}, []),
        max_run_pages=1,
    )[0]
    if not page_cap.stale or "run-page cap 1" not in page_cap.detail:
        failures.append("run-page exhaustion did not fail closed with an explicit reason")

    def api_failure(_path: str) -> dict:
        raise RuntimeError("synthetic API outage")

    failed_search = evaluate(
        [Gate("api-error.yml", 12, "self-test")], "o/r", "main", now, fetch=api_failure
    )[0]
    if not failed_search.stale or "API request failed" not in failed_search.detail:
        failures.append("API failure did not report an explicit incomplete-search verdict")

    repeated_fetch_calls: list[str] = []
    repeated_page = [run("schedule", "completed", "success", 50, 49, f"repeat-{i}") for i in range(100)]

    def repeating_fetch(path: str) -> dict:
        parsed = urlsplit(path)
        if "/actions/runs/" not in parsed.path:
            repeated_fetch_calls.append(path)
            return {"workflow_runs": repeated_page}
        return {"jobs": []}

    repeated = evaluate(
        [Gate("repeat.yml", 12, "self-test")], "o/r", "main", now, fetch=repeating_fetch
    )[0]
    if "repeated full schedule run page" not in repeated.detail or len(repeated_fetch_calls) > 5:
        failures.append("repeated full run pages were not bounded and diagnosed")

    # If the request cap interrupts later work, an already-observed fresh
    # completion is still sufficient execution evidence.
    cap_fresh_runs = [
        {**run("schedule", "completed", "failure", 3, 1, "fresh-before-cap"), "id": 701},
        {**run("schedule", "completed", "success", 3, 1, "second-group"), "id": 702},
    ]
    cap_fresh_jobs = {
        701: [{"name": "subject", "status": "completed", "conclusion": "failure", "completed_at": stamp(1)}],
    }
    cap_fresh = evaluate(
        [Gate("cap-fresh.yml", 12, "self-test", source_workflow="cap-parent.yml", job_names=("subject",))],
        "o/r",
        "main",
        now,
        fetch=offline_fetch({"cap-parent.yml": cap_fresh_runs}, cap_fresh_jobs, []),
        max_api_requests=2,
    )[0]
    if cap_fresh.stale or cap_fresh.last_sha != "fresh-before-cap":
        failures.append("request exhaustion discarded already-observed fresh evidence")

    # Parent run and jobs pages are cached across logical gates sharing a source.
    shared_runs = [{**run("schedule", "completed", "failure", 3, 1, "shared"), "id": 801}]
    shared_jobs = [{
        "name": name,
        "status": "completed",
        "conclusion": "success",
        "completed_at": stamp(1),
    } for name in ("subject A", "subject B")]
    shared_calls: list[str] = []
    shared = evaluate(
        [
            Gate("logical-a.yml", 12, "self-test", source_workflow="shared-parent.yml", job_names=("subject A",)),
            Gate("logical-b.yml", 12, "self-test", source_workflow="shared-parent.yml", job_names=("subject B",)),
        ],
        "o/r",
        "main",
        now,
        fetch=offline_fetch({"shared-parent.yml": shared_runs}, {801: shared_jobs}, shared_calls),
    )
    if any(verdict.stale for verdict in shared) or len(shared_calls) != 2:
        failures.append("shared parent runs/jobs were not cached across logical gates")

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

    # A later stale episode must reopen the original sticky issue. Searching only
    # open issues makes the first successful close orphan the discussion and creates
    # a new issue on every recurrence while claiming to maintain one forever.
    issue_calls: list[tuple[str, str, dict[str, str] | None]] = []

    def closed_issue_request(
        method: str, path: str, fields: dict[str, str] | None = None
    ) -> dict:
        issue_calls.append((method, path, fields))
        if method == "GET" and path == "search/issues":
            return {
                "items": [
                    {
                        "number": 7966,
                        "title": f"{ISSUE_MARKER}: older episode",
                        "state": "closed",
                    },
                    {
                        "number": 8085,
                        "title": f"{ISSUE_MARKER}: prior episode",
                        "state": "closed",
                    },
                ]
            }
        return {}

    sync_issue(
        "PerryTS/perry",
        [verdicts["stale.yml"]],
        "main",
        request=closed_issue_request,
    )
    if len(issue_calls) != 2 or issue_calls[1][:2] != (
        "PATCH",
        "repos/PerryTS/perry/issues/8085",
    ):
        failures.append(
            "closed sticky issue was not reused via REST; expected search/PATCH of "
            f"#8085, got {[(method, path) for method, path, _ in issue_calls]}"
        )
    elif issue_calls[1][2] is None or issue_calls[1][2].get("state") != "open":
        failures.append("closed sticky issue PATCH did not reopen the issue")

    # The healthy path must close first and then comment, entirely through REST.
    issue_calls.clear()

    def open_issue_request(
        method: str, path: str, fields: dict[str, str] | None = None
    ) -> dict:
        issue_calls.append((method, path, fields))
        if method == "GET" and path == "search/issues":
            return {
                "items": [
                    {
                        "number": 8085,
                        "title": f"{ISSUE_MARKER}: active episode",
                        "state": "open",
                    }
                ]
            }
        return {}

    sync_issue("PerryTS/perry", only_fresh, "main", request=open_issue_request)
    close_shape = [(method, path) for method, path, _ in issue_calls]
    if close_shape != [
        ("GET", "search/issues"),
        ("PATCH", "repos/PerryTS/perry/issues/8085"),
        ("POST", "repos/PerryTS/perry/issues/8085/comments"),
    ]:
        failures.append(
            "fresh sticky issue did not close/comment through REST in order; got "
            f"{close_shape}"
        )
    elif issue_calls[1][2] != {"state": "closed", "state_reason": "completed"}:
        failures.append("fresh sticky issue PATCH did not mark the issue completed")

    print(table)
    if failures:
        print("\nSELF-TEST FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print(f"\nself-test OK: base fixtures plus bounded-search controls, all verdicts as expected")
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
