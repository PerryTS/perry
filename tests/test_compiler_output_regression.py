import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "compiler_output_regression.py"

SPEC = importlib.util.spec_from_file_location("compiler_output_regression", SCRIPT_PATH)
assert SPEC is not None
HARNESS = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = HARNESS
SPEC.loader.exec_module(HARNESS)


GOOD_IR = """
define i32 @main() {
entry:
  call void @llvm.assume(i1 true)
  br label %for.body.20
for.body.20:
  %row = mul i32 %y, 255
  br label %for.body.24
for.body.24:
  %p0 = getelementptr inbounds i8, ptr %base, i64 %i
  store i8 1, ptr %p0, align 1, !alias.scope !2, !noalias !3
  %p1 = getelementptr inbounds i8, ptr %base, i64 %i1
  store i8 2, ptr %p1, align 1, !alias.scope !2, !noalias !3
  %p2 = getelementptr inbounds i8, ptr %base, i64 %i2
  store i8 3, ptr %p2, align 1, !alias.scope !2, !noalias !3
  br label %while.body.28
while.body.28:
  %noise0 = load i8, ptr %p0, align 1, !invariant.load !1, !alias.scope !2, !noalias !3
  %noise1 = load i8, ptr %p1, align 1, !invariant.load !1, !alias.scope !2, !noalias !3
  %noise2 = load i8, ptr %p2, align 1, !invariant.load !1, !alias.scope !2, !noalias !3
  %n0 = zext i8 %noise0 to i32
  %n1 = xor i32 %n0, %seed
  %n2 = xor i32 %n1, %seed2
  %n3 = xor i32 %n2, %seed3
  %nb = trunc i32 %n3 to i8
  store i8 %nb, ptr %p0, align 1, !alias.scope !2, !noalias !3
  br label %for.body.38
for.body.38:
  %b0 = load i8, ptr %p0, align 1, !invariant.load !1, !alias.scope !2, !noalias !3
  %b1 = load i8, ptr %p1, align 1, !invariant.load !1, !alias.scope !2, !noalias !3
  %b2 = load i8, ptr %p2, align 1, !invariant.load !1, !alias.scope !2, !noalias !3
  store i8 %b0, ptr %p2, align 1, !alias.scope !2, !noalias !3
  br label %for.body.42
for.body.42:
  %hbyte = load i8, ptr %p2, align 1, !invariant.load !1, !alias.scope !2, !noalias !3
  %x = zext i8 %hbyte to i32
  %h = xor i32 %prev, %x
  %m = mul i32 %h, 16777619
  br label %for.body.42
}
!1 = !{}
!2 = !{}
!3 = !{}
"""

GOOD_ASM = """
main:
  imull $16777619, %eax, %eax
  retq
"""


class CompilerOutputRegressionTests(unittest.TestCase):
    def test_image_convolution_good_shape_passes(self):
        report = HARNESS.verify_artifacts(
            workload="image_convolution",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0}]},
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
        )
        self.assertEqual(report["status"], "pass", report["errors"])

    def test_hot_loop_runtime_call_fails_gate(self):
        bad_ir = GOOD_IR.replace(
            "  %p0 = getelementptr inbounds i8, ptr %base, i64 %i\n",
            "  call void @js_shadow_slot_set(i32 0, i64 0)\n"
            "  %p0 = getelementptr inbounds i8, ptr %base, i64 %i\n",
        )
        report = HARNESS.verify_artifacts(
            workload="image_convolution",
            ir_before=bad_ir,
            ir_after=bad_ir,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0}]},
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("hot_loops_no_runtime_calls" in error for error in report["errors"])
        )

    def test_image_convolution_requires_named_regions(self):
        bad_ir = GOOD_IR.replace("for.body.42:", "for.body.77:").replace(
            "  %m = mul i32 %h, 16777619\n", ""
        )
        report = HARNESS.verify_artifacts(
            workload="image_convolution",
            ir_before=bad_ir,
            ir_after=bad_ir,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0}]},
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("named_region_fnv_hash_present" in error for error in report["errors"])
        )

    def test_manifest_region_counters_include_named_regions(self):
        regions = HARNESS.region_counters("image_convolution", GOOD_IR)
        self.assertIn("hot_loops", regions)
        self.assertIn("named", regions)
        self.assertIn("input_generation", regions["named"])
        self.assertIn("blur", regions["named"])
        self.assertIn("fnv_hash", regions["named"])

    def test_numeric_loop_does_not_require_typed_buffer_metadata(self):
        numeric_ir = """
define i32 @main() {
entry:
  br label %for.body.11
for.body.11:
  %x = fmul double %a, %b
  %y = fadd double %x, %c
  br label %for.body.11
}
"""
        report = HARNESS.verify_artifacts(
            workload="loop_data_dependent",
            ir_before=numeric_ir,
            ir_after=numeric_ir,
            assembly="main:\n  retq\n",
            benchmark=None,
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
        )
        self.assertEqual(report["status"], "pass", report["errors"])

    def test_verify_existing_artifacts_writes_report(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "llvm-before-opt.ll").write_text(GOOD_IR, encoding="utf-8")
            (root / "llvm-after-opt.ll").write_text(GOOD_IR, encoding="utf-8")
            (root / "assembly.s").write_text(GOOD_ASM, encoding="utf-8")
            args = type(
                "Args",
                (),
                {
                    "artifact_dir": str(root),
                    "workload": "image_convolution",
                    "gate": True,
                    "print_summary": False,
                    "target": None,
                    "clang_arg": None,
                    "fp_contract": None,
                    "expect_fma": "auto",
                },
            )()
            self.assertEqual(HARNESS.verify_existing(args), 0)
            self.assertTrue((root / "structural-report.json").exists())

    def test_verify_existing_uses_analysis_ir_object_disassembly_and_manifest_plan(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "llvm-before-opt.ll").write_text(GOOD_IR, encoding="utf-8")
            (root / "llvm-after-opt.analysis.ll").write_text(GOOD_IR, encoding="utf-8")
            (root / "object-disassembly.s").write_text(GOOD_ASM, encoding="utf-8")
            (root / "manifest.json").write_text(
                """
{
  "compile_plan": {
    "effective_target": "x86_64-unknown-linux-gnu",
    "clang_args": ["-c", "-O3", "-fno-math-errno", "-march=native"]
  }
}
""",
                encoding="utf-8",
            )
            args = type(
                "Args",
                (),
                {
                    "artifact_dir": str(root),
                    "workload": "image_convolution",
                    "gate": True,
                    "print_summary": False,
                    "target": None,
                    "clang_arg": None,
                    "fp_contract": None,
                    "expect_fma": "auto",
                },
            )()
            self.assertEqual(HARNESS.verify_existing(args), 0)
            report = (root / "structural-report.json").read_text(encoding="utf-8")
            self.assertIn("object_disassembly_present", report)

    def test_explicit_perry_path_is_repo_relative(self):
        resolved = HARNESS.resolve_perry("target/debug/perry")
        self.assertEqual(resolved, [str(REPO_ROOT / "target/debug/perry")])

    def test_workload_spec_loads_current_workloads(self):
        spec = HARNESS.load_workload_spec(HARNESS.DEFAULT_SPEC_PATH)
        self.assertIn("image_convolution", spec["workloads"])
        self.assertIn("fma_contract", spec["workloads"])
        self.assertTrue(spec["workloads"]["fma_contract"]["fma_gate"]["enabled"])
        for name, workload in spec["workloads"].items():
            self.assertIn("source", workload, name)
            self.assertIn("vectorization", workload, name)
            self.assertIn("runtime_budgets", workload, name)

    def test_workload_spec_rejects_missing_required_fields(self):
        with self.assertRaises(HARNESS.HarnessError):
            HARNESS.validate_workload_spec(
                {
                    "schema_version": 1,
                    "workloads": {
                        "bad": {
                            "kind": "numeric_loop",
                            "vectorization": {
                                "min_vectorized_loops": 0,
                                "allowed_missed_reason_kinds": [],
                            },
                            "runtime_budgets": {},
                        }
                    },
                }
            )

    def test_parse_kept_paths_includes_compile_metadata(self):
        irs, objects, metadata = HARNESS.parse_kept_paths(
            "[perry-codegen] kept LLVM IR: /tmp/a.ll\n"
            "[perry-codegen] kept object:  /tmp/a.o\n"
            "[perry-codegen] kept compile metadata: /tmp/a.o.compile-plan.json\n"
        )
        self.assertEqual(irs, [Path("/tmp/a.ll")])
        self.assertEqual(objects, [Path("/tmp/a.o")])
        self.assertEqual(metadata, [Path("/tmp/a.o.compile-plan.json")])

    def test_runtime_counter_summary_combines_static_and_trace_counts(self):
        counters = HARNESS.structural_counters(
            GOOD_IR,
            GOOD_IR + "\n  call double @js_boxed_number_new(double 1.0)\n",
            GOOD_ASM,
        )
        summary = HARNESS.runtime_counter_summary(
            {
                "runs": [
                    {
                        "gc_trace_summary": {
                            "gc_events": 2,
                            "write_barrier_calls": 3,
                            "malloc_kind_allocations": 4,
                        }
                    }
                ]
            },
            counters,
        )
        self.assertEqual(summary["gc_collections_traced"], 2)
        self.assertEqual(summary["allocations_traced"], 4)
        self.assertEqual(summary["write_barriers_traced"], 3)
        self.assertEqual(summary["boxed_number_allocations_static"], 1)

    def test_vectorization_unexpected_reason_fails_gate(self):
        report = HARNESS.verify_artifacts(
            workload="image_convolution",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={
                "vectorized_count": 0,
                "missed_count": 1,
                "analysis_count": 0,
                "missed_reason_kinds": {"aliasing": 1},
            },
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("vectorization_expectation" in error for error in report["errors"])
        )

    def test_vectorization_required_loop_count_fails_gate(self):
        report = HARNESS.verify_artifacts(
            workload="vectorized_buffer_transform",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
                "missed_reason_kinds": {},
            },
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("vectorization_expectation" in error for error in report["errors"])
        )

    def test_hir_fact_rewrite_requires_direct_buffer_shape(self):
        report = HARNESS.verify_artifacts(
            workload="hir_fact_rewrite",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={
                "vectorized_count": 1,
                "missed_count": 0,
                "analysis_count": 0,
                "missed_reason_kinds": {},
            },
            counters=HARNESS.structural_counters(GOOD_IR, GOOD_IR, GOOD_ASM),
        )
        self.assertEqual(report["status"], "pass", report["errors"])

        slow_ir = GOOD_IR + "\n  call i32 @js_buffer_get(i64 0, i32 0)\n"
        slow_report = HARNESS.verify_artifacts(
            workload="hir_fact_rewrite",
            ir_before=slow_ir,
            ir_after=slow_ir,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={
                "vectorized_count": 1,
                "missed_count": 0,
                "analysis_count": 0,
                "missed_reason_kinds": {},
            },
            counters=HARNESS.structural_counters(slow_ir, slow_ir, GOOD_ASM),
        )
        self.assertEqual(slow_report["status"], "fail")
        self.assertTrue(
            any(
                "hir_fact_rewrite_no_buffer_slow_path" in error
                for error in slow_report["errors"]
            )
        )

    def test_benchmark_summary_reports_p95_and_stddev(self):
        summary = HARNESS.benchmark_summary(
            [
                {"exit_code": 0, "wall_ms": 10.0},
                {"exit_code": 0, "wall_ms": 20.0},
                {"exit_code": 0, "wall_ms": 30.0},
                {"exit_code": 0, "wall_ms": 40.0},
                {"exit_code": 0, "wall_ms": 50.0},
            ],
            "standard",
        )
        self.assertEqual(summary["successful_runs"], 5)
        self.assertEqual(summary["stat_quality"], "timing")
        self.assertIsNotNone(summary["stddev_wall_ms"])
        self.assertAlmostEqual(summary["p95_wall_ms"], 48.0)

    def test_fma_fixture_requires_fma_when_requested(self):
        ir = """
define double @main() {
entry:
  br label %for.body
for.body:
  %x = fmul contract double %a, %b
  %y = fadd contract double %x, %c
  br label %for.body
}
"""
        report = HARNESS.verify_artifacts(
            workload="fma_contract",
            ir_before=ir,
            ir_after=ir,
            assembly="main:\n  retq\n",
            benchmark=None,
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
            fp_contract_mode="on",
            target="x86_64-unknown-linux-gnu",
            clang_args=["-march=haswell"],
            expect_fma="on",
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("fma_instruction_when_contraction_expected" in error for error in report["errors"])
        )

    def test_fma_fixture_forbids_fma_when_contract_off(self):
        ir = """
define double @main() {
entry:
  br label %for.body
for.body:
  %x = fmul reassoc double %a, %b
  %y = fadd reassoc double %x, %c
  br label %for.body
}
"""
        report = HARNESS.verify_artifacts(
            workload="fma_contract",
            ir_before=ir,
            ir_after=ir,
            assembly="main:\n  vfmadd213sd %xmm0, %xmm1, %xmm2\n  retq\n",
            benchmark=None,
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
            fp_contract_mode="off",
            target="x86_64-unknown-linux-gnu",
            clang_args=["-march=haswell"],
            expect_fma="off",
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("no_fma_instruction_when_fp_contract_off" in error for error in report["errors"])
        )

    def test_fma_fixture_accepts_vectorized_numeric_region(self):
        ir = """
define double @main() {
entry:
  br label %vector.body
vector.body:
  %x = fmul reassoc <4 x double> %a, %b
  %y = fadd reassoc <4 x double> %x, %c
  br label %middle.block
middle.block:
  %z = fadd reassoc <4 x double> %y, %x
  ret double 0.0
}
"""
        report = HARNESS.verify_artifacts(
            workload="fma_contract",
            ir_before=ir,
            ir_after=ir,
            assembly="main:\n  retq\n",
            benchmark=None,
            vectorization={
                "vectorized_count": 1,
                "missed_count": 0,
                "analysis_count": 0,
                "missed_reason_kinds": {},
            },
            fp_contract_mode="off",
            target="x86_64-unknown-linux-gnu",
            clang_args=["-march=haswell"],
            expect_fma="off",
        )
        self.assertEqual(report["status"], "pass", report["errors"])
        self.assertIn("numeric_loop_body", report["named_regions"])


if __name__ == "__main__":
    unittest.main()
