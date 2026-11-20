"""The supplemental relocation check must reject in-place-only collection."""

import tempfile
import unittest
from pathlib import Path
from unittest import mock

from benchmarks.gc_ratchet import check_large_eden_relocation as relocation


class LargeEdenRelocationTests(unittest.TestCase):
    def check(self, stderr, *, returncode=0, parity="pass", oracle_version=None):
        if oracle_version is None:
            oracle_version = "v" + (relocation.ratchet.REPO_ROOT / ".node-version").read_text().strip().lstrip("v")
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "probe"
            binary.write_bytes(b"mock binary; never executed")
            run = {"stdout": "checksum:1\n", "stderr": stderr, "returncode": returncode}
            with mock.patch.object(relocation.ratchet, "run_once", return_value=run) as runner:
                with mock.patch.object(relocation.ratchet, "_check_against_node",
                                       return_value={"status": parity, "oracle_version": oracle_version}):
                    result = relocation.check_relocation(binary, Path("node"))
            self.assertEqual(runner.call_count, 2)
            for call in runner.call_args_list:
                self.assertEqual(call.kwargs["extra_env"], {
                    "PERRY_GC_SCAVENGE_NURSERY_MB": "64",
                    "PERRY_GC_PROMOTE_IN_PLACE": "0", "PERRY_GC_DIAG": "1",
                })
            return result

    def test_copied_survivors_pass_and_raw_runs_are_retained(self):
        trace = "[gc-copy-minor] ran copied_objects=12 copied_bytes=768 promoted_objects=5\n"
        result = self.check(trace)
        self.assertEqual(result["status"], "pass")
        self.assertEqual([run["stderr"] for run in result["runs"]], [trace, trace])

    def test_promotion_in_place_does_not_prove_relocation(self):
        result = self.check("[gc-copy-minor] ran in_place=true copied_objects=0 "
                            "copied_bytes=0 promoted_objects=310413 promoted_bytes=17694408\n")
        self.assertEqual(result["status"], "fail")
        self.assertTrue(any("copied_objects" in failure for failure in result["failures"]))

    def test_no_trace_fails(self):
        self.assertEqual(self.check("")["status"], "fail")

    def test_crash_with_positive_trace_fails(self):
        trace = "[gc-copy-minor] ran copied_objects=12 copied_bytes=768\n"
        self.assertEqual(self.check(trace, returncode=-11)["status"], "fail")

    def test_wrong_or_unavailable_oracle_fails(self):
        for parity in ("fail", "unchecked"):
            with self.subTest(parity=parity):
                self.assertEqual(self.check("[gc-copy-minor] ran copied_objects=12 "
                                            "copied_bytes=768\n", parity=parity)["status"], "fail")

    def test_wrong_oracle_version_fails_even_with_matching_output(self):
        result = self.check("[gc-copy-minor] ran copied_objects=12 copied_bytes=768\n",
                            oracle_version="v0.0.0")
        self.assertEqual(result["status"], "fail")
        self.assertTrue(any("expected Node" in failure for failure in result["failures"]))


if __name__ == "__main__":
    unittest.main()
