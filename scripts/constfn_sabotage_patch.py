#!/usr/bin/env python3
"""Emit a targeted mutant patch; apply only in a separate disposable worktree.

After rebuilding the identical three-package set, the named gate must fail:
held-closure -> executable factory distinct captures/output;
drop-worker-info -> constfn_transfer_production_worker_seed...;
skip-transfer -> constfn_transfer_seed_first...;
ignore-deprecation -> constfn_transfer_does_not_resurrect...;
unsafe-proven-store -> executable classes resetMethod output/field-rep verifier.
Use `git apply --reverse` to restore this exact patch before the next mutant.
"""
import argparse
import difflib
from pathlib import Path
import sys


def replace_one(text, before, after):
    if text.count(before) != 1:
        raise RuntimeError(f"anchor must occur exactly once: {before[:80]!r}")
    return text.replace(before, after, 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=["held-closure", "drop-worker-info", "skip-transfer", "ignore-deprecation", "unsafe-proven-store"])
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    patches = {}
    if args.kind == "held-closure":
        path = "crates/perry-runtime/src/object/method_site.rs"
        text = (args.repo / path).read_text()
        start = text.index("        let entry = MethodEntry {", text.index("// The shape, not this closure object"))
        end = text.index("        if publish(slot, entry)", start)
        region = text[start:end]
        changed = replace_one(region, "            closure: 0,", "            closure: value & crate::value::POINTER_MASK,")
        patches[path] = text[:start] + changed + text[end:]
        path = "crates/perry-codegen/src/expr/method_site.rs"
        text = (args.repo / path).read_text()
        start = text.index("    ctx.current_block = constfn_idx;")
        end = text.index("    // own spill:", start)
        region = text[start:end]
        changed = replace_one(region, "        let h = emit_handle(blk, &ub);", "        let cp = blk.gep(crate::types::I8, &entry, &[(I64, &abi_closure)]);\n        let h = blk.load(I64, &cp);\n        let _ = ub;")
        patches[path] = text[:start] + changed + text[end:]
    elif args.kind == "drop-worker-info":
        path = "crates/perry-runtime/src/object/shapes_worker_seed.rs"
        patches[path] = replace_one((args.repo / path).read_text(), "infos: r.constfn_infos().to_vec(),", "infos: Vec::new(),")
    elif args.kind == "skip-transfer":
        path = "crates/perry-runtime/src/thread.rs"
        patches[path] = replace_one((args.repo / path).read_text(), "                constfn_transfer::restore(obj, *class_id, fields.len(), names, facts)", "                let _ = (names, facts);\n                obj")
    elif args.kind == "ignore-deprecation":
        path = "crates/perry-runtime/src/object/static_shapes.rs"
        patches[path] = replace_one((args.repo / path).read_text(), "                    && d.deprecation_targets() == (0, 0)", "                    && true")
    else:
        path = "crates/perry-codegen/src/expr/property_set.rs"
        text = (args.repo / path).read_text()
        start = text.index("                                let ptr_shape_proven = ptr_shape_proven")
        end = text.index("                                if ptr_shape_proven", start)
        patches[path] = text[:start] + text[end:]
    for path, updated in patches.items():
        original = (args.repo / path).read_text()
        sys.stdout.writelines(difflib.unified_diff(original.splitlines(True), updated.splitlines(True), fromfile="a/" + path, tofile="b/" + path))


if __name__ == "__main__":
    main()
