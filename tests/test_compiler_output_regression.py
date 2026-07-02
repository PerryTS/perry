import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


if sys.version_info < (3, 11):
    print("SKIP: Python 3.11+ is required for stdlib TOML parsing")
    raise SystemExit(0)


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "compiler_output_regression.py"

SPEC = importlib.util.spec_from_file_location("compiler_output_regression", SCRIPT_PATH)
assert SPEC is not None
HARNESS = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = HARNESS
SPEC.loader.exec_module(HARNESS)

from compiler_output_harness import capture as CAPTURE_MODULE
from compiler_output_harness.capture import SUITES


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

H1_MIN_IR = """
define i32 @main() {
entry:
  br label %for.body.2
for.body.2:
  %i = load i32, ptr %slot
  store i32 %i, ptr %slot
  %ok = icmp slt i32 %i, %n
  %p0 = getelementptr i8, ptr %src, i32 %i
  %b = load i8, ptr %p0
  store i8 %b, ptr %p0
  br label %for.body.6
for.body.6:
  %p1 = getelementptr i8, ptr %src, i32 %i
  %b1 = load i8, ptr %p1
  store i8 %b1, ptr %p1
  br label %for.body.10
for.body.10:
  %p2 = getelementptr i8, ptr %src, i32 %i
  %b2 = load i8, ptr %p2
  store i8 %b2, ptr %p2
  br label %for.body.2.i
for.body.2.i:
  %p3 = getelementptr i8, ptr %src, i32 %i
  %b3 = load i8, ptr %p3
  store i8 %b3, ptr %p3
  ret i32 0
}
"""


def native_record(function="main", block="for.body.2", rep="i32", **overrides):
    row = {
        "function": function,
        "block_label": block,
        "region_id": None,
        "source_function": "module_init",
        "lowering_block": block,
        "expr_kind": "test",
        "native_rep_name": rep,
        "consumer": "test",
        "bounds_state": None,
        "alias_state": None,
        "access_mode": None,
        "materialization_reason": None,
        "fallback_reason": None,
        "native_value_state": "region_local",
        "emitted_inbounds": False,
        "emitted_noalias": False,
    }
    row.update(overrides)
    if row.get("native_rep_name") != "js_value":
        row.setdefault("consumed_facts", []).append(
            native_fact(
                "representation",
                "consumed",
                str(row.get("native_rep_name") or "unknown"),
            )
        )
    bounds_state = row.get("bounds_state")
    if isinstance(bounds_state, dict) and "guarded" in bounds_state:
        guard = bounds_state["guarded"] or {}
        row.setdefault("consumed_facts", []).append(
            native_fact(
                "bounds",
                "consumed",
                str(guard.get("guard_id") or "guarded"),
            )
        )
    elif isinstance(bounds_state, dict) and "proven" in bounds_state:
        proof = bounds_state["proven"] or {}
        row.setdefault("consumed_facts", []).append(
            native_fact(
                "bounds",
                "consumed",
                str(proof.get("proof") or "proven"),
            )
        )
    if row.get("access_mode") == "dynamic_fallback":
        row["fallback_reason"] = row.get("fallback_reason") or row.get(
            "materialization_reason"
        )
        row["native_value_state"] = "dynamic_fallback"
        if row.get("bounds_state") is None or row.get("bounds_state") == "unknown":
            row.setdefault("rejected_facts", []).append(
                native_fact(
                    "bounds",
                    "missing",
                    "unknown",
                    row.get("materialization_reason"),
                )
            )
        if row.get("alias_state") in {"unknown", "may_alias", None}:
            row.setdefault("rejected_facts", []).append(
                native_fact(
                    "alias_noalias",
                    "missing",
                    "unknown_or_may_alias",
                    row.get("materialization_reason"),
                )
            )
        if row.get("materialization_reason"):
            row.setdefault("rejected_facts", []).append(
                native_fact(
                    "materialization_hazard",
                    "invalidated",
                    str(row.get("materialization_reason")),
                    row.get("materialization_reason"),
                )
            )
    elif row.get("materialization_reason"):
        row["native_value_state"] = "materialized"
    return row


def native_fact(kind, state, detail, reason=None):
    return {
        "fact_id": f"native_region.{kind}.test.{detail}",
        "kind": kind,
        "local_id": None,
        "state": state,
        "reason": reason,
    }


def raw_f64_layout_fact(state):
    return {
        "fact_id": f"native_region.raw_f64_layout.test.{state}",
        "kind": "raw_f64_layout",
        "local_id": None,
        "state": state,
        "reason": "runtime_api" if state != "consumed" else None,
    }


def array_kind_fact(state="consumed", reason=None):
    return {
        "fact_id": f"native_region.array_kind.test.{state}",
        "kind": "array_kind",
        "local_id": None,
        "state": state,
        "reason": reason,
    }


def attach_raw_f64_layout_facts(records):
    for record in records:
        if record.get("access_mode") == "checked_native":
            record.setdefault("consumed_facts", []).append(raw_f64_layout_fact("consumed"))
        elif record.get("access_mode") == "dynamic_fallback":
            record.setdefault("rejected_facts", []).extend(
                [
                    raw_f64_layout_fact("rejected"),
                    raw_f64_layout_fact("invalidated"),
                ]
            )
    return records


def image_native_records():
    proven = {"proven": {"proof": "loop_guard"}}
    input_records = [
        native_record(
            block="for.body.24",
            rep="buffer_view",
            expr_kind="Uint8ArraySet.array",
            consumer="Uint8ArraySet.BufferView",
            bounds_state=proven,
            alias_state="may_alias",
            access_mode="unchecked_native",
            emitted_inbounds=True,
        ),
        native_record(
            block="for.body.24",
            rep="u8",
            expr_kind="Uint8ArraySet",
            consumer="u8_store_trunc_i32",
            bounds_state=proven,
            alias_state="may_alias",
            access_mode="unchecked_native",
            emitted_inbounds=True,
        ),
    ]
    return [
        *input_records,
        *input_records,
        *input_records,
        native_record(
            block="while.body.28",
            rep="u8",
            expr_kind="Uint8ArrayGet",
            consumer="u8_load_zext_i32",
            bounds_state=proven,
            alias_state="may_alias",
            access_mode="unchecked_native",
            emitted_inbounds=True,
        ),
        native_record(
            block="while.body.28",
            rep="u8",
            expr_kind="Uint8ArraySet",
            consumer="u8_store_trunc_i32",
            bounds_state=proven,
            alias_state="may_alias",
            access_mode="unchecked_native",
            emitted_inbounds=True,
        ),
        native_record(
            block="for.body.38",
            rep="u8",
            expr_kind="Uint8ArrayGet",
            consumer="u8_load_zext_i32",
            bounds_state=proven,
            alias_state="no_alias_proven",
            access_mode="unchecked_native",
            emitted_inbounds=True,
            emitted_noalias=True,
        ),
        native_record(
            block="for.body.38",
            rep="u8",
            expr_kind="Uint8ArrayGet",
            consumer="u8_load_zext_i32",
            bounds_state=proven,
            alias_state="no_alias_proven",
            access_mode="unchecked_native",
            emitted_inbounds=True,
            emitted_noalias=True,
        ),
        native_record(
            block="for.body.38",
            rep="u8",
            expr_kind="Uint8ArrayGet",
            consumer="u8_load_zext_i32",
            bounds_state=proven,
            alias_state="no_alias_proven",
            access_mode="unchecked_native",
            emitted_inbounds=True,
            emitted_noalias=True,
        ),
        native_record(
            block="for.body.38",
            rep="u8",
            expr_kind="Uint8ArraySet",
            consumer="u8_store_trunc_i32",
            bounds_state=proven,
            alias_state="no_alias_proven",
            access_mode="unchecked_native",
            emitted_inbounds=True,
            emitted_noalias=True,
        ),
        native_record(
            block="for.body.42",
            rep="u8",
            expr_kind="Uint8ArrayGet",
            consumer="u8_load_zext_i32",
            bounds_state=proven,
            alias_state="no_alias_proven",
            access_mode="unchecked_native",
            emitted_inbounds=True,
            emitted_noalias=True,
        ),
        native_record(
            block="for.body.42",
            rep="i32",
            expr_kind="MathImul",
            consumer="lower_expr_native_i32.structural",
        ),
    ]


def loop_data_dependent_native_records():
    return attach_raw_f64_layout_facts([
        native_record(
            block="apush.numeric_fast.5",
            rep="f64",
            expr_kind="NumericArrayPush",
            consumer="js_array_numeric_push_f64_unboxed",
            access_mode="checked_native",
            bounds_state={"guarded": {"guard_id": "numeric_array_push_guard"}},
        ),
        native_record(
            block="apush.numeric_fallback.6",
            rep="js_value",
            expr_kind="NumericArrayPush",
            consumer="js_array_push_f64",
            access_mode="dynamic_fallback",
            bounds_state="unknown",
            materialization_reason="runtime_api",
        ),
        native_record(
            block="arr.fast.12",
            rep="f64",
            expr_kind="NumericArrayIndexGet",
            consumer="js_array_numeric_get_f64_unboxed",
            access_mode="checked_native",
            bounds_state={"guarded": {"guard_id": "numeric_array_index_get_guard"}},
        ),
        native_record(
            block="arr.fast.15",
            rep="f64",
            expr_kind="NumericArrayIndexGet",
            consumer="js_array_numeric_get_f64_unboxed",
            access_mode="checked_native",
            bounds_state={"guarded": {"guard_id": "numeric_array_index_get_guard"}},
        ),
        native_record(
            block="arr.fallback.13",
            rep="js_value",
            expr_kind="NumericArrayIndexGet",
            consumer="js_typed_feedback_array_index_get_fallback_boxed",
            access_mode="dynamic_fallback",
            bounds_state="unknown",
            materialization_reason="runtime_api",
        ),
        native_record(
            block="arr.fallback.16",
            rep="js_value",
            expr_kind="NumericArrayIndexGet",
            consumer="js_typed_feedback_array_index_get_fallback_boxed",
            access_mode="dynamic_fallback",
            bounds_state="unknown",
            materialization_reason="runtime_api",
        ),
    ])


def numeric_array_native_records():
    return attach_raw_f64_layout_facts([
        native_record(
            block="apush.numeric_merge.6",
            rep="js_value",
            expr_kind="PackedF64LoopGuard",
            consumer="packed_f64_loop_guard",
            access_mode="checked_native",
            bounds_state={"guarded": {"guard_id": "packed_f64_array_loop_guard"}},
            consumed_facts=[array_kind_fact()],
        ),
        native_record(
            block="for.packed_f64_fast.body.15",
            rep="f64",
            expr_kind="PackedF64LoopLoad",
            consumer="packed_f64_loop_load",
            access_mode="checked_native",
            bounds_state={"guarded": {"guard_id": "packed_f64_array_loop_guard"}},
            consumed_facts=[array_kind_fact()],
        ),
        native_record(
            rep="f64",
            expr_kind="NumericArrayPush",
            consumer="js_array_numeric_push_f64_unboxed",
            access_mode="checked_native",
            bounds_state={"guarded": {"guard_id": "numeric_array_push_guard"}},
        ),
        native_record(
            rep="js_value",
            expr_kind="NumericArrayPush",
            consumer="js_array_push_f64",
            access_mode="dynamic_fallback",
            bounds_state="unknown",
            materialization_reason="runtime_api",
        ),
        native_record(
            rep="f64",
            expr_kind="NumericArrayIndexGet",
            consumer="js_array_numeric_get_f64_unboxed",
            access_mode="checked_native",
            bounds_state={"guarded": {"guard_id": "numeric_array_index_get_guard"}},
        ),
        native_record(
            rep="js_value",
            expr_kind="NumericArrayIndexGet",
            consumer="js_typed_feedback_array_index_get_fallback_boxed",
            access_mode="dynamic_fallback",
            bounds_state="unknown",
            materialization_reason="runtime_api",
        ),
        native_record(
            rep="f64",
            expr_kind="NumericArrayIndexSet",
            consumer="js_array_numeric_set_f64_unboxed",
            access_mode="checked_native",
            bounds_state={"guarded": {"guard_id": "numeric_array_index_set_guard"}},
        ),
        native_record(
            rep="js_value",
            expr_kind="NumericArrayIndexSet",
            consumer="js_typed_feedback_array_index_set_fallback_boxed",
            access_mode="dynamic_fallback",
            bounds_state="unknown",
            materialization_reason="runtime_api",
        ),
    ])


def numeric_arrays_inline_ir():
    return """
define i32 @main() {
entry:
  call i64 @js_array_numeric_push_f64_unboxed(i64 1, double 2.0)
  %g = call i32 @js_typed_feedback_numeric_array_index_get_guard(i64 1, double 0.0, double 0.0, i32 0, i32 1)
  %gc = icmp ne i32 %g, 0
  br i1 %gc, label %bidx.num.fast.1, label %bidx.num.fallback.2

bidx.num.fast.1:
  %addr = add i64 1, 8
  %p = inttoptr i64 %addr to ptr
  %v = load double, ptr %p, align 8
  br label %bidx.num.merge.3

bidx.num.fallback.2:
  br label %bidx.num.merge.3

bidx.num.merge.3:
  %sg = call i32 @js_typed_feedback_numeric_array_index_set_guard(i64 1, double 0.0, i32 0, double 3.0, i32 1)
  %sc = icmp ne i32 %sg, 0
  br i1 %sc, label %idxset.bounded_numeric_fast.4, label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_fast.4:
  %sval = fadd double 3.0, 0.0
  %saddr = add i64 1, 8
  %sp = inttoptr i64 %saddr to ptr
  %sraw = call double @js_array_numeric_value_to_raw_f64(double %sval)
  store double %sraw, ptr %sp, align 8
  br label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_merge.5:
  ret i32 0
}
"""


def h1_equivalence_native_records():
    region_ids = {
        "direct_bounded": "h1_native_rep_equivalence_ts.module_init.direct_bounded",
        "local_cast": "h1_native_rep_equivalence_ts.module_init.local_cast",
        "helper_index": "h1_native_rep_equivalence_ts.module_init.helper_index",
        "same_buffer": "h1_native_rep_equivalence_ts.incinplace.same_buffer",
    }
    blocks = {
        "direct_bounded": "for.body.2",
        "local_cast": "for.body.6",
        "helper_index": "for.body.10",
        "same_buffer": "for.body.2.i",
    }
    records = []
    proven = {"proven": {"proof": "loop_guard"}}
    for name, region_id in region_ids.items():
        alias_state = "may_alias" if name == "same_buffer" else "no_alias_proven"
        records.extend(
            [
                native_record(
                    block=blocks[name],
                    rep="i32",
                    region_id=region_id,
                    bounds_state=proven,
                ),
                native_record(
                    block=blocks[name],
                    rep="buffer_view",
                    region_id=region_id,
                    bounds_state=proven,
                    alias_state=alias_state,
                    access_mode="unchecked_native",
                ),
                native_record(
                    block=blocks[name],
                    rep="u8",
                    region_id=region_id,
                    bounds_state=proven,
                    alias_state=alias_state,
                    access_mode="unchecked_native",
                    consumer="u8_load_zext_i32",
                ),
                native_record(
                    block=blocks[name],
                    rep="u8",
                    region_id=region_id,
                    bounds_state=proven,
                    alias_state=alias_state,
                    access_mode="unchecked_native",
                    consumer="u8_store_trunc_i32",
                ),
            ]
        )
    return records


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
            native_reps=[{"records": image_native_records()}],
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

    def test_image_convolution_allows_split_input_generation_region(self):
        split_ir = GOOD_IR.replace(
            "  %p1 = getelementptr inbounds i8, ptr %base, i64 %i1\n"
            "  store i8 2, ptr %p1, align 1, !alias.scope !2, !noalias !3\n"
            "  %p2 = getelementptr inbounds i8, ptr %base, i64 %i2\n"
            "  store i8 3, ptr %p2, align 1, !alias.scope !2, !noalias !3\n",
            "  %p1 = getelementptr inbounds i8, ptr %base, i64 %i1\n"
            "  %p2 = getelementptr inbounds i8, ptr %base, i64 %i2\n",
        )
        report = HARNESS.verify_artifacts(
            workload="image_convolution",
            ir_before=split_ir,
            ir_after=split_ir,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0}]},
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
            native_reps=[{"records": image_native_records()}],
        )
        self.assertEqual(report["status"], "pass", report["errors"])

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
            native_reps=[{"records": loop_data_dependent_native_records()}],
        )
        self.assertEqual(report["status"], "pass", report["errors"])

    def test_native_region_workloads_require_native_rep_artifacts(self):
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
        cases = [
            ("image_convolution", GOOD_IR, GOOD_ASM),
            ("loop_data_dependent", numeric_ir, "main:\n  retq\n"),
        ]
        for workload, ir, assembly in cases:
            with self.subTest(workload=workload):
                report = HARNESS.verify_artifacts(
                    workload=workload,
                    ir_before=ir,
                    ir_after=ir,
                    assembly=assembly,
                    benchmark=None,
                    vectorization={
                        "vectorized_count": 0,
                        "missed_count": 0,
                        "analysis_count": 0,
                    },
                )
                self.assertEqual(report["status"], "fail")
                self.assertTrue(
                    any(
                        "native_reps_artifact_present" in error
                        for error in report["errors"]
                    )
                )

    def test_numeric_arrays_requires_runtime_api_fallback_reasons(self):
        numeric_ir = """
define i32 @main() {
entry:
  call i64 @js_array_numeric_push_f64_unboxed(i64 1, double 2.0)
  call double @js_array_numeric_get_f64_unboxed(i64 1, i32 0)
  %sg = call i32 @js_typed_feedback_numeric_array_index_set_guard(i64 1, double 0.0, i32 0, double 3.0, i32 1)
  %sc = icmp ne i32 %sg, 0
  br i1 %sc, label %idxset.bounded_numeric_fast.4, label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_fast.4:
  %sval = fadd double 3.0, 0.0
  %saddr = add i64 1, 8
  %sp = inttoptr i64 %saddr to ptr
  %sraw = call double @js_array_numeric_value_to_raw_f64(double %sval)
  store double %sraw, ptr %sp, align 8
  br label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_merge.5:
  ret i32 0
}
"""
        records = numeric_array_native_records()
        for record in records:
            if record.get("access_mode") == "dynamic_fallback":
                record["materialization_reason"] = None
        report = HARNESS.verify_artifacts(
            workload="numeric_arrays",
            ir_before=numeric_ir,
            ir_after=numeric_ir,
            assembly="main:\n  retq\n",
            benchmark={"runs": [{"exit_code": 0, "stdout_first": "25\n"}]},
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_required_numeric_array_get_dynamic_fallback" in error
                for error in report["errors"]
            )
        )

    def test_verify_existing_artifacts_writes_report(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "llvm-before-opt.ll").write_text(GOOD_IR, encoding="utf-8")
            (root / "llvm-after-opt.ll").write_text(GOOD_IR, encoding="utf-8")
            (root / "assembly.s").write_text(GOOD_ASM, encoding="utf-8")
            (root / "native-reps.json").write_text(
                json.dumps({"records": image_native_records()}),
                encoding="utf-8",
            )
            (root / "manifest.json").write_text(
                json.dumps(
                    {
                        "benchmark": {
                            "gc_trace_enabled": True,
                            "runs": [
                                {
                                    "run": 1,
                                    "exit_code": 0,
                                    "gc_trace_enabled": True,
                                    "gc_trace_summary": {},
                                }
                            ],
                        }
                    }
                ),
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
            self.assertTrue((root / "structural-report.json").exists())

    def test_verify_existing_uses_analysis_ir_object_disassembly_and_manifest_plan(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "llvm-before-opt.ll").write_text(GOOD_IR, encoding="utf-8")
            (root / "llvm-after-opt.analysis.ll").write_text(GOOD_IR, encoding="utf-8")
            (root / "object-disassembly.s").write_text(GOOD_ASM, encoding="utf-8")
            (root / "native-reps.json").write_text(
                json.dumps({"records": image_native_records()}),
                encoding="utf-8",
            )
            (root / "manifest.json").write_text(
                """
{
  "compile_plan": {
    "effective_target": "x86_64-unknown-linux-gnu",
    "clang_args": ["-c", "-O3", "-fno-math-errno", "-march=native"]
  },
  "benchmark": {
    "gc_trace_enabled": true,
    "runs": [
      {
        "run": 1,
        "exit_code": 0,
        "gc_trace_enabled": true,
        "gc_trace_summary": {}
      }
    ]
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

    def test_verify_existing_uses_manifest_benchmark_stdout_for_stdout_checks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ir = numeric_arrays_inline_ir()
            (root / "llvm-before-opt.ll").write_text(ir, encoding="utf-8")
            (root / "llvm-after-opt.analysis.ll").write_text(ir, encoding="utf-8")
            (root / "object-disassembly.s").write_text(GOOD_ASM, encoding="utf-8")
            (root / "native-reps.json").write_text(
                json.dumps({"records": numeric_array_native_records()}),
                encoding="utf-8",
            )
            (root / "manifest.json").write_text(
                json.dumps(
                    {
                        "benchmark": {
                            "gc_trace_enabled": True,
                            "runs": [
                                {
                                    "run": 1,
                                    "exit_code": 0,
                                    "stdout_first": "25\n",
                                    "gc_trace_enabled": True,
                                    "gc_trace_summary": {},
                                }
                            ]
                        }
                    }
                ),
                encoding="utf-8",
            )
            args = type(
                "Args",
                (),
                {
                    "artifact_dir": str(root),
                    "workload": "numeric_arrays",
                    "gate": True,
                    "print_summary": False,
                    "target": None,
                    "clang_arg": None,
                    "fp_contract": None,
                    "expect_fma": "auto",
                },
            )()
            self.assertEqual(HARNESS.verify_existing(args), 0)

    def test_verify_existing_stdout_checks_fail_without_manifest_benchmark(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ir = numeric_arrays_inline_ir()
            (root / "llvm-before-opt.ll").write_text(ir, encoding="utf-8")
            (root / "llvm-after-opt.analysis.ll").write_text(ir, encoding="utf-8")
            (root / "object-disassembly.s").write_text(GOOD_ASM, encoding="utf-8")
            (root / "native-reps.json").write_text(
                json.dumps({"records": numeric_array_native_records()}),
                encoding="utf-8",
            )
            args = type(
                "Args",
                (),
                {
                    "artifact_dir": str(root),
                    "workload": "numeric_arrays",
                    "gate": True,
                    "print_summary": False,
                    "target": None,
                    "clang_arg": None,
                    "fp_contract": None,
                    "expect_fma": "auto",
                },
            )()
            self.assertEqual(HARNESS.verify_existing(args), 1)
            report = (root / "structural-report.json").read_text(encoding="utf-8")
            self.assertIn("numeric_arrays_checksum", report)
            self.assertIn("no benchmark stdout captured", report)

    def test_verify_existing_loads_all_native_rep_shards(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ir = numeric_arrays_inline_ir()
            records = numeric_array_native_records()
            (root / "llvm-before-opt.ll").write_text(ir, encoding="utf-8")
            (root / "llvm-after-opt.analysis.ll").write_text(ir, encoding="utf-8")
            (root / "object-disassembly.s").write_text(GOOD_ASM, encoding="utf-8")
            (root / "native-reps.json").write_text(
                json.dumps({"records": records[:2]}),
                encoding="utf-8",
            )
            (root / "native-reps-0.json").write_text(
                json.dumps({"records": records[:4]}),
                encoding="utf-8",
            )
            (root / "native-reps-1.json").write_text(
                json.dumps({"records": records[4:]}),
                encoding="utf-8",
            )
            (root / "manifest.json").write_text(
                json.dumps(
                    {
                        "benchmark": {
                            "gc_trace_enabled": True,
                            "runs": [
                                {
                                    "run": 1,
                                    "exit_code": 0,
                                    "stdout_first": "25\n",
                                    "gc_trace_enabled": True,
                                    "gc_trace_summary": {},
                                }
                            ],
                        }
                    }
                ),
                encoding="utf-8",
            )
            args = type(
                "Args",
                (),
                {
                    "artifact_dir": str(root),
                    "workload": "numeric_arrays",
                    "gate": True,
                    "print_summary": False,
                    "target": None,
                    "clang_arg": None,
                    "fp_contract": None,
                    "expect_fma": "auto",
                },
            )()
            self.assertEqual(HARNESS.verify_existing(args), 0)

    def test_verify_existing_fails_when_manifest_native_rep_shard_is_missing(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ir = numeric_arrays_inline_ir()
            records = numeric_array_native_records()
            (root / "llvm-before-opt.ll").write_text(ir, encoding="utf-8")
            (root / "llvm-after-opt.analysis.ll").write_text(ir, encoding="utf-8")
            (root / "object-disassembly.s").write_text(GOOD_ASM, encoding="utf-8")
            (root / "native-reps-0.json").write_text(
                json.dumps({"records": records[:4]}),
                encoding="utf-8",
            )
            (root / "manifest.json").write_text(
                json.dumps(
                    {
                        "artifacts": {
                            "native_reps": [
                                {"native_reps_artifact": "native-reps-0.json"},
                                {"native_reps_artifact": "native-reps-1.json"},
                            ]
                        },
                        "benchmark": {
                            "gc_trace_enabled": True,
                            "runs": [
                                {
                                    "run": 1,
                                    "exit_code": 0,
                                    "stdout_first": "25\n",
                                    "gc_trace_enabled": True,
                                    "gc_trace_summary": {},
                                }
                            ],
                        },
                    }
                ),
                encoding="utf-8",
            )
            args = type(
                "Args",
                (),
                {
                    "artifact_dir": str(root),
                    "workload": "numeric_arrays",
                    "gate": True,
                    "print_summary": False,
                    "target": None,
                    "clang_arg": None,
                    "fp_contract": None,
                    "expect_fma": "auto",
                },
            )()
            with self.assertRaisesRegex(
                HARNESS.HarnessError,
                "missing native reps artifacts listed in manifest",
            ):
                HARNESS.verify_existing(args)

    def test_explicit_perry_path_is_repo_relative(self):
        resolved = HARNESS.resolve_perry("target/debug/perry")
        self.assertEqual(resolved, [str(REPO_ROOT / "target/debug/perry")])

    def test_workload_spec_loads_current_workloads(self):
        spec = HARNESS.load_workload_spec(HARNESS.DEFAULT_SPEC_PATH)
        self.assertIn("image_convolution", spec["workloads"])
        self.assertIn("fma_contract", spec["workloads"])
        self.assertIn("numeric_arrays", spec["workloads"])
        self.assertIn("packed_f64_loop_versioning", spec["workloads"])
        self.assertIn("packed_f64_loop_versioning_negative", spec["workloads"])
        self.assertIn("dynamic_fractional_array_index", spec["workloads"])
        self.assertIn("loop_bound_semantics", spec["workloads"])
        self.assertIn("raw_numeric_object_fields", spec["workloads"])
        self.assertIn("scalar_replacement_literals", spec["workloads"])
        self.assertIn("native_pod_layout_constants", spec["workloads"])
        self.assertIn("native_memory_bulk_fill", spec["workloads"])
        self.assertIn("native_memory_fixture", spec["workloads"])
        self.assertIn("native_abi_packet_typed", spec["workloads"])
        self.assertIn("native_abi_packet_control", spec["workloads"])
        self.assertTrue(spec["workloads"]["fma_contract"]["fma_gate"]["enabled"])
        for name, workload in spec["workloads"].items():
            self.assertIn("source", workload, name)
            self.assertIn("vectorization", workload, name)
            self.assertIn("runtime_budgets", workload, name)

    def test_native_abi_root_barrier_budgets_are_explicit(self):
        spec = HARNESS.load_workload_spec(HARNESS.DEFAULT_SPEC_PATH)
        expected = {
            "h1_native_rep_equivalence": 1,
            "h1_buffer_alias_negative": 3,
            "native_owned_typed_views": 1,
            "native_pod_layout_constants": 2,
            "native_memory_bulk_fill": 2,
            "native_memory_fixture": 2,
        }
        for name, maximum in expected.items():
            self.assertEqual(
                spec["workloads"][name]["runtime_budgets"][
                    "write_barriers_static"
                ],
                maximum,
                name,
            )

    def test_suite_parser_accepts_native_region_proof(self):
        parser = HARNESS.build_parser()
        args = parser.parse_args(
            [
                "suite",
                "--suite",
                "native-region-proof",
                "--perry",
                "target/debug/perry",
                "--benchmark-mode",
                "smoke",
                "--runs",
                "1",
                "--perf-counters",
                "off",
            ]
        )
        self.assertEqual(args.suite, "native-region-proof")

    def test_suite_parser_accepts_native_abi_proof(self):
        parser = HARNESS.build_parser()
        args = parser.parse_args(
            [
                "suite",
                "--suite",
                "native-abi-proof",
                "--perry",
                "target/debug/perry",
                "--benchmark-mode",
                "smoke",
                "--runs",
                "1",
                "--perf-counters",
                "off",
                "--gate",
            ]
        )
        self.assertEqual(args.suite, "native-abi-proof")
        self.assertTrue(args.gate)

    def test_native_abi_proof_suite_includes_native_memory_workloads(self):
        suite = SUITES["native-abi-proof"]
        packet_typed_index = suite.index("native_abi_packet_typed")
        for workload in (
            "width_aware_buffer_kernels",
            "native_owned_typed_views",
            "native_pod_layout_constants",
            "native_memory_bulk_fill",
            "native_memory_fixture",
        ):
            self.assertIn(workload, suite)
            self.assertLess(suite.index(workload), packet_typed_index)

    def test_ci_wires_native_abi_proof_suite(self):
        workflow = (REPO_ROOT / ".github" / "workflows" / "test.yml").read_text(
            encoding="utf-8"
        )
        native_region_block = workflow.split("Gate native-region proof compiler output", 1)[
            1
        ].split("Gate native-ABI proof compiler output", 1)[0]
        native_abi_block = workflow.split("Gate native-ABI proof compiler output", 1)[
            1
        ].split("Gate typed feedback runtime evidence", 1)[0]
        self.assertIn("--gate", native_region_block)
        self.assertIn("--gate", native_abi_block)
        self.assertIn("Gate native-ABI proof compiler output", workflow)
        self.assertIn("--suite native-abi-proof", workflow)
        self.assertIn("Run native ABI evidence report unit tests", workflow)
        self.assertIn("tests.test_native_abi_evidence_report", workflow)
        self.assertIn("native-abi-evidence-packet:", workflow)
        self.assertIn("Gate native ABI evidence packet", workflow)
        self.assertIn('RUSTC_WRAPPER: ""', workflow)
        self.assertIn("RUSTFLAGS: -Awarnings", workflow)
        self.assertIn("tests/test_native_abi_evidence_packet_smoke.sh", workflow)
        self.assertIn("target/native-abi-evidence-packet", workflow)
        self.assertIn("Gate typed feedback runtime evidence", workflow)
        self.assertIn("tests.test_typed_feedback_runtime_evidence", workflow)

    def test_capture_suite_propagates_gate_to_workload_capture(self):
        observed_gate_values = []
        original_capture = CAPTURE_MODULE.capture
        CAPTURE_MODULE.SUITES["__gate_propagation_test__"] = ["dummy_workload"]

        def fake_capture(args):
            observed_gate_values.append(args.gate)
            out_dir = Path(args.out_dir)
            out_dir.mkdir(parents=True, exist_ok=True)
            (out_dir / "structural-report.json").write_text(
                json.dumps({"status": "pass", "errors": []}),
                encoding="utf-8",
            )
            return 0

        CAPTURE_MODULE.capture = fake_capture
        try:
            with tempfile.TemporaryDirectory() as temp:
                args = SimpleNamespace(
                    suite="__gate_propagation_test__",
                    out_dir=temp,
                    gate=True,
                    print_summary=False,
                )
                self.assertEqual(CAPTURE_MODULE.capture_suite(args), 0)
        finally:
            CAPTURE_MODULE.capture = original_capture
            CAPTURE_MODULE.SUITES.pop("__gate_propagation_test__", None)

        self.assertEqual(observed_gate_values, [True])

    def test_trace_budget_compile_env_requests_gc_trace(self):
        env = CAPTURE_MODULE._compile_env("clang", enable_gc_trace=True)
        self.assertEqual(env["PERRY_GC_TRACE"], "1")
        self.assertNotIn("PERRY_GC_TRACE", CAPTURE_MODULE._compile_env("clang"))

    def test_auto_optimize_enables_diagnostics_for_gc_trace_evidence(self):
        # The PERRY_GC_TRACE -> perry-runtime/diagnostics wiring lives in the
        # cross-feature derivation, split out of optimized_libs.rs into
        # optimized_libs/freshness.rs on main (#1435 file-size split).
        freshness = (
            REPO_ROOT
            / "crates"
            / "perry"
            / "src"
            / "commands"
            / "compile"
            / "optimized_libs"
            / "freshness.rs"
        ).read_text(encoding="utf-8")
        self.assertIn('std::env::var("PERRY_GC_TRACE")', freshness)
        self.assertIn("perry-runtime/diagnostics", freshness)

    def test_release_sweep_wires_native_abi_evidence_packet_smoke(self):
        release_sweep = (REPO_ROOT / "scripts" / "release_sweep.sh").read_text(
            encoding="utf-8"
        )
        tier = (
            REPO_ROOT
            / "scripts"
            / "release_sweep_tiers"
            / "tier13_native_abi_evidence.sh"
        ).read_text(encoding="utf-8")
        smoke = (
            REPO_ROOT / "tests" / "test_native_abi_evidence_packet_smoke.sh"
        ).read_text(encoding="utf-8")
        packet = (
            REPO_ROOT / "scripts" / "native_abi_evidence_packet.sh"
        ).read_text(encoding="utf-8")
        self.assertIn(
            "13|native_abi_evidence|all|native ABI evidence packet smoke gate",
            release_sweep,
        )
        self.assertIn("tests/test_native_abi_evidence_packet_smoke.sh", tier)
        self.assertIn("PERRY_BIN", tier)
        self.assertIn("--runs 5", smoke)
        self.assertIn("--gate", smoke)
        self.assertIn("scripts/check_runtime_symbols.sh", packet)
        self.assertIn('export RUSTC_WRAPPER=""', packet)
        self.assertIn('export RUSTFLAGS="-Awarnings"', packet)
        self.assertIn('"rustc_wrapper_effective"', packet)
        self.assertIn('"rustflags_effective"', packet)
        self.assertIn("release_symbol_guard", smoke)
        self.assertIn("--gate requires --runs >= 5", packet)
        self.assertIn("build_runtime_archive", packet)
        self.assertIn("snapshot_tool_artifacts", packet)
        self.assertIn("rustc_wrapper_scrubbed", packet)

    def test_runtime_symbol_guard_roots_numeric_array_helpers(self):
        guard = (REPO_ROOT / "scripts" / "check_runtime_symbols.sh").read_text(
            encoding="utf-8"
        )
        for symbol in (
            "js_array_numeric_value_to_raw_f64",
            "js_array_mark_numeric_f64_layout",
            "js_array_clear_numeric_layout",
            "js_array_note_numeric_write",
            "js_array_is_numeric_f64_layout",
            "js_array_numeric_get_f64_unboxed",
            "js_array_numeric_set_f64_unboxed",
            "js_array_numeric_push_f64_unboxed",
        ):
            self.assertIn(symbol, guard)
        self.assertIn("nm -s", guard)

    def test_runtime_symbol_guard_roots_representation_lowering_helpers(self):
        guard = (REPO_ROOT / "scripts" / "check_runtime_symbols.sh").read_text(
            encoding="utf-8"
        )
        for symbol in (
            "js_typed_feedback_maybe_dump_trace",
            "js_typed_f64_arg_guard",
            "js_typed_f64_arg_to_raw",
            "js_typed_i1_arg_guard",
            "js_typed_i1_arg_to_raw",
            "js_typed_string_arg_guard",
            "js_typed_string_arg_to_raw",
            "js_object_get_field_by_property_id_f64",
            "js_object_set_field_by_property_id",
            "js_native_call_method_by_id",
            "js_native_call_method_apply_by_id",
            "js_class_method_bind_by_id",
            "js_method_direct_shape_guard",
            "js_typed_feedback_class_field_get_guard",
            "js_typed_feedback_class_field_set_guard",
            "js_typed_feedback_method_direct_call_guard",
            "js_typed_feedback_closure_direct_call_guard",
            "js_typed_feedback_native_call_method_by_id",
            "js_typed_feedback_native_call_method_apply_by_id",
        ):
            self.assertIn(symbol, guard)

    def test_runtime_symbol_guard_roots_typed_feedback_array_helpers(self):
        guard = (REPO_ROOT / "scripts" / "check_runtime_symbols.sh").read_text(
            encoding="utf-8"
        )
        for symbol in (
            "js_typed_feedback_array_get_f64",
            "js_typed_feedback_plain_array_index_get_guard",
            "js_typed_feedback_numeric_array_index_get_guard",
            "js_typed_feedback_packed_f64_array_loop_guard",
            "js_typed_feedback_array_index_get_fallback_boxed",
            "js_typed_feedback_array_set_f64",
            "js_typed_feedback_array_set_f64_extend",
            "js_typed_feedback_plain_array_index_set_guard",
            "js_typed_feedback_numeric_array_index_set_guard",
            "js_typed_feedback_numeric_array_push_guard",
            "js_typed_feedback_array_index_set_fallback_boxed",
            "js_typed_feedback_observe_array_element",
            "js_typed_feedback_array_set_string_key",
            "js_typed_feedback_array_set_index_or_string",
            "js_typed_feedback_object_set_index_polymorphic",
            "js_typed_feedback_object_set_unboxed_f64_field",
        ):
            self.assertIn(symbol, guard)

    def test_runtime_symbol_guard_roots_map_set_string_lowering_helpers(self):
        guard = (REPO_ROOT / "scripts" / "check_runtime_symbols.sh").read_text(
            encoding="utf-8"
        )
        for symbol in (
            "js_map_set_string_number",
            "js_map_set_string_key",
            "js_map_set_string_i32",
            "js_map_set_string_u32",
            "js_map_set_string_f32",
            "js_map_set_string_bool",
            "js_map_set_string_string",
            "js_map_set_number_key",
            "js_map_get_string_key",
            "js_map_get_number_key",
            "js_map_has_string_key",
            "js_map_has_number_key",
            "js_map_delete_string_key",
            "js_map_delete_number_key",
            "js_set_add_string",
            "js_set_add_number",
            "js_set_has_string",
            "js_set_has_number",
            "js_set_delete_string",
            "js_set_delete_number",
            "js_set_add_i32",
            "js_set_has_i32",
            "js_set_delete_i32",
            "js_set_add_u32",
            "js_set_has_u32",
            "js_set_delete_u32",
            "js_set_add_f32",
            "js_set_has_f32",
            "js_set_delete_f32",
            "js_set_add_bool",
            "js_set_has_bool",
            "js_set_delete_bool",
        ):
            self.assertIn(symbol, guard)

    def test_runtime_symbol_guard_roots_async_control_box_helpers(self):
        guard = (REPO_ROOT / "scripts" / "check_runtime_symbols.sh").read_text(
            encoding="utf-8"
        )
        for symbol in (
            "js_i32_box_alloc",
            "js_i32_box_get",
            "js_i32_box_set",
            "js_bool_box_alloc",
            "js_bool_box_get",
            "js_bool_box_set",
            "js_iter_result_set",
            "js_iter_result_set_f64",
            "js_iter_result_set_i1",
            "js_iter_result_get_value",
            "js_iter_result_get_value_f64",
            "js_iter_result_get_value_i1",
            "js_iter_result_get_done",
        ):
            self.assertIn(symbol, guard)

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

    def test_workload_spec_rejects_substring_stdout_checks(self):
        with self.assertRaises(HARNESS.HarnessError):
            HARNESS.validate_workload_spec(
                {
                    "schema_version": 1,
                    "workloads": {
                        "bad": {
                            "source": "fixture.ts",
                            "kind": "numeric_loop",
                            "vectorization": {
                                "min_vectorized_loops": 0,
                                "allowed_missed_reason_kinds": [],
                            },
                            "runtime_budgets": {},
                            "stdout_checks": [
                                {
                                    "name": "bad_stdout",
                                    "contains": "25",
                                }
                            ],
                        }
                    },
                }
            )

    def test_parse_kept_paths_includes_compile_metadata(self):
        irs, objects, metadata, native_reps = HARNESS.parse_kept_paths(
            "[perry-codegen] kept LLVM IR: /tmp/a.ll\n"
            "[perry-codegen] kept object:  /tmp/a.o\n"
            "[perry-codegen] kept compile metadata: /tmp/a.o.compile-plan.json\n"
            "[perry-codegen] kept native reps: /tmp/native-reps.json\n"
        )
        self.assertEqual(irs, [Path("/tmp/a.ll")])
        self.assertEqual(objects, [Path("/tmp/a.o")])
        self.assertEqual(metadata, [Path("/tmp/a.o.compile-plan.json")])
        self.assertEqual(native_reps, [Path("/tmp/native-reps.json")])

    def test_runtime_counter_summary_combines_static_and_trace_counts(self):
        counters = HARNESS.structural_counters(
            GOOD_IR,
            GOOD_IR
            + "\n  call double @js_boxed_number_new(double 1.0)\n"
            + "  call double @js_buffer_get(double 1.0, double 0.0)\n"
            + "  call double @js_typed_array_get(double 1.0, double 0.0)\n"
            + "  call void @js_write_barrier(i64 1, i64 2)\n"
            + "  call void @js_write_barrier_slot(i64 1, i64 8, i64 2)\n"
            + "  call void @js_write_barrier_root_nanbox(i64 2)\n"
            + "  call void @js_write_barrier_root_heap_word(i64 2)\n"
            + "  call i32 @js_uint8array_get(i64 1, i32 0)\n",
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
        self.assertEqual(summary["write_barriers_static"], 4)
        self.assertEqual(summary["boxed_number_allocations_static"], 1)
        self.assertEqual(summary["buffer_slow_path_accesses_static"], 2)
        self.assertEqual(summary["array_slow_path_accesses_static"], 2)

    def test_trace_runtime_budgets_fail_when_gc_trace_disabled(self):
        benchmark = {
            "gc_trace_enabled": False,
            "runs": [
                {
                    "run": 1,
                    "exit_code": 0,
                    "gc_trace_enabled": False,
                    "gc_trace_summary": {},
                }
            ],
        }
        counters = HARNESS.structural_counters(GOOD_IR, GOOD_IR, GOOD_ASM)
        report = HARNESS.verify_artifacts(
            workload="image_convolution",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=benchmark,
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
            counters=counters,
            runtime_summary=HARNESS.runtime_counter_summary(benchmark, counters),
            native_reps=[{"records": image_native_records()}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("runtime_budget_gc_trace_enabled" in error for error in report["errors"]),
            report["errors"],
        )

    def test_trace_runtime_budgets_fail_when_gc_trace_state_missing(self):
        benchmark = {
            "runs": [
                {
                    "run": 1,
                    "exit_code": 0,
                    "gc_trace_summary": {},
                }
            ],
        }
        counters = HARNESS.structural_counters(GOOD_IR, GOOD_IR, GOOD_ASM)
        report = HARNESS.verify_artifacts(
            workload="image_convolution",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=benchmark,
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
            },
            counters=counters,
            runtime_summary=HARNESS.runtime_counter_summary(benchmark, counters),
            native_reps=[{"records": image_native_records()}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("runtime_budget_gc_trace_enabled" in error for error in report["errors"]),
            report["errors"],
        )

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

    def test_native_rep_unsafe_inbounds_fails_gate(self):
        report = HARNESS.verify_artifacts(
            workload="h1_native_rep_synthetic",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[
                {
                    "records": [
                        native_record(
                            rep="u8",
                            bounds_state="unknown",
                            emitted_inbounds=True,
                        )
                    ]
                }
            ],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("native_reps_no_unsafe_inbounds_claims" in error for error in report["errors"])
        )

    def test_native_rep_unchecked_unknown_bounds_fails_gate(self):
        report = HARNESS.verify_artifacts(
            workload="h1_native_rep_synthetic",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[
                {
                    "records": [
                        native_record(
                            rep="u8",
                            bounds_state="unknown",
                            access_mode="unchecked_native",
                        )
                    ]
                }
            ],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("native_reps_no_unchecked_unknown_bounds" in error for error in report["errors"])
        )

    def test_generic_native_rep_checks_require_configured_records(self):
        # The numeric indexed read is inlined: a guarded fast block computes the
        # element pointer (inttoptr) and performs a direct `load double` instead
        # of calling js_array_numeric_get_f64_unboxed. The indexed write
        # canonicalizes the input and stores inline after its guard instead of
        # calling the raw-f64 set helper.
        ir = """
define i32 @main() {
entry:
  call i64 @js_array_numeric_push_f64_unboxed(i64 1, double 2.0)
  %g = call i32 @js_typed_feedback_numeric_array_index_get_guard(i64 1, double 0.0, double 0.0, i32 0, i32 1)
  %gc = icmp ne i32 %g, 0
  br i1 %gc, label %bidx.num.fast.1, label %bidx.num.fallback.2

bidx.num.fast.1:
  %addr = add i64 1, 8
  %p = inttoptr i64 %addr to ptr
  %v = load double, ptr %p, align 8
  br label %bidx.num.merge.3

bidx.num.fallback.2:
  br label %bidx.num.merge.3

bidx.num.merge.3:
  %sg = call i32 @js_typed_feedback_numeric_array_index_set_guard(i64 1, double 0.0, i32 0, double 3.0, i32 1)
  %sc = icmp ne i32 %sg, 0
  br i1 %sc, label %idxset.bounded_numeric_fast.4, label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_fast.4:
  %sval = fadd double 3.0, 0.0
  %saddr = add i64 1, 8
  %sp = inttoptr i64 %saddr to ptr
  %sraw = call double @js_array_numeric_value_to_raw_f64(double %sval)
  store double %sraw, ptr %sp, align 8
  br label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_merge.5:
  ret i32 0
}
"""
        records = numeric_array_native_records()
        report = HARNESS.verify_artifacts(
            workload="numeric_arrays",
            ir_before=ir,
            ir_after=ir,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0, "stdout_first": "25\n"}]},
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "pass", report["errors"])

    def test_stdout_checks_require_benchmark_data(self):
        ir = numeric_arrays_inline_ir()
        report = HARNESS.verify_artifacts(
            workload="numeric_arrays",
            ir_before=ir,
            ir_after=ir,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": numeric_array_native_records()}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("numeric_arrays_checksum" in error for error in report["errors"]),
            report["errors"],
        )
        self.assertTrue(
            any("no benchmark stdout captured" in error for error in report["errors"]),
            report["errors"],
        )

    def test_stdout_checks_are_exact_for_every_run(self):
        ir = numeric_arrays_inline_ir()
        report = HARNESS.verify_artifacts(
            workload="numeric_arrays",
            ir_before=ir,
            ir_after=ir,
            assembly=GOOD_ASM,
            benchmark={
                "runs": [
                    {"run": 1, "exit_code": 0, "stdout_first": "25\n"},
                    {"run": 2, "exit_code": 0, "stdout_first": "125\n"},
                ]
            },
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": numeric_array_native_records()}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "numeric_arrays_checksum" in error and "failed_runs=[2]" in error
                for error in report["errors"]
            ),
            report["errors"],
        )

    def test_numeric_array_native_rep_checks_require_raw_layout_facts(self):
        ir = """
define i32 @main() {
entry:
  call i64 @js_array_numeric_push_f64_unboxed(i64 1, double 2.0)
  call double @js_array_numeric_get_f64_unboxed(i64 1, i32 0)
  %sg = call i32 @js_typed_feedback_numeric_array_index_set_guard(i64 1, double 0.0, i32 0, double 3.0, i32 1)
  %sc = icmp ne i32 %sg, 0
  br i1 %sc, label %idxset.bounded_numeric_fast.4, label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_fast.4:
  %sval = fadd double 3.0, 0.0
  %saddr = add i64 1, 8
  %sp = inttoptr i64 %saddr to ptr
  %sraw = call double @js_array_numeric_value_to_raw_f64(double %sval)
  store double %sraw, ptr %sp, align 8
  br label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_merge.5:
  ret i32 0
}
"""
        checked_records = numeric_array_native_records()
        for record in checked_records:
            if record.get("access_mode") == "checked_native":
                record["consumed_facts"] = []
        checked_report = HARNESS.verify_artifacts(
            workload="numeric_arrays",
            ir_before=ir,
            ir_after=ir,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0, "stdout_first": "25\n"}]},
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": checked_records}],
        )
        self.assertEqual(checked_report["status"], "fail")
        self.assertTrue(
            any("native_reps_required_numeric_array_push_fast_f64" in error for error in checked_report["errors"]),
            checked_report["errors"],
        )

        fallback_records = numeric_array_native_records()
        for record in fallback_records:
            if record.get("access_mode") == "dynamic_fallback":
                record["rejected_facts"] = []
        fallback_report = HARNESS.verify_artifacts(
            workload="numeric_arrays",
            ir_before=ir,
            ir_after=ir,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0, "stdout_first": "25\n"}]},
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": fallback_records}],
        )
        self.assertEqual(fallback_report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_required_numeric_array_get_dynamic_fallback" in error
                for error in fallback_report["errors"]
            ),
            fallback_report["errors"],
        )

    def test_numeric_array_native_rep_checks_require_fallback_reason(self):
        ir = """
define i32 @main() {
entry:
  call i64 @js_array_numeric_push_f64_unboxed(i64 1, double 2.0)
  call double @js_array_numeric_get_f64_unboxed(i64 1, i32 0)
  %sg = call i32 @js_typed_feedback_numeric_array_index_set_guard(i64 1, double 0.0, i32 0, double 3.0, i32 1)
  %sc = icmp ne i32 %sg, 0
  br i1 %sc, label %idxset.bounded_numeric_fast.4, label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_fast.4:
  %sval = fadd double 3.0, 0.0
  %saddr = add i64 1, 8
  %sp = inttoptr i64 %saddr to ptr
  %sraw = call double @js_array_numeric_value_to_raw_f64(double %sval)
  store double %sraw, ptr %sp, align 8
  br label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_merge.5:
  ret i32 0
}
"""
        records = numeric_array_native_records()
        for record in records:
            if record.get("access_mode") == "dynamic_fallback":
                record["fallback_reason"] = None
                break
        report = HARNESS.verify_artifacts(
            workload="numeric_arrays",
            ir_before=ir,
            ir_after=ir,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0, "stdout_first": "25\n"}]},
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_dynamic_fallbacks_have_reasons" in error
                for error in report["errors"]
            ),
            report["errors"],
        )

    def test_numeric_array_native_rep_checks_require_fact_reason(self):
        ir = """
define i32 @main() {
entry:
  call i64 @js_array_numeric_push_f64_unboxed(i64 1, double 2.0)
  call double @js_array_numeric_get_f64_unboxed(i64 1, i32 0)
  %sg = call i32 @js_typed_feedback_numeric_array_index_set_guard(i64 1, double 0.0, i32 0, double 3.0, i32 1)
  %sc = icmp ne i32 %sg, 0
  br i1 %sc, label %idxset.bounded_numeric_fast.4, label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_fast.4:
  %sval = fadd double 3.0, 0.0
  %saddr = add i64 1, 8
  %sp = inttoptr i64 %saddr to ptr
  %sraw = call double @js_array_numeric_value_to_raw_f64(double %sval)
  store double %sraw, ptr %sp, align 8
  br label %idxset.bounded_numeric_merge.5

idxset.bounded_numeric_merge.5:
  ret i32 0
}
"""
        records = numeric_array_native_records()
        for record in records:
            if record.get("access_mode") == "dynamic_fallback":
                for fact in record.get("rejected_facts", []):
                    if fact.get("kind") == "raw_f64_layout":
                        fact["reason"] = None
                break
        report = HARNESS.verify_artifacts(
            workload="numeric_arrays",
            ir_before=ir,
            ir_after=ir,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0, "stdout_first": "25\n"}]},
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_required_numeric_array_push_dynamic_fallback" in error
                for error in report["errors"]
            ),
            report["errors"],
        )

    def test_generic_native_rep_checks_reject_unexpected_materialization(self):
        ir = "define i32 @main() { entry: ret i32 0 }\n"
        report = HARNESS.verify_artifacts(
            workload="scalar_replacement_literals",
            ir_before=ir,
            ir_after=ir,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0, "stdout_first": "17\n"}]},
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[
                {
                    "records": [
                        native_record(
                            function="perry_fn_scalarReplacementChecksum",
                            rep="js_value",
                            expr_kind="ScalarObjectLiteralInit",
                            consumer="scalar_object_field_store",
                            access_mode=None,
                            source_function="scalarReplacementChecksum",
                            materialization_reason="return_abi",
                        )
                    ]
                }
            ],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_no_unexpected_materialization_reasons" in error
                for error in report["errors"]
            )
        )

    def test_scoped_materialization_checks_ignore_out_of_region_records(self):
        workloads = {
            "scoped_materialization": {
                "native_rep_checks": {
                    "materialization_regions": ["input_generation"],
                    "allow_materialization_reasons": [],
                },
                "named_regions": [
                    {
                        "name": "input_generation",
                        "selectors": [{"label_prefix_any": ["for.body.20"]}],
                    }
                ],
            }
        }
        report = HARNESS.verify_artifacts(
            workload="scoped_materialization",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0}]},
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            workloads=workloads,
            native_reps=[
                {
                    "records": [
                        native_record(
                            block="entry.0",
                            rep="js_value",
                            materialization_reason="runtime_api",
                        )
                    ]
                }
            ],
        )
        self.assertEqual(report["status"], "pass", report["errors"])

    def test_scoped_materialization_checks_reject_in_region_records(self):
        workloads = {
            "scoped_materialization": {
                "native_rep_checks": {
                    "materialization_regions": ["input_generation"],
                    "allow_materialization_reasons": [],
                },
                "named_regions": [
                    {
                        "name": "input_generation",
                        "selectors": [{"label_prefix_any": ["for.body.20"]}],
                    }
                ],
            }
        }
        report = HARNESS.verify_artifacts(
            workload="scoped_materialization",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark={"runs": [{"exit_code": 0}]},
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            workloads=workloads,
            native_reps=[
                {
                    "records": [
                        native_record(
                            block="for.body.20",
                            rep="js_value",
                            materialization_reason="runtime_api",
                        )
                    ]
                }
            ],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_no_unexpected_materialization_reasons" in error
                for error in report["errors"]
            ),
            report["errors"],
        )

    def h1_alias_negative_records(self, length_records, mutated_records=None):
        alias_region = "h1_buffer_alias_negative_ts.aliaslocal.alias_local"
        reassignment_region = (
            "h1_buffer_alias_negative_ts.reassignment.reassignment_region"
        )
        unknown_call_region = (
            "h1_buffer_alias_negative_ts.unknowncallescape.unknown_call_escape"
        )
        mutated_for_region = (
            "h1_buffer_alias_negative_ts.mutatedforindex.mutated_for_index"
        )
        mutated_while_region = (
            "h1_buffer_alias_negative_ts.mutatedwhileindex.mutated_while_index"
        )
        stale_native_alias_region = (
            "h1_buffer_alias_negative_ts.stalenativealias.stale_native_alias"
        )
        stale_allocation_length_region = (
            "h1_buffer_alias_negative_ts.staleallocationlength.stale_allocation_length"
        )
        array_buffer_view_region = (
            "h1_buffer_alias_negative_ts.arraybufferviews.array_buffer_views"
        )
        if mutated_records is None:
            mutated_records = [
                native_record(
                    function="mutatedForIndex",
                    rep="i32",
                    region_id=mutated_for_region,
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexGet",
                    consumer="BufferIndexGet.slow_path_i32",
                ),
                native_record(
                    function="mutatedWhileIndex",
                    rep="i32",
                    region_id=mutated_while_region,
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexGet",
                    consumer="BufferIndexGet.slow_path_i32",
                ),
            ]
        records = [
            native_record(
                function="aliasLocal",
                rep="buffer_view",
                region_id=alias_region,
                bounds_state={"proven": {"proof": "loop_guard"}},
                alias_state="may_alias",
                access_mode="unchecked_native",
            ),
            native_record(
                function="reassignment",
                rep="i32",
                region_id=reassignment_region,
                bounds_state="unknown",
                access_mode="dynamic_fallback",
                expr_kind="BufferIndexGet",
                consumer="BufferIndexGet.slow_path_i32",
                materialization_reason="reassignment",
            ),
            native_record(
                function="unknownCallEscape",
                rep="buffer_view",
                region_id=unknown_call_region,
                bounds_state={"proven": {"proof": "loop_guard"}},
                alias_state="may_alias",
                access_mode="unchecked_native",
                materialization_reason="unknown_call_escape",
            ),
            native_record(
                function="closureCapture",
                rep="js_value",
                materialization_reason="closure_capture",
            ),
            native_record(
                function="sharedBacking",
                rep="i32",
                bounds_state="unknown",
                access_mode="dynamic_fallback",
                expr_kind="BufferIndexGet",
                consumer="BufferIndexGet.slow_path_i32",
            ),
            native_record(
                function="arrayBufferViews",
                rep="i32",
                region_id=array_buffer_view_region,
                bounds_state="unknown",
                access_mode="dynamic_fallback",
                expr_kind="Uint8ArrayGet",
                consumer="Uint8ArrayGet.slow_path_i32",
            ),
            native_record(
                function="staleNativeAlias",
                rep="i32",
                region_id=stale_native_alias_region,
                bounds_state="unknown",
                access_mode="dynamic_fallback",
                expr_kind="BufferIndexSet",
                consumer="BufferIndexSet.slow_path",
            ),
            native_record(
                function="staleAllocationLength",
                rep="i32",
                region_id=stale_allocation_length_region,
                bounds_state="unknown",
                access_mode="dynamic_fallback",
                expr_kind="BufferIndexSet",
                consumer="BufferIndexSet.slow_path",
            ),
            *length_records,
            *mutated_records,
        ]
        for record in records:
            if record.get("access_mode") != "dynamic_fallback":
                continue
            reason = record.get("materialization_reason") or "unknown_bounds"
            record["materialization_reason"] = reason
            record["fallback_reason"] = record.get("fallback_reason") or reason
            record["native_value_state"] = "dynamic_fallback"
            if record.get("bounds_state") is None or record.get("bounds_state") == "unknown":
                record.setdefault("rejected_facts", []).append(
                    native_fact("bounds", "missing", "unknown", reason)
                )
            if record.get("alias_state") in {"unknown", "may_alias", None}:
                record.setdefault("rejected_facts", []).append(
                    native_fact("alias_noalias", "missing", "unknown_or_may_alias", reason)
                )
            record.setdefault("rejected_facts", []).append(
                native_fact("materialization_hazard", "invalidated", str(reason), reason)
            )
        return records

    def test_length_mismatch_unchecked_unknown_bounds_fails_gate(self):
        length_region = "h1_buffer_alias_negative_ts.lengthmismatch.length_mismatch"
        records = self.h1_alias_negative_records(
            [
                native_record(
                    function="lengthMismatch",
                    rep="u8",
                    region_id=length_region,
                    bounds_state="unknown",
                    access_mode="unchecked_native",
                    expr_kind="BufferIndexSet",
                    consumer="u8_store_trunc_i32",
                )
            ]
        )
        report = HARNESS.verify_artifacts(
            workload="h1_buffer_alias_negative",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_negative_length_mismatch_no_unchecked_unknown" in error
                for error in report["errors"]
            )
        )

    def test_length_mismatch_dynamic_fallback_passes_gate(self):
        length_region = "h1_buffer_alias_negative_ts.lengthmismatch.length_mismatch"
        records = self.h1_alias_negative_records(
            [
                native_record(
                    function="lengthMismatch",
                    rep="i32",
                    region_id=length_region,
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexSet",
                    consumer="BufferIndexSet.slow_path",
                )
            ]
        )
        report = HARNESS.verify_artifacts(
            workload="h1_buffer_alias_negative",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "pass", report["errors"])

    def test_array_buffer_view_noalias_fails_gate(self):
        length_region = "h1_buffer_alias_negative_ts.lengthmismatch.length_mismatch"
        array_buffer_view_region = (
            "h1_buffer_alias_negative_ts.arraybufferviews.array_buffer_views"
        )
        records = self.h1_alias_negative_records(
            [
                native_record(
                    function="lengthMismatch",
                    rep="i32",
                    region_id=length_region,
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexSet",
                    consumer="BufferIndexSet.slow_path",
                )
            ]
        )
        records = [
            r
            for r in records
            if r.get("region_id") != array_buffer_view_region
        ]
        records.append(
            native_record(
                function="arrayBufferViews",
                rep="buffer_view",
                region_id=array_buffer_view_region,
                bounds_state={"proven": {"proof": "loop_guard"}},
                alias_state="no_alias_proven",
                access_mode="unchecked_native",
                emitted_noalias=True,
            )
        )

        report = HARNESS.verify_artifacts(
            workload="h1_buffer_alias_negative",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_negative_array_buffer_views_denies_noalias" in error
                for error in report["errors"]
            )
        )

    def test_array_buffer_view_raw_may_alias_passes_gate(self):
        length_region = "h1_buffer_alias_negative_ts.lengthmismatch.length_mismatch"
        array_buffer_view_region = (
            "h1_buffer_alias_negative_ts.arraybufferviews.array_buffer_views"
        )
        records = self.h1_alias_negative_records(
            [
                native_record(
                    function="lengthMismatch",
                    rep="i32",
                    region_id=length_region,
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexSet",
                    consumer="BufferIndexSet.slow_path",
                )
            ]
        )
        records = [
            r
            for r in records
            if r.get("region_id") != array_buffer_view_region
        ]
        records.extend(
            [
                native_record(
                    function="arrayBufferViews",
                    rep="buffer_view",
                    region_id=array_buffer_view_region,
                    bounds_state={"proven": {"proof": "loop_guard"}},
                    alias_state="may_alias",
                    access_mode="unchecked_native",
                ),
                native_record(
                    function="arrayBufferViews",
                    rep="u8",
                    region_id=array_buffer_view_region,
                    bounds_state={"proven": {"proof": "loop_guard"}},
                    alias_state="may_alias",
                    access_mode="unchecked_native",
                    expr_kind="Uint8ArrayGet",
                    consumer="u8_load_zext_i32",
                ),
            ]
        )

        report = HARNESS.verify_artifacts(
            workload="h1_buffer_alias_negative",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "pass", report["errors"])

    def test_mutated_for_index_unchecked_native_fails_gate(self):
        length_region = "h1_buffer_alias_negative_ts.lengthmismatch.length_mismatch"
        mutated_for_region = (
            "h1_buffer_alias_negative_ts.mutatedforindex.mutated_for_index"
        )
        records = self.h1_alias_negative_records(
            [
                native_record(
                    function="lengthMismatch",
                    rep="i32",
                    region_id=length_region,
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexSet",
                    consumer="BufferIndexSet.slow_path",
                )
            ],
            mutated_records=[
                native_record(
                    function="mutatedForIndex",
                    rep="u8",
                    region_id=mutated_for_region,
                    bounds_state={"proven": {"proof": "loop_guard"}},
                    access_mode="unchecked_native",
                    expr_kind="BufferIndexGet",
                    consumer="u8_load_zext_i32",
                ),
                native_record(
                    function="mutatedWhileIndex",
                    rep="i32",
                    region_id="h1_buffer_alias_negative_ts.mutatedwhileindex.mutated_while_index",
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexGet",
                    consumer="BufferIndexGet.slow_path_i32",
                ),
            ],
        )
        report = HARNESS.verify_artifacts(
            workload="h1_buffer_alias_negative",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_negative_mutated_for_index_no_unchecked_native" in error
                for error in report["errors"]
            )
        )

    def test_mutated_while_index_unchecked_native_fails_gate(self):
        length_region = "h1_buffer_alias_negative_ts.lengthmismatch.length_mismatch"
        mutated_while_region = (
            "h1_buffer_alias_negative_ts.mutatedwhileindex.mutated_while_index"
        )
        records = self.h1_alias_negative_records(
            [
                native_record(
                    function="lengthMismatch",
                    rep="i32",
                    region_id=length_region,
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexSet",
                    consumer="BufferIndexSet.slow_path",
                )
            ],
            mutated_records=[
                native_record(
                    function="mutatedForIndex",
                    rep="i32",
                    region_id="h1_buffer_alias_negative_ts.mutatedforindex.mutated_for_index",
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexGet",
                    consumer="BufferIndexGet.slow_path_i32",
                ),
                native_record(
                    function="mutatedWhileIndex",
                    rep="u8",
                    region_id=mutated_while_region,
                    bounds_state={"proven": {"proof": "loop_guard"}},
                    access_mode="unchecked_native",
                    expr_kind="BufferIndexGet",
                    consumer="u8_load_zext_i32",
                ),
            ],
        )
        report = HARNESS.verify_artifacts(
            workload="h1_buffer_alias_negative",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_negative_mutated_while_index_no_unchecked_native" in error
                for error in report["errors"]
            )
        )

    def test_stale_native_alias_unchecked_native_fails_gate(self):
        length_region = "h1_buffer_alias_negative_ts.lengthmismatch.length_mismatch"
        stale_native_alias_region = (
            "h1_buffer_alias_negative_ts.stalenativealias.stale_native_alias"
        )
        records = self.h1_alias_negative_records(
            [
                native_record(
                    function="lengthMismatch",
                    rep="i32",
                    region_id=length_region,
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexSet",
                    consumer="BufferIndexSet.slow_path",
                )
            ]
        )
        records = [
            r
            for r in records
            if r.get("region_id") != stale_native_alias_region
        ]
        records.append(
            native_record(
                function="staleNativeAlias",
                rep="u8",
                region_id=stale_native_alias_region,
                bounds_state={"proven": {"proof": "loop_guard"}},
                access_mode="unchecked_native",
                expr_kind="BufferIndexSet",
                consumer="u8_store_trunc_i32",
                emitted_inbounds=True,
            )
        )
        report = HARNESS.verify_artifacts(
            workload="h1_buffer_alias_negative",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_negative_stale_native_alias_no_unchecked_or_native_claims"
                in error
                for error in report["errors"]
            )
        )

    def test_stale_allocation_length_inbounds_fails_gate(self):
        length_region = "h1_buffer_alias_negative_ts.lengthmismatch.length_mismatch"
        stale_allocation_length_region = (
            "h1_buffer_alias_negative_ts.staleallocationlength.stale_allocation_length"
        )
        records = self.h1_alias_negative_records(
            [
                native_record(
                    function="lengthMismatch",
                    rep="i32",
                    region_id=length_region,
                    bounds_state="unknown",
                    access_mode="dynamic_fallback",
                    expr_kind="BufferIndexSet",
                    consumer="BufferIndexSet.slow_path",
                )
            ]
        )
        records = [
            r
            for r in records
            if r.get("region_id") != stale_allocation_length_region
        ]
        records.append(
            native_record(
                function="staleAllocationLength",
                rep="u8",
                region_id=stale_allocation_length_region,
                bounds_state={"proven": {"proof": "loop_guard"}},
                access_mode="dynamic_fallback",
                expr_kind="BufferIndexSet",
                consumer="u8_store_trunc_i32",
                emitted_inbounds=True,
            )
        )
        report = HARNESS.verify_artifacts(
            workload="h1_buffer_alias_negative",
            ir_before=GOOD_IR,
            ir_after=GOOD_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_negative_stale_allocation_length_no_unchecked_or_native_claims"
                in error
                for error in report["errors"]
            )
        )

    def test_native_region_materialization_fails_gate(self):
        direct_region = (
            "h1_native_rep_equivalence_ts.module_init.direct_bounded"
        )
        records = [
            native_record(rep="i32", region_id=direct_region),
            native_record(
                rep="buffer_view",
                region_id=direct_region,
                bounds_state={"proven": {"proof": "min_length"}},
            ),
            native_record(
                rep="u8",
                region_id=direct_region,
                consumer="u8_load_zext_i32",
                bounds_state={"proven": {"proof": "min_length"}},
            ),
            native_record(
                rep="js_value",
                region_id=direct_region,
                consumer="materialize_js_value",
                materialization_reason="function_abi",
            ),
        ]
        report = HARNESS.verify_artifacts(
            workload="h1_native_rep_equivalence",
            ir_before=H1_MIN_IR,
            ir_after=H1_MIN_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any("native_reps_direct_bounded_no_materialization" in error for error in report["errors"])
        )

    def test_h1_native_rep_equivalence_consumed_facts_pass_gate(self):
        report = HARNESS.verify_artifacts(
            workload="h1_native_rep_equivalence",
            ir_before=H1_MIN_IR,
            ir_after=H1_MIN_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": h1_equivalence_native_records()}],
        )
        self.assertEqual(report["status"], "pass", report["errors"])

    def test_h1_native_rep_equivalence_requires_consumed_facts(self):
        records = h1_equivalence_native_records()
        direct_region = "h1_native_rep_equivalence_ts.module_init.direct_bounded"
        for record in records:
            if record.get("region_id") == direct_region:
                record["consumed_facts"] = []
        report = HARNESS.verify_artifacts(
            workload="h1_native_rep_equivalence",
            ir_before=H1_MIN_IR,
            ir_after=H1_MIN_IR,
            assembly=GOOD_ASM,
            benchmark=None,
            vectorization={"vectorized_count": 0, "missed_count": 0, "analysis_count": 0},
            native_reps=[{"records": records}],
        )
        self.assertEqual(report["status"], "fail")
        self.assertTrue(
            any(
                "native_reps_direct_bounded_consumes_representation_facts" in error
                for error in report["errors"]
            ),
            report["errors"],
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

    def test_loop_data_dependent_allows_setup_conversion_only(self):
        ir = """
define double @main() {
entry:
  br label %for.body.2
for.body.2:
  %setup = sitofp i32 %seed to double
  br label %for.body.11
for.body.11:
  %mul = fmul double %sum, %x
  %add = fadd double %mul, %y
  br label %exit
exit:
  ret double %add
}
"""
        report = HARNESS.verify_artifacts(
            workload="loop_data_dependent",
            ir_before=ir,
            ir_after=ir,
            assembly="main:\n  retq\n",
            benchmark=None,
            vectorization={
                "vectorized_count": 0,
                "missed_count": 0,
                "analysis_count": 0,
                "missed_reason_kinds": {},
            },
            target="x86_64-unknown-linux-gnu",
            native_reps=[{"records": loop_data_dependent_native_records()}],
        )
        self.assertEqual(report["status"], "pass", report["errors"])
        self.assertTrue(
            any(
                check["name"] == "named_region_numeric_loop_no_fp_int_conversions"
                and check["status"] == "pass"
                for check in report["checks"]
            )
        )


if __name__ == "__main__":
    unittest.main()
