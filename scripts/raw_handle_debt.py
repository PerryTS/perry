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

FILES = ROOT / "scripts" / "raw_handle_debt_files.txt"


def load_ceilings():
    """`{path: ceiling}` from the per-module file. Comments and blanks ignored."""
    out = {}
    for line in FILES.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        n, path = line.split(None, 1)
        out[path.strip()] = int(n)
    return out


def check_per_module(per_file):
    """Per-module rules. Returns a list of human-readable violations.

    All three directions are closed, and the third is the one that makes the
    list shrink rather than drift: an entry that no longer matches anything is
    a FAILURE, exactly as in `gc_root_dominance_allowlist.json`. Without it a
    cleaned module keeps its line forever and quietly re-permits the debt the
    next time someone edits that file.
    """
    ceilings = load_ceilings()
    bad = []
    for path, n in sorted(per_file.items()):
        if path not in ceilings:
            bad.append(
                f"{path}: {n} bare read(s) in a module with no ceiling. New code must "
                f"use RuntimeHandle::across_{{mut,const,nanbox}}; see #7341."
            )
        elif n > ceilings[path]:
            bad.append(f"{path}: {n} bare reads exceeds its ceiling of {ceilings[path]}")
    for path, ceiling in sorted(ceilings.items()):
        if path not in per_file:
            bad.append(
                f"{path}: ceiling of {ceiling} matches nothing -- the module is clean "
                f"(or gone). DELETE its line so the cleanup cannot be undone."
            )
    return bad


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
    # The per-module rules are what make the discipline non-optional, so each
    # of the three directions is asserted rather than trusted. A rule that
    # silently stops firing is worse than no rule: it reads as "every other
    # module is locked at zero" while locking nothing.
    saved = globals()["load_ceilings"]
    globals()["load_ceilings"] = lambda: {"a.rs": 2, "gone.rs": 1}
    try:
        checks = [
            ("unlisted module carrying debt", {"a.rs": 2, "new.rs": 1}, "no ceiling"),
            ("listed module over its ceiling", {"a.rs": 3, "gone.rs": 1}, "exceeds its ceiling"),
            ("cleaned module still listed", {"a.rs": 2}, "matches nothing"),
        ]
        for label, per, needle in checks:
            if not any(needle in v for v in check_per_module(per)):
                print(f"self-test FAILED: rule did not fire: {label}")
                return 1
        if check_per_module({"a.rs": 2, "gone.rs": 1}):
            print("self-test FAILED: the clean case reported a violation")
            return 1
    finally:
        globals()["load_ceilings"] = saved

    print(f"self-test ok ({total} sites across {len(per_file)} files); "
          f"all three per-module rules fire, clean case silent")
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
        # Rewrite the per-module ceilings too, preserving the header. Entries
        # that reached zero simply do not come back -- rule 3.
        header = []
        for line in FILES.read_text(encoding="utf-8").splitlines():
            if line.startswith("#") or not line.strip():
                header.append(line)
            else:
                break
        FILES.write_text(
            "\n".join(header) + "\n"
            + "".join(f"{n} {p}\n" for p, n in sorted(per_file.items()))
        )
        print(f"baseline set to {total}" + (f" (was {prev})" if prev is not None else ""))
        print(f"per-module ceilings rewritten: {len(per_file)} entries")
        return 0
    if not BASELINE.exists():
        print(f"no baseline; run --update. current={total}")
        return 1
    prev = int(BASELINE.read_text().split()[0])
    print(f"bare raw-handle reads: {total} (baseline {prev})")

    # Per-module rules run FIRST and unconditionally. They are strictly more
    # specific than the total -- "symbol.rs exceeds its ceiling of 1" names the
    # file and the fix, where "the total rose" does not -- and if the total
    # check returned first the specific diagnostic would never be printed for
    # the most common failure (a module gaining a read).
    module_violations = check_per_module(per_file)
    if module_violations:
        print(f"::error::per-module raw-handle rules: {len(module_violations)} violation(s)")
        for b in module_violations:
            print(f"  {b}")
        print("Use RuntimeHandle::across_{mut,const,nanbox} -- it runs the")
        print("allocating call and returns the post-collection address, so the")
        print("stale pointer is never bound. See #7341 and the header of")
        print("scripts/raw_handle_debt_files.txt.")
        return 1

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

    print(f"per-module: {len(per_file)} module(s) within ceilings; every other "
          f"runtime module is locked at zero")
    return 0

if __name__ == "__main__":
    sys.exit(main())
