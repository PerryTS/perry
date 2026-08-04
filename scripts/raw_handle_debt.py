#!/usr/bin/env python3
"""Ratchet the number of bare raw-pointer reads out of GC root handles.

A `RuntimeHandleScope` gives an object liveness -- the collector marks it and
rewrites the slot. It does nothing for a raw pointer already read out of that
slot: that copy is invisible to the collector, and if the object moves it names
from-space. Every rooting bug fixed in the #7341 quarantine sweep had rooting
ALREADY; what was missing was ordering the re-read against the collection point.

`RuntimeHandle::across_{mut,const,nanbox}` expresses that ordering in one call
and never binds the pre-call address. Each bare `get_raw_*_ptr` is a site where
that ordering is a review question instead of a shape.

This is a DEBT COUNTER, not a soundness proof. Rust has no effect system to mark
"this call may allocate", so no signature can reject holding a stale copy. Not
every bare read is a bug -- many are the final read in a scope with nothing
after them. The number is meaningful because it can only be paid down.

Usage:
    scripts/raw_handle_debt.py            # report, fail if above the baseline
    scripts/raw_handle_debt.py --update   # rewrite the baseline (must go DOWN)
"""
import re, sys, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "crates" / "perry-runtime" / "src"
BASELINE = ROOT / "scripts" / "raw_handle_debt_baseline.txt"
PAT = re.compile(r"\.get_raw_(?:mut|const)_ptr\b")

# The accessors and the `across_*` combinators are DEFINED here and call each
# other; counting this file would make the ratchet count its own implementation
# and rise every time a combinator is added. Exclude it.
EXCLUDE = {"crates/perry-runtime/src/gc/roots/runtime_handles.rs"}

def count():
    total, per_file = 0, {}
    for f in sorted(SRC.rglob("*.rs")):
        rel = str(f.relative_to(ROOT))
        if rel in EXCLUDE:
            continue
        n = len(PAT.findall(f.read_text(encoding="utf-8", errors="replace")))
        if n:
            per_file[rel] = n
            total += n
    return total, per_file

def self_test():
    """Guard the gate against its own regressions.

    A ratchet whose matcher silently stops matching reports 0 and passes
    forever. Assert the pattern still fires on the shapes it exists to count,
    and still ignores the combinator that replaces them.
    """
    must_match = [
        "let obj = obj_h.get_raw_mut_ptr::<ObjectHeader>();",
        "src_h.get_raw_const_ptr::<u8>()",
    ]
    must_not_match = [
        "let (found, obj) = h.across_mut::<ObjectHeader, _>(|| f());",
        "h.across_const::<ObjectHeader, _>(|| g())",
        "h.get_nanbox_f64()",
    ]
    for line in must_match:
        if not PAT.search(line):
            print(f"self-test FAILED: pattern no longer matches: {line}")
            return 1
    for line in must_not_match:
        if PAT.search(line):
            print(f"self-test FAILED: pattern wrongly matches: {line}")
            return 1
    if not SRC.is_dir():
        print(f"self-test FAILED: source tree missing at {SRC}")
        return 1
    total, per_file = count()
    if total == 0 or not per_file:
        print("self-test FAILED: counted zero sites -- the walk is broken")
        return 1
    print(f"self-test ok ({total} sites across {len(per_file)} files)")
    return 0

def main():
    if "--self-test" in sys.argv:
        return self_test()
    total, per_file = count()
    if "--update" in sys.argv:
        prev = int(BASELINE.read_text().split()[0]) if BASELINE.exists() else None
        if prev is not None and total > prev:
            print(f"refusing to raise the baseline: {prev} -> {total}")
            print("the ratchet only goes down; convert sites to across_* instead")
            return 1
        BASELINE.write_text(f"{total}\n")
        print(f"baseline set to {total}" + (f" (was {prev})" if prev is not None else ""))
        return 0
    if not BASELINE.exists():
        print(f"no baseline; run --update. current={total}")
        return 1
    prev = int(BASELINE.read_text().split()[0])
    print(f"bare raw-handle reads: {total} (baseline {prev})")
    if total > prev:
        print(f"::error::raw-handle debt rose {prev} -> {total}")
        print("Use RuntimeHandle::across_{mut,const,nanbox} -- it runs the")
        print("allocating call and returns the post-collection address, so the")
        print("stale pointer is never bound. See #7341.")
        for path, n in sorted(per_file.items(), key=lambda kv: -kv[1])[:10]:
            print(f"  {n:4d}  {path}")
        return 1
    if total < prev:
        print(f"debt fell by {prev - total}; run --update to lock it in")
    return 0

if __name__ == "__main__":
    sys.exit(main())
