#!/usr/bin/env python3
"""Deterministic /proc fixtures for the incident collector."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import capture_linux_incident as capture


def stat(cpu=10, start=123, rss=100, comm="server (worker) name"):
    fields = ["0"] * 22
    fields[0], fields[11], fields[12] = "R", str(cpu), "2"
    fields[19], fields[20], fields[21] = str(start), "8192000", str(rss)
    return "42 (" + comm + ") " + " ".join(fields)


def sample(at, cpu, rss, start=123):
    return {"monotonic_seconds": at, "files": {"stat": stat(cpu, rss=rss)},
            "threads": {"42": {"stat": stat(cpu, start), "wchan": "0"}}}


class CaptureTests(unittest.TestCase):
    def test_stat_with_parentheses_and_spaces(self):
        parsed = capture.parse_stat(stat())
        self.assertEqual(parsed["comm"], "server (worker) name")
        self.assertEqual(parsed["cpu_ticks"], 12)
        self.assertEqual(parsed["start_ticks"], 123)
        self.assertEqual(parsed["rss_pages"], 100)

    def test_cpu_and_growth_use_actual_time_ticks_and_page_size(self):
        result = capture.summarize([sample(10, 0, 100), sample(12, 200, 110)], 100, 4096)
        row = result["intervals"][0]
        self.assertEqual(row["rss_delta_bytes"], 40960)
        self.assertEqual(row["rss_bytes_per_second"], 20480)
        self.assertEqual(row["threads"][0]["cpu_percent_one_core"], 100)

    def test_reused_tid_is_not_reported_as_cpu(self):
        result = capture.summarize([sample(10, 0, 100), sample(12, 200, 110, start=456)], 100, 4096)
        self.assertEqual(result["intervals"][0]["threads"], [])

    def test_missing_thread_stat_preserves_memory_summary(self):
        before, after = sample(10, 0, 100), sample(12, 200, 110)
        after["threads"]["42"]["stat"] = None
        result = capture.summarize([before, after], 100, 4096)["intervals"][0]
        self.assertEqual(result["rss_delta_bytes"], 40960)
        self.assertTrue(result["errors"])

    def test_partial_snapshot_records_missing_files(self):
        with tempfile.TemporaryDirectory() as directory:
            process = Path(directory)
            (process / "stat").write_text(stat())
            task = process / "task" / "42"
            task.mkdir(parents=True)
            (task / "stat").write_text(stat())
            result = capture.snapshot(process, 123)
            self.assertEqual(result["threads_seen"], 1)
            self.assertIsNone(result["files"]["smaps_rollup"])
            self.assertTrue(result["errors"])

    def test_pid_reuse_before_or_during_capture_is_rejected(self):
        with patch.object(capture, "identity", return_value=456):
            with self.assertRaisesRegex(RuntimeError, "reused"):
                capture.snapshot(Path("/unused"), 123)
        with tempfile.TemporaryDirectory() as directory:
            process = Path(directory)
            (process / "task").mkdir()
            with patch.object(capture, "identity", side_effect=[123, 456]):
                with self.assertRaisesRegex(RuntimeError, "changed"):
                    capture.snapshot(process, 123)

    def test_read_is_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "maps"
            source.write_text("x" * 50)
            errors = []
            with patch.object(capture, "MAX_FILE_BYTES", 10):
                self.assertEqual(capture.read_file(source, errors), "x" * 10)
            self.assertIn("truncated", errors[0])


if __name__ == "__main__":
    unittest.main()
