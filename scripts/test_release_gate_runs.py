"""Offline fixtures for release-gate selection of nested simulator jobs."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from release_gate_runs import simulator_dispatch_needed, simulator_state


SHA_A = "a" * 40
SHA_B = "b" * 40


def run(run_id: int, sha: str = SHA_A) -> dict:
    return {"id": run_id, "head_sha": sha, "conclusion": "failure"}


def job(conclusion: str | None, status: str = "completed") -> dict:
    return {"name": "simctl-tests / simctl", "status": status, "conclusion": conclusion}


def test_exact_sha_and_actual_subject_are_required():
    assert simulator_state([run(1, SHA_B)], {1: [job("success")]}, SHA_A) == "absent"
    assert simulator_state([run(2)], {2: [{"name": "container-tests / container-tests", "status": "completed", "conclusion": "success"}]}, SHA_A) == "absent"
    assert simulator_dispatch_needed([run(2)], {2: [{"name": "container-tests / container-tests", "status": "completed", "conclusion": "success"}]}, SHA_A)


def test_unrelated_parent_failure_does_not_erase_simulator_success():
    assert simulator_state([run(3)], {3: [job("success")]}, SHA_A) == "success"


def test_skipped_job_is_not_a_verdict_or_dispatch_suppressor():
    runs = [run(4)]
    jobs = {4: [job("skipped")]}
    assert simulator_state(runs, jobs, SHA_A) == "absent"
    assert simulator_dispatch_needed(runs, jobs, SHA_A)


def test_pending_and_failed_subjects_are_distinct():
    assert simulator_state([run(5)], {5: [job(None, "in_progress")]}, SHA_A) == "pending"
    assert simulator_state([run(6)], {6: [job("failure")]}, SHA_A) == "failed"


if __name__ == "__main__":
    import unittest

    class ReleaseGateRunTests(unittest.TestCase):
        def test_exact_sha_and_subject(self):
            self.assertEqual(simulator_state([run(1, SHA_B)], {1: [job("success")]}, SHA_A), "absent")
            runs = [run(2)]
            sibling = {2: [{"name": "container-tests / container-tests", "status": "completed", "conclusion": "success"}]}
            self.assertEqual(simulator_state(runs, sibling, SHA_A), "absent")
            self.assertTrue(simulator_dispatch_needed(runs, sibling, SHA_A))

        def test_sibling_failure_is_ignored(self):
            test_unrelated_parent_failure_does_not_erase_simulator_success()

        def test_skipped_is_not_a_verdict(self):
            runs = [run(4)]
            jobs = {4: [job("skipped")]}
            self.assertEqual(simulator_state(runs, jobs, SHA_A), "absent")
            self.assertTrue(simulator_dispatch_needed(runs, jobs, SHA_A))

        def test_pending_and_failure_are_distinct(self):
            test_pending_and_failed_subjects_are_distinct()

        def test_jobs_are_paginated(self):
            import json
            import subprocess
            from release_gate_runs import _jobs_with_pagination

            calls = []
            def fake_run(args, **kwargs):
                query = args[2]
                calls.append(query)
                page = int(query.split("page=")[2])
                rows = ([{"name": f"job-{n}"} for n in range(100)] if page == 1
                        else [job("success")])
                return subprocess.CompletedProcess(args, 0, json.dumps({"jobs": rows}), "")
            original = subprocess.run
            subprocess.run = fake_run
            try:
                self.assertEqual(len(_jobs_with_pagination("o/r", 123)), 101)
            finally:
                subprocess.run = original
            self.assertEqual(len(calls), 2)

    unittest.main()
