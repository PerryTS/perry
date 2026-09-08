#!/usr/bin/env python3
"""Keep one open incident for failed main-line GC ratchet runs (#9829).

Only the reporting job receives issues:write. PR code never invokes this path.
The measurement/check exit status remains the GC gate's verdict.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess

MARKER = "<!-- gc-ratchet-main-alert -->"
STATE_RE = re.compile(r"<!-- gc-ratchet-state (.+) -->")
TITLE = "GC ratchet is failing on main"


class GitHub:
    def api(self, path, *, method="GET", payload=None, raw=False):
        command = ["gh", "api", path, "--method", method]
        if payload is not None:
            command += ["--input", "-"]
        result = subprocess.run(
            command, input=json.dumps(payload) if payload is not None else None,
            capture_output=True, text=True, check=True,
        )
        return result.stdout if raw else json.loads(result.stdout)

    def pages(self, path, key=None):
        separator = "&" if "?" in path else "?"
        page = 1
        while True:
            data = self.api(f"{path}{separator}per_page=100&page={page}")
            entries = data[key] if key else data
            yield from entries
            if len(entries) < 100:
                return
            page += 1


def eligible(run):
    return run["head_branch"] == "main" and run["event"] in {
        "schedule", "workflow_dispatch",
    }


def state_from(body):
    match = STATE_RE.search(body or "")
    return json.loads(match[1]) if match else {}


def regression_rows(log):
    rows = {}
    for line in log.splitlines():
        start = line.find("| " + chr(96))
        if start < 0:
            continue
        row = line[start:].strip()
        fields = [field.strip() for field in row.strip("|").split("|")]
        if len(fields) == 8 and fields[-1] == "REGRESSION":
            key = fields[0].strip(chr(96)) + "." + fields[1]
            rows[key] = row
    return rows


def successful_runs(client, base, run):
    return [
        previous for previous in client.pages(
            f"{base}/actions/workflows/{run['workflow_id']}/runs?branch=main&status=success",
            "workflow_runs",
        ) if eligible(previous)
    ]


def report(client, repository, run_id, result):
    base = f"repos/{repository}"
    run = client.api(f"{base}/actions/runs/{run_id}")
    if not eligible(run) or result not in {"success", "failure"}:
        return None
    issue = next((
        entry for entry in client.pages(f"{base}/issues?state=open&creator=github-actions%5Bbot%5D")
        if not entry.get("pull_request") and MARKER in (entry.get("body") or "")
    ), None)
    old = state_from(issue["body"]) if issue else {}
    order = (run["id"], run.get("run_attempt", 1))
    if order <= (old.get("run_id", 0), old.get("attempt", 0)):
        return None
    if result == "success":
        if issue is None:
            return None
        body = issue["body"] + (
            f"\n\nRecovered in [run {run_id}]({run['html_url']}) "
            f"at {run['head_sha']}.\n"
        )
        return client.api(
            f"{base}/issues/{issue['number']}", method="PATCH",
            payload={"body": body, "state": "closed", "state_reason": "completed"},
        )

    successes = successful_runs(client, base, run)
    # A delayed failure must not reopen an incident already recovered by a
    # newer run. The reporting job itself is serialized to avoid create races.
    if any(previous["id"] > run["id"] for previous in successes):
        return None
    previous_green = next((p for p in successes if p["id"] < run["id"]), None)
    green = (
        {"sha": previous_green["head_sha"], "url": previous_green["html_url"]}
        if previous_green else old.get("last_green")
    )
    jobs = list(client.pages(
        f"{base}/actions/runs/{run_id}/attempts/{run.get('run_attempt', 1)}/jobs", "jobs",
    ))
    rows, failures, log_errors = {}, [], []
    for job in jobs:
        if job["conclusion"] != "failure":
            continue
        failures.append(f"- [{job['name']}]({job['html_url']})")
        failures.extend(
            "  - " + step["name"] for step in job.get("steps", [])
            if step["conclusion"] == "failure"
        )
        try:
            rows.update(regression_rows(client.api(
                f"{base}/actions/jobs/{job['id']}/logs", raw=True,
            )))
        except subprocess.CalledProcessError:
            log_errors.append(f"Logs unavailable for {job['name']}; follow the job link.")

    previous_rows = set(old.get("rows", []))
    added, cleared = sorted(set(rows) - previous_rows), sorted(previous_rows - set(rows))
    state = {
        "run_id": run["id"], "attempt": run.get("run_attempt", 1),
        "rows": sorted(rows), "last_green": green,
    }
    lines = [
        MARKER, TITLE, "",
        f"Latest failure: [run {run_id}]({run['html_url']}) at {run['head_sha']}.",
        f"Event: {run['event']}.",
        (
            f"Last green: [{green['sha']}]({green['url']})."
            if green else "Last green: unavailable in retained workflow history."
        ),
        "", "Failing jobs/steps:", *failures, "",
        f"Regression cells: {len(rows)}.",
    ]
    if issue and rows and previous_rows:
        lines += [
            f"Since the previous failure: {len(added)} added, {len(cleared)} cleared.",
            "Added: " + (", ".join(added) or "none"),
            "Cleared: " + (", ".join(cleared) or "none"),
        ]
    if rows:
        lines += [
            "", "| Probe | Metric | Baseline | Current | Delta | Allowance | Gating | Status |",
            "|---|---|---:|---:|---:|---:|:---:|---|",
            *[rows[key] for key in sorted(rows)],
        ]
    else:
        lines += ["No regression table was emitted; this is a setup/harness failure, not a clean measurement."]
    lines += [
        "", *log_errors,
        "", "Investigate changes since the last green; do not re-pin unexplained drift.",
        "This issue is updated on failure and closed after a successful main-line run.",
        "", "<!-- gc-ratchet-state " + json.dumps(state, sort_keys=True) + " -->",
    ]
    payload = {"title": TITLE, "body": "\n".join(lines)}
    if issue:
        return client.api(f"{base}/issues/{issue['number']}", method="PATCH", payload=payload)
    return client.api(f"{base}/issues", method="POST", payload=payload)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--run-id", required=True, type=int)
    parser.add_argument("--result", required=True)
    args = parser.parse_args()
    report(GitHub(), args.repository, args.run_id, args.result)


if __name__ == "__main__":
    main()
