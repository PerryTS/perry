import re
import unittest
from pathlib import Path
import link_absence


class GcDiagnosticBoundary(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        source = (Path(__file__).parents[1] / "tests/link_absence/gc_diagnostics.ts").read_text()
        cls.rule = {
            "prefixes": re.search(r"^// HWS prefixes: (.+)$", source, re.M)[1].split("|"),
            "implementations": re.search(r"^// HWS implementations: (.+)$", source, re.M)[1].split("|"),
        }

    def test_diagnostic_implementation_cannot_hide_in_collector_module(self):
        for name in ["mark::verify_heap_objects", "remembered::verify_future_invariant::{closure#0}",
                     "mark::GcCycleTrace>::new", "rewrite::record_layout_child_slot_read",
                     "mark::report_heap_counts"]:
            with self.subTest(name=name):
                self.assertTrue(link_absence.matches({"name": "perry_runtime::gc::" + name}, self.rule))

    def test_collector_operations_are_not_diagnostics(self):
        for name in ["mark::try_mark_young_value_as_seed",
                     "remembered::rebuild_minor_old_to_young_remembered_set",
                     "rewrite::follow_forwarding_raw_addr", "progress::GcDebtSnapshot>::current"]:
            with self.subTest(name=name):
                self.assertFalse(link_absence.matches({"name": "perry_runtime::gc::" + name}, self.rule))

    def test_generic_arguments_do_not_decide(self):
        # Collector generic instantiated with a diagnostic closure: not a diagnostic.
        collector = ("perry_runtime::gc::roots::stack_maps::unwind::walk_frame::<perry_runtime::gc::"
                     "roots::stack_maps::verify_native_slots_post_walk::{closure#2}>::{closure#0}")
        self.assertFalse(link_absence.matches({"name": collector}, self.rule))
        # Diagnostic generic over a collector type: still a diagnostic.
        for name in ["perry_runtime::gc::mark::report_heap_counts::<perry_runtime::gc::mark::Marker>",
                     "perry_runtime::gc::roots::stack_maps::verify::visit::<perry_runtime::gc::mark::Marker>::{closure#0}",
                     "<perry_runtime::gc::mark::Marker as perry_runtime::gc::mark::Visit<perry_runtime::gc::mark::X>>::verify_slot"]:
            with self.subTest(name=name):
                self.assertTrue(link_absence.matches({"name": name}, self.rule))


if __name__ == "__main__":
    unittest.main()
