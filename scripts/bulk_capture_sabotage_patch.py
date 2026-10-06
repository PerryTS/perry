#!/usr/bin/env python3
"""Emit one capture-birth mutant; apply in a disposable copy and rebuild.

Named tests must pass before mutation and fail at an assertion afterward:
no-refresh -> bulk_boxed_birth_survives_collection_inside_allocation and
              plain_bulk_birth_refreshes_tagged_captures_inside_allocation;
no-layout -> bulk_boxed_birth_survives_collection_inside_allocation;
no-barrier -> old_arena_bulk_capture_install_remembers_young_box and
              bulk_boxed_birth_shades_captures_during_incremental_marking;
strip-flags -> bulk_boxed_birth_preserves_body_shape_flags_and_fresh_identity;
wrong-root-base -> heap_word_iterator_reads_refreshed_slots_after_stack_growth;
no-boxed-bulk -> escaped_boxed_closure_birth_uses_rooted_bulk_initializer.

Compile errors do not prove a mutant was caught. Restore this exact patch
before applying another one. The emitter never modifies the source tree.
"""
import argparse
import difflib
from pathlib import Path
import sys


def replace_one(text, before, after):
    if text.count(before) != 1:
        raise RuntimeError(f"anchor must occur exactly once: {before!r}")
    return text.replace(before, after, 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=["no-refresh", "no-layout", "no-barrier",
                                         "strip-flags", "wrong-root-base", "no-boxed-bulk"])
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    path = "crates/perry-runtime/src/closure/alloc.rs"
    if args.kind == "wrong-root-base":
        path = "crates/perry-runtime/src/gc/roots/runtime_handles.rs"
    elif args.kind == "no-boxed-bulk":
        path = "crates/perry-codegen/src/expr/closure.rs"
    original = (args.repo / path).read_text()
    updated = original
    if args.kind == "no-refresh":
        updated = replace_one(updated,
            "closure_install_fresh_capture_words(closure, rooted, boxed)",
            "closure_install_fresh_capture_words(closure, values.iter().copied(), boxed)")
    elif args.kind == "no-layout":
        updated = replace_one(updated,
            "            crate::gc::layout_init_unknown_fresh(closure as *mut u8);",
            "            // Mutant: keep the pointer-free birth layout.")
    elif args.kind == "no-barrier":
        updated = replace_one(updated,
            "    if pointer_layout {\n        if crate::gc::newborn_parent_needs_barrier(closure as usize) {",
            "    if pointer_layout {\n        if false {")
    elif args.kind == "strip-flags":
        start = updated.index("pub extern \"C\" fn js_closure_alloc_init_boxed(")
        end = updated.index("\n#[cold]", start)
        region = replace_one(updated[start:end],
            "(*closure).capture_count = capture_count;",
            "(*closure).capture_count = real_capture_count(capture_count);")
        updated = updated[:start] + region + updated[end:]
    elif args.kind == "wrong-root-base":
        updated = replace_one(updated, "        let start = self.stack.len();",
                               "        let start = self.base;")
    else:
        updated = replace_one(updated,
            "                && !captured_value_bits.is_empty();",
            "                && !captured_value_bits.is_empty()\n"
            "                && auto_captures.iter().all(|cap_id| {\n"
            "                    !ctx.boxed_vars.contains(cap_id) || uncounted_box_capture(cap_id)\n"
            "                });")
    sys.stdout.writelines(difflib.unified_diff(
        original.splitlines(True), updated.splitlines(True),
        fromfile="a/" + path, tofile="b/" + path))


if __name__ == "__main__":
    main()
