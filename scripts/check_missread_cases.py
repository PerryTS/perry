#!/usr/bin/env python3
"""Node differentials for the missread mechanisms; run on the lane Linux host."""
import argparse
from pathlib import Path
from perf_missread_lane import environment, run, NODE

CASES = [
    "benchmarks/read_miss_shape.ts",
    "test-files/test_gap_missread_shape_authority.ts",
    "test-files/test_gap_prototype_in_shape.ts",
    "test-files/test_gap_10497_dispatch_function_prototype_mutation.ts",
    "test-files/test_gap_class_static_attrs_delete.ts",
    "test-files/test_gap_function_deleted_method.ts",
    "test-files/test_gap_array_named_descriptor_forwarding.ts",
    "test-files/test_gap_iterator_prototype_next_patch.ts",
    "test-files/test_gap_typed_array_proto_methods.ts",
    "test-files/test_gap_dynamic_key_read_paths.ts",
]

p = argparse.ArgumentParser()
p.add_argument("--hostdir", type=Path, required=True)
p.add_argument("--arm", choices=["main", "head"], required=True)
p.add_argument("--stress", action="store_true")
opts = p.parse_args()
root = opts.hostdir
_, target, env = environment(root, opts.arm)
env["PERRY_NO_AUTO_OPTIMIZE"] = "1"
out = root / "cases" / opts.arm
out.mkdir(parents=True, exist_ok=True)
for case in CASES:
    name = Path(case).stem
    binary = out / name
    run([str(target / "release/perry"), "compile", case, "-o", str(binary)],
        root / "src", env, out / f"{name}.compile")
    expected = run([*NODE, case], root / "src", env, out / f"{name}.node")
    actual = run([str(binary)], root / "src", env, out / f"{name}.perry")
    if actual != expected:
        raise RuntimeError(f"{opts.arm}/{name}: output differs from Node")
    print(f"{opts.arm}/{name}: PASS", flush=True)
    # These four cases reach allocation-capable back edges. Linear fixtures
    # cannot satisfy the rate-1 loop-coverage witness and remain Node checks.
    if opts.stress and case in CASES[:4]:
        stress = dict(env, PERRY_GC_SCHEDULE_SEED="20261009", PERRY_GC_SCHEDULE_RATE="1",
                      PERRY_GC_PROTECT_FROMSPACE="1", PERRY_GC_VERIFY_MARK="1", PERRY_GC_DIAG="1")
        actual = run([str(binary)], root / "src", stress, out / f"{name}.stress")
        if actual != expected:
            raise RuntimeError(f"{opts.arm}/{name}: stress output differs from Node")
        print(f"{opts.arm}/{name}: stress PASS", flush=True)
