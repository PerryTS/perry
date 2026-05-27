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


def create_workload(suite_root, name, runtime_summary, median):
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
    manifest = {
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
            "runs": [{"exit_code": 0}],
        },
    }
    checks = [
        {"name": name, "status": "pass", "detail": ""}
        for name in REPORT.SAFETY_CHECK_NAMES
    ]
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
            "allocations_traced": 1,
            "write_barriers_static": 0,
            "runtime_calls_static": 2,
        },
        10.0,
    )
    control = create_workload(
        suite_root,
        "native_abi_packet_control",
        {
            "boxed_number_allocations_static": 4,
            "buffer_slow_path_accesses_static": 8,
            "allocations_traced": 9,
            "write_barriers_static": 6,
            "runtime_calls_static": 12,
        },
        25.0,
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
    write_text(runtime_log, "\n".join(REPORT.REQUIRED_RUNTIME_TESTS) + "\n")
    write_json(
        root / "metadata.json",
        {
            "schema_version": 1,
            "commands": {
                "correctness": {
                    "native_abi_contract": command(),
                    "c_layout_pod_records": command(),
                },
                "packet": {
                    "compiler_output": command(),
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

    def test_missing_artifact_fails_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            missing = root / "compiler-output" / "native-abi-proof" / "native_abi_packet_typed" / "llvm-before-opt.ll"
            missing.unlink()
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            self.assertTrue(any("native_abi_packet_typed" in error for error in packet["errors"]))

    def test_command_status_failure_fails_gate(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
            metadata["commands"]["correctness"]["native_abi_contract"] = command("fail")
            write_json(root / "metadata.json", metadata)
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            self.assertEqual(packet["status"], "fail")
            self.assertTrue(any("correctness:native_abi_contract" in error for error in packet["errors"]))

    def test_benchmark_delta_calculation(self):
        temp, root, repo_root = self.make_packet()
        with temp:
            packet = REPORT.build_packet(root, root / "metadata.json", repo_root, gate=True)
            fields = packet["benchmark_deltas"]["fields"]
            self.assertEqual(fields["buffer_slow_path_accesses_static"]["delta"], -8.0)
            self.assertEqual(fields["median_wall_ms"]["delta_pct"], -60.0)

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


if __name__ == "__main__":
    unittest.main()
