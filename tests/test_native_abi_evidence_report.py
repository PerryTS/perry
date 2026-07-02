import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "native_abi_evidence_report.py"

SPEC = importlib.util.spec_from_file_location("native_abi_evidence_report", SCRIPT_PATH)
assert SPEC is not None
REPORT = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(REPORT)


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_text(path, text="x\n"):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def command(status="pass", log=None):
    entry = {"status": status, "exit_code": 0 if status == "pass" else 1}
    if log is not None:
        entry["log"] = str(log)
    return entry


def correctness_tokens(tokens):
    return "\n".join(tokens) + "\n"


def native_record(rep, consumer, *, access_mode="unchecked_native", bounds_state=None, **overrides):
    row = {
        "native_rep_name": rep,
        "consumer": consumer,
        "access_mode": access_mode,
        "bounds_state": bounds_state or {"proven": {"proof": "loop_guard"}},
    }
    row.update(overrides)
    return row


def create_correctness(root):
    contract = root / "correctness" / "native-abi-contract"
    pod = root / "correctness" / "c-layout-pod-records"
    write_text(contract / "compile.log")
    write_text(contract / "runtime.stdout", "PASS\n")
    write_text(contract / "native-reps" / "native-reps-0.json", "{}\n")
    write_text(contract / "native-reps.txt", correctness_tokens(REPORT.REQUIRED_CORRECTNESS["native_abi_contract"]["tokens"]))
    write_text(pod / "compile.log")
    write_text(pod / "runtime.stdout", "read=7,1.5,2.25,4\n")
    write_text(pod / "native-reps" / "native-reps-0.json", "{}\n")
    write_text(pod / "native-reps.txt", correctness_tokens(REPORT.REQUIRED_CORRECTNESS["c_layout_pod_records"]["tokens"]))


def create_workload(
    suite_root,
    name,
    runtime_summary,
    median,
    p95=None,
    stat_quality="timing",
    native_records=None,
):
    root = suite_root / name
    artifacts = {
        "hir": root / "hir.txt",
        "llvm_before_opt": root / "llvm-before-opt.ll",
        "llvm_after_opt_analysis": root / "llvm-after-opt.analysis.ll",
        "object_disassembly": root / "object-disassembly.s",
        "object": root / "object-0.o",
        "plan": root / "object-0.compile-plan.json",
        "native_reps": root / "native-reps-0.json",
    }
    for path in artifacts.values():
        write_text(path)
    write_json(artifacts["native_reps"], {"records": native_records or []})
    manifest = {
        "workload": name,
        "workload_kind": name,
        "source": f"benchmarks/compiler_output/fixtures/{name}.ts",
        "artifacts": {
            "hir": str(artifacts["hir"]),
            "llvm_before_opt": str(artifacts["llvm_before_opt"]),
            "llvm_after_opt_analysis": {"path": str(artifacts["llvm_after_opt_analysis"])},
            "object_disassembly": {"path": str(artifacts["object_disassembly"])},
            "retained_objects": [
                {
                    "object_artifact": str(artifacts["object"]),
                    "compile_plan_artifact": str(artifacts["plan"]),
                }
            ],
            "native_reps": [
                {"native_reps_artifact": str(artifacts["native_reps"])}
            ],
        },
        "runtime_counter_summary": runtime_summary,
        "benchmark": {
            "median_wall_ms": median,
            "mean_wall_ms": median,
            "p95_wall_ms": median if p95 is None else p95,
            "stat_quality": stat_quality,
            "runs": [{"exit_code": 0}],
        },
    }
    checks = [
        {"name": name, "status": "pass", "detail": ""}
        for name in REPORT.SAFETY_CHECK_NAMES
    ]
    stdout_check = REPORT.REQUIRED_PACKET_STDOUT_CHECKS.get(name)
    if stdout_check:
        checks.append({"name": stdout_check, "status": "pass", "detail": ""})
    write_json(root / "manifest.json", manifest)
    write_json(root / "structural-report.json", {"status": "pass", "checks": checks, "errors": []})
    return {
        "workload": name,
        "status": "pass",
        "exit_code": 0,
        "artifact_dir": str(root),
        "structural_report": str(root / "structural-report.json"),
        "errors": [],
    }


def create_compiler_output(root):
    suite_root = root / "compiler-output" / "native-abi-proof"
    typed = create_workload(
        suite_root,
        "native_abi_packet_typed",
        {
            "boxed_number_allocations_static": 0,
            "buffer_slow_path_accesses_static": 0,
            "array_slow_path_accesses_static": 0,
            "allocations_traced": 5,
            "write_barriers_static": 0,
            "write_barriers_traced": 8,
            "runtime_calls_static": 2,
        },
        10.0,
        p95=20.0,
        native_records=[
            native_record("buffer_view", "BufferView"),
            native_record("u8", "u8_load_zext_i32"),
        ],
    )
    control = create_workload(
        suite_root,
        "native_abi_packet_control",
        {
            "boxed_number_allocations_static": 64,
            "buffer_slow_path_accesses_static": 128,
            "array_slow_path_accesses_static": 256,
            "allocations_traced": 640,
            "write_barriers_static": 6,
            "write_barriers_traced": 360,
            "runtime_calls_static": 12,
        },
        25.0,
        p95=32.0,
        native_records=[
            native_record(
                "js_value",
                "js_buffer_get",
                access_mode="dynamic_fallback",
                bounds_state="unknown",
                native_value_state="dynamic_fallback",
                materialization_reason="runtime_api",
                fallback_reason="runtime_api",
            ),
        ],
    )
    write_json(
        suite_root / "suite-report.json",
        {
            "schema_version": 1,
            "suite": "native-abi-proof",
            "status": "pass",
            "workloads": [typed, control],
            "failed_workloads": [],
        },
    )


def create_metadata(root):
    runtime_log = root / "logs" / "native-async.log"
    symbol_log = root / "logs" / "runtime-symbols.log"
    write_text(runtime_log, "\n".join(REPORT.REQUIRED_RUNTIME_TESTS) + "\n")
    write_text(
        symbol_log,
        f"ok: target/debug/libperry_runtime.a defines all {REPORT.REQUIRED_RELEASE_SENTINEL_COUNT} sentinel symbols\n",
    )
    write_json(
        root / "metadata.json",
        {
            "schema_version": 1,
            "runtime_archive": "target/debug/libperry_runtime.a",
            "runtime_archive_sha256": "a" * 64,
            "runtime_source_digest": "b" * 64,
            "commands": {
                "correctness": {
                    "native_abi_contract": command(),
                    "c_layout_pod_records": command(),
                },
                "packet": {
                    "compiler_output": command(),
                },
                "release": {
                    "runtime_symbols": command(log=symbol_log),
                },
                "runtime": {
                    "native_async": command(log=runtime_log),
                },
            },
            "tool_versions": {},
        },
    )


class NativeAbiEvidenceReportTests(unittest.TestCase):
    def make_packet(self):
        temp = tempfile.TemporaryDirectory()
        root = Path(temp.name) / "packet"
        repo_root = Path(temp.name) / "repo"
        root.mkdir(parents=True)
        create_correctness(root)
        create_compiler_output(root)
        create_metadata(root)
        return temp, root, repo_root

    def test_synthetic_packet_passes_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "pass", packet["errors"])
            self.assertEqual(packet["benchmark_deltas"]["status"], "pass")
            typed_contract = packet["native_call_lowering"]["workloads"][
                "native_abi_packet_typed"
            ]["packet_contract"]
            control_contract = packet["native_call_lowering"]["workloads"][
                "native_abi_packet_control"
            ]["packet_contract"]
            self.assertEqual(typed_contract["status"], "pass", typed_contract)
            self.assertEqual(control_contract["status"], "pass", control_contract)
            self.assertIn("region-local native type lowering", packet["scope"]["summary"])
            self.assertIn(
                "does not claim a general typed function/method/closure ABI",
                packet["scope"]["not_covered"],
            )

            markdown = REPORT.markdown_for_packet(packet, repo_root)
            self.assertIn("# Selected Native / Region-Local Evidence Packet: PASS", markdown)
            self.assertIn("## Scope", markdown)
            self.assertIn("## Gate Matrix", markdown)
            self.assertIn("typed clones, or generic trampoline dispatch", markdown)
            self.assertIn("packet_contract=`pass`", markdown)
            self.assertIn("## Selected Native / Region-Local Lowering", markdown)
            self.assertIn("explain_records=", markdown)
            self.assertIn("stdout_missing=0", markdown)
            self.assertIn("## Release / LTO Symbol Guard", markdown)
            self.assertIn("Runtime symbol guard: `pass`", markdown)
            self.assertIn("source_digest=", markdown)
            self.assertIn("## Packet Deltas", markdown)
            self.assertIn("Contract: reductions=", markdown)
            self.assertIn("## Material Accounting", markdown)
            self.assertTrue(
                all(row["status"] == "pass" for row in packet["gate_matrix"]),
                packet["gate_matrix"],
            )
            self.assertEqual(
                packet["native_call_lowering"]["workloads"]["native_abi_packet_control"][
                    "explain_lowering_accounting"
                ]["dynamic_fallbacks"],
                1,
            )

    def test_missing_artifact_fails_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            missing = root / "compiler-output" / "native-abi-proof" / "native_abi_packet_typed" / "llvm-before-opt.ll"
            missing.unlink()
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            self.assertTrue(any("native_abi_packet_typed" in error for error in packet["errors"]))

    def test_typed_packet_requires_native_rep_evidence(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            reps_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_typed"
                / "native-reps-0.json"
            )
            write_json(reps_path, {"records": []})
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            typed_contract = packet["native_call_lowering"]["workloads"][
                "native_abi_packet_typed"
            ]["packet_contract"]
            self.assertEqual(typed_contract["status"], "fail")
            self.assertTrue(
                any("typed_unchecked_buffer_view" in error for error in packet["errors"]),
                packet["errors"],
            )

    def test_control_packet_requires_positive_static_baseline(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            manifest_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_control"
                / "manifest.json"
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["runtime_counter_summary"]["boxed_number_allocations_static"] = 0
            write_json(manifest_path, manifest)
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            control_contract = packet["native_call_lowering"]["workloads"][
                "native_abi_packet_control"
            ]["packet_contract"]
            self.assertEqual(control_contract["status"], "fail")
            self.assertTrue(
                any(
                    "boxed_number_allocations_static must be positive" in error
                    for error in packet["errors"]
                ),
                packet["errors"],
            )

    def test_packet_contract_rejects_swapped_workload_manifest(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            manifest_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_typed"
                / "manifest.json"
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["source"] = "benchmarks/compiler_output/fixtures/native_abi_packet_control.ts"
            manifest["workload_kind"] = "native_abi_packet_control"
            write_json(manifest_path, manifest)
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            typed_contract = packet["native_call_lowering"]["workloads"][
                "native_abi_packet_typed"
            ]["packet_contract"]
            self.assertEqual(typed_contract["status"], "fail")
            self.assertTrue(
                any("manifest source must be" in error for error in packet["errors"]),
                packet["errors"],
            )

    def test_missing_packet_stdout_check_fails_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            report_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_typed"
                / "structural-report.json"
            )
            structural = json.loads(report_path.read_text(encoding="utf-8"))
            structural["checks"] = [
                check
                for check in structural["checks"]
                if check["name"] != "native_abi_packet_typed_checksum"
            ]
            write_json(report_path, structural)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            workload = packet["native_call_lowering"]["workloads"]["native_abi_packet_typed"]
            self.assertEqual(
                workload["missing_stdout_checks"],
                ["native_abi_packet_typed_checksum"],
            )
            self.assertTrue(
                any("native_abi_packet_typed_checksum" in error for error in packet["errors"])
            )

    def test_command_status_failure_fails_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
            metadata["commands"]["correctness"]["native_abi_contract"] = command("fail")
            write_json(root / "metadata.json", metadata)
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            self.assertTrue(any("correctness:native_abi_contract" in error for error in packet["errors"]))

    def test_missing_runtime_symbol_proof_fails_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
            log_path = Path(metadata["commands"]["release"]["runtime_symbols"]["log"])
            write_text(log_path, "::warning::check_runtime_symbols: no llvm-nm/nm available\n")

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            self.assertEqual(packet["release_symbol_guard"]["status"], "fail")
            self.assertTrue(
                any("release:runtime_symbols" in error for error in packet["errors"])
            )

    def test_stale_runtime_symbol_count_fails_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
            log_path = Path(metadata["commands"]["release"]["runtime_symbols"]["log"])
            write_text(
                log_path,
                f"ok: target/debug/libperry_runtime.a defines all {REPORT.REQUIRED_RELEASE_SENTINEL_COUNT - 1} sentinel symbols\n",
            )

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            self.assertEqual(packet["release_symbol_guard"]["status"], "fail")
            self.assertTrue(
                any("sentinel count is below" in error for error in packet["errors"]),
                packet["errors"],
            )

    def test_missing_runtime_fingerprints_fail_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
            metadata.pop("runtime_archive_sha256")
            metadata.pop("runtime_source_digest")
            write_json(root / "metadata.json", metadata)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            self.assertEqual(packet["release_symbol_guard"]["status"], "fail")
            self.assertEqual(
                packet["release_symbol_guard"]["missing_fingerprints"],
                ["runtime_archive_sha256", "runtime_source_digest"],
            )

    def test_benchmark_delta_calculation(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            fields = packet["benchmark_deltas"]["fields"]
            self.assertEqual(fields["buffer_slow_path_accesses_static"]["delta"], -128.0)
            self.assertEqual(fields["buffer_slow_path_accesses_static"]["reduction_pct"], 100.0)
            self.assertEqual(fields["array_slow_path_accesses_static"]["delta"], -256.0)
            self.assertEqual(fields["array_slow_path_accesses_static"]["reduction_pct"], 100.0)
            self.assertEqual(fields["median_wall_ms"]["delta_pct"], -60.0)
            self.assertEqual(fields["median_wall_ms"]["speedup"], 2.5)
            self.assertEqual(fields["p95_wall_ms"]["speedup"], 1.6)
            self.assertIn("write_barriers_traced", fields)
            self.assertIn("runtime_calls_static", fields)
            accounting = {
                row["field"]: row
                for row in packet["benchmark_deltas"]["material_accounting"]
            }
            self.assertEqual(accounting["runtime_calls_static"]["status"], "pass")
            self.assertEqual(accounting["write_barriers_static"]["status"], "pass")
            self.assertEqual(
                packet["benchmark_deltas"]["benchmark_stat_quality"],
                {"typed": "timing", "control": "timing"},
            )

    def test_zero_baseline_material_delta_fails_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            for workload in ("native_abi_packet_typed", "native_abi_packet_control"):
                manifest_path = (
                    root
                    / "compiler-output"
                    / "native-abi-proof"
                    / workload
                    / "manifest.json"
                )
                manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
                manifest["runtime_counter_summary"]["allocations_traced"] = 0
                write_json(manifest_path, manifest)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            self.assertTrue(
                any("material performance gate" in error for error in packet["errors"])
            )

    def test_required_allocation_deltas_must_improve(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            manifest_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_control"
                / "manifest.json"
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            for field in REPORT.REQUIRED_IMPROVEMENT_FIELDS:
                manifest["runtime_counter_summary"][field] = 0
            write_json(manifest_path, manifest)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            self.assertEqual(packet["benchmark_deltas"]["status"], "fail")
            self.assertTrue(
                any("benchmark deltas missing required improvements" in error for error in packet["errors"])
            )

    def test_material_reduction_thresholds_must_pass(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            manifest_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_typed"
                / "manifest.json"
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["runtime_counter_summary"]["allocations_traced"] = 40
            write_json(manifest_path, manifest)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            failures = packet["benchmark_deltas"]["material_failures"]
            self.assertTrue(any("allocations_traced" in failure for failure in failures))

    def test_material_elimination_thresholds_must_pass(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            manifest_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_typed"
                / "manifest.json"
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["runtime_counter_summary"]["boxed_number_allocations_static"] = 1
            write_json(manifest_path, manifest)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            failures = packet["benchmark_deltas"]["material_failures"]
            self.assertTrue(any("boxed_number_allocations_static" in failure for failure in failures))

    def test_material_speedup_thresholds_must_pass(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            manifest_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_typed"
                / "manifest.json"
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["benchmark"]["median_wall_ms"] = 20.0
            write_json(manifest_path, manifest)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            failures = packet["benchmark_deltas"]["material_failures"]
            self.assertTrue(any("median_wall_ms" in failure for failure in failures))

    def test_material_runtime_helper_thresholds_must_pass(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            manifest_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_typed"
                / "manifest.json"
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["runtime_counter_summary"]["runtime_calls_static"] = 11
            write_json(manifest_path, manifest)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            failures = packet["benchmark_deltas"]["material_failures"]
            self.assertTrue(any("runtime_calls_static" in failure for failure in failures))

    def test_material_static_barrier_thresholds_must_pass(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            manifest_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_typed"
                / "manifest.json"
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["runtime_counter_summary"]["write_barriers_static"] = 5
            write_json(manifest_path, manifest)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            failures = packet["benchmark_deltas"]["material_failures"]
            self.assertTrue(any("write_barriers_static" in failure for failure in failures))

    def test_material_speedup_requires_timing_quality(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            manifest_path = (
                root
                / "compiler-output"
                / "native-abi-proof"
                / "native_abi_packet_typed"
                / "manifest.json"
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["benchmark"]["stat_quality"] = "smoke"
            write_json(manifest_path, manifest)

            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            failures = packet["benchmark_deltas"]["material_failures"]
            self.assertTrue(any("stat_quality" in failure for failure in failures))


if __name__ == "__main__":
    unittest.main()
