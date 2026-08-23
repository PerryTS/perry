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
    consume(state);                                     // `state` is stale

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
    scripts/unrooted_local_shape.py --no-raise-vs <ref>
    scripts/unrooted_local_shape.py --self-test     # prove it can still fail
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "scripts" / "unrooted_local_shape_baseline.json"
BASELINE_SCHEMA = 2

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

# Pure pointer extractions do not collect, but they put the same invisible raw
# address in a local as an allocator return does. #8233 explicitly names this
# half of the shape; omitting it would leave the original denominator gap open
# for every pointer that entered the function as a NaN-boxed value.
POINTER_EXTRACTORS = (
    "js_nanbox_get_pointer",
    "js_nanbox_get_string_pointer",
    "js_nanbox_get_bigint",
    "js_get_string_pointer_unified",
)
POINTER_SOURCES = ALLOCATORS + POINTER_EXTRACTORS

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
ROOTED_MARKERS = (
    "RuntimeHandleScope",
    "TransientRootScope",
    "across_mut",
    "across_const",
    "across_nanbox",
)

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
    crossed: dict[str, int] = {}
    findings = []
    for offset, line in enumerate(body):
        m = LET_BIND.match(line)
        expression = m.group(2) if m else line

        # Inspect every expression, not only calls that are themselves
        # collection points. In `let copied = (*raw).field`, `raw` occurs only
        # in the RHS; excluding it was the blind spot called out in review of
        # #8253. Check before recording a collection on THIS line: the call has
        # not run yet when its arguments are evaluated, so it is only a hazard
        # to later expressions.
        used_here = set(IDENT.findall(expression))
        for local in sorted(used_here & crossed.keys()):
            findings.append(
                (
                    start + offset + 1,
                    local,
                    start + bound[local] + 1,
                    start + crossed[local] + 1,
                )
            )

        if calls_any(expression, COLLECTION_POINTS):
            for local in bound:
                crossed.setdefault(local, offset)

        if m:
            # A `let` shadows the old binding only after its RHS is evaluated.
            # Remove the old identity after scanning that RHS, then track the
            # new one only when it binds a raw heap address.
            local, rhs = m.groups()
            bound.pop(local, None)
            crossed.pop(local, None)
            if calls_any(rhs, POINTER_SOURCES):
                bound[local] = offset
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
unsafe fn planted_collecting_use() -> *mut ArrayHeader {
    let state = js_array_alloc(6);
    let buffer = js_array_alloc(0);
    let _ = js_array_push_f64(state, js_nanbox_pointer(buffer as i64));
    state
}

unsafe fn planted_plain_return() -> *mut ArrayHeader {
    let state = js_array_alloc(6);
    let _other = js_array_alloc(0);
    state
}

unsafe fn planted_later_rhs(value: f64) -> usize {
    let raw = js_nanbox_get_pointer(value) as *mut ObjectHeader;
    let _other = js_object_alloc(0);
    let copied = (*raw).shape_id;
    copied
}

unsafe fn clean_single_alloc() -> *mut ArrayHeader {
    let only = js_array_alloc(1);
    only
}

unsafe fn clean_use_on_first_collection() {
    let only = js_array_alloc(1);
    js_array_push_f64(only, 0.0);
}

unsafe fn clean_ffi_transient_root() -> *mut ArrayHeader {
    let scope = TransientRootScope::enter();
    let rooted = scope.root_nanbox(js_nanbox_pointer(js_array_alloc(1) as i64));
    let _other = js_array_alloc(0);
    rooted.get() as *mut ArrayHeader
}
'''


def compare_baselines(base: dict, head: dict) -> list[str]:
    """Return recorded-debt increases from BASE to HEAD.

    Schema 1 is the detector merged by #8253. Schema 2 fixes that detector's
    ordinary-use blind spot and adds NaN-box pointer sources, so its initial
    re-pin necessarily increases the measured surface. That one migration is
    explicit; after it lands, both total and per-file ceilings only go down.
    """
    base_schema = int(base.get("schema_version", 1))
    head_schema = int(head.get("schema_version", 1))
    if (base_schema, head_schema) == (1, BASELINE_SCHEMA):
        return []
    if base_schema != head_schema:
        return [f"baseline schema changed {base_schema} -> {head_schema} without an audited migration"]

    bad = []
    if int(head["total"]) > int(base["total"]):
        bad.append(f"baseline total raised {base['total']} -> {head['total']}")
    base_files = base.get("per_file", {})
    for path, ceiling in sorted(head.get("per_file", {}).items()):
        previous = int(base_files.get(path, 0))
        if int(ceiling) > previous:
            where = "was not listed" if path not in base_files else f"was {previous}"
            bad.append(f"{path}: ceiling raised to {ceiling} ({where})")
    return bad


def git_show_baseline(ref: str) -> dict | None:
    """Read the baseline at REF, failing closed when REF was not fetched."""
    resolved = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if resolved.returncode != 0:
        raise SystemExit(
            f"::error::cannot resolve {ref}. The merge base was not fetched, so "
            "the unrooted-local ratchet cannot compare against it -- failing "
            "rather than passing on a comparison that did not happen."
        )
    proc = subprocess.run(
        ["git", "show", f"{ref}:scripts/unrooted_local_shape_baseline.json"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        return None
    return json.loads(proc.stdout)


def no_raise_vs(ref: str) -> int:
    base = git_show_baseline(ref)
    if base is None:
        print(f"{ref} has no unrooted-local baseline; no recorded debt to compare")
        return 0
    head = json.loads(BASELINE.read_text(encoding="utf-8"))
    base_schema = int(base.get("schema_version", 1))
    head_schema = int(head.get("schema_version", 1))
    bad = compare_baselines(base, head)
    if bad:
        print(f"::error::recorded unrooted-local debt rose vs. {ref}: {len(bad)} violation(s)")
        for violation in bad:
            print(f"  {violation}")
        return 1
    if (base_schema, head_schema) == (1, BASELINE_SCHEMA):
        print(
            f"recorded unrooted-local debt vs. {ref}: audited schema migration "
            f"{base_schema} -> {head_schema}, baseline {base['total']} -> {head['total']}"
        )
    else:
        print(
            f"recorded unrooted-local debt vs. {ref}: {base['total']} -> {head['total']}, "
            "no ceiling raised"
        )
    return 0


def self_test() -> int:
    """Prove the detector and each baseline failure mode can still fail."""
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        p = Path(tmp) / "planted.rs"
        p.write_text(SELF_TEST_SRC)
        hits = scan_file(p)
    names = {h[0] for h in hits}
    ok = True
    required = {"planted_collecting_use", "planted_plain_return", "planted_later_rhs"}
    missing = required - names
    if missing:
        print(f"SELF-TEST FAIL: did not flag planted shape(s): {sorted(missing)}", file=sys.stderr)
        ok = False
    forbidden = {
        "clean_single_alloc",
        "clean_use_on_first_collection",
        "clean_ffi_transient_root",
    } & names
    if forbidden:
        print(f"SELF-TEST FAIL: flagged clean control(s): {sorted(forbidden)}", file=sys.stderr)
        ok = False

    base = {"schema_version": 2, "total": 2, "per_file": {"a.rs": 2}}
    comparisons = (
        ({"schema_version": 2, "total": 3, "per_file": {"a.rs": 3}}, "total raised"),
        (
            {"schema_version": 2, "total": 2, "per_file": {"a.rs": 1, "new.rs": 1}},
            "was not listed",
        ),
        ({"schema_version": 3, "total": 2, "per_file": {"a.rs": 2}}, "schema changed"),
    )
    for head, needle in comparisons:
        if not any(needle in violation for violation in compare_baselines(base, head)):
            print(f"SELF-TEST FAIL: baseline rule did not fire for {needle}", file=sys.stderr)
            ok = False
    if compare_baselines(base, base):
        print("SELF-TEST FAIL: unchanged baseline reported an increase", file=sys.stderr)
        ok = False
    if compare_baselines({"total": 218, "per_file": {}}, {"schema_version": 2, "total": 999, "per_file": {}}):
        print("SELF-TEST FAIL: audited schema-1 migration was rejected", file=sys.stderr)
        ok = False

    if ok:
        print(
            "self-test OK: collecting/plain-return/later-RHS sites flagged, "
            "clean controls ignored, baseline increases rejected"
        )
        return 0
    return 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="fail if the count exceeds the baseline")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--no-raise-vs", metavar="REF")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--verbose", action="store_true", help="list every finding")
    args = ap.parse_args()

    if args.self_test:
        return self_test()
    if args.no_raise_vs:
        return no_raise_vs(args.no_raise_vs)

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
        BASELINE.write_text(
            json.dumps(
                {
                    "schema_version": BASELINE_SCHEMA,
                    "total": total,
                    "per_file": {k: len(v) for k, v in results.items()},
                },
                indent=2,
                sort_keys=True,
            )
            + "\n"
        )
        print(f"wrote {BASELINE.relative_to(ROOT)}")
        return 0

    if args.check:
        if not BASELINE.exists():
            print("no baseline recorded; run --update-baseline first", file=sys.stderr)
            return 1
        base = json.loads(BASELINE.read_text())
        if int(base.get("schema_version", 1)) != BASELINE_SCHEMA:
            print(
                f"baseline schema is {base.get('schema_version', 1)}, expected {BASELINE_SCHEMA}; "
                "run --update-baseline",
                file=sys.stderr,
            )
            return 1
        if total > base["total"]:
            print(f"REGRESSION: {total} findings exceeds baseline {base['total']}", file=sys.stderr)
            return 1
        actual_per_file = {path: len(hits) for path, hits in results.items()}
        for path, count in sorted(actual_per_file.items()):
            ceiling = int(base["per_file"].get(path, 0))
            if count > ceiling:
                print(
                    f"REGRESSION: {path}: {count} findings exceeds per-file ceiling {ceiling}",
                    file=sys.stderr,
                )
                return 1
        stale = sorted(set(base["per_file"]) - set(actual_per_file))
        if stale:
            print(
                f"STALE BASELINE: {stale[0]} has no findings; run --update-baseline",
                file=sys.stderr,
            )
            return 1
        if total < base["total"]:
            print(f"improved: {total} < baseline {base['total']} -- run --update-baseline to ratchet")
        print("OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
