import contextlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "copied_minor_fallback_report.py"

SPEC = importlib.util.spec_from_file_location(
    "copied_minor_fallback_report", SCRIPT_PATH
)
assert SPEC is not None
REPORT = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(REPORT)


DEFAULT_SAFE_WORKLOADS = (
    "json_roundtrip",
    "string_churn",
    "object_property_churn",
    "mixed_request_shaping",
    "map_set_churn",
    "promise_churn",
)


def gc_cycle(
    fallback_reason="none",
    *,
    eligible=True,
    conservative_pinned_bytes=0,
    compiled_frame_conservative_pinned_bytes=0,
    runtime_conservative_pinned_bytes=0,
    conservative_stack_truncated=False,
    conservative_stack_unbounded=False,
    legacy_pinned_bytes=0,
    legacy_emitted_young_roots=0,
    legacy_emitted_malloc_roots=0,
    legacy_sources=None,
    malloc_registry_rebuilds=0,
    malloc_kinds=None,
    layout_scans=None,
    root_sources=None,
):
    cycle = {
        "event": "gc_cycle",
        "conservative_pinned_bytes": conservative_pinned_bytes,
        "compiled_frame_conservative_pinned_bytes": (
            compiled_frame_conservative_pinned_bytes
        ),
        "runtime_conservative_pinned_bytes": runtime_conservative_pinned_bytes,
        "conservative_stack_scan_bytes": 0,
        "conservative_stack_scan_limit_bytes": 8 * 1024 * 1024,
        "conservative_stack_truncated": conservative_stack_truncated,
        "conservative_stack_unbounded": conservative_stack_unbounded,
        "conservative_sources": {},
        "legacy_copy_only_scanner_pinned": {
            "bytes": legacy_pinned_bytes,
            "emitted_young_roots": legacy_emitted_young_roots,
            "emitted_malloc_roots": legacy_emitted_malloc_roots,
            "sources": legacy_sources or {},
        },
        "copying_nursery": {
            "fallback_reason": fallback_reason,
            "eligible": eligible,
            "copied_objects": 1,
            "copied_bytes": 16,
            "promoted_objects": 0,
            "promoted_bytes": 0,
            "large_excluded_objects": 0,
            "large_excluded_bytes": 0,
            "malloc_registry_rebuilds": malloc_registry_rebuilds,
        },
        "root_sources": root_sources
        if root_sources is not None
        else default_root_sources(),
    }
    if malloc_kinds is not None:
        cycle["malloc_kinds"] = malloc_kinds
    if layout_scans is not None:
        cycle["layout_scans"] = layout_scans
    return cycle


def root_source_slot(
    registered_scanners=0,
    slots_scanned=0,
    nonzero_slots=0,
    pointer_roots=0,
    rewritten_slots=0,
):
    return {
        "registered_scanners": registered_scanners,
        "slots_scanned": slots_scanned,
        "nonzero_slots": nonzero_slots,
        "pointer_roots": pointer_roots,
        "rewritten_slots": rewritten_slots,
    }


def default_root_sources():
    return {
        "compiled_shadow": root_source_slot(
            slots_scanned=1,
            nonzero_slots=1,
            pointer_roots=1,
            rewritten_slots=1,
        ),
        "module_globals": root_source_slot(),
        "runtime_handles": root_source_slot(),
        "runtime_mutable_scanners": root_source_slot(),
        "ffi_mutable_scanners": root_source_slot(),
        "native_stack_fallback": {
            "decision": "skip_shadow_stack_active",
            "scanned": False,
            "roots_found": 0,
            "pinned_roots": 0,
            "pinned_bytes": 0,
            "compiled_frame_pinned_roots": 0,
            "compiled_frame_pinned_bytes": 0,
        },
    }


def native_stack_root_sources():
    sources = default_root_sources()
    sources["native_stack_fallback"] = {
        "decision": "scan",
        "scanned": True,
        "roots_found": 1,
        "pinned_roots": 1,
        "pinned_bytes": 32,
        "compiled_frame_pinned_roots": 0,
        "compiled_frame_pinned_bytes": 0,
    }
    return sources


def layout_scans():
    return {
        "pointer_slots_read": 1,
        "masked_pointer_slots_read": 0,
        "unknown_layout_slots_read": 0,
        "pointer_free_ranges_skipped": 0,
        "pointer_free_slots_skipped": 0,
    }


def old_page_cycle(*, reusable_bytes=128, returned_bytes=0):
    cycle = gc_cycle(layout_scans=layout_scans())
    cycle["old_pages"] = {
        "allocated_bytes": 0,
        "live_bytes": 0,
        "dead_bytes": 0,
        "reusable_bytes": reusable_bytes,
        "returned_bytes": returned_bytes,
        "pinned_bytes": 0,
    }
    cycle["evacuation_policy"] = {
        "old_page_candidate_pages": 1,
        "old_page_selected_pages": 1,
        "old_page_selected_live_bytes": 64,
        "old_page_reclaimable_bytes": reusable_bytes + returned_bytes,
    }
    cycle["evacuation"] = {
        "old_page_moved_bytes": 64,
        "released_original_bytes": 64,
        "released_original_reusable_bytes": 0,
        "released_original_returned_bytes": 0,
    }
    return cycle


class CopiedMinorFallbackReportTests(unittest.TestCase):
    def run_report(
        self,
        workload_cycles,
        *,
        strict=False,
        target=False,
        allow_target_malloc_kind=(),
    ):
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            args = []
            for name, cycles in workload_cycles.items():
                trace_file = temp_path / f"{name}.jsonl"
                with trace_file.open("w", encoding="utf-8") as fh:
                    for cycle in cycles:
                        fh.write(json.dumps(cycle))
                        fh.write("\n")
                args.extend(["--workload", f"{name}={trace_file}"])

            report_file = temp_path / "report.json"
            if strict:
                args.append("--strict-fallback-evidence")
            if target:
                args.append("--target-collector-gates")
            for allowance in allow_target_malloc_kind:
                args.extend(["--allow-target-malloc-kind", allowance])
            args.extend(["--out", str(report_file)])

            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                exit_code = REPORT.main(args)

            with report_file.open("r", encoding="utf-8") as fh:
                report = json.load(fh)

        return exit_code, stderr.getvalue(), report

    def test_strict_mode_fails_copy_only_roots(self):
        exit_code, stderr, _ = self.run_report(
            {"json_roundtrip": [gc_cycle("copy_only_roots")]},
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("fallback reasons other than none", stderr)
        self.assertIn("copy_only_roots", stderr)

    def test_strict_mode_fails_conservative_stack(self):
        exit_code, stderr, _ = self.run_report(
            {"json_roundtrip": [gc_cycle("conservative_stack")]},
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("fallback reasons other than none", stderr)
        self.assertIn("conservative_stack", stderr)

    def test_strict_mode_fails_conservative_pinned_bytes(self):
        exit_code, stderr, _ = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle(conservative_pinned_bytes=32),
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("conservative_pinned_bytes=32, want 0", stderr)

    def test_strict_mode_fails_compiled_frame_pinned_bytes(self):
        exit_code, stderr, _ = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle(compiled_frame_conservative_pinned_bytes=16),
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn(
            "compiled_frame_conservative_pinned_bytes=16, want 0",
            stderr,
        )

    def test_strict_mode_fails_truncated_or_unbounded_conservative_stack(self):
        exit_code, stderr, _ = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle(conservative_stack_truncated=True),
                    gc_cycle(conservative_stack_unbounded=True),
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("conservative_stack_truncated cycles=1, want 0", stderr)
        self.assertIn("conservative_stack_unbounded cycles=1, want 0", stderr)

    def test_strict_mode_fails_unattributed_root_emissions(self):
        exit_code, stderr, _ = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle(
                        legacy_sources={
                            "unattributed": {
                                "emitted_roots": 1,
                            }
                        }
                    ),
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("unattributed root scanner emitted roots=1, want 0", stderr)

    def test_strict_mode_fails_copy_only_young_and_malloc_emissions(self):
        exit_code, stderr, _ = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle(
                        legacy_emitted_young_roots=2,
                        legacy_emitted_malloc_roots=1,
                    ),
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn(
            "legacy_copy_only_scanner_pinned.emitted_young_roots=2, want 0",
            stderr,
        )
        self.assertIn(
            "legacy_copy_only_scanner_pinned.emitted_malloc_roots=1, want 0",
            stderr,
        )

    def test_strict_mode_fails_ineligible_cycles(self):
        exit_code, stderr, _ = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle(eligible=False),
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("copied-minor ineligible cycles=1", stderr)

    def test_strict_mode_fails_legacy_copy_only_pinned_bytes(self):
        exit_code, stderr, _ = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle(legacy_pinned_bytes=48),
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("legacy_copy_only_scanner_pinned.bytes=48, want 0", stderr)

    def test_strict_mode_fails_malloc_registry_rebuilds(self):
        exit_code, stderr, _ = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle(malloc_registry_rebuilds=2),
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("malloc_registry_rebuilds=2, want 0", stderr)

    def test_strict_mode_fails_forbidden_promise_malloc_kind(self):
        exit_code, stderr, _ = self.run_report(
            {
                "promise_churn": [
                    gc_cycle(
                        malloc_kinds=[{"kind": "promise", "allocated_count": 1}],
                    ),
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn(
            "promise_churn: forbidden malloc allocation kind promise count=1",
            stderr,
        )

    def test_strict_mode_requires_a_default_safe_workload(self):
        exit_code, stderr, _ = self.run_report(
            {"diagnostic_conservative": [gc_cycle()]},
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("requires at least one known default-safe workload", stderr)

    def test_strict_mode_passes_default_safe_workloads_with_no_fallback(self):
        exit_code, stderr, report = self.run_report(
            {name: [gc_cycle()] for name in DEFAULT_SAFE_WORKLOADS},
            strict=True,
        )

        self.assertEqual(exit_code, 0, stderr)
        self.assertIsNone(report["top_remaining_reason"])
        self.assertEqual(report["summary"]["fallback_reason_counts"]["none"], 6)
        self.assertEqual(
            report["summary"]["root_sources"]["compiled_shadow"]["pointer_roots"],
            6,
        )

    def test_strict_mode_requires_mutable_or_shadow_root_source_evidence(self):
        root_sources = default_root_sources()
        root_sources["compiled_shadow"] = root_source_slot(slots_scanned=1)
        exit_code, stderr, _ = self.run_report(
            {"json_roundtrip": [gc_cycle(root_sources=root_sources)]},
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("no mutable or shadow root source evidence", stderr)

    def test_strict_mode_fails_missing_root_sources(self):
        exit_code, stderr, _ = self.run_report(
            {"json_roundtrip": [gc_cycle(root_sources={})]},
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("missing root_sources", stderr)

    def test_strict_mode_requires_native_attribution_for_conservative_stack(self):
        exit_code, stderr, _ = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle(
                        "conservative_stack",
                        eligible=False,
                        root_sources=default_root_sources(),
                    )
                ]
            },
            strict=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn("conservative_stack fallback has no native stack attribution", stderr)

    def test_non_strict_mode_permits_known_fallback_reasons_for_reporting(self):
        exit_code, stderr, report = self.run_report(
            {
                "json_roundtrip": [
                    gc_cycle("copy_only_roots", eligible=False),
                    gc_cycle("copy_only_roots", eligible=False),
                    gc_cycle("conservative_stack_truncated", eligible=False),
                    gc_cycle("conservative_stack_unbounded", eligible=False),
                    gc_cycle("unattributed_root_source", eligible=False),
                ]
            },
        )

        self.assertEqual(exit_code, 0, stderr)
        self.assertEqual(
            report["workloads"]["json_roundtrip"]["fallback_reason_counts"][
                "copy_only_roots"
            ],
            2,
        )
        self.assertEqual(
            report["workloads"]["json_roundtrip"]["fallback_reason_counts"][
                "conservative_stack_truncated"
            ],
            1,
        )
        self.assertEqual(
            report["workloads"]["json_roundtrip"]["fallback_reason_counts"][
                "conservative_stack_unbounded"
            ],
            1,
        )
        self.assertEqual(
            report["workloads"]["json_roundtrip"]["fallback_reason_counts"][
                "unattributed_root_source"
            ],
            1,
        )
        self.assertEqual(report["top_remaining_reason"]["reason"], "copy_only_roots")

    def test_target_gates_fail_forbidden_string_and_closure_malloc_kinds(self):
        exit_code, stderr, _ = self.run_report(
            {
                "string_heavy": [
                    gc_cycle(
                        malloc_kinds=[
                            {"kind": "string", "allocated_count": 2},
                            {"kind": "closure", "allocated_count": 1},
                            {"kind": "promise", "allocated_count": 4},
                        ],
                        layout_scans=layout_scans(),
                    )
                ]
            },
            target=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn(
            "string_heavy: forbidden malloc allocation kind string count=2 "
            "exceeds allowance=0",
            stderr,
        )
        self.assertIn(
            "string_heavy: forbidden malloc allocation kind closure count=1 "
            "exceeds allowance=0",
            stderr,
        )
        self.assertIn(
            "string_heavy: forbidden malloc allocation kind promise count=4 "
            "exceeds allowance=0",
            stderr,
        )

    def test_target_gates_ignore_non_forbidden_malloc_kinds(self):
        exit_code, stderr, report = self.run_report(
            {
                "default_copying": [
                    gc_cycle(
                        malloc_kinds=[
                            {"kind": "array", "allocated_count": 3},
                            {"kind": "object", "allocated_count": 5},
                        ],
                        layout_scans=layout_scans(),
                    )
                ]
            },
            target=True,
        )

        self.assertEqual(exit_code, 0, stderr)
        self.assertEqual(
            report["workloads"]["default_copying"]["malloc_kind_allocations"],
            {"string": 0, "closure": 0, "promise": 0},
        )

    def test_target_gates_honor_documented_malloc_kind_allowances(self):
        exit_code, stderr, report = self.run_report(
            {
                "string_heavy": [
                    gc_cycle(
                        malloc_kinds=[
                            {"kind": "string", "allocated_count": 2},
                            {"kind": "closure", "allocated_count": 1},
                            {"kind": "promise", "allocated_count": 3},
                        ],
                        layout_scans=layout_scans(),
                    )
                ]
            },
            target=True,
            allow_target_malloc_kind=(
                "string_heavy:string=2",
                "string_heavy:closure=1",
                "string_heavy:promise=3",
            ),
        )

        self.assertEqual(exit_code, 0, stderr)
        self.assertEqual(
            report["workloads"]["string_heavy"]["malloc_kind_allocations"],
            {"string": 2, "closure": 1, "promise": 3},
        )

    def test_target_gates_allow_async_forced_evacuation_fallback_trace(self):
        exit_code, stderr, report = self.run_report(
            {
                "async_promise_closures": [
                    gc_cycle(
                        "conservative_stack",
                        eligible=False,
                        malloc_kinds=[
                            {"kind": "promise", "allocated_count": 0},
                            {"kind": "string", "allocated_count": 0},
                            {"kind": "closure", "allocated_count": 0},
                        ],
                        layout_scans=layout_scans(),
                    )
                ],
                "default_copying": [
                    gc_cycle(
                        malloc_kinds=[
                            {"kind": "string", "allocated_count": 0},
                            {"kind": "closure", "allocated_count": 0},
                            {"kind": "promise", "allocated_count": 0},
                        ],
                        layout_scans=layout_scans(),
                    )
                ],
            },
            target=True,
        )

        self.assertEqual(exit_code, 0, stderr)
        self.assertEqual(
            report["workloads"]["async_promise_closures"][
                "fallback_reason_counts"
            ]["conservative_stack"],
            1,
        )

    def test_target_gates_require_old_page_reuse_or_return(self):
        exit_code, stderr, _ = self.run_report(
            {
                "default_copying": [gc_cycle(layout_scans=layout_scans())],
                "old_page_forced_defrag": [old_page_cycle(reusable_bytes=0)],
            },
            target=True,
        )

        self.assertNotEqual(exit_code, 0)
        self.assertIn(
            "old_page_forced_defrag: forced old-page workload reported no reusable or returned bytes",
            stderr,
        )

    def test_target_gates_accept_old_page_reuse_or_return(self):
        exit_code, stderr, report = self.run_report(
            {
                "default_copying": [gc_cycle(layout_scans=layout_scans())],
                "old_page_forced_defrag": [old_page_cycle(returned_bytes=256)],
            },
            target=True,
        )

        self.assertEqual(exit_code, 0, stderr)
        old_page = report["workloads"]["old_page_forced_defrag"][
            "old_page_accounting"
        ]
        self.assertEqual(old_page["old_page_moved_bytes"], 64)
        self.assertEqual(old_page["reusable_bytes"], 128)
        self.assertEqual(old_page["returned_bytes"], 256)


if __name__ == "__main__":
    unittest.main()
