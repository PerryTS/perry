"""Exercise incident lifecycle without posting to GitHub."""
import copy
import subprocess
import unittest
from pathlib import Path

from scripts.report_gc_ratchet import MARKER, regression_rows, report, state_from

REPO = "PerryTS/perry"
TICK = chr(96)


def row(probe, metric="copied_bytes"):
    return f"| {TICK}{probe}{TICK} | {metric} | 100 | 200 | +100% | 5 | yes | REGRESSION |"


class FakeGitHub:
    def __init__(self):
        self.run = {
            "id": 200, "run_attempt": 1, "workflow_id": 10,
            "head_sha": "b" * 40, "head_branch": "main", "event": "schedule",
            "html_url": "https://github.com/PerryTS/perry/actions/runs/200",
        }
        self.issues = []
        self.successes = [
            {**self.run, "id": 100, "head_sha": "a" * 40,
             "html_url": "https://github.com/PerryTS/perry/actions/runs/100"},
        ]
        self.log = "2026-09-06T00:00:00Z " + row("01_probe")
        self.log_error = False
        self.writes = []

    def api(self, path, *, method="GET", payload=None, raw=False):
        if method != "GET":
            self.writes.append((method, path, copy.deepcopy(payload)))
            issue = {"number": 99, **payload}
            self.issues = [] if payload.get("state") == "closed" else [issue]
            return issue
        if path.endswith("/logs"):
            if self.log_error:
                raise subprocess.CalledProcessError(1, ["gh"])
            return self.log
        if path == f"repos/{REPO}/actions/runs/{self.run['id']}":
            return self.run
        raise AssertionError(path)

    def pages(self, path, key=None):
        if "/issues?" in path:
            return iter(self.issues)
        if "/workflows/" in path:
            return iter(self.successes)
        if path.endswith("/jobs"):
            return iter([{
                "id": 50, "name": "gc-ratchet", "conclusion": "failure",
                "html_url": self.run["html_url"] + "/job/50",
                "steps": [{"name": "Check against the pinned baseline", "conclusion": "failure"}],
            }])
        raise AssertionError(path)


class IncidentTests(unittest.TestCase):
    def test_first_failure_creates_one_issue_with_evidence(self):
        client = FakeGitHub()
        report(client, REPO, 200, "failure")
        method, _, payload = client.writes[0]
        self.assertEqual(method, "POST")
        self.assertIn("a" * 40, payload["body"])
        self.assertIn("b" * 40, payload["body"])
        self.assertIn(row("01_probe"), payload["body"])
        self.assertIn("Check against the pinned baseline", payload["body"])
        self.assertEqual(state_from(payload["body"])["rows"], ["01_probe.copied_bytes"])

    def test_repeated_failure_updates_and_reports_added_and_cleared_cells(self):
        client = FakeGitHub()
        report(client, REPO, 200, "failure")
        client.run["id"] = 201
        client.log = row("02_probe")
        report(client, REPO, 201, "failure")
        method, path, payload = client.writes[-1]
        self.assertEqual((method, path), ("PATCH", f"repos/{REPO}/issues/99"))
        self.assertIn("1 added, 1 cleared", payload["body"])
        self.assertIn("Added: 02_probe.copied_bytes", payload["body"])
        self.assertIn("Cleared: 01_probe.copied_bytes", payload["body"])
        self.assertEqual(len(client.issues), 1)

    def test_recovery_closes_existing_issue_and_fresh_green_does_nothing(self):
        client = FakeGitHub()
        report(client, REPO, 200, "success")
        self.assertEqual(client.writes, [])
        report(client, REPO, 200, "failure")
        client.run["id"] = 201
        report(client, REPO, 201, "success")
        self.assertEqual(client.writes[-1][2]["state"], "closed")
        self.assertIn("Recovered in", client.writes[-1][2]["body"])

    def test_unmeasured_failure_and_missing_logs_still_open_incident(self):
        for missing in (False, True):
            with self.subTest(missing=missing):
                client = FakeGitHub()
                client.log, client.log_error = "", missing
                report(client, REPO, 200, "failure")
                body = client.writes[0][2]["body"]
                self.assertIn("not a clean measurement", body)
                if missing:
                    self.assertIn("Logs unavailable", body)

    def test_pr_tag_cancelled_and_unrelated_dispatch_do_not_write(self):
        for event, branch, result in [
            ("pull_request", "main", "failure"),
            ("push", "v1.0", "failure"),
            ("workflow_dispatch", "topic", "failure"),
            ("schedule", "main", "cancelled"),
            ("schedule", "main", "skipped"),
        ]:
            with self.subTest(event=event, branch=branch, result=result):
                client = FakeGitHub()
                client.run.update(event=event, head_branch=branch)
                report(client, REPO, 200, result)
                self.assertEqual(client.writes, [])

    def test_retry_is_idempotent_and_older_success_cannot_close_newer_failure(self):
        client = FakeGitHub()
        report(client, REPO, 200, "failure")
        report(client, REPO, 200, "failure")
        client.run["id"] = 199
        report(client, REPO, 199, "success")
        self.assertEqual(len(client.writes), 1)
        client.run.update(id=200, run_attempt=2)
        report(client, REPO, 200, "success")
        self.assertEqual(client.writes[-1][2]["state"], "closed")

    def test_delayed_failure_cannot_reopen_after_newer_success(self):
        client = FakeGitHub()
        client.successes.insert(0, {**client.run, "id": 201})
        report(client, REPO, 200, "failure")
        self.assertEqual(client.writes, [])

    def test_parser_ignores_non_gating_and_non_regression_rows(self):
        valid = row("01_probe")
        log = "\n".join([
            valid, valid.replace("REGRESSION", "ok"),
            valid.replace("REGRESSION", "recorded"),
            "compiler failure",
        ])
        self.assertEqual(regression_rows(log), {"01_probe.copied_bytes": valid})

    def test_workflow_keeps_writes_out_of_measurement_job(self):
        path = Path(__file__).resolve().parents[1] / ".github/workflows/gc-ratchet.yml"
        workflow = path.read_text(encoding="utf-8")
        measurement, reporting = workflow.split("  report-main-result:", 1)
        self.assertNotIn("issues: write", measurement)
        self.assertIn("needs: gc-ratchet", reporting)
        self.assertIn("issues: write", reporting)
        self.assertIn("always()", reporting)
        self.assertIn("refs/heads/main", reporting)
        self.assertIn("github.event_name == 'schedule'", reporting)
        self.assertIn("-p 'test*gc_ratchet.py'", measurement)
