"""Negative controls for receiver attribution; these are harness, not runtime proofs."""
import unittest

from constfn_executable_gates import (
    isolated_refusal_formation, isolated_refusal_routes, numeric_routes,
)


def census(**overrides):
    counters = dict(rloop_f=200, rloop_f_rep=200, rloop_bare=200,
                    rloop_g=2376, rloop_static=1)
    counters.update(overrides)
    return ("[store-census] emit.cfield.loop_raw_store=0 emit.cfield.ic_call=200 "
            "emit.cfield.guard_fallback_call=200 "
            "emit.elem.read.versioned_indexed=0 emit.elem.read.fallback_call=128\n"
            "PERRY_RECV_ROUTES " + " ".join(f"{k}={v}" for k, v in counters.items()) + "\n")


CELL_IR = '''define void @perry_fn_test__untouchedCell() {
entry:
  br label %rloop.guard.1
rloop.guard.1:
  br i1 %valid, label %rloop.fast.1, label %rloop.generic.1
rloop.fast.1:
  call void @js_recv_route_note(i32 34)
  call void @js_recv_route_note(i32 18)
  store double 4.2e1, ptr %slot
  br label %rloop.join.1
rloop.generic.1:
  br label %rloop.join.1
rloop.join.1:
  ret void
}
'''


class RefusalAttributionTests(unittest.TestCase):
    def test_original_refusal_keeps_only_untouched_cell(self):
        numeric_routes(census(), "combined", constfn=False, refusal=True)

    def test_global_zero_cannot_hide_lost_cell_coverage(self):
        with self.assertRaises(RuntimeError):
            numeric_routes(census(rloop_f=0, rloop_f_rep=0, rloop_bare=0),
                           "lost cell", constfn=False, refusal=True)

    def test_additional_affected_pair_admissions_fail(self):
        with self.assertRaises(RuntimeError):
            numeric_routes(census(rloop_f=201, rloop_f_rep=201, rloop_bare=203),
                           "affected Pair", constfn=False, refusal=True)

    def test_bare_count_cannot_borrow_read_store_witness(self):
        with self.assertRaises(RuntimeError):
            numeric_routes(census(rloop_bare=400), "wrong region", constfn=False, refusal=True)

    def test_isolated_pair_refuses(self):
        isolated_refusal_routes(census(rloop_f=0, rloop_f_rep=0, rloop_bare=0,
                                      rloop_g=0, rloop_plain=1, rloop_guard=1),
                                "Pair", kind="pair")

    def test_unrelated_cell_cannot_mask_pair_failure(self):
        with self.assertRaises(RuntimeError):
            isolated_refusal_routes(census(), "Pair", kind="pair")

    def test_refusal_must_execute_its_guarded_plain_loop(self):
        with self.assertRaises(RuntimeError):
            isolated_refusal_routes(census(rloop_f=0, rloop_f_rep=0, rloop_bare=0, rloop_g=0),
                                    "dead Pair", kind="pair")

    def test_iteration_g_cannot_replace_preheader_refusal(self):
        with self.assertRaises(RuntimeError):
            isolated_refusal_routes(census(rloop_f=0, rloop_f_rep=0, rloop_bare=0,
                                          rloop_g=200, rloop_plain=1, rloop_guard=1),
                                    "wrong entry", kind="pair")

    def test_plain_loop_without_exact_stores_is_dead_coverage(self):
        for count in [0, 199, 201]:
            stderr = census(rloop_f=0, rloop_f_rep=0, rloop_bare=0,
                            rloop_g=0, rloop_plain=1, rloop_guard=1).replace(
                "emit.cfield.ic_call=200", f"emit.cfield.ic_call={count}")
            with self.subTest(count=count), self.assertRaises(RuntimeError):
                isolated_refusal_routes(stderr, "missing stores", kind="pair")

    def test_cell_requires_own_static_store_only_region(self):
        isolated_refusal_routes(census(rloop_g=0), "cell", kind="cell", mutated=False)
        with self.assertRaises(RuntimeError):
            isolated_refusal_routes(census(rloop_g=0, rloop_static=0),
                                    "unattributed cell", kind="cell", mutated=False)

    def test_cell_ir_cannot_be_dead_or_read_store_twin(self):
        isolated_refusal_formation(CELL_IR, "cell")
        for altered in (
            CELL_IR.replace("br i1 %valid, label %rloop.fast.1, label %rloop.generic.1",
                            "br label %rloop.generic.1"),
            CELL_IR.replace("store double 4.2e1, ptr %slot", "ret void"),
            CELL_IR.replace("i32 34", "i32 14"),
            CELL_IR.replace("i32 18)", "i32 18)\n  call void @js_recv_route_note(i32 18)"),
            CELL_IR.replace("rloop.generic.1", "class_field.loop.slow"),
        ):
            with self.subTest(altered=altered), self.assertRaises(RuntimeError):
                isolated_refusal_formation(altered, "cell")

    def test_preheader_versioning_is_a_real_conditional_entry(self):
        versioned = CELL_IR.replace("rloop.guard.1", "rloop.version.split.1").replace(
            "label %rloop.fast.1,", "label %for.body.1,").replace(
            "rloop.fast.1:\n", "for.body.1:\n  br label %rloop.fast.1\nrloop.fast.1:\n")
        isolated_refusal_formation(versioned, "cell")
        dead = versioned.replace("br label %rloop.fast.1", "br label %rloop.generic.1")
        with self.assertRaises(RuntimeError):
            isolated_refusal_formation(dead, "cell")

    def test_an_unguarded_entry_cannot_borrow_another_guard(self):
        unguarded = CELL_IR.replace("br label %rloop.guard.1",
            "br i1 %bypass, label %rloop.fast.1, label %rloop.guard.1")
        with self.assertRaises(RuntimeError):
            isolated_refusal_formation(unguarded, "cell")


if __name__ == "__main__":
    unittest.main()
