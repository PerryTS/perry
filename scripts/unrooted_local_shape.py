#!/usr/bin/env python3
"""Report locals that hold a raw heap pointer across a collection point.

`scripts/raw_handle_debt.py` counts bare reads OUT OF a `RuntimeHandle` -- i.e.
debt in code that already adopted the rooting API and then degraded a use. Code
that never roots at all has no `get_raw_*_ptr` to count and scores ZERO, the
ratchet's best possible result. Its scope is `perry-runtime` only, so
`perry-stdlib` and every `perry-ext-*` crate sit outside the denominator
entirely (#8233).

This instrument detects the SHAPE instead of the API misuse:

    let state   = js_array_alloc(6);                    // binds a raw pointer
    let buffer  = js_array_alloc(0);                    // may move `state`
    let _ = js_array_push_f64(state, ...);              // `state` is stale

A local bound from an allocator return, used again after an intervening call
that can allocate or run JS. That is the #8217 / #8163 shape, and neither the
LLVM-IR checker (blind to Rust locals) nor the root-holder census (enumerates
`static`s) can see it.

This is a REPORT, not a proof. Rust has no effect system marking "this call may
allocate", so the collection-point list is a curated denylist and the binding
detection is line-order over source text. Expect false positives where the
allocation provably cannot trigger a collection, and false negatives wherever a
pointer flows through a shape this does not spell. The number is useful as an
EXPOSURE SURFACE -- how much of the surface no instrument is watching -- not as
a bug count.

Per CLAUDE.md, a new gate has never been green, so this ships as a report and
`--check` compares against a recorded baseline rather than demanding zero.

Usage:
    scripts/unrooted_local_shape.py                 # report
    scripts/unrooted_local_shape.py --check         # fail if above baseline
    scripts/unrooted_local_shape.py --update-baseline
    scripts/unrooted_local_shape.py --self-test     # prove it can still fail
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "scripts" / "unrooted_local_shape_baseline.json"

# Crate families outside `raw_handle_debt.py`'s scope -- the whole point.
SCAN_GLOBS = (
    "crates/perry-stdlib/src/**/*.rs",
    "crates/perry-ext-*/src/**/*.rs",
)

# Calls that RETURN a raw heap pointer into a local. Binding one of these is
# what puts an unrooted address in a frame slot.
ALLOCATORS = (
    "js_array_alloc",
    "js_object_alloc",
    "js_closure_alloc",
    "js_map_alloc",
    "js_set_alloc",
    "js_string_from_bytes",
    "js_string_alloc",
    "alloc_string",
    "alloc_buffer",
    "js_buffer_alloc",
    "js_typed_array_alloc",
)

# Calls that can allocate or run user JS, i.e. can move the heap. Deliberately
# conservative: every entry either allocates outright or can re-enter the
# interpreter. `js_nanbox_*` and pure predicates are NOT here -- they cannot
# collect, and including them would drown the report.
COLLECTION_POINTS = ALLOCATORS + (
    "js_array_push",
    "js_array_set",
    "js_object_set_field",
    "js_object_set_property",
    "js_map_set",
    "js_set_add",
    "js_closure_call",
    "js_call_function",
    "js_invoke",
    "js_string_concat",
    "js_to_string",
    "js_jsvalue_to_string",
    "js_throw",
    "gc(",
)

# A function that opens a handle scope has adopted the rooting API; its bindings
# are `raw_handle_debt.py`'s denominator, not this one's.
ROOTED_MARKERS = ("RuntimeHandleScope", "across_mut", "across_const", "across_nanbox")

FN_START = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*fn\s+(\w+)")
LET_BIND = re.compile(r"^\s*let\s+(?:mut\s+)?(\w+)\s*(?::[^=]+)?=\s*(.+)$")
IDENT = re.compile(r"\b\w+\b")


def strip_comments(text: str) -> list[str]:
    """Blank out // comments and string literals, preserving line numbering."""
    out = []
    in_block = False
    for line in text.split("\n"):
        if in_block:
            if "*/" in line:
                line = line.split("*/", 1)[1]
                in_block = False
            else:
                out.append("")
                continue
        if "/*" in line and "*/" not in line:
            line = line.split("/*", 1)[0]
            in_block = True
        line = re.sub(r"//.*$", "", line)
        line = re.sub(r'"(?:[^"\\]|\\.)*"', '""', line)
        out.append(line)
    return out


def split_functions(lines: list[str]):
    """Yield (name, start_index, end_index) by brace balance from each `fn`."""
    i = 0
    while i < len(lines):
        m = FN_START.match(lines[i])
        if not m:
            i += 1
            continue
        depth = 0
        seen_open = False
        j = i
        while j < len(lines):
            depth += lines[j].count("{") - lines[j].count("}")
            if "{" in lines[j]:
                seen_open = True
            if seen_open and depth <= 0:
                break
            j += 1
        yield m.group(1), i, min(j, len(lines) - 1)
        i = j + 1


def calls_any(line: str, names) -> bool:
    return any(n in line for n in names)


def scan_function(name: str, lines: list[str], start: int, end: int):
    """Return findings: (line_no, local, binding_line_no, collection_line_no)."""
    body = lines[start : end + 1]
    if any(marker in "\n".join(body) for marker in ROOTED_MARKERS):
        return []

    bound: dict[str, int] = {}
    findings = []
    for offset, line in enumerate(body):
        m = LET_BIND.match(line)
        if m and calls_any(m.group(2), ALLOCATORS):
            bound[m.group(1)] = offset
            continue
        if not bound:
            continue
        # A line that can collect invalidates every local bound before it.
        if calls_any(line, COLLECTION_POINTS):
            used_here = {t for t in IDENT.findall(line) if t in bound}
            for local in sorted(used_here):
                # Used ON the collecting line itself, after an earlier one.
                for other, other_off in bound.items():
                    if other_off < bound[local] or other == local:
                        continue
                for prev_off in range(bound[local] + 1, offset):
                    if calls_any(body[prev_off], COLLECTION_POINTS):
                        findings.append(
                            (start + offset + 1, local, start + bound[local] + 1, start + prev_off + 1)
                        )
                        break
    return findings


def scan_file(path: Path):
    lines = strip_comments(path.read_text(encoding="utf-8", errors="replace"))
    out = []
    for name, start, end in split_functions(lines):
        for finding in scan_function(name, lines, start, end):
            out.append((name,) + finding)
    return out


def collect(root: Path = ROOT):
    results = {}
    for glob in SCAN_GLOBS:
        for path in sorted(root.glob(glob)):
            hits = scan_file(path)
            if hits:
                results[str(path.relative_to(root))] = hits
    return results


SELF_TEST_SRC = '''
unsafe fn planted() -> *mut ArrayHeader {
    let state = js_array_alloc(6);
    let buffer = js_array_alloc(0);
    let _ = js_array_push_f64(state, js_nanbox_pointer(buffer as i64));
    state
}

unsafe fn clean_single_alloc() -> *mut ArrayHeader {
    let only = js_array_alloc(1);
    only
}
'''


def self_test() -> int:
    """Prove the detector can still fail: a planted site must be found, and a
    single-allocation function must NOT be."""
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        p = Path(tmp) / "planted.rs"
        p.write_text(SELF_TEST_SRC)
        hits = scan_file(p)
    names = {h[0] for h in hits}
    ok = True
    if "planted" not in names:
        print("SELF-TEST FAIL: did not flag the planted stale-local site", file=sys.stderr)
        ok = False
    if "clean_single_alloc" in names:
        print("SELF-TEST FAIL: flagged a function with no intervening collection point", file=sys.stderr)
        ok = False
    stale = [h for h in hits if h[0] == "planted"]
    if ok:
        print(f"self-test OK: planted site flagged ({len(stale)} finding(s)), clean function not flagged")
        return 0
    return 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="fail if the count exceeds the baseline")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--verbose", action="store_true", help="list every finding")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    results = collect()
    total = sum(len(v) for v in results.values())

    per_file = sorted(((len(v), k) for k, v in results.items()), reverse=True)
    print(f"unrooted-local shape: {total} finding(s) across {len(results)} file(s)")
    print("(exposure surface, not a bug count -- see the module docstring)")
    for count, path in per_file[:20]:
        print(f"  {count:4d}  {path}")
    if len(per_file) > 20:
        print(f"  ... and {len(per_file) - 20} more file(s)")

    if args.verbose:
        for path, hits in sorted(results.items()):
            for fn, use_line, local, bind_line, collect_line in hits:
                print(f"  {path}:{use_line}: `{local}` bound at :{bind_line}, may have moved at :{collect_line} (fn {fn})")

    if args.update_baseline:
        BASELINE.write_text(json.dumps({"total": total, "per_file": {k: len(v) for k, v in results.items()}}, indent=2, sort_keys=True) + "\n")
        print(f"wrote {BASELINE.relative_to(ROOT)}")
        return 0

    if args.check:
        if not BASELINE.exists():
            print("no baseline recorded; run --update-baseline first", file=sys.stderr)
            return 1
        base = json.loads(BASELINE.read_text())
        if total > base["total"]:
            print(f"REGRESSION: {total} findings exceeds baseline {base['total']}", file=sys.stderr)
            return 1
        if total < base["total"]:
            print(f"improved: {total} < baseline {base['total']} -- run --update-baseline to ratchet")
        print("OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
