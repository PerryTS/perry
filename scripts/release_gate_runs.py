#!/usr/bin/env python3
"""Evaluate the simulator job inside consolidated Extended Tests runs."""

from __future__ import annotations

import json
import os
from typing import Any


SIMULATOR_JOB = "simctl-tests / simctl"


def simulator_state(runs: list[dict[str, Any]], jobs_by_run: dict[int, list[dict[str, Any]]], sha: str) -> str:
    """Return success, failed, pending, or absent for the simulator subject on SHA."""
    pending = False
    failed = False
    for run in runs:
        if run.get("head_sha") != sha:
            continue
        jobs = jobs_by_run.get(run.get("id"), [])
        subject = next((job for job in jobs if job.get("name") == SIMULATOR_JOB), None)
        if subject is None:
            continue
        status = subject.get("status")
        conclusion = subject.get("conclusion")
        if status != "completed":
            pending = True
        elif conclusion == "success":
            return "success"
        elif conclusion not in (None, "skipped", "cancelled"):
            failed = True
    if pending:
        return "pending"
    return "failed" if failed else "absent"


def simulator_dispatch_needed(runs: list[dict[str, Any]], jobs_by_run: dict[int, list[dict[str, Any]]], sha: str) -> bool:
    """Only a selected/pending/terminal simulator subject suppresses dispatch."""
    return simulator_state(runs, jobs_by_run, sha) == "absent"


def _jobs_with_pagination(repo: str, run_id: int) -> list[dict[str, Any]]:
    import subprocess

    jobs: list[dict[str, Any]] = []
    page = 1
    while True:
        proc = subprocess.run(
            ["gh", "api", f"/repos/{repo}/actions/runs/{run_id}/jobs?per_page=100&page={page}"],
            capture_output=True,
            text=True,
            check=True,
        )
        batch = json.loads(proc.stdout).get("jobs", [])
        jobs.extend(batch)
        if len(batch) < 100:
            return jobs
        page += 1


def main() -> int:
    import argparse
    from pathlib import Path

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", required=True, type=Path)
    parser.add_argument("--jobs-dir", required=True, type=Path)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--fetch-jobs", action="store_true")
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY"))
    args = parser.parse_args()
    runs = json.loads(args.runs.read_text()).get("workflow_runs", [])
    jobs_by_run: dict[int, list[dict[str, Any]]] = {}
    for run in runs:
        if args.fetch_jobs and run.get("head_sha") == args.sha:
            if not args.repo:
                parser.error("--repo is required with --fetch-jobs")
            jobs_by_run[run["id"]] = _jobs_with_pagination(args.repo, run["id"])
            continue
        path = args.jobs_dir / f"{run['id']}.json"
        if path.exists():
            jobs_by_run[run["id"]] = json.loads(path.read_text()).get("jobs", [])
    state = simulator_state(runs, jobs_by_run, args.sha)
    if args.fetch_jobs:
        print("dispatch-needed" if state == "absent" else state)
    else:
        print(state)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
