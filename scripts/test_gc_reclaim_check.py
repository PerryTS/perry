"""Single-threaded parser regression tests; live sabotage is in the runbook."""
import unittest
import sys
from gc_reclaim_check import coverage, expected


SAMPLE = """[gc-trigger] site=alloc_point kind=OldReclaim old_reclaimable=100663296 old_threshold=50331648 old_baseline=67108864 retaining=false
[gc-full] site=alloc_point_old_reclaim trigger=OldGenBytes count_at_site=1 old_reclaimable=100663296 old_baseline=67108864
[gc] blocks: general=0 non_general=100 freed_bytes=33554432
[gc-trigger] site=safepoint kind=ArenaBytes old_reclaimable=68157440 old_threshold=50331648 old_baseline=67108864 retaining=false
"""


class CoverageTest(unittest.TestCase):
    def test_productive_old_reclaim(self):
        self.assertEqual(coverage(SAMPLE)["productive_nonretaining_old_full"], 1)

    def test_empty_diagnostic_is_not_zero_activity(self):
        with self.assertRaisesRegex(ValueError, "GC COVERAGE FAIL"):
            coverage("")

    def test_retaining_is_not_eligible(self):
        with self.assertRaisesRegex(ValueError, "nonretaining_old_full"):
            coverage(SAMPLE.replace("retaining=false", "retaining=true"))

    def test_promoted_cohort_is_not_pacing(self):
        with self.assertRaisesRegex(ValueError, "nonretaining_old_full"):
            coverage(SAMPLE.replace("kind=OldReclaim", "kind=PromotedCohort"))

    def test_manual_gc_is_not_pacing(self):
        with self.assertRaisesRegex(ValueError, "old_full"):
            coverage(SAMPLE.replace("trigger=OldGenBytes", "trigger=Manual"))

    def test_unproductive_collection_is_not_evidence(self):
        with self.assertRaisesRegex(ValueError, "productive_nonretaining_old_full"):
            coverage(SAMPLE.replace("freed_bytes=33554432", "freed_bytes=0"))

    def test_nursery_only_reclaim_is_not_old_reclaim(self):
        with self.assertRaisesRegex(ValueError, "confirmed_old_reclaims"):
            coverage(SAMPLE.replace("old_baseline=67108864", "old_baseline=100663296"))

    def test_changed_format_is_not_zero(self):
        with self.assertRaisesRegex(ValueError, "missing integer old_threshold"):
            coverage(SAMPLE.replace("old_threshold=", "new_threshold="))

    def test_below_stock_threshold_is_not_eligible(self):
        with self.assertRaisesRegex(ValueError, "nonretaining_old_full"):
            coverage(SAMPLE.replace("old_threshold=50331648", "old_threshold=1048576"))

    def test_unmatched_full_cannot_reuse_previous_decision(self):
        text = SAMPLE + SAMPLE.splitlines()[1] + "\n" + SAMPLE.splitlines()[2]
        self.assertEqual(coverage(text)["productive_nonretaining_old_full"], 1)

    def test_checksum_small_control(self):
        self.assertEqual(expected(1, 1), "reclaim 1 1 2099329")


if __name__ == "__main__":
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(CoverageTest)
    if suite.countTestCases() < 11:
        sys.exit("GC coverage parser suite is dark: expected at least 11 tests")
    sys.exit(not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful())
