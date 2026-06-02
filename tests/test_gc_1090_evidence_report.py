import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "gc_1090_evidence_report.py"

SPEC = importlib.util.spec_from_file_location("gc_1090_evidence_report", SCRIPT_PATH)
assert SPEC is not None
REPORT = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(REPORT)


REQUIRED_BENCHMARKS = (
    "bench_json_roundtrip",
    "bench_gc_pressure",
    "07_object_create",
    "12_binary_trees",
)


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def copied_workload(
    *,
    fallback_reason="none",
    ineligible_cycles=0,
    conservative_pinned_bytes=0,
    compiled_frame_conservative_pinned_bytes=0,
    conservative_stack_truncated_cycles=0,
    conservative_stack_unbounded_cycles=0,
    copy_only_pinned_bytes=0,
    copy_only_young_roots=0,
    copy_only_malloc_roots=0,
    unattributed_roots=0,
    malloc_registry_rebuilds=0,
    non_minor_cycles=0,
    phase_sweep_us=0,
    phase_block_persistence_us=0,
    phase_root_marking_us=0,
    phase_trace_worklist_us=0,
    phase_reference_rewrite_us=0,
    block_persist_iterations=0,
    block_persist_candidate_blocks=0,
    block_persist_live_blocks=0,
    block_persist_marked_objects=0,
    root_growth_mutable_first=1,
    root_growth_mutable_max=1,
    root_growth_registered_first=0,
    root_growth_registered_max=0,
    remembered_set_stale_entries=0,
    pause_us=10,
    dirty_pages_scanned=0,
    dirty_slots_scanned=0,
    old_objects_considered=0,
    mutable_root_slots_scanned=1,
    mutable_registered_slots_scanned=0,
    external_live_bytes_last=0,
    external_live_bytes_max=0,
    external_cache_reserved_bytes_last=0,
    external_registered_bytes=0,
    external_finalized_bytes=0,
    external_owner_moves=0,
    external_young_owner_count_max=0,
    external_young_owner_checks=0,
):
    counts = {reason: 0 for reason in REPORT.FALLBACK_REASONS}
    counts[fallback_reason] = 1
    return {
        "fallback_reason_counts": counts,
        "conservative_pinned_bytes": conservative_pinned_bytes,
        "compiled_frame_conservative_pinned_bytes": (
            compiled_frame_conservative_pinned_bytes
        ),
        "conservative_stack": {
            "truncated_cycles": conservative_stack_truncated_cycles,
            "unbounded_cycles": conservative_stack_unbounded_cycles,
        },
        "legacy_copy_only_scanner_pinned": {
            "bytes": copy_only_pinned_bytes,
            "emitted_young_roots": copy_only_young_roots,
            "emitted_malloc_roots": copy_only_malloc_roots,
            "sources": {
                "unattributed": {"emitted_roots": unattributed_roots}
            },
        },
        "copying_nursery": {
            "copied_objects": 1,
            "copied_bytes": 16,
            "promoted_objects": 0,
            "promoted_bytes": 0,
            "malloc_registry_rebuilds": malloc_registry_rebuilds,
            "ineligible_cycles": ineligible_cycles,
        },
        "non_minor_cycles": non_minor_cycles,
        "phase_us": {
            "sweep": phase_sweep_us,
            "block_persistence": phase_block_persistence_us,
            "root_marking": phase_root_marking_us,
            "trace_worklist": phase_trace_worklist_us,
            "reference_rewrite": phase_reference_rewrite_us,
        },
        "block_persist": {
            "iterations": block_persist_iterations,
            "candidate_blocks": block_persist_candidate_blocks,
            "live_blocks": block_persist_live_blocks,
            "marked_objects": block_persist_marked_objects,
        },
        "root_growth": {
            "mutable_slots_scanned": {
                "first": root_growth_mutable_first,
                "last": root_growth_mutable_max,
                "min": root_growth_mutable_first,
                "max": root_growth_mutable_max,
            },
            "mutable_registered_slots_scanned": {
                "first": root_growth_registered_first,
                "last": root_growth_registered_max,
                "min": root_growth_registered_first,
                "max": root_growth_registered_max,
            },
        },
        "pause_us": pause_us,
        "remembered_set": {
            "stale_entries": remembered_set_stale_entries,
            "dirty_old_pages_before": 0,
            "external_dirty_slot_pages_before": 0,
            "external_dirty_entries_before": 0,
            "dirty_pages_scanned": dirty_pages_scanned,
            "dirty_slots_scanned": dirty_slots_scanned,
            "old_objects_considered": old_objects_considered,
        },
        "external_memory": {
            "live_bytes": {
                "first": external_live_bytes_last,
                "last": external_live_bytes_last,
                "min": 0,
                "max": external_live_bytes_max,
            },
            "cache_reserved_bytes": {
                "first": external_cache_reserved_bytes_last,
                "last": external_cache_reserved_bytes_last,
                "min": 0,
                "max": external_cache_reserved_bytes_last,
            },
            "young_owner_count": {
                "first": external_young_owner_count_max,
                "last": external_young_owner_count_max,
                "min": 0,
                "max": external_young_owner_count_max,
            },
            "registered_bytes": external_registered_bytes,
            "finalized_bytes": external_finalized_bytes,
            "owner_moves": external_owner_moves,
            "copied_minor_young_owner_checks": external_young_owner_checks,
            "kinds": {},
        },
        "mutable_roots": {
            "slots_scanned": mutable_root_slots_scanned,
            "nonzero_slots": 1 if mutable_root_slots_scanned else 0,
            "pointer_roots": 1 if mutable_root_slots_scanned else 0,
            "rewritten_slots": 1 if mutable_root_slots_scanned else 0,
            "shadow_slots_scanned": 1 if mutable_root_slots_scanned else 0,
            "global_slots_scanned": 0,
            "registered_slots_scanned": mutable_registered_slots_scanned,
            "metadata_slots_scanned": 0,
        },
    }


def copied_report(**overrides):
    workloads = {
        name: copied_workload()
        for name in REPORT.STRICT_COPIED_MINOR_WORKLOADS
    }
    workloads.update(overrides)
    external_registered_bytes = sum(
        REPORT.nested(workload, "external_memory", "registered_bytes", default=0)
        for workload in workloads.values()
    )
    external_finalized_bytes = sum(
        REPORT.nested(workload, "external_memory", "finalized_bytes", default=0)
        for workload in workloads.values()
    )
    external_owner_moves = sum(
        REPORT.nested(workload, "external_memory", "owner_moves", default=0)
        for workload in workloads.values()
    )
    external_young_owner_checks = sum(
        REPORT.nested(
            workload,
            "external_memory",
            "copied_minor_young_owner_checks",
            default=0,
        )
        for workload in workloads.values()
    )
    external_live_bytes_last = max(
        REPORT.nested(workload, "external_memory", "live_bytes", "last", default=0)
        for workload in workloads.values()
    )
    external_live_bytes_max = max(
        REPORT.nested(workload, "external_memory", "live_bytes", "max", default=0)
        for workload in workloads.values()
    )
    external_cache_reserved_bytes_last = max(
        REPORT.nested(workload, "external_memory", "cache_reserved_bytes", "last", default=0)
        for workload in workloads.values()
    )
    external_young_owner_count_max = max(
        REPORT.nested(workload, "external_memory", "young_owner_count", "max", default=0)
        for workload in workloads.values()
    )
    return {
        "summary": {
            "cycles": len(workloads),
            "fallback_reason_counts": {"none": len(workloads)},
            "conservative_pinned_bytes": 0,
            "compiled_frame_conservative_pinned_bytes": 0,
            "conservative_stack": {
                "truncated_cycles": 0,
                "unbounded_cycles": 0,
            },
            "legacy_copy_only_scanner_pinned": {
                "bytes": 0,
                "emitted_young_roots": 0,
                "emitted_malloc_roots": 0,
                "sources": {"unattributed": {"emitted_roots": 0}},
            },
            "copying_nursery": {
                "copied_objects": len(workloads),
                "copied_bytes": len(workloads) * 16,
                "promoted_objects": 0,
                "promoted_bytes": 0,
                "malloc_registry_rebuilds": 0,
            },
            "external_memory": {
                "live_bytes": {
                    "first": 0,
                    "last": external_live_bytes_last,
                    "min": 0,
                    "max": external_live_bytes_max,
                },
                "cache_reserved_bytes": {
                    "first": 0,
                    "last": external_cache_reserved_bytes_last,
                    "min": 0,
                    "max": external_cache_reserved_bytes_last,
                },
                "young_owner_count": {
                    "first": 0,
                    "last": external_young_owner_count_max,
                    "min": 0,
                    "max": external_young_owner_count_max,
                },
                "registered_bytes": external_registered_bytes,
                "finalized_bytes": external_finalized_bytes,
                "owner_moves": external_owner_moves,
                "copied_minor_young_owner_checks": external_young_owner_checks,
            },
            "remembered_set": {
                "stale_entries": 0,
                "dirty_old_pages_before": 0,
                "external_dirty_slot_pages_before": 0,
                "external_dirty_entries_before": 0,
                "dirty_pages_scanned": 0,
                "dirty_slots_scanned": 0,
                "old_objects_considered": 0,
            },
            "mutable_roots": {
                "slots_scanned": len(workloads),
                "registered_slots_scanned": 0,
            },
        },
        "scaling": {},
        "workloads": workloads,
    }


def target_report():
    return {
        "summary": {
            "cycles": 1,
            "fallback_reason_counts": {"none": 1},
            "copying_nursery": {
                "copied_objects": 1,
                "copied_bytes": 16,
                "promoted_objects": 0,
                "promoted_bytes": 0,
                "malloc_registry_rebuilds": 0,
            },
            "old_page_accounting": {},
        }
    }


def benchmark_report(multiplier=1, correctness="pass"):
    benchmarks = {}
    for name in REQUIRED_BENCHMARKS:
        benchmarks[name] = {
            "perry_ms": 100 * multiplier,
            "perry_rss_kb": 100_000 * multiplier,
            "correctness": {
                "status": correctness,
                "reason": "matched",
                "actual_lines": ["checksum:1"],
                "expected_lines": ["checksum:1"],
            },
        }
    return {"commit": "abc", "benchmarks": benchmarks}


def perf_frontier_packet():
    classifications = {
        name: {
            "class": "numeric-representation-bound",
            "reasons": ["synthetic"],
            "evidence": {},
        }
        for name in REQUIRED_BENCHMARKS
    }
    return {
        "schema_version": 1,
        "status": "pass",
        "errors": [],
        "warnings": [],
        "classification": classifications,
        "profile_summary": {
            "status": "pass",
            "row": "class_method_no_field_access",
            "top_non_gc_costs": [
                {"symbol": "js_object_get_own_field_or_undef", "samples": 10}
            ],
        },
        "baseline": {
            "input_path": "tmp/perf-frontier-baseline.json",
            "baseline_sha": "c" * 40,
            "present": True,
        },
    }


def gc_store_inventory_packet(**summary_overrides):
    summary = {
        "annotations": 61,
        "audited_sites": 76,
        "files_scanned": 333,
        "unaudited_sites": 0,
        "invalid_annotations": 0,
        "stale_annotations": 0,
        "missing_gc_type_metadata": 0,
        "duplicate_gc_type_metadata": 0,
    }
    summary.update(summary_overrides)
    return {
        "schema_version": 1,
        "status": "pass" if all(
            summary.get(field, 0) == 0
            for field in (
                "unaudited_sites",
                "invalid_annotations",
                "stale_annotations",
                "missing_gc_type_metadata",
                "duplicate_gc_type_metadata",
            )
        ) else "fail",
        "summary": summary,
        "errors": [],
    }


def old_page_policy_packet(
    *,
    base_peak_kb=120_000,
    head_peak_kb=90_000,
    base_retained_kb=120_000,
    head_retained_kb=90_000,
    checksum=42,
    structural=True,
    churn_samples=None,
):
    if churn_samples is None:
        churn_samples = [100_000, 104_000, 106_000, 107_000, 108_000, 109_000]
    old_page = {
        "selected_pages": 1 if structural else 0,
        "old_page_scanned_objects": 2 if structural else 0,
        "old_page_moved_objects": 1 if structural else 0,
        "old_page_moved_bytes": 64 if structural else 0,
        "released_original_bytes": 64 if structural else 0,
        "released_original_reusable_bytes": 128 if structural else 0,
        "released_original_returned_bytes": 0,
        "reusable_bytes": 128 if structural else 0,
        "returned_bytes": 0,
    }
    return {
        "schema_version": 1,
        "bench_json_roundtrip_retained": {
            "base": {
                "checksum": checksum,
                "peak_rss_kb": base_peak_kb,
                "retained_rss_kb": base_retained_kb,
                "trace_path": "base.trace",
                "old_page": {},
            },
            "head": {
                "checksum": checksum,
                "peak_rss_kb": head_peak_kb,
                "retained_rss_kb": head_retained_kb,
                "trace_path": "head.trace",
                "old_page": old_page,
            },
        },
        "old_gen_churn_retained": {
            "samples_rss_kb": churn_samples,
            "warmup_samples": 2,
            "plateau_allowance_kb": REPORT.OLD_GEN_CHURN_PLATEAU_ALLOWANCE_KB,
            "old_page": {},
        },
    }


class Gc1090EvidenceReportTests(unittest.TestCase):
    def make_root(
        self,
        *,
        head_copied=None,
        base_benchmarks=None,
        head_benchmarks=None,
        head_memory_failed=0,
    ):
        temp = tempfile.TemporaryDirectory()
        root = Path(temp.name)
        metadata = {
            "base_ref": "origin/main",
            "head_ref": "HEAD",
            "base_sha": "a" * 40,
            "head_sha": "b" * 40,
            "commands": {
                "base": {
                    "build": {"status": "pass", "exit_code": 0},
                    "memory_stability": {"status": "pass", "exit_code": 0},
                    "benchmarks": {"status": "pass", "exit_code": 0},
                },
                "head": {
                    "build": {"status": "pass", "exit_code": 0},
                    "memory_stability": {"status": "pass", "exit_code": 0},
                    "benchmarks": {"status": "pass", "exit_code": 0},
                },
            },
        }
        write_json(root / "metadata.json", metadata)
        for label in ("base", "head"):
            write_json(
                root / label / "memory" / "reports" / "memory_stability_summary.json",
                {
                    "script": "run_memory_stability_tests.sh",
                    "passed": 58,
                    "failed": head_memory_failed if label == "head" else 0,
                    "skipped": 0,
                },
            )
            write_json(
                root / label / "memory" / "reports" / "copied_minor_fallback_report.json",
                head_copied if label == "head" and head_copied is not None else copied_report(),
            )
            write_json(
                root / label / "memory" / "reports" / "target_collector_gates_report.json",
                target_report(),
            )
            write_json(
                root / label / "benchmarks" / "full.json",
                (
                    head_benchmarks
                    if label == "head" and head_benchmarks is not None
                    else base_benchmarks
                    if label == "base" and base_benchmarks is not None
                    else benchmark_report()
                ),
            )
        return temp, root

    def add_perf_frontier(self, root, *, old_page_policy=True, old_page_policy_data=None):
        metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
        metadata.setdefault("commands", {}).setdefault("packet", {})["perf_frontier"] = {
            "status": "pass",
            "exit_code": 0,
        }
        metadata.setdefault("commands", {}).setdefault("packet", {})["gc_store_inventory"] = {
            "status": "pass",
            "exit_code": 0,
        }
        if old_page_policy:
            metadata.setdefault("commands", {}).setdefault("packet", {})["old_page_policy"] = {
                "status": "pass",
                "exit_code": 0,
            }
        write_json(root / "metadata.json", metadata)
        write_json(root / "perf-frontier" / "perf-frontier-packet.json", perf_frontier_packet())
        write_json(root / "gc-store-site-inventory.json", gc_store_inventory_packet())
        if old_page_policy:
            write_json(
                root / "old-page-policy.json",
                old_page_policy_data
                if old_page_policy_data is not None
                else old_page_policy_packet(),
            )

    def collect(self, **kwargs):
        temp, root = self.make_root(**kwargs)
        self.addCleanup(temp.cleanup)
        return REPORT.collect_report(root, "base", "head")

    def collect_gate(self, **kwargs):
        temp, root = self.make_root(**kwargs)
        self.addCleanup(temp.cleanup)
        self.add_perf_frontier(root)
        return REPORT.collect_report(root, "base", "head", gate=True)

    def test_pass_case(self):
        packet = self.collect()
        self.assertEqual(packet["status"], "pass")
        self.assertEqual(packet["errors"], [])

    def test_main_writes_packet_files(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        exit_code = REPORT.main(["--root", str(root)])
        self.assertEqual(exit_code, 0)
        self.assertTrue((root / "gc-1090-packet.json").exists())
        self.assertTrue((root / "gc-1090-packet.md").exists())
        packet = json.loads((root / "gc-1090-packet.json").read_text(encoding="utf-8"))
        self.assertEqual(packet["status"], "pass")
        self.assertIn("# #1090 GC Evidence Packet: PASS", (root / "gc-1090-packet.md").read_text(encoding="utf-8"))

    def test_fails_conservative_stack(self):
        packet = self.collect(
            head_copied=copied_report(
                json_roundtrip=copied_workload(fallback_reason="conservative_stack")
            )
        )
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any("fallback reasons other than none" in error for error in packet["errors"])
        )

    def test_fails_conservative_pinned_bytes(self):
        packet = self.collect(
            head_copied=copied_report(
                json_roundtrip=copied_workload(conservative_pinned_bytes=8)
            )
        )
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any("conservative_pinned_bytes=8" in error for error in packet["errors"])
        )

    def test_fails_compiled_frame_pinned_bytes(self):
        packet = self.collect(
            head_copied=copied_report(
                json_roundtrip=copied_workload(
                    compiled_frame_conservative_pinned_bytes=8
                )
            )
        )
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any(
                "compiled_frame_conservative_pinned_bytes=8" in error
                for error in packet["errors"]
            )
        )

    def test_fails_truncated_unbounded_or_unattributed_roots(self):
        packet = self.collect(
            head_copied=copied_report(
                json_roundtrip=copied_workload(
                    conservative_stack_truncated_cycles=1,
                    conservative_stack_unbounded_cycles=1,
                    unattributed_roots=1,
                    copy_only_young_roots=1,
                    copy_only_malloc_roots=1,
                )
            )
        )
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any("conservative_stack_truncated cycles=1" in error for error in packet["errors"])
        )
        self.assertTrue(
            any("conservative_stack_unbounded cycles=1" in error for error in packet["errors"])
        )
        self.assertTrue(
            any("unattributed root scanner emitted roots=1" in error for error in packet["errors"])
        )
        self.assertTrue(
            any("emitted_young_roots=1" in error for error in packet["errors"])
        )
        self.assertTrue(
            any("emitted_malloc_roots=1" in error for error in packet["errors"])
        )

    def test_fails_benchmark_correctness(self):
        packet = self.collect(head_benchmarks=benchmark_report(correctness="fail"))
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("correctness failed" in error for error in packet["errors"]))

    def test_fails_memory_stability(self):
        packet = self.collect(head_memory_failed=1)
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("memory stability failed=1" in error for error in packet["errors"]))

    def test_gate_includes_perf_frontier_fields(self):
        packet = self.collect_gate()
        self.assertEqual(packet["status"], "pass")
        self.assertIn("tool_versions", packet)
        self.assertEqual(packet["gc_store_inventory"]["status"], "pass")
        self.assertEqual(packet["gc_store_inventory"]["summary"]["unaudited_sites"], 0)
        self.assertEqual(packet["old_page_policy"]["status"], "pass")
        self.assertEqual(
            packet["old_page_policy"]["bench_json_roundtrip"]["rss_gate"],
            "pass",
        )
        self.assertEqual(packet["perf_frontier"]["status"], "pass")
        self.assertIn("bench_json_roundtrip", packet["perf_frontier"]["classification"])
        self.assertEqual(
            packet["perf_frontier"]["baseline"]["input_path"],
            "tmp/perf-frontier-baseline.json",
        )

    def test_old_page_policy_retained_rss_improvement_can_pass_when_peak_does_not(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        self.add_perf_frontier(
            root,
            old_page_policy_data=old_page_policy_packet(
                base_peak_kb=120_000,
                head_peak_kb=119_000,
                base_retained_kb=120_000,
                head_retained_kb=90_000,
            ),
        )

        packet = REPORT.collect_report(root, "base", "head", gate=True)

        self.assertEqual(packet["status"], "pass")
        old_page = packet["old_page_policy"]["bench_json_roundtrip"]
        self.assertEqual(old_page["peak_gate"], "fail")
        self.assertEqual(old_page["retained_gate"], "pass")
        self.assertEqual(old_page["rss_gate"], "pass")
        self.assertEqual(old_page["gate_reason"], "retained_improved")

    def test_old_page_policy_fails_without_peak_or_retained_threshold(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        self.add_perf_frontier(
            root,
            old_page_policy_data=old_page_policy_packet(
                base_peak_kb=120_000,
                head_peak_kb=112_000,
                base_retained_kb=120_000,
                head_retained_kb=111_000,
            ),
        )

        packet = REPORT.collect_report(root, "base", "head", gate=True)

        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any("neither peak RSS nor retained RSS" in error for error in packet["errors"])
        )

    def test_old_page_policy_small_baseline_uses_non_regression_with_structural_proof(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        self.add_perf_frontier(
            root,
            old_page_policy_data=old_page_policy_packet(
                base_peak_kb=60_000,
                head_peak_kb=59_000,
                base_retained_kb=58_000,
                head_retained_kb=57_000,
            ),
        )

        packet = REPORT.collect_report(root, "base", "head", gate=True)

        self.assertEqual(packet["status"], "pass")
        old_page = packet["old_page_policy"]["bench_json_roundtrip"]
        self.assertTrue(old_page["small_baseline"])
        self.assertEqual(old_page["rss_gate"], "pass")
        self.assertEqual(old_page["gate_reason"], "small_baseline_non_regression")

    def test_old_page_policy_accepts_reclaimable_returned_pages(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        packet_data = old_page_policy_packet(
            base_peak_kb=120_000,
            head_peak_kb=119_000,
            base_retained_kb=120_000,
            head_retained_kb=90_000,
        )
        old_page = packet_data["bench_json_roundtrip_retained"]["head"]["old_page"]
        old_page.update({
            "reclaimable_bytes": 256 * 1024,
            "old_page_moved_bytes": 0,
            "released_original_bytes": 0,
            "released_original_reusable_bytes": 0,
            "reusable_bytes": 0,
            "returned_bytes": 256 * 1024,
        })
        self.add_perf_frontier(root, old_page_policy_data=packet_data)

        packet = REPORT.collect_report(root, "base", "head", gate=True)

        self.assertEqual(packet["status"], "pass")
        structural = packet["old_page_policy"]["structural_old_page"]
        self.assertEqual(structural["status"], "pass")
        self.assertTrue(
            structural["requirements"]["moved_or_reclaimable_returned_pages"]
        )

    def test_old_page_policy_missing_evidence_fails_gate(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        self.add_perf_frontier(root, old_page_policy=False)

        packet = REPORT.collect_report(root, "base", "head", gate=True)

        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any("old-page policy evidence is missing" in error for error in packet["errors"])
        )

    def test_old_page_policy_fails_old_gen_churn_plateau(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        self.add_perf_frontier(
            root,
            old_page_policy_data=old_page_policy_packet(
                churn_samples=[100_000, 101_000, 102_000, 190_000, 250_000],
            ),
        )

        packet = REPORT.collect_report(root, "base", "head", gate=True)

        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any("old_gen_churn_retained RSS did not plateau" in error for error in packet["errors"])
        )

    def test_gate_requires_exact_sha_and_perf_frontier(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
        metadata["head_sha"] = "b" * 39
        write_json(root / "metadata.json", metadata)
        packet = REPORT.collect_report(root, "base", "head", gate=True)
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("exact 40-char SHA" in error for error in packet["errors"]))
        self.assertTrue(any("perf frontier packet is missing" in error for error in packet["errors"]))
        self.assertTrue(any("GC store-site inventory is missing" in error for error in packet["errors"]))

    def test_gate_fails_unaudited_store_sites(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        self.add_perf_frontier(root)
        metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
        metadata["commands"]["packet"]["gc_store_inventory"] = {
            "status": "fail",
            "exit_code": 1,
        }
        write_json(root / "metadata.json", metadata)
        write_json(
            root / "gc-store-site-inventory.json",
            gc_store_inventory_packet(unaudited_sites=2),
        )
        packet = REPORT.collect_report(root, "base", "head", gate=True)
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("gc_store_inventory command status is fail" in error for error in packet["errors"]))
        self.assertTrue(any("unaudited_sites=2" in error for error in packet["errors"]))

    def test_fails_remembered_set_stale_entries(self):
        packet = self.collect(
            head_copied=copied_report(
                json_roundtrip=copied_workload(remembered_set_stale_entries=1)
            )
        )
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any("remembered_set.stale_entries=1" in error for error in packet["errors"])
        )

    def test_reports_external_memory_and_allows_young_owner_checks(self):
        packet = self.collect(
            head_copied=copied_report(
                buffer_churn=copied_workload(
                    external_live_bytes_last=1024,
                    external_live_bytes_max=2048,
                    external_registered_bytes=4096,
                    external_finalized_bytes=3072,
                    external_owner_moves=2,
                    external_young_owner_count_max=2,
                    external_young_owner_checks=3,
                )
            )
        )

        self.assertEqual(packet["status"], "pass")
        summary = packet["copied_minor"]["head"]["summary"]
        self.assertEqual(summary["external_live_bytes_last"], 1024)
        self.assertEqual(summary["external_registered_bytes"], 4096)
        self.assertEqual(summary["external_finalized_bytes"], 3072)
        self.assertEqual(summary["external_owner_moves"], 2)
        self.assertEqual(summary["external_copied_minor_young_owner_checks"], 3)
        self.assertEqual(
            packet["strict_head_workloads"]["buffer_churn"][
                "external_copied_minor_young_owner_checks"
            ],
            3,
        )

    def test_fails_external_checks_without_young_owners(self):
        packet = self.collect(
            head_copied=copied_report(
                buffer_churn=copied_workload(
                    external_young_owner_count_max=0,
                    external_young_owner_checks=1,
                )
            )
        )

        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any(
                "copied-minor external young-owner checks=1 with no young external owners"
                in error
                for error in packet["errors"]
            )
        )

    def test_fails_non_minor_and_full_old_gen_work(self):
        packet = self.collect(
            head_copied=copied_report(
                json_roundtrip=copied_workload(
                    non_minor_cycles=1,
                    phase_sweep_us=5,
                    phase_block_persistence_us=7,
                    phase_root_marking_us=11,
                    phase_trace_worklist_us=13,
                    phase_reference_rewrite_us=17,
                    block_persist_iterations=1,
                    root_growth_mutable_first=1,
                    root_growth_mutable_max=10000,
                )
            )
        )

        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("non-minor gc cycles=1" in error for error in packet["errors"]))
        self.assertTrue(any("phase_us.sweep=5" in error for error in packet["errors"]))
        self.assertTrue(any("broad old-gen walk phase_us=41" in error for error in packet["errors"]))
        self.assertTrue(any("block_persistence work=" in error for error in packet["errors"]))
        self.assertTrue(any("root_growth.mutable_slots_scanned.max=10000" in error for error in packet["errors"]))


if __name__ == "__main__":
    unittest.main()
