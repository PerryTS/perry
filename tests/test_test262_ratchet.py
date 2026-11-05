import importlib.util
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "scripts" / "test262_ratchet.py"
SPEC = importlib.util.spec_from_file_location("test262_ratchet", SCRIPT_PATH)
RATCHET = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RATCHET)


def shard(passed, failures, skip=0, node="v26.12.1"):
    return {
        "test262_pinned": "abc",
        "node_runtime": node,
        "totals": {"pass": passed, "diff": 0, "runtime-fail": len(failures),
                   "compile-fail": 0, "skip": skip},
        "failures": [{"test": t, "bucket": "runtime-fail", "reason": ""} for t in failures],
    }


class Test262RatchetTests(unittest.TestCase):
    def setUp(self):
        self.base = RATCHET.baseline_from(
            RATCHET.merge([shard(5, ["a.js"]), shard(4, ["b.js"])]))

    def test_merge_sums_shards(self):
        agg = RATCHET.merge([shard(5, ["a.js"]), shard(4, ["b.js"], skip=2)])
        self.assertEqual(agg["totals"]["pass"], 9)
        self.assertEqual(agg["judged"], 13)
        self.assertEqual(agg["failures"], ["a.js", "b.js"])

    def test_unchanged_run_passes(self):
        agg = RATCHET.merge([shard(5, ["a.js"]), shard(4, ["b.js"])])
        self.assertEqual(RATCHET.check(agg, self.base), [])

    def test_new_failure_is_regression(self):
        agg = RATCHET.merge([shard(4, ["a.js", "c.js"]), shard(4, ["b.js"])])
        self.assertIn("regression: c.js", RATCHET.check(agg, self.base))

    def test_fixed_known_failure_is_stale(self):
        agg = RATCHET.merge([shard(6, []), shard(4, ["b.js"])])
        self.assertEqual(RATCHET.check(agg, self.base),
                         ["stale known failure (now passes, remove it): a.js"])

    def test_pass_turning_skip_drops_below_floor(self):
        agg = RATCHET.merge([shard(4, ["a.js"], skip=1), shard(4, ["b.js"])])
        self.assertEqual(len(RATCHET.check(agg, self.base)), 1)

    def test_node_mismatch_is_reported(self):
        agg = RATCHET.merge([shard(5, ["a.js"], node="v1"), shard(4, ["b.js"], node="v1")])
        self.assertTrue(any("node_runtime" in e for e in RATCHET.check(agg, self.base)))

    def test_shards_on_different_nodes_refused(self):
        with self.assertRaises(SystemExit):
            RATCHET.merge([shard(1, [], node="v1"), shard(1, [], node="v2")])


if __name__ == "__main__":
    unittest.main()
