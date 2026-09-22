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
  * `RealmAtomicI64` / `RealmAtomicU64` -> immutable handle to a
    `perry_thread_local!` backing slot, safe
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
    # #9613's FSEvents symbol table: a OnceLock caching dlopen'd CoreServices
    # function addresses. Process-wide by nature (dynamic-loader handles),
    # write-once, holds no JS-heap pointers and no per-test state.
    "API": "#9613",
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


def rust_module_path(path: Path) -> tuple[str, ...]:
    """Best-effort Rust module path for a source file in the runtime tree."""
    path = Path(path)
    try:
        relative = path.relative_to(RUNTIME_SRC)
    except ValueError:
        relative = path
    parts = list(relative.parts)
    if not parts:
        return ()
    if parts[-1] in ("lib.rs", "mod.rs"):
        parts.pop()
    elif parts[-1].endswith(".rs"):
        parts[-1] = parts[-1][:-3]
    return tuple(parts)


def is_runtime_realm_atomic(sources: dict, path: Path, declared: str) -> bool:
    """Whether `declared` resolves to a real runtime RealmAtomic wrapper.

    Matching the final identifier alone would exempt a same-named local type
    alias. Resolve the small set of Rust paths this source audit can prove and
    require the target module to contain the wrapper's `struct` definition.
    """
    match = re.fullmatch(
        r"(?:(crate|self|super(?:::\s*super)*)::\s*)?"
        r"((?:[a-z_][a-z0-9_]*::\s*)*)"
        r"(RealmAtomic(?:I64|U64))",
        declared,
    )
    if not match:
        return False

    prefix, middle, type_name = match.groups()
    current = list(rust_module_path(path))
    if prefix == "crate":
        target = []
    elif prefix == "self":
        target = current
    elif prefix and prefix.startswith("super"):
        target = current
        for _ in prefix.split("::"):
            if not target:
                return False
            target.pop()
    elif middle:
        # A relative multi-segment path may depend on a `use` alias that this
        # source-only resolver cannot prove. Fail closed.
        return False
    else:
        target = current

    target.extend(part.strip() for part in middle.split("::") if part.strip())
    definition_re = re.compile(
        r"\b(?:pub(?:\([^)]*\))?\s+)?struct\s+" + re.escape(type_name) + r"\b"
    )
    return any(
        rust_module_path(candidate) == tuple(target) and definition_re.search(text)
        for candidate, text in sources.items()
    )


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
            # These wrappers hold no mutable process-global value: their only
            # field is a `&'static HotKey<Atomic*>`, and every load/store goes
            # through that `perry_thread_local!` backing slot. Treating the
            # immutable handle as the data falsely reports a cross-test sink
            # while ignoring the realm-local slot that actually owns it.
            if is_runtime_realm_atomic(sources, path, declared):
                return "thread_local", path
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
    crate::demo::test_clear_realm_atomic();
    crate::demo::test_clear_qualified_realm_atomic();
}
"""

_FAKE_SRC = """
struct RealmAtomicU64;
per_test_global! {
    static PARTITIONED_TABLE: Mutex<u64> = Mutex::new(0);
}
per_test_global!(static PAREN_TABLE: Mutex<u64> = Mutex::new(0));
static PURE_LOCK: Mutex<()> = Mutex::new(());
static BARE_TABLE: Mutex<u64> = Mutex::new(0);
static REALM_CACHE: RealmAtomicU64 = RealmAtomicU64::new(&REALM_CACHE_SLOT);
thread_local! {
    static TLS_TABLE: RefCell<u64> = RefCell::new(0);
}
pub(crate) fn test_clear_partitioned() { *PARTITIONED_TABLE.lock().unwrap() = 0; }
pub(crate) fn test_clear_bare() { *BARE_TABLE.lock().unwrap() = 0; }
pub(crate) fn test_clear_tls() { TLS_TABLE.with(|t| *t.borrow_mut() = 0); }
pub(crate) fn test_clear_paren() { *PAREN_TABLE.lock().unwrap() = 0; let _g = PURE_LOCK.lock(); }
pub(crate) fn test_clear_realm_atomic() { REALM_CACHE.with_slot(|slot| slot.store(0)); }
"""

_FAKE_CHILD_SRC = """
static QUALIFIED_REALM_CACHE: super::RealmAtomicU64 =
    super::RealmAtomicU64::new(&QUALIFIED_REALM_CACHE_SLOT);
pub(crate) fn test_clear_qualified_realm_atomic() {
    QUALIFIED_REALM_CACHE.with_slot(|slot| slot.store(0));
}
"""

_FAKE_ALIAS_SRC = """
type RealmAtomicU64 = FakeAtomic;
static SHADOWED_REALM_CACHE: RealmAtomicU64 = FakeAtomic::new();
"""


def self_test() -> int:
    import io

    failures = []
    fake = {
        Path("object/mod.rs"): _FAKE_SRC,
        Path("object/child.rs"): _FAKE_CHILD_SRC,
        Path("alias.rs"): _FAKE_ALIAS_SRC,
    }
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

    # 3d. RealmAtomic wrappers are immutable handles whose mutable value lives
    # in a perry_thread_local! HotKey. The wrapper itself is not a shared sink.
    kind, _ = declaration_kind(fake, "REALM_CACHE")
    if kind != "thread_local":
        failures.append("REALM_CACHE classified as %r, not 'thread_local'" % (kind,))
    if any("REALM_CACHE" in v for v in audit(fake, _FAKE_SUPPORT, {"BARE_TABLE": "#1"}, sink)):
        failures.append("a RealmAtomic thread-local proxy was reported as a hazard")

    kind, _ = declaration_kind(fake, "QUALIFIED_REALM_CACHE")
    if kind != "thread_local":
        failures.append("qualified RealmAtomic proxy classified as %r, not 'thread_local'" % (kind,))

    # A same-named alias is not proof that the value is backed by the runtime's
    # perry_thread_local! HotKey. It must remain a process-global hazard.
    kind, _ = declaration_kind(fake, "SHADOWED_REALM_CACHE")
    if kind != "static":
        failures.append("unrelated RealmAtomic alias classified as %r, not 'static'" % (kind,))

    # 3e. THE FLOOR: a matcher that stops matching must not read as clean.
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
        kind, _ = declaration_kind(sources, "ITERATOR_PROTOTYPE_PTR")
        if kind != "thread_local":
            failures.append("qualified ITERATOR_PROTOTYPE_PTR classified as %r on the real tree" % (kind,))
    except Violation as exc:
        failures.append("parsing the real tree failed: %s" % exc)

    for failure in failures:
        print("SELF-TEST FAIL: %s" % failure, file=sys.stderr)
    print("self-test: 20 checks, %d failures" % len(failures))
    return 1 if failures else 0


# ---------------------------------------------------------------------------
# #10944: the SECOND class — a process-global counter a TEST ASSERTS ON.
# ---------------------------------------------------------------------------
#
# The rule above covers tables the GC guards CLEAR. It does not cover the much
# larger population of process-global counters that tests read directly, and
# that population is why `cargo test -p perry-runtime` cannot attribute a
# regression today:
#
#     --test-threads=1   4215 passed;  0 failed
#     parallel (x6)      4198-4205 passed; 10-17 failed, a DIFFERENT set each run
#
# ZERO genuine failures. Every failure the suite produces is one test's
# assertion disturbed by another test's increment, and it always presents the
# same way -- off by exactly one:
#
#     assertion `left == right` failed: ... reuse the prior negative verdict
#       left: 2, right: 1
#
# The premise was written down in `json_tape/cached_read.rs`:
#
#     // The runtime suite is serial. This witness holds no managed values.
#     static ROOTED_READS: AtomicU32 = AtomicU32::new(0);
#
# It is not serial. libtest runs tests in one process across many threads.
#
# WHY A RATCHET AND NOT A SWEEP
# -----------------------------
# The population is not enumerable by inspection -- six parallel runs after
# three modules were converted still produced 17 distinct failures, including
# names no earlier run had shown. Converting every one at a time means editing
# modules owned by several lanes at once. So this records today's set and
# fails only on ADDITIONS, exactly like `raw_handle_debt.py` and
# `unrooted_local_shape.py`: existing entries get converted by whoever owns
# each file, opportunistically, and no new instance can arrive quietly.
#
# `per_test_global!` is the fix for an entry, and its own module docs are the
# justification: "a new sink cannot be added quietly, and a new *reader* never
# has to remember anything." #7665, #7671, #7672 and #7975 are the first four
# instances of this class; #10944 is the fifth, which is the argument for a
# gate rather than a fifth patch.
#
# OUT OF SCOPE: the timing-shaped family (`child_process::reactor`, `pty`,
# `stdlib_pump`) fails under load on a shared box and has nothing to do with
# shared counters. It needs its own triage and must not be swept in here.

ASSERTED_BASELINE = REPO_ROOT / "scripts" / "global_sink_asserted_baseline.txt"

# Types whose whole purpose is cross-thread mutation. A `static` of one of
# these is shared state; a `const`, a plain integer or a `&str` table is not.
_SHARED_TY = re.compile(
    r"\b(Atomic(?:Bool|I8|I16|I32|I64|Isize|U8|U16|U32|U64|Usize|Ptr)"
    r"|Mutex|RwLock|OnceLock|OnceCell|ImageTable|RegistryLatch)\b"
)
# A declaration the macros already make safe.
_SAFE_BLOCK = re.compile(r"\b(thread_local|per_test_global|perry_thread_local)\s*!")
# An `assert*!(...)` invocation, body included (non-greedy to the first `);`
# at the end of a line, which is how this codebase formats them).
_ASSERT_CALL = re.compile(r"\bassert(?:_eq|_ne)?!\s*\(.*?\)\s*;", re.S)
# `mut` is optional and CAPTURED. A `static mut` is shared mutable state by
# definition -- racing on one is UB, not merely a wrong count -- so it is the
# most dangerous shape this rule claims to cover, and it escaped twice over:
# the identifier is not immediately after `static`, so it never matched, and
# its type is usually a plain integer, so `_SHARED_TY` would have excluded it
# even if it had. A gate with a hole exactly where the hazard is worst.
#
# Latent today: the tree's only two (`ohos_napi.rs`) are asserted by no test,
# so the baseline is unchanged. This closes the gap before one arrives.
_STATIC = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?static\s+(mut\s+)?([A-Z][A-Z0-9_]*)\s*:\s*(.+?)\s*="
)


def _test_region(text: str) -> str:
    """The part of a file that is test code.

    Everything from the first `#[cfg(test)]` to EOF, which is where this
    codebase puts its test modules, plus the whole file when it is a
    `*_tests.rs`. Over-inclusive on purpose: a ratchet may record a few
    entries no test actually asserts on, and the cost of that is one baseline
    line, whereas a miss is a flake nobody can attribute.
    """
    marker = text.find("#[cfg(test)]")
    return text[marker:] if marker >= 0 else ""


def asserted_globals(sources) -> set[str]:
    """`path::NAME` for every bare shared `static` a test reads.

    `sources` is `rust_sources()`'s `{path: text}` mapping, or a list of
    `(name, text)` pairs in the self-test.
    """
    items = sources.items() if isinstance(sources, dict) else list(sources)
    items = [(str(p), t) for p, t in items]
    bare: dict[str, list[tuple[str, str]]] = {}
    for path, text in items:
        depth = 0
        in_safe = False
        found: list[tuple[str, str]] = []
        for line in text.splitlines():
            if not in_safe and _SAFE_BLOCK.search(line):
                in_safe, depth = True, 0
            if in_safe:
                depth += line.count("{") - line.count("}")
                if depth <= 0 and "{" in line or (in_safe and depth <= 0):
                    if depth <= 0:
                        in_safe = False
                continue
            m = _STATIC.match(line)
            if m and (m.group(1) or _SHARED_TY.search(m.group(3))):
                found.append((m.group(2), m.group(3)))
        if found:
            bare[path] = found

    hits: set[str] = set()
    tests_by_path = {p: (t if p.endswith("_tests.rs") else _test_region(t)) for p, t in items}
    # Only an ASSERTION on the static is the hazard. A test that merely
    # mentions one -- arming a feature flag, reading a census counter it does
    # not check -- cannot be broken by a sibling's increment, and flagging
    # those made the first draft of this list 481 entries of mostly noise.
    # The failure this gate exists for always looks the same: a test asserts a
    # global count and a sibling makes it off by one.
    asserted_text = "\n".join(
        m.group(0)
        for text in tests_by_path.values()
        for m in _ASSERT_CALL.finditer(text)
    )
    for path, decls in bare.items():
        for name, _ty in decls:
            if re.search(r"\b%s\b" % re.escape(name), asserted_text):
                # repo-relative, so the baseline is stable across checkouts
                rel = path.split("/crates/", 1)
                key = ("crates/" + rel[1]) if len(rel) == 2 else path
                hits.add("%s::%s" % (key, name))
    return hits


def _load_asserted_baseline() -> set[str] | None:
    if not ASSERTED_BASELINE.exists():
        return None
    return {
        ln.strip()
        for ln in ASSERTED_BASELINE.read_text(encoding="utf-8").splitlines()
        if ln.strip() and not ln.startswith("#")
    }


def check_asserted(update: bool = False) -> int:
    sources = rust_sources()
    head = asserted_globals(sources)
    base = _load_asserted_baseline()

    if update:
        if base is not None:
            added = head - base
            if added:
                print(
                    "refusing to raise the baseline; convert these to "
                    "per_test_global! instead:\n  " + "\n  ".join(sorted(added)),
                    file=sys.stderr,
                )
                return 1
        header = (
            "# #10944: bare process-global `static`s that TEST code reads.\n"
            "# Each is a flake waiting for a scheduling change. Fix one by moving it\n"
            "# into `per_test_global!` and deleting its line here. This list may only\n"
            "# shrink -- `--update` refuses to add.\n"
        )
        ASSERTED_BASELINE.write_text(header + "\n".join(sorted(head)) + "\n", encoding="utf-8")
        removed = len(base - head) if base else 0
        print("asserted-global baseline: %d entries (%d removed)" % (len(head), removed))
        return 0

    if base is None:
        print("no asserted-global baseline; run --update-asserted. current=%d" % len(head))
        return 1
    added = sorted(head - base)
    print("asserted process-global statics: %d (baseline %d)" % (len(head), len(base)))
    if added:
        for entry in added:
            print("NEW ASSERTED GLOBAL: %s" % entry, file=sys.stderr)
        print(
            "\n%d bare process-global `static`(s) newly readable from test code. "
            "libtest runs tests in one process on many threads, so a sibling's "
            "increment breaks another test's assertion by exactly one and the "
            "failing SET moves between runs -- see #10944, where the suite was "
            "4215/0 single-threaded and 10-17 failures in parallel. Declare it "
            "with `per_test_global!` (per-thread in a test build, the plain "
            "`static` byte for byte outside one)." % len(added),
            file=sys.stderr,
        )
        return 1
    return 0


def asserted_no_raise_vs(ref: str) -> int:
    """Reject a diff that ADDS an entry and baselines it in the same commit."""
    import subprocess

    try:
        base_text = subprocess.run(
            ["git", "show", "%s:scripts/global_sink_asserted_baseline.txt" % ref],
            capture_output=True, text=True, cwd=REPO_ROOT, check=False,
        ).stdout
    except OSError as exc:
        print("cannot resolve %s: %s" % (ref, exc), file=sys.stderr)
        return 1
    if not base_text.strip():
        print("merge base recorded no asserted-global baseline; nothing to compare")
        return 0
    base = {ln.strip() for ln in base_text.splitlines() if ln.strip() and not ln.startswith("#")}
    head = _load_asserted_baseline() or set()
    added = sorted(head - base)
    if added:
        for entry in added:
            print("BASELINE RAISED: %s" % entry, file=sys.stderr)
        print(
            "\nthe baseline gained %d entr(y/ies) relative to %s. The ratchet only "
            "goes down: convert them with `per_test_global!` rather than recording "
            "them." % (len(added), ref),
            file=sys.stderr,
        )
        return 1
    print("asserted-global baseline vs %s: %d -> %d, no additions" % (ref, len(base), len(head)))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--update-asserted", action="store_true")
    parser.add_argument("--asserted-no-raise-vs", metavar="REF")
    args = parser.parse_args()

    if args.self_test:
        return self_test() or asserted_self_test()
    if args.update_asserted:
        return check_asserted(update=True)
    if args.asserted_no_raise_vs:
        return asserted_no_raise_vs(args.asserted_no_raise_vs)

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
    # #10944's ratchet runs unconditionally alongside the clear-list rule.
    return check_asserted()


def asserted_self_test() -> int:
    """A gate that cannot fail is documentation.

    Four fixtures: the hazard must be reported, and each of the three ways a
    static is NOT the hazard must not be.
    """
    hazard = [(
        "b.rs",
        "static HITS: AtomicU64 = AtomicU64::new(0);\n"
        "#[cfg(test)]\nmod tests {\n"
        "    #[test]\n    fn t() { assert_eq!(HITS.load(Relaxed), 1); }\n}\n",
    )]
    if asserted_globals(hazard) != {"b.rs::HITS"}:
        print("self-test FAILED: a bare asserted static was NOT reported", file=sys.stderr)
        return 1

    safe_macro = [(
        "a.rs",
        "per_test_global! {\n    static HITS: AtomicU64 = AtomicU64::new(0);\n}\n"
        "#[cfg(test)]\nmod tests {\n"
        "    #[test]\n    fn t() { assert_eq!(HITS.load(Relaxed), 1); }\n}\n",
    )]
    if asserted_globals(safe_macro):
        print("self-test FAILED: a per_test_global! static was reported", file=sys.stderr)
        return 1

    mentioned_not_asserted = [(
        "c.rs",
        "static HITS: AtomicU64 = AtomicU64::new(0);\n"
        "#[cfg(test)]\nmod tests {\n"
        "    #[test]\n    fn t() { HITS.store(1, Relaxed); }\n}\n",
    )]
    if asserted_globals(mentioned_not_asserted):
        print("self-test FAILED: a static no test ASSERTS on was reported", file=sys.stderr)
        return 1

    static_mut = [(
        "e.rs",
        "static mut HITS: u64 = 0;\n"
        "#[cfg(test)]\nmod tests {\n"
        "    #[test]\n    fn t() { unsafe { assert_eq!(HITS, 1); } }\n}\n",
    )]
    if asserted_globals(static_mut) != {"e.rs::HITS"}:
        print("self-test FAILED: an asserted `static mut` was NOT reported "
              "(it matches neither the identifier position nor the shared-type "
              "filter, which is the gap this case exists for)", file=sys.stderr)
        return 1

    production_only = [(
        "d.rs",
        "static HITS: AtomicU64 = AtomicU64::new(0);\n"
        "fn p() { assert_eq!(HITS.load(Relaxed), 1); }\n",
    )]
    if asserted_globals(production_only):
        print("self-test FAILED: a non-test assertion was reported", file=sys.stderr)
        return 1

    print("asserted-global self-test: reports the hazard (including `static "
          "mut`) and none of the three near-misses")
    return 0


if __name__ == "__main__":
    sys.exit(main())
