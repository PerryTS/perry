#!/usr/bin/env python3
"""Every table the GC test guards CLEAR must be out of reach of another test.

WHY THIS EXISTS (#7672)
-----------------------
`gc::tests::support::reset_copying_nursery_runtime_test_state()` runs from
`GcTestIsolationGuard` and `CopyingNurseryTestGuard`, on whatever libtest thread
happens to construct one, and calls ~20 `test_clear_*` helpers that empty
PROCESS-global side tables. The guards serialize against each other, and against
the handful of tests that remember to take
`crate::gc::global_side_table_test_lock()`. Nothing requires a *reader* to take
it, so the defence is opt-in and the opt-in is invisible at the read site.

Three flakes in two days came from exactly that, each exposed by an unrelated PR
that changed the parallel schedule, and each diagnosed from the wrong VALUE
rather than the timing:

  #7665  opt_report's row sink        `rows.len() == 2` failed at 3
  #7665  ext_registry USED_PROVIDERS  "empty" failed with `ioredis` present
  #7671  closure CLOSURE_PROPS        a static method read back TAG_UNDEFINED

The fix is per-thread storage in test builds (`per_test_global!`), because
the damage window is "between this test's write and this test's read" and only
the test knows that span — a lock the accessor takes for one call does not cover
it, and a lock the test takes is the opt-in the class is made of.

WHAT THIS SCRIPT ASSERTS
------------------------
It derives the clear list from the guards' own source, resolves the storage
behind every helper, and classifies each `static` it writes to:

  * `thread_local!`                 -> safe by construction
  * `per_test_global!`         -> per-thread in test builds, safe
  * a bare `static`                 -> HAZARD

A hazard fails the build unless it is named in ALLOWLIST below with the issue
that blocks it — and an allowlist entry that matches nothing ALSO fails, so an
entry cannot outlive its cause. The burden lands on the ~20 table authors, who
are finite and gated, instead of on the ~180 readers, who are not.

USAGE
-----
    python3 scripts/global_sink_isolation.py              # the gate
    python3 scripts/global_sink_isolation.py --self-test  # proves it can fail
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RUNTIME_SRC = REPO_ROOT / "crates" / "perry-runtime" / "src"
SUPPORT = RUNTIME_SRC / "gc" / "tests" / "support.rs"
RESET_FN = "reset_copying_nursery_runtime_test_state"
# 93 classify today; the floor only has to be high enough that a broken
# matcher cannot pass as a clean tree.
CLASSIFIED_FLOOR = 60

# name -> issue that blocks converting it. An entry matching nothing FAILS.
ALLOWLIST = {
    # #7645 made this latch deliberately process-wide and monotone: one earlier
    # pinning test must leave every later copying test on the same side of the
    # preflight, or the skip path is masked entirely. It has no reader outside
    # the guard, so it cannot damage another test's assertion.
    "YOUNG_PIN_EVER": "#7645",
    # Read, never written, by `test_clear_symbol_side_table_roots`: these two are
    # the process-lifetime registries the per-thread `SYMBOL_POINTERS` rebuild is
    # derived FROM. Their symbols are `Box::leak`ed, so a process-wide identity
    # is the correct one — `Symbol.for("x") === Symbol.for("x")` depends on it.
    "SYMBOL_REGISTRY": "#7672",
    "WELL_KNOWN_SYMBOLS": "#7672",
    # `plugin::REGISTRY` is the one table already defended the sound way, and
    # the survey behind #7672 called it the model: the guard takes
    # `PLUGIN_REGISTRY_TEST_LOCK` (support.rs) and every plugin test takes the
    # same lock, so the clear and the readers share one lock domain. The split
    # lock domain is the root cause in all three fixed flakes; this one is not
    # split.
    "REGISTRY": "#7672",
    # A monotone unique-id source for test fixtures (`young_leaf_{id:x}` names,
    # synthetic symbol ids). Nothing reads its VALUE back and nothing asserts on
    # it; making it per-thread would only weaken the uniqueness it exists for.
    "YOUNG_LEAF_COUNTER": "#7672",
}


class Violation(Exception):
    pass


def rel(path) -> str:
    """Repo-relative path, tolerant of the self-test's synthetic sources."""
    if path is None:
        return "?"
    try:
        return str(Path(path).relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


PAIRS = {"{": "}", "(": ")", "[": "]"}


def brace_body(text: str, open_idx: int, opener: str = "{") -> str:
    """Body of the block whose `opener` is at/after `open_idx`."""
    closer = PAIRS[opener]
    start = text.index(opener, open_idx)
    depth = 0
    for i in range(start, len(text)):
        if text[i] == opener:
            depth += 1
        elif text[i] == closer:
            depth -= 1
            if depth == 0:
                return text[start + 1 : i]
    raise Violation("unterminated block")


def reset_body(support_text: str) -> str:
    match = re.search(r"fn\s+%s\s*\(\s*\)" % RESET_FN, support_text)
    if not match:
        raise Violation(
            "could not find `fn %s` in %s. The clear list is derived from that "
            "function; if it was renamed this gate is reading nothing."
            % (RESET_FN, SUPPORT.name)
        )
    return brace_body(support_text, match.end())


def clear_helpers(body: str) -> list:
    """`test_clear_*` / `test_set_*` / `reset_*` calls the guards' reset makes."""
    names = re.findall(r"::((?:test_clear|test_set|test_reset|reset)_[a-z0-9_]+)\s*\(", body)
    if not names:
        raise Violation(
            "parsed zero clear helpers out of %s's body — the extraction is broken, "
            "and a gate over an empty list cannot fail." % RESET_FN
        )
    return sorted(set(names))


def rust_sources() -> dict:
    return {p: p.read_text(encoding="utf-8") for p in RUNTIME_SRC.rglob("*.rs")}


def find_fn_body(sources: dict, name: str):
    """(path, body) of the first `fn <name>(` definition found."""
    pattern = re.compile(r"\bfn\s+" + re.escape(name) + r"\s*(<[^>]*>)?\s*\(")
    for path, text in sources.items():
        match = pattern.search(text)
        if match:
            return path, brace_body(text, match.end())
    return None, None


def declaration_kind(sources: dict, ident: str, prefer=None):
    """'thread_local' | 'per_test' | 'static' | None, plus where.

    `prefer` is the file that mentioned `ident`; Rust resolves a bare name in
    its own module first, and several of these names (`REGISTRY`) exist in more
    than one file. Searching the whole crate first named the wrong file and, for
    `per_test_global!`-converted tables, the wrong VERDICT.
    """
    static_re = re.compile(
        r"^([ \t]*)(?:pub(?:\([^)]*\))?\s+)?static\s+" + re.escape(ident) + r"\s*:\s*([^=]*)=", re.M
    )
    tls_re = re.compile(r"\b(?:pub(?:\([^)]*\))?\s+)?static\s+" + re.escape(ident) + r"\s*:")
    inline_static_re = re.compile(
        r"\b(?:pub(?:\([^)]*\))?\s+)?static\s+" + re.escape(ident) + r"\s*:\s*[^=]*="
    )
    ordered = list(sources.items())
    if prefer is not None and prefer in sources:
        ordered.sort(key=lambda kv: 0 if kv[0] == prefer else 1)
    for path, text in ordered:
        # thread_local! { ... IDENT: ... }
        for tls in re.finditer(r"thread_local!\s*\{", text):
            body = brace_body(text, tls.end() - 1)
            if tls_re.search(body):
                return "thread_local", path
        # per_test_global! { ... } AND per_test_global!( ... ) — `timer.rs` uses
        # the paren form to stay under the 2000-line cap, and a `{`-only regex
        # silently dropped its three tables to "(no static storage)". A gate that
        # stops matching is a gate that stops failing.
        for g in re.finditer(r"per_test_global!\s*([\{\(\[])", text):
            body = brace_body(text, g.end() - 1, g.group(1))
            if static_re.search(body) or inline_static_re.search(body):
                return "per_test", path
        found = static_re.search(text)
        if found:
            declared = found.group(2).strip()
            # A LOCK IS NOT DATA. `static X: Mutex<()>` carries no state; making
            # it per-thread would turn it into a no-op, which is the opposite of
            # the fix. Same for the global allocator.
            if re.fullmatch(r"(std::sync::)?(Mutex|RwLock)\s*<\s*\(\s*\)\s*>", declared):
                return "lock", path
            if "#[global_allocator]" in text[max(0, found.start() - 80) : found.start()]:
                return "allocator", path
            return "static", path
    return None, None


def audit(sources: dict, support_text: str, allowlist: dict, out=sys.stdout, floor=None):
    body = reset_body(support_text)
    helpers = clear_helpers(body)
    violations = []
    hazards = {}
    seen_idents = set()

    # The guards' own module is audited as if it were a helper. #7672's FIFTH
    # instance was `GENERATED_WRITE_BARRIERS_EMITTED`, which no `test_clear_*`
    # touches: two guards in this very file own it under two DIFFERENT locks
    # (`copying_nursery_isolation_lock` and `GENERATED_BARRIER_TEST_LOCK`) and
    # every runtime write barrier reads it holding neither. Deriving the audit
    # set from the clear list alone could not see it, and it cost one red run in
    # a 22-run soak before anyone looked.
    subjects = [(helper, None) for helper in helpers]
    subjects.append(("gc/tests/support.rs (the guards themselves)", (SUPPORT, support_text)))

    out.write(
        "per-test global sinks (#7672): %d clear helper(s) reached from %s, "
        "plus the guards' own module\n" % (len(helpers), RESET_FN)
    )
    for helper, preloaded in subjects:
        if preloaded is not None:
            path, helper_body = preloaded
        else:
            path, helper_body = find_fn_body(sources, helper)
        if helper_body is None:
            violations.append(
                "%s is called by %s but has no definition under crates/perry-runtime/src — "
                "the gate cannot classify what it clears." % (helper, RESET_FN)
            )
            continue
        # Follow one level of same-file accessor calls. `test_clear_closure_
        # side_tables` names no static at all — it goes through
        # `get_closure_props()` — so a body-only scan classified it as "no
        # storage" and would not have noticed CLOSURE_PROPS reverting.
        expanded = helper_body
        for callee in sorted(set(re.findall(r"\b(get_[a-z0-9_]+)\s*\(", helper_body))):
            callee_path, callee_body = find_fn_body({path: sources[path]}, callee)
            if callee_body:
                expanded += "\n" + callee_body
        idents = sorted(set(re.findall(r"\b([A-Z][A-Z0-9_]{2,})\b", expanded)))
        classified = []
        for ident in idents:
            kind, decl = declaration_kind(sources, ident, prefer=path)
            if kind is None:
                continue  # a constant, a type, an Ordering variant, ...
            seen_idents.add(ident)
            classified.append((ident, kind))
            if kind == "static" and ident not in allowlist:
                hazards[ident] = (helper, decl)
        out.write(
            "  %-40s %s\n"
            % (
                helper,
                ", ".join("%s=%s" % (i, k) for i, k in classified) or "(no static storage)",
            )
        )

    for ident, (helper, decl) in sorted(hazards.items()):
        violations.append(
            "%s is a BARE process-global `static` (%s), written by %s. One libtest "
            "thread's test then reaches another's copy of it. Declare it "
            "with `per_test_global!` (crates/perry-runtime/src/per_test_global.rs) "
            "or, if it truly must stay process-wide, add it to ALLOWLIST in %s with the "
            "issue that says why."
            % (
                ident,
                rel(decl),
                helper,
                Path(__file__).name,
            )
        )

    for ident, issue in sorted(allowlist.items()):
        if ident not in seen_idents:
            violations.append(
                "ALLOWLIST names %r (%s), which is no longer written by any helper %s "
                "calls. An entry that matches nothing hides nothing and outlives its "
                "reason — delete it." % (ident, issue, RESET_FN)
            )

    out.write(
        "  -> %d hazard(s), %d allowlisted, %d static(s) classified\n"
        % (len(hazards), len(allowlist), len(seen_idents))
    )
    # FLOOR. Every check in this file is a regex over Rust source, and the
    # `per_test_global!(...)` paren form already slipped past a `{`-only pattern
    # once, silently taking the three timer tables to "(no static storage)". A
    # gate whose matcher rots reports zero hazards and exits 0, which is
    # indistinguishable from a clean tree. Refuse to be that.
    if floor is not None and len(seen_idents) < floor:
        violations.append(
            "only %d static(s) were classified, below the floor of %d. The source "
            "matchers have stopped matching — this run proves nothing, and a zero "
            "hazard count from it means nothing." % (len(seen_idents), floor)
        )
    return violations


# ---------------------------------------------------------------------------
# Self-test: the gate must be able to fail, and must fail for the right reason.
# ---------------------------------------------------------------------------

_FAKE_SUPPORT = """
pub(super) fn reset_copying_nursery_runtime_test_state() {
    crate::demo::test_clear_partitioned();
    crate::demo::test_clear_bare();
    crate::demo::test_clear_tls();
    crate::demo::test_clear_paren();
}
"""

_FAKE_SRC = """
per_test_global! {
    static PARTITIONED_TABLE: Mutex<u64> = Mutex::new(0);
}
per_test_global!(static PAREN_TABLE: Mutex<u64> = Mutex::new(0));
static PURE_LOCK: Mutex<()> = Mutex::new(());
static BARE_TABLE: Mutex<u64> = Mutex::new(0);
thread_local! {
    static TLS_TABLE: RefCell<u64> = RefCell::new(0);
}
pub(crate) fn test_clear_partitioned() { *PARTITIONED_TABLE.lock().unwrap() = 0; }
pub(crate) fn test_clear_bare() { *BARE_TABLE.lock().unwrap() = 0; }
pub(crate) fn test_clear_tls() { TLS_TABLE.with(|t| *t.borrow_mut() = 0); }
pub(crate) fn test_clear_paren() { *PAREN_TABLE.lock().unwrap() = 0; let _g = PURE_LOCK.lock(); }
"""


def self_test() -> int:
    import io

    failures = []
    fake = {Path("fake.rs"): _FAKE_SRC}
    sink = io.StringIO()

    # 1. A bare process-global static is a violation; the partitioned and
    #    thread-local ones are not.
    violations = audit(fake, _FAKE_SUPPORT, {}, sink)
    if len(violations) != 1 or "BARE_TABLE" not in violations[0]:
        failures.append("expected exactly one BARE_TABLE violation, got %r" % (violations,))
    if any("PARTITIONED_TABLE" in v or "TLS_TABLE" in v for v in violations):
        failures.append("a converted or thread-local table was reported: %r" % (violations,))

    # 2. Allowlisting it silences exactly that one.
    if audit(fake, _FAKE_SUPPORT, {"BARE_TABLE": "#1"}, sink):
        failures.append("an allowlisted hazard still failed")

    # 3. An allowlist entry that matches nothing must fail — otherwise the list
    #    only grows and a fix never has to delete its line.
    stale = audit(fake, _FAKE_SUPPORT, {"BARE_TABLE": "#1", "GONE_TABLE": "#2"}, sink)
    if not any("GONE_TABLE" in v for v in stale):
        failures.append("a stale allowlist entry was accepted: %r" % (stale,))

    # 3b. THE PAREN FORM. `per_test_global!(...)` on one line is how `timer.rs`
    #     stays under the 2000-line cap, and a `{`-only matcher classified its
    #     three tables as "(no static storage)" — a silent loss of coverage that
    #     reads exactly like a clean tree.
    if any("PAREN_TABLE" in v for v in audit(fake, _FAKE_SUPPORT, {"BARE_TABLE": "#1"}, sink)):
        failures.append("a per_test_global!(...) paren-form declaration was reported as a hazard")
    kind, _ = declaration_kind(fake, "PAREN_TABLE")
    if kind != "per_test":
        failures.append("paren-form PAREN_TABLE classified as %r" % (kind,))

    # 3c. A LOCK IS NOT DATA: `static X: Mutex<()>` must never be a hazard, or
    #     the gate would demand that the serializers themselves be per-thread,
    #     which would turn every one of them into a no-op.
    kind, _ = declaration_kind(fake, "PURE_LOCK")
    if kind != "lock":
        failures.append("PURE_LOCK classified as %r, not 'lock'" % (kind,))
    if any("PURE_LOCK" in v for v in audit(fake, _FAKE_SUPPORT, {"BARE_TABLE": "#1"}, sink)):
        failures.append("a Mutex<()> serializer was reported as a hazard")

    # 3d. THE FLOOR: a matcher that stops matching must not read as clean.
    floored = audit(fake, _FAKE_SUPPORT, {"BARE_TABLE": "#1"}, sink, floor=99)
    if not any("below the floor" in v for v in floored):
        failures.append("the classified-statics floor did not fire: %r" % (floored,))

    # 4. A renamed/absent reset function must be an error, not an empty pass.
    for text, why in [
        ("fn something_else() { }", "missing reset fn"),
        ("pub(super) fn reset_copying_nursery_runtime_test_state() {\n}\n", "empty reset body"),
    ]:
        try:
            audit(fake, text, {}, sink)
            failures.append("%s was accepted instead of raising" % why)
        except Violation:
            pass

    # 5. The parsers must survive the REAL tree, or the gate is vacuous.
    try:
        real_support = SUPPORT.read_text(encoding="utf-8")
        helpers = clear_helpers(reset_body(real_support))
        if len(helpers) < 10:
            failures.append("only %d clear helpers parsed from the real %s" % (len(helpers), SUPPORT.name))
        if "test_clear_closure_side_tables" not in helpers:
            failures.append("the real clear list is missing a helper it certainly calls: %r" % (helpers,))
        sources = rust_sources()
        kind, _ = declaration_kind(sources, "CLOSURE_PROPS")
        if kind != "per_test":
            failures.append("CLOSURE_PROPS classified as %r on the real tree" % (kind,))
        kind, _ = declaration_kind(sources, "ARGUMENTS_OBJECTS")
        if kind != "thread_local":
            failures.append("ARGUMENTS_OBJECTS classified as %r on the real tree" % (kind,))
    except Violation as exc:
        failures.append("parsing the real tree failed: %s" % exc)

    for failure in failures:
        print("SELF-TEST FAIL: %s" % failure, file=sys.stderr)
    print("self-test: 15 checks, %d failures" % len(failures))
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    try:
        violations = audit(
            rust_sources(),
            SUPPORT.read_text(encoding="utf-8"),
            ALLOWLIST,
            floor=CLASSIFIED_FLOOR,
        )
    except Violation as exc:
        print("ERROR: %s" % exc, file=sys.stderr)
        return 1

    for violation in violations:
        print("GLOBAL SINK: %s" % violation, file=sys.stderr)
    if violations:
        print(
            "\n%d process-global side table(s) cleared by the GC test guards are reachable "
            "from another test's assertions. See #7672: this class is diagnosed by luck, "
            "in a PR that did not cause it." % len(violations),
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
