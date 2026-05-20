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
)


def gc_cycle(
    fallback_reason="none",
    *,
    eligible=True,
    conservative_pinned_bytes=0,
    legacy_pinned_bytes=0,
    malloc_registry_rebuilds=0,
):
    return {
        "event": "gc_cycle",
        "conservative_pinned_bytes": conservative_pinned_bytes,
        "legacy_copy_only_scanner_pinned": {
            "bytes": legacy_pinned_bytes,
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
    }


class CopiedMinorFallbackReportTests(unittest.TestCase):
    def run_report(self, workload_cycles, *, strict=False):
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
        self.assertEqual(report["summary"]["fallback_reason_counts"]["none"], 4)

    def test_non_strict_mode_permits_known_fallback_reasons_for_reporting(self):
        exit_code, stderr, report = self.run_report(
            {"json_roundtrip": [gc_cycle("copy_only_roots", eligible=False)]},
        )

        self.assertEqual(exit_code, 0, stderr)
        self.assertEqual(
            report["workloads"]["json_roundtrip"]["fallback_reason_counts"][
                "copy_only_roots"
            ],
            1,
        )
        self.assertEqual(report["top_remaining_reason"]["reason"], "copy_only_roots")


if __name__ == "__main__":
    unittest.main()
