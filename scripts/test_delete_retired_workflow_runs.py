"""Offline checks for retirement cleanup while pre-merge jobs are still running."""
import contextlib
import io
import json
import os
import unittest
from unittest.mock import patch

import delete_retired_workflow_runs as cleanup


class RetirementCleanupTests(unittest.TestCase):
    def test_active_retired_run_finishes_before_deletion_and_live_workflows_are_spared(self):
        calls = []
        completed_polls = 0
        workflows = [
            {"id": 1, "path": ".github/workflows/compatibility.yml"},
            {"id": 2, "path": ".github/workflows/test.yml"},
            {"id": 3, "path": "dynamic/dependabot/dependabot-updates"},
            {"id": 4, "path": "dynamic/agents/copilot-pull-request-reviewer"},
            {"id": 5, "path": "dynamic/pages/pages-build-deployment"},
            {"id": 6, "path": "dynamic/dependabot/update-graph"},
        ]

        def request(_self, method, path, body=None):
            nonlocal completed_polls
            calls.append((method, path))
            status = 200
            if path == "/actions/workflows?per_page=100":
                data = {"workflows": workflows}
            elif path.endswith("status=completed"):
                completed_polls += 1
                data = {"workflow_runs": ([{"id": 10, "status": "completed"}]
                                          if completed_polls == 2 else [])}
            elif method == "DELETE":
                status, data = 204, {}
            elif path.endswith("runs?per_page=1"):
                data = {"workflow_runs": ([{"id": 10, "status": "in_progress"}]
                                          if completed_polls == 1 else [])}
            else:
                self.fail(f"Unexpected request: {method} {path}")
            return status, {"X-RateLimit-Remaining": "1000"}, json.dumps(data).encode()

        env = {"GH_TOKEN": "offline-test", "GITHUB_REPOSITORY": "PerryTS/perry",
               "GITHUB_REF": "refs/heads/main"}
        with patch.dict(os.environ, env), patch.object(cleanup.GitHub, "request", request), \
                patch.object(cleanup.time, "sleep") as sleep, \
                contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(cleanup.main(), 0)
        sleep.assert_called_once_with(60)
        self.assertEqual([path for method, path in calls if method == "DELETE"], ["/actions/runs/10"])
        self.assertFalse(any("/workflows/2/" in path or "/workflows/3/" in path
                             or "/workflows/4/" in path or "/workflows/5/" in path
                             or "/workflows/6/" in path for _, path in calls))

    def test_off_main_cleanup_is_refused_before_any_api_call(self):
        with patch.dict(os.environ, {"GH_TOKEN": "offline-test", "GITHUB_REPOSITORY": "PerryTS/perry",
                                    "GITHUB_REF": "refs/heads/topic"}), \
                patch.object(cleanup.GitHub, "request") as request, \
                contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(cleanup.main(), 2)
        request.assert_not_called()


if __name__ == "__main__":
    unittest.main()
