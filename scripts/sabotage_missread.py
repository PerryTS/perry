#!/usr/bin/env python3
"""Linux lane only: break each proof, require its unit witness to fail, restore.

Run after capturing accepted standalone program binaries. This deliberately
rebuilds the lane's test executable; it never changes the reference arm.
"""
import argparse
import os
from pathlib import Path
import subprocess

p = argparse.ArgumentParser()
p.add_argument("--hostdir", type=Path, required=True)
opts = p.parse_args()
root = opts.hostdir.resolve()
src = root / "src"
runtime = src / "crates/perry-runtime/src/object"
faults = [
    ("native_get.rs", "own_absent = lookup == Some(None);", "own_absent = false;",
     "an_own_absence_proof_is_not_searched_again_before_the_holder"),
    ("method_site/read_holder/function_own.rs",
     "bag.is_null() || object_shape_stamp(bag) != entry[HOLDER_SHAPE] as u32",
     "bag.is_null()", "own_function_bags_load_the_current_receiver_and_guard_every_shape"),
    ("object_ops/prototype.rs",
     "let record = s::shape_record_by_id(s::object_shape_stamp(obj))?;",
     "return None;\n    let record = s::shape_record_by_id(s::object_shape_stamp(obj))?;",
     "ordinary_shape_links_are_complete_and_exotic_links_decline"),
    ("shapes.rs",
     "crate::object::is_anon_shape_class_id(class_id)\n        && !crate::object::class_registry::declared_class_outranks_anon_shape(class_id)",
     "crate::object::is_anon_shape_class_id(class_id)",
     "declaration_collision_is_projected_when_the_shape_is_minted"),
]
originals = {}
try:
    for relative, before, after, _ in faults:
        path = runtime / relative
        original = path.read_text()
        if original.count(before) != 1:
            raise RuntimeError(f"sabotage anchor changed: {relative}")
        originals[path] = original
        path.write_text(original.replace(before, after, 1))
    for _, _, _, witness in faults:
        log = root / f"sabotage-{witness}.log"
        with log.open("w") as stream:
            result = subprocess.run(
                ["taskset", "-c", "0-55", "cargo", "test", "--release", "-j8",
                 "-p", "perry-runtime", "--lib", witness], cwd=src,
                env=dict(os.environ, RUST_TEST_THREADS="1"), stdout=stream,
                stderr=subprocess.STDOUT, timeout=1800)
        evidence = log.read_text()
        if result.returncode == 0 or "running 1 test" not in evidence or "FAILED" not in evidence:
            raise RuntimeError(f"witness did not detect sabotage: {witness}; inspect {log}")
        print(f"DETECTED: {witness}", flush=True)
finally:
    for path, original in originals.items():
        path.write_text(original)
    print("Restored accepted runtime sources", flush=True)
