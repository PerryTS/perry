#!/usr/bin/env python3
"""Delete completed Actions runs whose workflow YAML no longer exists on main.

The script is intentionally restartable: each invocation keeps deleting from
the first page until the API quota is nearly exhausted, then dispatches itself
again on main. It never touches active runs or workflows that still exist.
"""
from __future__ import annotations

import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class GitHub:
    def __init__(self, token: str, api_url: str, repo: str):
        self.token = token
        self.api_url = api_url.rstrip("/")
        self.repo = repo

    def request(self, method: str, path: str, body: dict | None = None):
        url = self.api_url + "/repos/" + self.repo + path
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(url, data=data, method=method, headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": "2022-11-28",
            "Content-Type": "application/json",
            "User-Agent": "perry-retired-actions-cleanup",
        })
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.status, response.headers, response.read()
        except urllib.error.HTTPError as error:
            return error.code, error.headers, error.read()


def main() -> int:
    token = os.environ.get("GH_TOKEN", "")
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    api_url = os.environ.get("GITHUB_API_URL", "https://api.github.com")
    if not token or not repo or os.environ.get("GITHUB_REF") != "refs/heads/main":
        print("Refusing cleanup without GH_TOKEN, repository, and main ref", file=sys.stderr)
        return 2
    api = GitHub(token, api_url, repo)

    status, headers, payload = api.request("GET", "/actions/workflows?per_page=100")
    if status != 200:
        print(f"Unable to list workflows: HTTP {status}: {payload[:500]!r}", file=sys.stderr)
        return 1
    workflows = json.loads(payload).get("workflows", [])
    retired = []
    for workflow in workflows:
        path = workflow.get("path", "")
        # Limit deletions to ordinary repository YAML workflow files absent
        # from this main checkout. Never infer retirement from display names.
        if (path.startswith(".github/workflows/") and path.endswith(".yml")
                and not (ROOT / path).is_file()):
            retired.append((workflow["id"], path))
    print(f"Retired workflow files eligible for cleanup: {len(retired)}")
    deleted = 0
    reset_at = int(headers.get("X-RateLimit-Reset", "0") or 0)
    remaining = int(headers.get("X-RateLimit-Remaining", "0") or 0)
    deadline = time.monotonic() + 5.7 * 60 * 60

    for workflow_id, workflow_path in retired:
        while time.monotonic() < deadline:
            # Stay below the GITHUB_TOKEN hourly ceiling and preserve calls
            # for pagination, retry behavior, and the continuation dispatch.
            if remaining < 25:
                break
            status, headers, payload = api.request(
                "GET", f"/actions/workflows/{workflow_id}/runs?per_page=100&status=completed")
            remaining = int(headers.get("X-RateLimit-Remaining", remaining) or remaining)
            reset_at = int(headers.get("X-RateLimit-Reset", reset_at) or reset_at)
            if status != 200:
                print(f"Unable to list runs for {workflow_path}: HTTP {status}: {payload[:300]!r}", file=sys.stderr)
                return 1
            runs = json.loads(payload).get("workflow_runs", [])
            if not runs:
                break
            for run in runs:
                if time.monotonic() >= deadline or remaining < 25:
                    break
                status, headers, payload = api.request("DELETE", f"/actions/runs/{run['id']}")
                remaining = int(headers.get("X-RateLimit-Remaining", remaining) or remaining)
                reset_at = int(headers.get("X-RateLimit-Reset", reset_at) or reset_at)
                if status in (204, 404):
                    deleted += status == 204
                elif status in (403, 429):
                    print(f"Rate limit or permission boundary reached after {deleted} deletions")
                    break
                else:
                    print(f"Failed deleting run {run['id']} ({workflow_path}): HTTP {status}: {payload[:300]!r}", file=sys.stderr)
                    return 1
            else:
                continue
            break
        if remaining < 25 or time.monotonic() >= deadline:
            break

    print(f"Deleted {deleted} completed retired-workflow runs in this pass")
    if remaining < 10 and reset_at:
        delay = max(1, reset_at - int(time.time()) + 5)
        if time.monotonic() + delay < deadline:
            print(f"Waiting {delay}s for API quota reset")
            time.sleep(delay)
    if remaining < 2:
        print("Insufficient API quota to safely queue continuation", file=sys.stderr)
        return 1
    status, _, payload = api.request("POST", "/actions/workflows/maintenance.yml/dispatches", {
        "ref": "main",
        "inputs": {"suite": "delete-retired-runs", "confirm_delete_retired_runs": "true"},
    })
    if status not in (201, 204):
        print(f"Could not queue continuation: HTTP {status}: {payload[:500]!r}", file=sys.stderr)
        return 1
    print("Queued next rate-limited cleanup pass")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
