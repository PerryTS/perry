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
detection is statement-order over source text. Expect false positives where the
allocation provably cannot trigger a collection, and false negatives wherever a
pointer flows through a shape this does not spell. The number is useful as an
EXPOSURE SURFACE -- how much of the surface no instrument is watching -- not as
a bug count.

STATEMENT-order, not line-order, since #10715: a `let` whose initializer
rustfmt wrapped onto the following lines is folded back into one statement
before matching. It used to be matched per line, so a binding broken after its
`=` -- a function of indentation depth and identifier length, not of anything
about the code -- was simply not counted. Deeply nested code, which is where
rooting bugs live, was the least scanned, and the totals were in part a measure
of formatting. Statements containing a brace are still read line by line, on
purpose: see `join_let_statement`.

`--no-raise-vs` SCANS THE WORKTREE, since #10713. It used to compare the merge
base's recorded baseline against the checked-out one and nothing else, so a
branch that added findings without touching the baseline passed it while
`--check` failed on the same tree, seconds later, in the same run.

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
    "across_mut",
    "across_const",
    "across_nanbox",
)

# These expressions return a root-handle token, not the raw pointer returned by
# an allocator nested inside the expression. Do not start tracking the token as
# though it were itself a heap address. Unlike ROOTED_MARKERS this is local to
# the binding and does not exempt the rest of the function.
ROOT_HANDLE_BINDINGS = (
    "root_addr(",
    "root_addrs(",
    "root_bigint_ptr(",
    "root_heap_word_u64(",
    "root_nanbox(",
    "root_nanbox_f64(",
    "root_nanbox_f64_slice(",
    "root_nanbox_u64(",
    "root_raw_const_ptr(",
    "root_raw_mut_ptr(",
    "root_string_ptr(",
)

FN_START = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*fn\s+(\w+)")
LET_BIND = re.compile(r"^\s*let\s+(?:mut\s+)?(\w+)\s*(?::[^=]+)?=\s*(.+)$")
LET_HEAD = re.compile(r"^\s*let\s")
IDENT = re.compile(r"\b\w+\b")

# A wrapped `let` cannot span more lines than this before the fold gives up and
# the statement is read one line at a time again. A bound purely so a malformed
# or unterminated statement cannot walk to the end of the function.
JOIN_MAX_LINES = 40


def statement_is_complete(text: str) -> bool:
    """True when TEXT holds a whole statement: a `;` outside every bracket."""
    depth = 0
    for ch in text:
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
        elif ch == ";" and depth <= 0:
            return True
    return False


def join_let_statement(body: list[str], offset: int) -> tuple[str, int]:
    """Fold a `let` whose initializer rustfmt wrapped onto the lines after it.

    #10715: `LET_BIND` was matched per line, so `rustfmt` breaking a binding
    after the `=` left a head line with nothing on its right-hand side. Nothing
    matched, the local was never tracked, and the finding disappeared -- a false
    negative produced by indentation depth and identifier length rather than by
    anything about the code. That made the deepest-nested code, which is exactly
    where rooting bugs live, the least scanned, and made the totals partly a
    measure of formatting. Returns the folded statement and the last line it ate.

    Statements containing a brace are deliberately left alone. A `{` means a
    closure, `match`, block or struct-literal initializer whose body carries its
    OWN bindings and collection points; folding those into a single expression
    would hide every one of them, trading this blind spot for a worse one. Line
    order is already correct for that shape, since the head line keeps a
    non-empty right-hand side.
    """
    head = body[offset]
    if "{" in head or "}" in head or statement_is_complete(head):
        return head, offset
    text = head.rstrip()
    for j in range(offset + 1, min(offset + 1 + JOIN_MAX_LINES, len(body))):
        nxt = body[j]
        if "{" in nxt or "}" in nxt:
            return head, offset
        text = f"{text} {nxt.strip()}"
        if statement_is_complete(text):
            return text, j
    return head, offset


def fold_wrapped_lets(body: list[str]) -> tuple[dict[int, str], set[int]]:
    """Return (folded text by head offset, offsets absorbed into a head).

    Absorbed lines are scanned as empty rather than dropped, so every line still
    contributes its brace delta to the lexical depth and the reported line
    numbers stay the function's own.
    """
    joined: dict[int, str] = {}
    absorbed: set[int] = set()
    for offset, line in enumerate(body):
        if offset in absorbed or not LET_HEAD.match(line):
            continue
        text, last = join_let_statement(body, offset)
        if last > offset:
            joined[offset] = text
            absorbed.update(range(offset + 1, last + 1))
    return joined, absorbed


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
    lexical_depth = 0
    runtime_scopes: list[tuple[str, int]] = []
    joined, absorbed = fold_wrapped_lets(body)
    for offset, source_line in enumerate(body):
        # A folded continuation is scanned as empty: its text was already read
        # as part of the `let` at the head offset, and reading it twice would
        # report the same use once per line it wrapped onto.
        line = "" if offset in absorbed else joined.get(offset, source_line)
        m = LET_BIND.match(line)
        expression = m.group(2) if m else line
        runtime_scopes = [
            (local, depth)
            for local, depth in runtime_scopes
            if lexical_depth >= depth and f"drop({local})" not in line
        ]
        if m and "RuntimeHandleScope::new(" in expression:
            runtime_scopes.append((m.group(1), lexical_depth))

        # Inspect every expression, not only calls that are themselves
        # collection points. In `let copied = (*raw).field`, `raw` occurs only
        # in the RHS; excluding it was the blind spot called out in review of
        # #8253. Check before recording a collection on THIS line: the call has
        # not run yet when its arguments are evaluated, so it is only a hazard
        # to later expressions.
        used_here = set(IDENT.findall(expression))
        for local in sorted(used_here & crossed.keys()):
            if not runtime_scopes:
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
            if calls_any(rhs, POINTER_SOURCES) and not calls_any(rhs, ROOT_HANDLE_BINDINGS):
                bound[local] = offset
        # Depth comes from the SOURCE line, never the folded text: an absorbed
        # line is scanned as empty but still owns its braces. Folded statements
        # are brace-free by construction, so this is the pre-#10715 arithmetic.
        lexical_depth += source_line.count("{") - source_line.count("}")
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

unsafe fn clean_active_runtime_scope() {
    let scope = RuntimeHandleScope::new();
    let raw = js_array_alloc(1);
    let _rooted = scope.root_raw_mut_ptr(raw);
    let _other = js_array_alloc(0);
    consume(raw);
}

unsafe fn planted_after_transient_scope() -> *mut ArrayHeader {
    let stale = js_array_alloc(1);
    {
        let scope = TransientRootScope::enter();
        let _rooted = scope.root_nanbox(js_nanbox_pointer(stale as i64));
    }
    let _other = js_array_alloc(0);
    stale
}

unsafe fn planted_after_runtime_scope() -> *mut ArrayHeader {
    let stale = js_array_alloc(1);
    {
        let scope = RuntimeHandleScope::new();
        let _rooted = scope.root_raw_mut_ptr(stale);
    }
    let _other = js_array_alloc(0);
    stale
}

unsafe fn planted_wrapped_binding() -> *mut ObjectHeader {
    let object =
        js_object_alloc_with_shape(shape, 3, keys.as_ptr(), keys.len() as u32);
    js_object_set_field(object, 0, first);
    js_object_set_field(object, 1, second);
    object
}

unsafe fn clean_wrapped_shadow_rebinds() -> usize {
    let err_str = js_string_from_bytes(first.as_ptr(), first.len() as u32);
    let _other = js_array_alloc(0);
    let err_str =
        js_string_from_bytes(second.as_ptr(), second.len() as u32);
    string_len(err_str)
}

unsafe fn planted_inside_wrapped_closure() {
    let handler = move |arg: f64| {
        let inner = js_array_alloc(1);
        let _other = js_array_alloc(0);
        consume(inner);
    };
    register(handler);
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


def compare_measured(base: dict, total: int, per_file: dict[str, int]) -> list[str]:
    """Return rises of the MEASURED worktree over BASE's recorded ceilings.

    #10713: this comparison did not exist, and its absence was the whole bug.
    `--no-raise-vs` read two recorded BASELINE FILES -- the merge base's and the
    checked-out one -- and never scanned a line of source. A branch that ADDED
    findings without touching the baseline therefore compared 561 against 561
    and printed "no ceiling raised" while `--check`, seconds later in the same
    `run_lint_gates.sh` run on the same worktree, failed with
    `REGRESSION: 563 findings exceeds baseline 561`. The variant whose entire
    purpose is catching a rise against the base could not see one, because the
    only number it ever looked at was one the diff had left alone.

    Measuring against the base's RECORDED ceilings rather than the base's own
    measurement is the closest comparison available without a second checkout,
    and it is the same yardstick `--check` uses, so the two forms now agree.
    """
    bad = []
    base_total = int(base["total"])
    if total > base_total:
        bad.append(f"measured total {total} exceeds merge-base recorded total {base_total}")
    base_files = base.get("per_file", {})
    for path, count in sorted(per_file.items()):
        ceiling = int(base_files.get(path, 0))
        if count > ceiling:
            where = "not recorded at the merge base" if path not in base_files else f"ceiling {ceiling}"
            bad.append(f"{path}: {count} measured findings exceeds {where}")
    return bad


def resolve_ref(ref: str) -> str:
    """Return REF's commit SHA, failing closed on an empty or unfetched ref.

    Resolving FIRST is the point, and `raw_handle_debt.py`'s `git_show` makes
    the argument: a merge base the runner never fetched reports every file as
    absent, which reads as "the base recorded nothing" -- a comparison that did
    not happen, reported as a pass. It must be a RED build instead.

    The empty string gets the same treatment, and for the same reason. It used
    to get worse: `--no-raise-vs ""` is falsy, so `if args.no_raise_vs` was
    False, the vs-base mode never ran at all, and the script fell through to the
    plain report, which exits 0. An unset `$BASE_SHA` thus printed a perfectly
    ordinary-looking finding table and passed, having compared nothing.
    """
    if not ref.strip():
        raise SystemExit(
            "::error::--no-raise-vs was given an empty ref. That is an unset "
            "$BASE_SHA, not a request to skip the comparison -- failing rather "
            "than passing on a comparison that did not happen."
        )
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
            "rather than passing on a comparison that did not happen. Fetch it "
            "with `git fetch --no-tags --depth=1 origin <sha>`."
        )
    return resolved.stdout.strip()


def git_show_baseline(ref: str) -> dict | None:
    """Read the baseline at REF. REF must already be resolved."""
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
    resolved = resolve_ref(ref)
    base = git_show_baseline(resolved)
    if base is None:
        print(f"{ref} ({resolved}) has no unrooted-local baseline; no recorded debt to compare")
        return 0
    head = json.loads(BASELINE.read_text(encoding="utf-8"))
    base_schema = int(base.get("schema_version", 1))
    head_schema = int(head.get("schema_version", 1))
    migration = (base_schema, head_schema) == (1, BASELINE_SCHEMA)
    bad = compare_baselines(base, head)

    # Scan the worktree too. Without this the whole mode is recorded-vs-recorded
    # (#10713). Skipped only across the audited schema migration, where the two
    # sides were measured by different detectors and the numbers are not
    # comparable -- the same exemption `compare_baselines` already makes.
    measured = "not measured (schema migration)"
    if not migration:
        results = collect()
        total = sum(len(v) for v in results.values())
        measured = str(total)
        bad += compare_measured(base, total, {k: len(v) for k, v in results.items()})

    where = (
        f"vs. {ref} ({resolved}): recorded {base['total']} -> {head['total']}, "
        f"measured {measured}"
    )
    if bad:
        print(f"::error::unrooted-local debt rose {where}: {len(bad)} violation(s)")
        for violation in bad:
            print(f"  {violation}")
        return 1
    if migration:
        print(
            f"unrooted-local debt {where}: audited schema migration "
            f"{base_schema} -> {head_schema}"
        )
    else:
        print(f"unrooted-local debt {where}, no ceiling raised")
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
    required = {
        "planted_collecting_use",
        "planted_plain_return",
        "planted_later_rhs",
        "planted_after_transient_scope",
        "planted_after_runtime_scope",
        # #10715. `let object =` with the initializer on the next line, the
        # shape rustfmt produces once the binding is nested a few levels deep.
        # The line-oriented matcher saw no right-hand side, never tracked
        # `object`, and reported nothing -- a false negative bought with an
        # indent. Taken from a live site (perry-ext-ws `js_ws_server_address`).
        "planted_wrapped_binding",
        # The fold must stop at a brace. A closure body carries its own
        # bindings and collection points; swallowing it into one expression
        # would trade #10715's blind spot for a strictly larger one.
        "planted_inside_wrapped_closure",
    }
    missing = required - names
    if missing:
        print(f"SELF-TEST FAIL: did not flag planted shape(s): {sorted(missing)}", file=sys.stderr)
        ok = False
    forbidden = {
        "clean_single_alloc",
        "clean_use_on_first_collection",
        "clean_ffi_transient_root",
        "clean_active_runtime_scope",
        # The same #10715 defect in the other direction: a WRAPPED shadowing
        # `let` matched nothing, so the dead identity from the earlier binding
        # of that name stayed live and every use of the fresh one was reported.
        # Eleven of the findings in perry-stdlib/src/ioredis.rs were this.
        "clean_wrapped_shadow_rebinds",
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

    # #10713. Everything above this line passed on the day `--no-raise-vs`
    # returned green on a worktree `--check` rejected, which is the point: a
    # self-test that does not cover the failing mode is not evidence about it.
    measured_cases = (
        ((3, {"a.rs": 3}), "measured total 3 exceeds merge-base recorded total 2"),
        ((2, {"a.rs": 1, "new.rs": 1}), "new.rs: 1 measured findings exceeds not recorded"),
    )
    for (total, per_file), needle in measured_cases:
        if not any(needle in violation for violation in compare_measured(base, total, per_file)):
            print(f"SELF-TEST FAIL: measured rule did not fire for {needle!r}", file=sys.stderr)
            ok = False
    if compare_measured(base, 2, {"a.rs": 2}):
        print("SELF-TEST FAIL: an unchanged worktree reported a measured rise", file=sys.stderr)
        ok = False

    # An empty ref is an unset $BASE_SHA -- a comparison that did not happen,
    # which must be red. `resolve_ref` says so in those words; `git rev-parse`
    # would reject it regardless, so this pair asserts the message, not the
    # behaviour. The behaviour is the DISPATCH, checked below.
    for bad_ref in ("", "   "):
        try:
            resolve_ref(bad_ref)
        except SystemExit:
            continue
        print(f"SELF-TEST FAIL: resolve_ref({bad_ref!r}) did not fail closed", file=sys.stderr)
        ok = False

    ok = _self_test_empty_ref_dispatch() and ok
    ok = _self_test_no_raise_vs() and ok

    if ok:
        print(
            "self-test OK: collecting/plain-return/later-RHS/wrapped sites flagged, "
            "clean controls ignored, baseline and measured increases rejected"
        )
        return 0
    return 1


def _self_test_empty_ref_dispatch() -> bool:
    """`--no-raise-vs ""` must fail closed, not fall through to the report.

    The hole was in the dispatch, one character wide: `if args.no_raise_vs`
    tests TRUTHINESS, and the empty string an unset `$BASE_SHA` expands to is
    falsy. The vs-base mode was therefore never entered at all -- no ref was
    resolved, no baseline read, nothing compared -- and the script printed an
    ordinary finding table and exited 0. Guarding inside `resolve_ref` alone
    does not cover this, because nothing called it.
    """
    import contextlib
    import io

    saved = sys.argv
    outcome: object = None
    try:
        sys.argv = ["unrooted_local_shape.py", "--no-raise-vs", ""]
        with contextlib.redirect_stdout(io.StringIO()):
            outcome = main()
    except SystemExit:
        return True
    finally:
        sys.argv = saved
    print(
        f"SELF-TEST FAIL: `--no-raise-vs \"\"` returned {outcome!r} instead of failing "
        "closed; an unset $BASE_SHA compared nothing and passed -- this is #10713",
        file=sys.stderr,
    )
    return False


def _self_test_no_raise_vs() -> bool:
    """Drive the real `--no-raise-vs` over the tree that fooled it (#10713).

    Reproduces the observed combination exactly: the merge base and the
    checked-out baseline both recording 561, and a worktree measuring 563. That
    is a pass for a recorded-vs-recorded comparison and a `REGRESSION` for
    `--check`, and both ran seconds apart in one `run_lint_gates.sh` invocation.
    Stubs stand in for git and the scan so the case is a fixture rather than a
    property of whatever this repo happens to measure today.
    """
    import contextlib
    import io
    import tempfile

    recorded = {"schema_version": BASELINE_SCHEMA, "total": 561, "per_file": {"a.rs": 561}}
    scope = globals()
    saved = {k: scope[k] for k in ("resolve_ref", "git_show_baseline", "collect", "BASELINE")}
    out = io.StringIO()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            head = Path(tmp) / "baseline.json"
            head.write_text(json.dumps(recorded), encoding="utf-8")
            scope["BASELINE"] = head
            scope["resolve_ref"] = lambda ref: "0" * 40
            scope["git_show_baseline"] = lambda ref: recorded
            with contextlib.redirect_stdout(out):
                scope["collect"] = lambda root=None: {"a.rs": [None] * 563}
                regressed = no_raise_vs("planted-base")
                scope["collect"] = lambda root=None: {"a.rs": [None] * 561}
                unchanged = no_raise_vs("planted-base")
    finally:
        scope.update(saved)

    ok = True
    if regressed != 1:
        print(
            "SELF-TEST FAIL: --no-raise-vs passed a worktree measuring 563 against "
            "a merge base recording 561 (both baselines identical) -- this is #10713",
            file=sys.stderr,
        )
        ok = False
    if unchanged != 0:
        print("SELF-TEST FAIL: --no-raise-vs failed an unchanged worktree", file=sys.stderr)
        ok = False
    if not ok:
        print(out.getvalue(), file=sys.stderr)
    return ok


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
    # `is not None`, not truthiness: `--no-raise-vs ""` is an unset $BASE_SHA,
    # and dropping through to the report on it is a pass without a comparison
    # (#10713). `resolve_ref` rejects it.
    if args.no_raise_vs is not None:
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
