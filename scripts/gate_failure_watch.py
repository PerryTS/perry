#!/usr/bin/env python3
"""Maintain one issue per failing scheduled/main gate workflow.

GitHub's red Actions result is passive state. This observer turns completed
scheduled, main-dispatch, main-push, and release-tag gate failures into a
durable issue. The issue records the run, head SHA, failed job/step rows, the
delta from the prior failed run, and the last green main SHA. A later green run
closes the same issue; a recurrence reopens it instead of creating a duplicate.

The workflow_run job executes this file from the default branch. Pull requests
exercise only --self-test and --check-config without an issue-writing token.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import subprocess
import sys
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Any


ROOT = pathlib.Path(__file__).resolve().parent.parent
CONFIG = ROOT / "scripts/gate_failure_watch.json"
FRESHNESS_CONFIG = ROOT / "scripts/gate_freshness.json"
WATCH_WORKFLOW = ROOT / ".github/workflows/maintenance.yml"
WORKFLOW_DIR = ROOT / ".github/workflows"
RED_CONCLUSIONS = {"action_required", "failure", "startup_failure", "timed_out"}
ApiRequest = Callable[[str, str, dict[str, str] | None], dict[str, Any]]


@dataclass(frozen=True)
class WatchConfig:
    branch: str
    tag_pattern: str
    workflows: dict[str, list[dict[str, Any]]]
    excluded: dict[str, str]


def load_config(path: pathlib.Path = CONFIG) -> WatchConfig:
    raw = json.loads(path.read_text())
    workflows = {
        item["path"]: item.get("subjects", [{"name": item["name"], "id": item["path"]}])
        for item in raw["workflows"]
    }
    excluded = {item["path"]: item["why"] for item in raw.get("excluded", [])}
    if len(workflows) != len(raw["workflows"]):
        raise ValueError("gate_failure_watch.json contains duplicate workflow paths")
    if not raw.get("default_branch") or not raw.get("tag_pattern"):
        raise ValueError("default_branch and tag_pattern are required")
    return WatchConfig(raw["default_branch"], raw["tag_pattern"], workflows, excluded)


def workflow_path(run: dict[str, Any]) -> str:
    path = str(run.get("path") or "").split("@", 1)[0]
    return pathlib.PurePosixPath(path).name


def eligible_run(run: dict[str, Any], config: WatchConfig) -> bool:
    """Whether RUN is a post-merge result this observer owns."""
    if workflow_path(run) not in config.workflows:
        return False
    event = run.get("event")
    branch = run.get("head_branch")
    if event == "schedule":
        return branch == config.branch
    if event in {"repository_dispatch", "workflow_dispatch"}:
        return branch == config.branch
    if event == "push":
        return branch == config.branch or bool(re.match(config.tag_pattern, branch or ""))
    return False


def failure_rows(jobs: Sequence[dict[str, Any]]) -> list[str]:
    """Stable failed job/step identifiers, including matrix job names."""
    rows: set[str] = set()
    for job in jobs:
        if job.get("conclusion") not in RED_CONCLUSIONS:
            continue
        job_name = str(job.get("name") or "unnamed job")
        failed_steps = [
            str(step.get("name") or "unnamed step")
            for step in job.get("steps") or []
            if step.get("conclusion") in RED_CONCLUSIONS
        ]
        if failed_steps:
            rows.update(f"{job_name} / {step}" for step in failed_steps)
        else:
            rows.add(job_name)
    return sorted(rows)


def subject_jobs(jobs: Sequence[dict[str, Any]], subject: dict[str, Any]) -> list[dict[str, Any]]:
    """Select the reusable caller and all its nested jobs for one logical suite."""
    exact_names = subject.get("job_names")
    if exact_names:
        return [job for job in jobs if job.get("name") in exact_names]
    prefix = str(subject["job"])
    return [job for job in jobs if str(job.get("name", "")).startswith(prefix + " / ") or job.get("name") == prefix]


def subject_verdict(jobs: Sequence[dict[str, Any]], subject: dict[str, Any]) -> str | None:
    """Return success/failure only when this suite was selected and completed."""
    rows = subject_jobs(jobs, subject)
    if not rows:
        return None
    if subject.get("job_names"):
        completed = [job for job in rows if job.get("status") == "completed"]
        if not completed:
            return None
        row = max(completed, key=lambda job: str(job.get("completed_at") or ""))
        value = row.get("conclusion")
        return "failure" if value in RED_CONCLUSIONS else ("success" if value == "success" else None)
    caller = next((job for job in rows if job.get("name") == subject["job"]), None)
    if caller and caller.get("conclusion") == "skipped":
        return None
    if caller and len(rows) == 1 and rows[0].get("name") == subject["job"]:
        # GitHub sometimes reports a reusable call only as one caller row.
        if rows[0].get("status") != "completed": return None
        value=rows[0].get("conclusion")
        return "failure" if value in RED_CONCLUSIONS else ("success" if value == "success" else None)
    nested=[job for job in rows if job is not caller and job.get("name") != subject["job"]]
    if not nested or any(job.get("status") != "completed" for job in nested):
        return None
    conclusions = [job.get("conclusion") for job in nested]
    if any(value in RED_CONCLUSIONS for value in conclusions):
        return "failure"
    if all(value == "success" for value in conclusions):
        return "success"
    return None


def _gh_api(method: str, path: str, fields: dict[str, str] | None = None) -> dict[str, Any]:
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


def get_jobs(repo: str, run_id: int, request: ApiRequest = _gh_api) -> list[dict[str, Any]]:
    rows=[]; page=1
    while True:
        response = request(
            "GET",
            f"repos/{repo}/actions/runs/{run_id}/jobs",
            {"filter": "all", "per_page": "100", "page": str(page)},
        )
        batch=list(response.get("jobs") or []); rows.extend(batch)
        if len(batch)<100: return rows
        page+=1


def get_history(
    repo: str, workflow_id: int, branch: str, request: ApiRequest = _gh_api
) -> list[dict[str, Any]]:
    response = request(
        "GET",
        f"repos/{repo}/actions/workflows/{workflow_id}/runs",
        {"branch": branch, "status": "completed", "per_page": "100"},
    )
    return list(response.get("workflow_runs") or [])


def previous_runs(
    current: dict[str, Any], history: Sequence[dict[str, Any]], config: WatchConfig
) -> tuple[dict[str, Any] | None, dict[str, Any] | None]:
    current_created = str(current.get("created_at") or "")
    prior = [
        run
        for run in history
        if run.get("id") != current.get("id")
        and eligible_run(run, config)
        and str(run.get("created_at") or "") < current_created
    ]
    prior.sort(key=lambda run: str(run.get("created_at") or ""), reverse=True)
    failed = next((run for run in prior if run.get("conclusion") in RED_CONCLUSIONS), None)
    green = next((run for run in prior if run.get("conclusion") == "success"), None)
    return failed, green


def issue_marker(path: str) -> str:
    return f"[scheduled-gate:{path}]"


def _run_link(run: dict[str, Any] | None, fallback: str) -> str:
    if run is None:
        return fallback
    url = run.get("html_url") or ""
    sha = str(run.get("head_sha") or "unknown")
    label = f"`{sha[:12]}`"
    return f"[{label}]({url})" if url else label


def _bullet_rows(rows: Sequence[str], empty: str) -> list[str]:
    return [f"- `{row}`" for row in rows] if rows else [f"- {empty}"]


def issue_body(
    path: str,
    name: str,
    current: dict[str, Any],
    current_rows: Sequence[str],
    previous: dict[str, Any] | None,
    previous_rows: Sequence[str],
    last_green: dict[str, Any] | None,
) -> str:
    current_set = set(current_rows)
    previous_set = set(previous_rows)
    added = sorted(current_set - previous_set)
    resolved = sorted(previous_set - current_set)
    run_url = current.get("html_url") or ""
    sha = str(current.get("head_sha") or "unknown")
    lines = [
        f"`{name}` has a completed red post-merge run.",
        "",
        "## Current failure",
        "",
        f"- Run: [{current.get('id')}]({run_url})",
        f"- Head: `{sha}`",
        f"- Trigger: `{current.get('event') or 'unknown'}`",
        "",
        "## Failing rows",
        "",
        *_bullet_rows(current_rows, "The API reported no failed job or step; inspect the run."),
        "",
        "## Delta from the previous failed run",
        "",
    ]
    if previous is None:
        lines.append("No earlier failed post-merge run was found in the last 100 results.")
    else:
        lines += [
            f"Previous failure: {_run_link(previous, 'unknown')}",
            "",
            "New failing rows:",
            *_bullet_rows(added, "None."),
            "",
            "Rows that recovered:",
            *_bullet_rows(resolved, "None."),
        ]
    lines += [
        "",
        "## Last green main result",
        "",
        _run_link(last_green, "No green post-merge run was found in the last 100 results."),
        "",
        f"_Maintained automatically by `gate-failure-watch.yml` for `{path}`. "
        "Repeated failures update this issue; the next green run closes it._",
    ]
    return "\n".join(lines)


def find_issue(
    repo: str, marker: str, request: ApiRequest = _gh_api
) -> dict[str, Any] | None:
    found = request(
        "GET",
        "search/issues",
        {"q": f'repo:{repo} is:issue in:title "{marker}"', "per_page": "100"},
    )
    matches = [item for item in found.get("items") or [] if marker in item.get("title", "")]
    return max(matches, key=lambda item: int(item["number"]), default=None)


def sync_failure_issue(
    repo: str,
    path: str,
    name: str,
    body: str,
    request: ApiRequest = _gh_api,
) -> None:
    marker = issue_marker(path)
    title = f"{marker} {name} is failing on main"
    existing = find_issue(repo, marker, request)
    if existing is None:
        created = request("POST", f"repos/{repo}/issues", {"title": title, "body": body})
        print(f"opened gate failure issue {created.get('html_url', '')}".rstrip())
        return
    number = int(existing["number"])
    request(
        "PATCH",
        f"repos/{repo}/issues/{number}",
        {"state": "open", "title": title, "body": body},
    )
    action = "reopened" if existing.get("state") != "open" else "updated"
    print(f"{action} gate failure issue #{number}")


def close_failure_issue(
    repo: str,
    path: str,
    name: str,
    run: dict[str, Any],
    request: ApiRequest = _gh_api,
) -> None:
    existing = find_issue(repo, issue_marker(path), request)
    if existing is None or existing.get("state") != "open":
        print(f"{name}: green; no open failure issue")
        return
    number = int(existing["number"])
    sha = str(run.get("head_sha") or "unknown")
    url = run.get("html_url") or ""
    request(
        "POST",
        f"repos/{repo}/issues/{number}/comments",
        {"body": f"Green again at [`{sha[:12]}`]({url}); closing automatically."},
    )
    request(
        "PATCH",
        f"repos/{repo}/issues/{number}",
        {"state": "closed", "state_reason": "completed"},
    )
    print(f"closed gate failure issue #{number}")


def handle_event(
    payload: dict[str, Any],
    repo: str,
    config: WatchConfig,
    request: ApiRequest = _gh_api,
    dry_run: bool = False,
) -> int:
    run = payload.get("workflow_run") or {}
    path = workflow_path(run)
    if not eligible_run(run, config):
        print(
            f"skip: {path or run.get('name') or 'unknown workflow'} "
            f"event={run.get('event')} branch={run.get('head_branch')}"
        )
        return 0
    jobs = get_jobs(repo, int(run["id"]), request)
    for subject in config.workflows[path]:
        name = subject["name"]
        subject_id = str(subject["id"])
        verdict = subject_verdict(jobs, subject)
        if verdict is None:
            print(f"skip: {name} was not selected or has not completed")
            continue
        logical_path = str(subject.get("path", path))
        if verdict == "success":
            if not dry_run:
                close_failure_issue(repo, logical_path, name, run, request)
            else:
                print(f"dry-run: would close {issue_marker(logical_path)} after green run")
            continue
        rows = failure_rows(subject_jobs(jobs, subject))
        history = get_history(repo, int(run["workflow_id"]), config.branch, request)
        previous, last_green = previous_runs(run, history, config)
        previous_rows = (
            failure_rows(subject_jobs(get_jobs(repo, int(previous["id"]), request), subject))
            if previous else []
        )
        body = issue_body(logical_path, name, run, rows, previous, previous_rows, last_green)
        print(body)
        if not dry_run:
            sync_failure_issue(repo, logical_path, name, body, request)
    return 0


def check_config() -> int:
    try:
        import yaml
    except ImportError:
        print("PyYAML is required for --check-config", file=sys.stderr)
        return 2

    config = load_config()
    failures: list[str] = []
    for path, subjects in config.workflows.items():
        workflow_file = WORKFLOW_DIR / path
        if not workflow_file.is_file():
            failures.append(f"watched workflow does not exist: {path}")
            continue
        actual_name = (yaml.safe_load(workflow_file.read_text()) or {}).get("name")
        if not actual_name:
            failures.append(f"{path}: workflow has no display name")
        workflow = yaml.load(workflow_file.read_text(), Loader=yaml.BaseLoader) or {}
        jobs = workflow.get("jobs") or {}
        seen_subjects: set[str] = set()
        for subject in subjects:
            for required in ("id", "name", "path"):
                if not subject.get(required):
                    failures.append(f"{path}: subject has empty/missing {required}: {subject!r}")
            selector = subject.get("job_names") or ([subject.get("job")] if subject.get("job") else [])
            if not selector or any(not value for value in selector):
                failures.append(f"{path}: subject {subject.get('id')} has no job selector")
            logical_path = str(subject.get("path") or "")
            if logical_path in seen_subjects:
                failures.append(f"{path}: duplicate logical subject path {logical_path}")
            seen_subjects.add(logical_path)
            if subject.get("job") and subject["job"] not in jobs:
                failures.append(f"{path}: subject {subject.get('id')} selects missing caller job {subject['job']}")
            if subject.get("job_names") and any(name not in {"pr-gate", "main-gate", "full-suite-gate"} for name in subject["job_names"]):
                failures.append(f"{path}: subject {subject.get('id')} has unsupported exact job selector")

    observer = yaml.load((WORKFLOW_DIR / "maintenance.yml").read_text(), Loader=yaml.BaseLoader)
    trigger_names = set(observer["on"]["workflow_run"]["workflows"])
    configured_names = {"CI", "GC", "Extended Tests"}
    if trigger_names != configured_names:
        failures.append(
            "workflow_run trigger/config mismatch: "
            f"missing={sorted(configured_names - trigger_names)} "
            f"extra={sorted(trigger_names - configured_names)}"
        )

    freshness = json.loads(FRESHNESS_CONFIG.read_text())
    required = {
        gate.get("source_workflow", gate["workflow"]) for gate in freshness["gates"]
    }
    covered = set(config.workflows) | set(config.excluded)
    covered |= {str(subject.get("path")) for subjects in config.workflows.values() for subject in subjects}
    missing = required - covered
    if missing:
        failures.append(f"freshness-tracked workflows neither watched nor excluded: {sorted(missing)}")
    stale_exclusions = set(config.excluded) - required
    if stale_exclusions:
        failures.append(f"exclusions no longer tracked by gate freshness: {sorted(stale_exclusions)}")

    if failures:
        print("gate failure watch configuration FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print(
        f"gate failure watch configuration: {len(config.workflows)} workflows watched, "
        f"{len(config.excluded)} explicitly excluded"
    )
    return 0


def self_test() -> int:
    config = WatchConfig("main", "^v[0-9]", {"gc.yml": [
        {"id":"gc-ratchet","name":"GC Ratchet","job":"gc-ratchet","path":"gc-ratchet.yml"},
        {"id":"gc-moving-witnesses","name":"GC Moving Witnesses","job":"gc-moving-witnesses","path":"gc-moving-witnesses.yml"},
    ]}, {})
    failures: list[str] = []

    def run(**overrides: Any) -> dict[str, Any]:
        base = {
            "id": 10,
            "workflow_id": 20,
            "path": ".github/workflows/gc.yml",
            "name": "GC",
            "event": "schedule",
            "head_branch": "main",
            "head_sha": "a" * 40,
            "html_url": "https://example.test/runs/10",
            "created_at": "2026-09-06T10:00:00Z",
            "conclusion": "failure",
        }
        base.update(overrides)
        return base

    for label, candidate, want in [
        ("scheduled main", run(), True),
        ("main dispatch", run(event="workflow_dispatch"), True),
        ("release tag", run(event="push", head_branch="v0.5.1"), True),
        ("pull request", run(event="pull_request", head_branch="feature"), False),
        ("feature dispatch", run(event="workflow_dispatch", head_branch="feature"), False),
        ("unknown workflow", run(path=".github/workflows/other.yml"), False),
    ]:
        if eligible_run(candidate, config) != want:
            failures.append(f"eligibility: {label}")

    jobs = [
        {
            "name": "matrix (linux)",
            "conclusion": "failure",
            "steps": [
                {"name": "Build", "conclusion": "success"},
                {"name": "Compare rows", "conclusion": "failure"},
            ],
        },
        {"name": "aggregate", "conclusion": "timed_out", "steps": []},
        {"name": "green", "conclusion": "success", "steps": []},
    ]
    rows = failure_rows(jobs)
    if rows != ["aggregate", "matrix (linux) / Compare rows"]:
        failures.append(f"failure row extraction: {rows}")

    subject=config.workflows["gc.yml"][0]
    if subject_verdict([{"name":"gc-ratchet","status":"completed","conclusion":"skipped"}],subject) is not None:
        failures.append("skipped logical suite was treated as a verdict")
    if subject_verdict([{"name":"gc-ratchet","status":"completed","conclusion":"success"}],subject) != "success":
        failures.append("completed caller success was not recognized")
    nested_jobs=[{"name":"gc-ratchet","status":"completed","conclusion":"failure"},
                 {"name":"gc-ratchet / matrix (linux)","status":"completed","conclusion":"success"},
                 {"name":"gc-ratchet / aggregate","status":"completed","conclusion":"failure","steps":[]}]
    if subject_verdict(nested_jobs,subject) != "failure": failures.append("nested failed subject was not recognized")
    sibling_failure_jobs = [
        {"name":"gc-ratchet","status":"completed","conclusion":"success"},
        {"name":"gc-ratchet / verdict","status":"completed","conclusion":"success"},
        {"name":"gc-moving-witnesses / verdict","status":"completed","conclusion":"failure"},
    ]
    if subject_verdict(sibling_failure_jobs, subject) != "success":
        failures.append("sibling failure contaminated a green subject")
    if failure_rows(subject_jobs(sibling_failure_jobs, subject)):
        failures.append("sibling failure rows leaked into the green subject")

    core_subject = {
        "id":"core", "name":"CI core gate",
        "job_names":["pr-gate", "main-gate", "full-suite-gate"], "path":"test.yml",
    }
    if subject_verdict([
        {"name":"main-gate","status":"completed","conclusion":"failure"},
        {"name":"coverage","status":"completed","conclusion":"success"},
    ], core_subject) != "failure":
        failures.append("core CI gate failure was not tracked independently from coverage")
    if subject_verdict([
        {"name":"coverage","status":"completed","conclusion":"success"},
    ], core_subject) is not None:
        failures.append("auxiliary coverage run was mistaken for a core CI verdict")

    page_calls: list[str] = []
    def paged_request(method: str, path: str, fields: dict[str, str] | None = None) -> dict[str, Any]:
        page = int((fields or {}).get("page", "1"))
        page_calls.append(str(page))
        rows = ([{"name": f"noise {i}"} for i in range(100)] if page == 1
                else [{"name":"gc-ratchet / page-two failure", "status":"completed", "conclusion":"failure"}])
        return {"jobs": rows}
    paged_rows = get_jobs("o/r", 77, request=paged_request)
    if len(paged_rows) != 101 or paged_rows[-1].get("conclusion") != "failure" or page_calls != ["1", "2"]:
        failures.append("observer dropped a failed subject on job API page two")

    history = [
        run(id=10),
        run(id=9, head_sha="b" * 40, created_at="2026-09-06T09:00:00Z"),
        run(
            id=8,
            head_sha="c" * 40,
            created_at="2026-09-06T08:00:00Z",
            conclusion="success",
        ),
        run(id=7, event="pull_request", head_branch="feature"),
    ]
    previous, green = previous_runs(run(), history, config)
    if previous is None or previous.get("id") != 9:
        failures.append("previous failure selection")
    if green is None or green.get("id") != 8:
        failures.append("last green selection")

    body = issue_body(
        "gc-ratchet.yml",
        "GC Ratchet",
        run(),
        ["matrix / new", "shared"],
        previous,
        ["matrix / old", "shared"],
        green,
    )
    for text in ["matrix / new", "matrix / old", "`cccccccccccc`", "Delta"]:
        if text not in body:
            failures.append(f"issue body omitted {text!r}")

    calls: list[tuple[str, str, dict[str, str] | None]] = []

    def closed_request(
        method: str, path: str, fields: dict[str, str] | None = None
    ) -> dict[str, Any]:
        calls.append((method, path, fields))
        if path == "search/issues":
            return {
                "items": [
                    {
                        "number": 41,
                        "title": f"{issue_marker('gc-ratchet.yml')} old",
                        "state": "closed",
                    }
                ]
            }
        return {}

    sync_failure_issue("PerryTS/perry", "gc-ratchet.yml", "GC Ratchet", body, closed_request)
    if [(method, path) for method, path, _ in calls] != [
        ("GET", "search/issues"),
        ("PATCH", "repos/PerryTS/perry/issues/41"),
    ]:
        failures.append("closed issue was not reused and reopened")
    elif calls[-1][2] is None or calls[-1][2].get("state") != "open":
        failures.append("recurring failure did not reopen the issue")

    calls.clear()

    def open_request(
        method: str, path: str, fields: dict[str, str] | None = None
    ) -> dict[str, Any]:
        calls.append((method, path, fields))
        if path == "search/issues":
            return {
                "items": [
                    {
                        "number": 42,
                        "title": f"{issue_marker('gc-ratchet.yml')} active",
                        "state": "open",
                    }
                ]
            }
        return {}

    close_failure_issue(
        "PerryTS/perry",
        "gc-ratchet.yml",
        "GC Ratchet",
        run(conclusion="success"),
        open_request,
    )
    if [(method, path) for method, path, _ in calls] != [
        ("GET", "search/issues"),
        ("POST", "repos/PerryTS/perry/issues/42/comments"),
        ("PATCH", "repos/PerryTS/perry/issues/42"),
    ]:
        failures.append("green run did not comment and close the open issue")
    elif calls[-1][2] != {"state": "closed", "state_reason": "completed"}:
        failures.append("green run did not close the issue as completed")

    if failures:
        print("gate failure watch self-test FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("gate failure watch self-test passed")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--check-config", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--event", type=pathlib.Path, default=None)
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "PerryTS/perry"))
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if args.check_config:
        return check_config()
    event_path = args.event or (
        pathlib.Path(os.environ["GITHUB_EVENT_PATH"]) if "GITHUB_EVENT_PATH" in os.environ else None
    )
    if event_path is None:
        parser.error("--event or GITHUB_EVENT_PATH is required")
    payload = json.loads(event_path.read_text())
    return handle_event(payload, args.repo, load_config(), dry_run=args.dry_run)


if __name__ == "__main__":
    sys.exit(main())
