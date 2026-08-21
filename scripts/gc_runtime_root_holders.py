#!/usr/bin/env python3
"""Runtime-side GC-pointer holder custody gate (#7231; ffi/ext/ui coverage 2026-08).

A `thread_local!` or `static` that stores a pointer into the GC heap **is a GC
root**, and the collector only knows that if something registers it via
`gc_register_*root_scanner*`. An unregistered holder is not an intermittent
bug: it goes bad at collection #0 and stays bad, so the symptom is a
*perfectly reproducible* use-after-free — the opposite tell from the #7154
stale-register class.

Nothing static could find this class before. `scripts/gc_root_dominance_check.py`
reads emitted LLVM IR, and a runtime table is not in it — that is not a gap in
that tool, it is outside its subject. #7226, #7239, #7268 and #7274 were all
found by hand. This script is the enumeration those fixes kept re-deriving,
turned into something that can fail.

That asymmetry is also why the CRATE SET below must be complete. The static
checker is structurally blind to every Rust-side table, in every crate, so
this census is the ONLY detector for the whole class — and a crate outside
its scope is not "less checked", it is UNCHECKED. The `perry-ext-*` crates
park user closures in handle side tables (a `WsClientListeners.listeners`
value, a `BackoffState.task`); those closures cross the C ABI as NaN-boxed
`i64`/`f64`, so nothing in their declared types says "heap pointer" — which
is exactly how this census reported clean over them for months: its two
original rules could not parse the shape, and its crate list did not include
the crates. A gate whose subject never ran. If you widen the crate set again,
extend the RULES to the new crates' value-representation first, or you are
adding green paint, not coverage.

What it does
------------

1. **Enumerate** every `static` / `thread_local!` / `lazy_static!` declaration
   whose stored type can hold a GC heap pointer. Three crate tiers:

   * **core** (`perry-runtime`, `perry-stdlib`): rule A (the type names a heap
     header type or `JSValue`) and rule B (an integer/`f64` cell that some
     function in its own file both names and allocates in, which is how
     `CACHED_ENV: Cell<f64>` — the highest-impact holder in #7231's original
     report — has to be caught).
   * **ffi-side, gated** (`perry-ffi`, every `crates/perry-ext-*` — discovered
     by GLOB, so a new ext crate is in scope the day it is created): JS values
     cross the C ABI as NaN-boxed `i64`/`f64`, so the rules read the TYPE
     SHAPE instead of type names — rule V (an `i64`/`u64`/`f64` in a container
     VALUE position: `Vec<i64>` listener lists, `HashMap<K, f64>` callback
     maps; map KEYS are exempt, they are handle ids), rule S (a container of a
     crate-local struct/enum whose fields can hold such a value —
     `HashMap<u64, BackoffState>` where `BackoffState.task` is NaN-box bits;
     resolved recursively through crate-local types), rule E (a type-erased
     `dyn` payload in value position — `DashMap<Handle, Box<dyn Any>>` can
     hold anything, including a struct full of closures), and rule F (a BARE
     integer/`f64` scalar in a file where some function both names it and
     touches closure/NaN-box machinery). Declarations inside function bodies
     (`static T: OnceLock<..> = ..` in an accessor fn) and `lazy_static!`'s
     `static ref` are parsed; multi-line declarations are joined.
   * **frontier** (`crates/perry-ui*` — also by glob): same rules, but tracked
     by an identity-pinned ratchet instead of per-holder verdicts. See
     "The frontier tier" below.

   Perry's custom TLS macro is also a deliberate fourth rule: every
   `perry_thread_local!` / `crate::perry_thread_local!` declaration in the core
   crates that rules A/B do not already recognize is enumerated as rule T. The
   macro accepts arbitrary crate-local types, so treating an unfamiliar type as
   safe would recreate the blind spot this census exists to close. Rule-T
   holders use the same identity-pinned ratchet as the UI frontier until each
   stored type has a researched verdict.

2. **Compute coverage** rather than trusting names. The registered scanner set
   is read from every `gc_register_*root_scanner*(...)` call site; a call graph
   over all `fn` bodies in every scanned crate is walked from those roots to
   `MAX_SCANNER_DEPTH`, and a holder counts as covered when its identifier
   appears in a reachable function **defined in the same file as the
   declaration**. The same-file requirement is not incidental: `REGISTRY`,
   `SLOTS`, `ROOTS`, `STATES`, `CACHED` and `CLOSE_CALLBACK` each name several
   different holders in this tree, and a name-only match certifies the wrong
   one. The call-graph walk is what finds the holders a scanner reaches through
   an accessor (`cp_live_lock()`, `get_closure_props()`, `buffer_props()`)
   rather than by name.

3. **Require a verdict** for everything left over. Each uncovered gated holder
   must appear in `scripts/gc_runtime_root_holders.json` with a `verdict` and a
   `why`. A holder with no entry fails; an entry that matches no holder fails
   (a stale exemption is how these gates rot — same rule as
   `scripts/gc_root_dominance_allowlist.json`).

The identity-pinned frontier
----------------------------

The `perry-ui*` crates hold hundreds of NaN-boxed callback tables
(`BUTTON_CALLBACKS: RefCell<HashMap<usize, f64>>` × every widget × eight
platform crates), none of them registered with the GC. Several of those crates
cannot even be BUILT on the host that runs this gate (windows/android/gtk4),
so "fix them all in the commit that widens the census" would mean shipping
unverifiable edits — the exact sin this repo's gate history warns about. The
honest alternative is neither exempting them (a `not_a_gc_pointer` verdict for
a real callback table would be a lie) nor ignoring them (the blind spot this
extension closes):

* every frontier holder is ENUMERATED by the same rules,
* the population is pinned BY IDENTITY (`file` + `name`) in the inventory's
  `frontier` list,
* a NEW frontier holder fails this gate immediately — growth is loud, the
  original blind spot cannot re-open,
* a frontier entry matching nothing also fails — the list can only shrink,
  and fixing a holder (registering a scanner that reaches it) forces its
  entry to be deleted.

A frontier entry is a precisely-named piece of debt, not a verdict. When a UI
crate gains real scanner coverage, its holders read as covered, their entries
go stale, and the deletion is the receipt.

The same ratchet also carries otherwise-unclassified core declarations inside
`perry_thread_local!`. Unlike a plain `static` declaration, each of these is a
known state-holding TLS slot even when its type is a crate-local struct that
rules A/B cannot resolve (`PathModuleRegistry`, `ExceptionState`, and
`YogaNode` are real examples). A newly declared slot therefore fails until it
is covered or deliberately pinned. More importantly, a currently covered slot
is absent from the baseline, so deleting the scanner that reaches it makes it a
NEW frontier holder and turns the gate red.

How it fails
------------

* a new uncovered gated holder with no inventory entry -> exit 1
* an inventory entry that no longer matches a declaration -> exit 1
* an `open_gap` or `unverified` verdict -> exit 1; old-page relocation ships
  enabled, so a known or unevaluated movable-address holder cannot be exempted
* a frontier/rule-T holder not in the pinned `frontier` list -> exit 1 (ratchet up)
* a `frontier` entry matching no holder -> exit 1 (ratchet down / stale)
* fewer than MIN_HOLDERS declarations matched -> exit 2, because a regex that
  stopped matching would otherwise report a clean, empty, green run
* fewer than MIN_REGISTERED registered scanners found -> exit 2, same reason:
  if the registration regex breaks, EVERYTHING reads as uncovered and the run
  is noise rather than a gate

`--self-test` plants each shape into a temp tree and requires the scanner to
reject it, and requires it NOT to flag a holder that a registered scanner
genuinely reaches — including through one hop of accessor indirection. Run it
before trusting a green scan.

What this gate CANNOT see
-------------------------

Named, because an unstated limit is how a gate gets trusted past its subject.

* **`RuntimeState`-owned tables.** `crates/perry-runtime/src/state.rs` absorbed
  roughly a dozen former `thread_local!`s (`descriptors`, `object_hot` and its
  `overflow_fields` / `shape_cache_overflow` / `transition_cache`,
  `field_lookup`, `shapes`, and `exotic_expando`). They are struct FIELDS,
  reached through `state()`, so no declaration-site scan sees them. All are
  covered today: in particular, `exotic_expando` is visited by
  `scan_exotic_expando_roots_mut`. A new field added there is invisible here.
  `STATE_FIELD_FLOOR` below asserts the struct has not grown past the field
  count this was checked at, so growth is at least *loud*.
* **An integer-typed holder whose own file never calls an allocator, in a CORE
  crate.** Rule B needs a function that both names the holder and allocates; a
  cell written purely from a value handed in across a module boundary has
  neither, and is invisible. The ffi-side shape rules (V/S/E/F) close exactly
  this hole for the ffi/ext/ui crates — where it is the NORM, every value
  arrives across the ABI — but they are deliberately NOT applied to
  `perry-runtime`/`perry-stdlib`: measured 2026-08, they would add ~150 new
  core candidates, each needing a researched verdict, which is a separate
  tightening campaign. Until someone runs it, a core `Mutex<Vec<i64>>` table
  in a non-allocating file is still invisible here.
* **A holder reached by a scanner in a DIFFERENT file.** It reads as uncovered
  and needs an inventory entry saying so; `verdict: "covered_elsewhere"` is that
  entry, and it records which scanner.
* **Module identity beyond the FIRST hop.** The registration text carries the
  path (`crate::json::raw_json::scan_raw_json_key_root_mut`), so the registered
  root set is resolved to a file — two modules defining `scan_tls_roots_mut`
  (which perry-runtime and perry-stdlib really do) cannot certify each other's
  holders. Deeper hops are matched on the bare name, because nothing in the text
  says which module a call resolved to; that direction over-approximates
  coverage, and an inventory entry is the correction.
* **Whether a "covered" holder is covered CORRECTLY.** The scanner may visit
  three of a table's four slots — the shape #7239 found in
  `scan_parent_port_event_roots_mut`. Reading the body is the only way, and this
  gate does not read semantics. It bounds the population; it does not audit it.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from pathlib import Path, PurePath, PureWindowsPath

REPO_ROOT = Path(__file__).resolve().parent.parent
INVENTORY_PATH = REPO_ROOT / "scripts" / "gc_runtime_root_holders.json"

# Crate tiers. Core keeps the original name-and-allocator rules; ffi-side
# crates get the shape rules (their JS values are NaN-boxed integers); ui
# crates are enumerated but ratcheted rather than verdict-gated (docstring:
# "The frontier tier"). ffi/ext/ui membership is discovered by GLOB so that a
# new crate is inside the census the day it is created — the original
# whole-crate blind spot must not be reproducible by `cargo new`.
CORE_CRATES = ("crates/perry-runtime/src", "crates/perry-stdlib/src")


def gated_ffi_crates(root: Path) -> tuple[str, ...]:
    found = ["crates/perry-ffi/src"]
    for crate_dir in sorted((root / "crates").glob("perry-ext-*")):
        found.append(f"crates/{crate_dir.name}/src")
    return tuple(c for c in found if (root / c).is_dir())


def frontier_ui_crates(root: Path) -> tuple[str, ...]:
    found = []
    for crate_dir in sorted((root / "crates").glob("perry-ui*")):
        found.append(f"crates/{crate_dir.name}/src")
    return tuple(c for c in found if (root / c).is_dir())


def repo_relative(path: PurePath, root: PurePath) -> str:
    """Return a stable repository-relative key on every host platform."""
    return path.relative_to(root).as_posix()

# Types whose NAME says "this is a pointer into the GC heap".
HEAP_TYPE_TOKENS = (
    "ObjectHeader",
    "ArrayHeader",
    "StringHeader",
    "ClosureHeader",
    "SymbolHeader",
    "RegExpHeader",
    "JSValue",
)

# Rule B: an integer/float cell can hold a NaN-boxed value or a bare address.
# `CACHED_ENV: Cell<f64>` (#7231's headline holder) has exactly this shape, and
# so do `ERROR_CONSTRUCTOR_PTR: Cell<usize>` and `INPUT_HANDLER: AtomicI64`.
INT_TYPE_TOKENS = ("f64", "usize", "u64", "i64", "AtomicI64", "AtomicUsize", "AtomicU64")

# ffi-side rule A adds the perry-ffi spellings. `Promise` is here because a
# `*mut Promise` parked in a pending-event struct is a movable GC object
# (ext-events' `pending_once_promises` really does this, and really scans it).
FFI_HEAP_TYPE_TOKENS = HEAP_TYPE_TOKENS + ("JsValue", "JsClosure", "Promise")

# Rule V/S value primitives: what a NaN-boxed JS value (or raw ClosureHeader
# address) is stored AS on the ffi side of the C ABI. `usize` is deliberately
# absent — in the ffi/ui crates it is an ObjC object address, a slab index or
# a map key, and including it triples the population with zero known true
# positives (measured 2026-08).
FFI_VALUE_PRIMITIVES = frozenset({"f64", "i64", "u64"})

# Containers whose FIRST generic argument is a KEY (exempt from rule V: keys
# are handle ids / addresses, not owned JS values).
KEYED_CONTAINERS = frozenset({"HashMap", "BTreeMap", "DashMap", "IndexMap", "PtrHashMap", "StdHashMap", "FxHashMap"})

# Transparent wrappers/containers whose arguments are all VALUE positions.
WALK_CONTAINERS = frozenset(
    {
        "Vec", "VecDeque", "HashSet", "BTreeSet", "LinkedList", "BinaryHeap",
        "Slab", "SmallVec", "Option", "Box", "Arc", "Rc", "Weak",
        "RefCell", "Cell", "UnsafeCell", "SyncUnsafeCell", "MaybeUninit", "ManuallyDrop",
        "Mutex", "RwLock", "StdMutex", "OnceLock", "OnceCell", "LazyLock", "Lazy",
        "AtomicRefCell",
    }
)

# Collection names whose presence distinguishes rule V (a table OF values)
# from rule F (a bare scalar cell, which needs closure-machinery context).
COLLECTION_NAMES = frozenset(
    {
        "Vec", "VecDeque", "HashSet", "BTreeSet", "LinkedList", "BinaryHeap",
        "Slab", "SmallVec", "HashMap", "BTreeMap", "DashMap", "IndexMap",
        "PtrHashMap", "StdHashMap", "FxHashMap",
    }
)

# Rule F's qualifier, at function granularity like rule B's: some function in
# the declaring file both names the bare-scalar holder and visibly handles
# closures / NaN-boxed values. Chosen narrow: `from_bits`/`to_bits` alone
# would match every bit-fiddling counter in the tree.
FFI_CLOSURE_CONTEXT_TOKENS = (
    "Closure",
    "closure",
    "nanbox",
    "visit_i64_slot",
    "visit_nanbox_f64_slot",
    "JsValue",
)

# A function POINTER is not data — `KEEP_*` dead-strip anchors and vtable slots
# hold code addresses, which the collector neither moves nor traces.
FN_POINTER = re.compile(r"\bfn\s*\(")

# Rule B's qualifier, at FUNCTION granularity rather than file granularity: some
# function in the declaring file both mentions this holder and calls a GC
# allocator. That is the textual shadow of "this cell is populated from
# something the allocator returned". A file-level test was tried first and
# reported 544 holders, four fifths of them counters and ids in files that
# happen to allocate somewhere — a gate nobody would read.
ALLOCATOR_TOKENS = (
    "js_object_alloc",
    "js_closure_alloc",
    "js_array_alloc",
    "js_string_from_",
    "gc_malloc",
    "alloc_symbol",
    "alloc_date_cell",
    "js_nanbox_string",
    "js_nanbox_pointer",
)

# The whole argument list, not just the first argument. `..._named(
# "stdlib:worker_threads:workers", scan_worker_roots_mut)` puts the scanner
# SECOND, and a first-argument-only regex silently reported every holder that
# scanner covers as uncovered — six of them, in one file.
#
# `reg_scanner!` / `reg_budgeted_scanner!` are `gc_init`'s wrappers (#7915):
# they exist only to attach `stringify!`'d registration-site names, and they
# expand to the same `gc_register_*` calls. They must be matched here or every
# holder reached only from `gc_init` reads as uncovered — which is exactly what
# happened when they were introduced, and the MIN_REGISTERED floor below is
# what caught it.
# The argument text may contain one level of nested calls:
# `perry_ffi_gc_register_mutable_root_scanner_named(SOURCE.as_ptr(),
# SOURCE.len(), 0, scan_events_roots_trampoline)` is the dependency-free
# C-ABI registration the ext crates use, and a `[^()]*` argument body
# silently missed it — every holder that trampoline covers then read as
# uncovered.
REGISTER_CALL = re.compile(
    r"(?:gc_register_\w*root_scanner\w*|reg_scanner!|reg_budgeted_scanner!)"
    r"\s*\((?P<args>(?:[^;()]|\([^()]*\))*)\)",
    re.S,
)
# `function_bodies` runs over strip_comments output, where every string
# literal — including the `"C"` in `extern "C" fn` — has been replaced with
# `""`. The original `extern\s+"C"` alternative therefore never matched, and
# every `extern "C" fn` body (each C-ABI scanner TRAMPOLINE) was invisible to
# the call graph: coverage that flowed through one silently read as
# uncovered. Accept the stripped form.
FN_DEF = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+|extern\s+\"[^\"]*\"\s+)*fn\s+(\w+)"
)
IDENT = re.compile(r"\b[A-Za-z_]\w*\b")

# A declaration: `static NAME: TYPE =` — covers `pub(crate) static`, the bodies
# of `thread_local!` blocks (which use the same syntax), `lazy_static!`'s
# `static ref NAME:`, `static mut`, function-body statics (`static T:
# OnceLock<..>` inside an accessor fn — the dominant ext-crate shape), and
# `static NAME: Lazy<...>`. The `=` may be on a later line; `declarations`
# joins continuation lines, so the type is NOT captured here.
DECL = re.compile(
    r"^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?static\s+(?:ref\s+)?(?:mut\s+)?"
    r"(?P<name>[A-Z][A-Z0-9_]*)\s*:\s*(?P<type>.*)$"
)

# Perry's hot-TLS macro accepts the same declaration syntax as
# `thread_local!`, but a declaration may name an opaque crate-local type that
# core rules A/B cannot see through. `declarations_in_perry_tls` makes the
# macro boundary explicit instead of relying on DECL's context-free match.
PERRY_TLS_BLOCK = re.compile(r"(?m)^[ \t]*(?:crate::)?perry_thread_local!\s*\{")

MAX_SCANNER_DEPTH = 3

# Floors. Each is a "the extraction still works" assertion, not a budget.
MIN_HOLDERS = 60
MIN_REGISTERED = 60

# See "What this gate CANNOT see". `RuntimeState`'s fields are not declarations
# and are invisible to DECL; this makes the struct growing at least loud.
STATE_FILE = "crates/perry-runtime/src/state.rs"
STATE_STRUCT = "struct RuntimeState"
STATE_FIELD_FLOOR = 5


def crate_source_files(root: Path, crates: tuple[str, ...]) -> list[Path]:
    files: list[Path] = []
    for crate in crates:
        base = root / crate
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            parts = path.parts
            if "target" in parts:
                continue
            # Test modules are out of subject: a `#[cfg(test)]` holder is never
            # live in a shipped binary, and the GC test guards deliberately
            # reset the ones that are (`reset_copying_nursery_runtime_test_state`,
            # itself gated by scripts/global_sink_isolation.py).
            if "tests" in parts or path.name.endswith("tests.rs"):
                continue
            files.append(path)
    return files


def tiered_source_files(root: Path) -> list[tuple[Path, str]]:
    """Every scanned file with its tier: core / ffi / frontier."""
    out: list[tuple[Path, str]] = []
    for path in crate_source_files(root, CORE_CRATES):
        out.append((path, "core"))
    for path in crate_source_files(root, gated_ffi_crates(root)):
        out.append((path, "ffi"))
    for path in crate_source_files(root, frontier_ui_crates(root)):
        out.append((path, "frontier"))
    return out


STRING_LITERAL = re.compile(r'"(?:[^"\\\n]|\\.)*"')
CHAR_LITERAL = re.compile(r"'(?:[^'\\\n]|\\.)'")


def strip_comments(text: str) -> str:
    """Drop line comments AND string/char literals, keeping the line count.

    Stripping literals is not tidiness: the brace counting in
    `function_bodies` is what delimits a scanner's body, and this tree is full
    of `"{"`. Left in, `json/raw_json.rs`'s `scan_raw_json_key_root_mut` was
    swallowed by the preceding function and `RAW_JSON_KEY` — which that scanner
    visits, three lines below its own declaration — reported as UNCOVERED. A
    gate whose FALSE POSITIVES are that easy to produce trains people to add
    inventory entries instead of reading the code.
    """
    out = []
    for line in text.splitlines():
        line = CHAR_LITERAL.sub("''", line)
        line = STRING_LITERAL.sub('""', line)
        out.append(line.split("//", 1)[0])
    return "\n".join(out)


def function_bodies(text: str) -> dict[str, str]:
    """Map fn name -> its body text. Brace-counted, comments stripped.

    Good enough for a call-graph reachability walk: a body that over-runs by a
    brace only ever makes MORE things reachable, i.e. errs toward calling a
    holder covered, which is the direction an inventory entry can correct.
    """
    code = strip_comments(text)
    lines = code.splitlines()
    bodies: dict[str, str] = {}
    index = 0
    while index < len(lines):
        match = FN_DEF.match(lines[index])
        if not match:
            index += 1
            continue
        name = match.group(1)
        depth = 0
        started = False
        chunk: list[str] = []
        declaration_only = False
        while index < len(lines):
            line = lines[index]
            # A body-less declaration — `fn f(...);` inside an `extern "C" {}`
            # block or a trait — has no body to count. Before this check
            # (#8211) the counter ran on into the NEXT item's braces and
            # swallowed every function up to wherever the depth happened to
            # balance; in `streams/gc.rs` and `fetch/gc.rs` that hid the
            # FFI-registered scanner entry point itself, so the walk never
            # started.
            if not started and line.rstrip().endswith(";") and "{" not in line:
                declaration_only = True
                index += 1
                break
            chunk.append(line)
            depth += line.count("{") - line.count("}")
            if "{" in line:
                started = True
            index += 1
            if started and depth <= 0:
                break
        if declaration_only:
            continue
        bodies.setdefault(name, "")
        bodies[name] += "\n".join(chunk)
    return bodies


MAX_DECL_CONTINUATION_LINES = 6


def _top_level_eq(text: str) -> int:
    """Index of the first `=` outside any `<...>` nesting, or -1."""
    depth = 0
    for index, char in enumerate(text):
        prev = text[index - 1] if index else ""
        if char == "<":
            depth += 1
        elif char == ">" and prev != "-":  # `->` in a fn-pointer type is not a close
            depth -= 1
        elif char == "=" and depth == 0 and prev not in "<>=!":
            return index
    return -1


def declarations(rel: str, text: str) -> list[tuple[str, int, str]]:
    """(name, line, type) for every static-shaped declaration in the file.

    The type may continue past the declaration line (`static DEFERRED:\n
    RefCell<VecDeque<(Handle, FastifyPendingRequest)>> =` is real code);
    continuation lines are joined until the initializer's `=` appears at
    generic-depth 0. A declaration with no `=` within the join window (an
    `extern` block static, defined and therefore scanned in its home crate)
    is skipped, matching the original single-line behavior.
    """
    out: list[tuple[str, int, str]] = []
    lines = strip_comments(text).splitlines()
    index = 0
    while index < len(lines):
        match = DECL.match(lines[index])
        if not match:
            index += 1
            continue
        first_line = index + 1  # 1-based
        joined = match.group("type")
        stop = index
        while (
            _top_level_eq(joined) < 0
            and stop + 1 < len(lines)
            and stop - index < MAX_DECL_CONTINUATION_LINES
        ):
            stop += 1
            joined += " " + lines[stop].strip()
        cut = _top_level_eq(joined)
        index = stop + 1
        if cut < 0:
            continue
        out.append((match.group("name"), first_line, " ".join(joined[:cut].split())))
    return out


def declarations_in_perry_tls(text: str) -> set[tuple[str, int]]:
    """Return (name, line) for declarations inside Perry TLS macro blocks.

    Brace matching runs on comment/string-stripped source, so braces in docs
    and initializers cannot terminate a block early. Both the exported and
    `crate::` spellings are accepted; the declaration syntax inside is the
    same as `thread_local!`.
    """
    code = strip_comments(text)
    found: set[tuple[str, int]] = set()
    for match in PERRY_TLS_BLOCK.finditer(code):
        open_at = code.find("{", match.start(), match.end())
        if open_at < 0:
            continue
        depth = 0
        close_at = -1
        for index in range(open_at, len(code)):
            char = code[index]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    close_at = index
                    break
        if close_at < 0:
            continue
        open_line = code.count("\n", 0, open_at) + 1
        block_body = code[open_at + 1 : close_at]
        for name, block_line, _type_text in declarations("", block_body):
            found.add((name, open_line + block_line - 1))
    return found


def holder_is_candidate(name: str, type_text: str, allocating_context: str) -> str | None:
    """Return the rule that makes this a candidate, or None (CORE crates).

    `allocating_context` is the concatenated text of every function in the
    declaring file that calls a GC allocator.
    """
    if FN_POINTER.search(type_text):
        return None
    if any(token in type_text for token in HEAP_TYPE_TOKENS):
        return "A"
    if not any(re.search(r"\b%s\b" % re.escape(t), type_text) for t in INT_TYPE_TOKENS):
        return None
    if re.search(r"\b%s\b" % re.escape(name), allocating_context):
        return "B"
    return None


# --- ffi-side type-shape rules --------------------------------------------


def _split_generic_args(text: str) -> list[str]:
    args: list[str] = []
    depth = 0
    current = ""
    for char in text:
        if char in "<(":
            depth += 1
        elif char in ">)":
            depth -= 1
        if char == "," and depth == 0:
            args.append(current.strip())
            current = ""
        else:
            current += char
    if current.strip():
        args.append(current.strip())
    return args


_OUTER_GENERIC = re.compile(r"^(?:[\w:]*::)?(\w+)\s*<(.*)>$", re.S)
_BARE_NAME = re.compile(r"^(?:[\w:]*::)?(\w+)$")


def _outer_type(type_text: str) -> tuple[str, str | None]:
    """('Vec', 'i64') for `Vec<i64>`, ('tuple', inner) for `(A, B)`, else (text, None)."""
    text = type_text.strip().rstrip(";").strip()
    while text.startswith("&"):
        text = text.lstrip("&").replace("'static", "", 1).strip()
    if text.startswith("(") and text.endswith(")"):
        return ("tuple", text[1:-1])
    match = _OUTER_GENERIC.match(text)
    if match:
        return (match.group(1), match.group(2))
    match = _BARE_NAME.match(text)
    if match:
        return (match.group(1), None)
    return (text, None)


def value_position_leaves(type_text: str, key_position: bool = False):
    """Yield (leaf_name, in_key_position) over a type's shape.

    Map KEYS are marked so rule V can exempt them: a `HashMap<i64, _>` key is
    a handle id, while a `Vec<i64>` ELEMENT is (in the ffi crates) very often
    a parked closure. Unknown generic wrappers contribute their own name as a
    leaf AND have their arguments walked as values — erring toward candidacy,
    which the inventory can correct; the reverse error is silent.
    """
    name, args = _outer_type(type_text)
    if name == "tuple":
        for arg in _split_generic_args(args or ""):
            yield from value_position_leaves(arg, key_position)
        return
    # An atomic integer cell IS its integer: `AtomicI64` parks NaN-box bits
    # exactly like a `Cell<i64>` (INPUT_HANDLER was the runtime's precedent).
    name = {"AtomicI64": "i64", "AtomicU64": "u64", "AtomicUsize": "usize"}.get(name, name)
    if args is None:
        yield (name, key_position)
        return
    arg_list = _split_generic_args(args)
    if name in KEYED_CONTAINERS:
        if arg_list:
            yield from value_position_leaves(arg_list[0], True)
        for arg in arg_list[1:]:
            yield from value_position_leaves(arg, key_position)
        return
    if name in WALK_CONTAINERS:
        for arg in arg_list:
            yield from value_position_leaves(arg, key_position)
        return
    yield (name, key_position)
    for arg in arg_list:
        yield from value_position_leaves(arg, key_position)


STRUCT_OR_ENUM_DEF = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum)\s+(\w+)")
_FIELD_PIECE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:r#)?\w+\s*:\s*(.+)$", re.S)
_VARIANT_PIECE = re.compile(r"^\s*(?:r#)?\w+\s*\((.*)\)\s*$", re.S)
MAX_STRUCT_RESOLUTION_DEPTH = 3


def _split_body_pieces(text: str) -> list[str]:
    """Split a struct/enum body on top-level commas ({}, <>, () all nest)."""
    pieces: list[str] = []
    depth = 0
    current = ""
    for char in text:
        if char in "<({":
            depth += 1
        elif char in ">)}":
            depth -= 1
        if char == "," and depth == 0:
            pieces.append(current.strip())
            current = ""
        else:
            current += char
    if current.strip():
        pieces.append(current.strip())
    return pieces


def crate_type_index(texts_by_file: list[str]) -> dict[str, list[str]]:
    """struct/enum name -> the field/variant-argument types it contains.

    Brace-counted over comment-stripped text, like `function_bodies`. Enum
    tuple variants count (`Retry(u64, i64)` can park a closure exactly like a
    named field can), and so do single-line bodies (`struct P { cb: i64 }`) —
    a line-anchored field regex silently dropped those, which un-resolved
    every type the self-test plants compactly.
    """
    index: dict[str, list[str]] = {}
    for text in texts_by_file:
        lines = strip_comments(text).splitlines()
        for start, line in enumerate(lines):
            match = STRUCT_OR_ENUM_DEF.match(line)
            if not match:
                continue
            depth = 0
            started = False
            body: list[str] = []
            for body_line in lines[start:]:
                body.append(body_line)
                depth += body_line.count("{") - body_line.count("}")
                if "{" in body_line:
                    started = True
                if started and depth <= 0:
                    break
                if not started and ";" in body_line:
                    break  # unit or tuple struct `struct X(A, B);`
            body_text = "\n".join(body)
            open_brace = body_text.find("{")
            if open_brace >= 0:
                inner = body_text[open_brace + 1 : body_text.rfind("}")]
            else:
                # tuple struct: `struct X(A, B);`
                open_paren = body_text.find("(")
                if open_paren < 0:
                    continue
                inner = body_text[open_paren + 1 : body_text.rfind(")")]
                index.setdefault(match.group(1), []).extend(_split_body_pieces(inner))
                continue
            field_types: list[str] = []

            def collect_pieces(chunk: str, depth: int = 0) -> None:
                for piece in _split_body_pieces(chunk):
                    if "{" in piece and depth < 2:
                        # enum STRUCT variant: `ClientClose { callback: i64 }`
                        # — the exact shape ext-http parks closures in.
                        collect_pieces(
                            piece[piece.find("{") + 1 : piece.rfind("}")], depth + 1
                        )
                        continue
                    field = _FIELD_PIECE.match(piece)
                    if field:
                        field_types.append(field.group(1).strip())
                        continue
                    variant = _VARIANT_PIECE.match(piece)
                    if variant:
                        field_types.extend(_split_generic_args(variant.group(1)))

            collect_pieces(inner)
            index.setdefault(match.group(1), []).extend(field_types)
    return index


def type_can_hold_js_value(
    type_text: str,
    type_index: dict[str, list[str]],
    depth: int = 0,
    seen: frozenset = frozenset(),
) -> bool:
    """Can this type's VALUE positions hold a NaN-boxed JS value / GC address?

    Resolves crate-local struct/enum names recursively (bounded, cycle-safe).
    """
    if any(token in type_text for token in FFI_HEAP_TYPE_TOKENS):
        return True
    for leaf, key_position in value_position_leaves(type_text):
        if key_position:
            continue
        if leaf in FFI_VALUE_PRIMITIVES:
            return True
        if "dyn" in leaf.split():
            return True
        if depth < MAX_STRUCT_RESOLUTION_DEPTH and leaf in type_index and leaf not in seen:
            for field_type in type_index[leaf]:
                if type_can_hold_js_value(
                    field_type, type_index, depth + 1, seen | {leaf}
                ):
                    return True
    return False


def ffi_holder_is_candidate(
    name: str,
    type_text: str,
    type_index: dict[str, list[str]],
    closure_context: str,
) -> str | None:
    """Return the rule that makes this ffi/ui-crate declaration a candidate.

    `closure_context` is the concatenated text of every function in the
    declaring file that touches closure/NaN-box machinery (rule F's
    qualifier).
    """
    if FN_POINTER.search(type_text):
        return None
    if any(token in type_text for token in FFI_HEAP_TYPE_TOKENS):
        return "A"
    leaves = list(value_position_leaves(type_text))
    has_collection = any(
        re.search(r"\b%s\s*<" % re.escape(container), type_text)
        for container in COLLECTION_NAMES
    )
    prim_in_value_position = any(
        leaf in FFI_VALUE_PRIMITIVES and not key for leaf, key in leaves
    )
    erased_in_value_position = any(
        "dyn" in leaf.split() and not key for leaf, key in leaves
    )
    if erased_in_value_position:
        return "E"
    if prim_in_value_position and has_collection:
        return "V"
    for leaf, key in leaves:
        if key or leaf in FFI_VALUE_PRIMITIVES:
            continue
        if leaf in type_index and type_can_hold_js_value(leaf, type_index):
            return "S"
    if prim_in_value_position and re.search(r"\b%s\b" % re.escape(name), closure_context):
        return "F"
    return None


def scan(root: Path) -> tuple[list[dict], int, set[str]]:
    tiered = tiered_source_files(root)
    tier_of: dict[Path, str] = {path: tier for path, tier in tiered}
    texts: dict[Path, str] = {}
    for path, _tier in tiered:
        try:
            texts[path] = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue

    # Per-crate struct/enum field index for the ffi-side rule S. Keyed by the
    # crate directory so `SocketState` in perry-ext-net cannot certify (or
    # condemn) a same-named type in another crate.
    crate_texts: dict[str, list[str]] = {}
    for path, text in texts.items():
        if tier_of[path] == "core":
            continue
        crate_key = repo_relative(path, root).split("/src/", 1)[0]
        crate_texts.setdefault(crate_key, []).append(text)
    type_index_by_crate = {
        crate: crate_type_index(crate_files) for crate, crate_files in crate_texts.items()
    }

    # 1. registered scanner entry points, WITH the module path they were
    #    registered under.
    #
    # Qualification matters at this hop specifically. `bodies` is keyed on the
    # bare function name, so two modules defining `scan_roots_mut` share a key —
    # and registering ONE of them would otherwise make the OTHER module's body
    # reachable, marking a holder in that module covered when nothing scans it.
    # This tree really does that: `perry-runtime` and `perry-stdlib` both define
    # `scan_tls_roots_mut`, and `worker_threads` has several `scan_*_roots_mut`
    # siblings. The registration text carries the path
    # (`crate::json::raw_json::scan_raw_json_key_root_mut`), so the ROOT set can
    # be resolved to a file even though later hops cannot.
    # Only BARE-PATH arguments seed the walk. The C-ABI registration form
    # (`..._named(SOURCE.as_ptr(), SOURCE.len(), 0, trampoline)`) puts method
    # calls in argument position, and harvesting every identifier out of them
    # seeds `as_ptr`/`len`/`state` — names that ARE `fn`s somewhere in these
    # crates (impl methods), which makes half the tree "reachable" and
    # certifies holders nothing scans. False coverage is the one direction the
    # inventory cannot correct, so the filter errs the other way: an argument
    # containing `.`/`!`/`$`/digits is not a scanner path and contributes
    # nothing.
    registered_paths: list[tuple[str, list[str]]] = []
    bare_path = re.compile(r"(?:\w+::)*[A-Za-z_]\w*")
    for text in texts.values():
        for match in REGISTER_CALL.finditer(strip_comments(text)):
            for arg in _split_generic_args(match.group("args")):
                if not bare_path.fullmatch(arg.strip()):
                    continue
                segments = arg.strip().split("::")
                registered_paths.append((segments[-1], segments[:-1]))

    # 2. call graph over every fn in the scanned crates
    bodies: dict[str, list[tuple[Path, str]]] = {}
    for path, text in texts.items():
        for name, body in function_bodies(text).items():
            bodies.setdefault(name, []).append((path, body))

    # Deep hops are matched on the BARE name, and the platform-ported crates
    # define whole files of same-named functions (every perry-ui* crate has a
    # `media_playback.rs` mirroring perry-runtime's). Without a fence, a
    # runtime scanner's helper name drags the UI twin's body into the
    # reachable set and 27 real frontier debt items read as covered. The
    # fence: a crate in which NOTHING registers a scanner cannot host
    # genuinely reachable scanner code — no registered scanner can call into
    # it (scanners only call within their own crate or into perry-ffi, both
    # of which register). Depth-0 seeds stay exempt: a registration that
    # NAMES a function in such a crate pins it explicitly, which is exactly
    # what fixing a UI crate will look like.
    registering_crates: set[str] = set()
    for path, text in texts.items():
        if REGISTER_CALL.search(strip_comments(text)):
            registering_crates.add(repo_relative(path, root).split("/src/", 1)[0])

    def crate_hosts_reachable_code(path: Path) -> bool:
        return repo_relative(path, root).split("/src/", 1)[0] in registering_crates

    def path_matches(defining: Path, segments: list[str]) -> bool:
        """Does `defining` plausibly hold the item named by `segments`?

        `crate::json::raw_json::f` -> `.../json/raw_json.rs` or
        `.../json/raw_json/mod.rs`. A bare `f` (no module path) matches
        anything: nothing was asserted, so nothing is excluded.
        """
        mods = [s for s in segments if s not in ("crate", "self", "super")]
        if not mods:
            return True
        parts = list(defining.with_suffix("").parts)
        if parts and parts[-1] == "mod":
            parts = parts[:-1]
        return mods[-1] in parts

    # Seed the frontier with (name, defining file) pairs the registration
    # actually names. A name that resolves to exactly one definition needs no
    # qualification; one that resolves to several must match the path.
    seeds: set[str] = set()
    seed_files: dict[str, set[Path]] = {}
    for name, segments in registered_paths:
        definitions = bodies.get(name)
        if not definitions:
            # Not a function in these crates — a type name (`as
            # MutableRootScanner`) or a path segment. Seeding it would make half
            # the crate reachable.
            continue
        if len(definitions) == 1:
            matched = definitions
        else:
            matched = [(p, b) for (p, b) in definitions if path_matches(p, segments)]
            if not matched:
                matched = definitions  # unresolvable: fall back, over-approximate
        seeds.add(name)
        seed_files.setdefault(name, set()).update(p for p, _ in matched)

    reachable: set[str] = set()
    frontier = set(seeds)
    # Files whose functions may be walked for a given reachable name. Root-level
    # names are pinned to the registration's file(s); deeper hops are not
    # (nothing in the text says which module a call resolved to), and the
    # docstring says so.
    for depth in range(MAX_SCANNER_DEPTH):
        nxt: set[str] = set()
        for name in frontier:
            if name in reachable:
                continue
            reachable.add(name)
            allowed = seed_files.get(name) if depth == 0 else None
            for path, body in bodies.get(name, []):
                if allowed is not None and path not in allowed:
                    continue
                if allowed is None and not crate_hosts_reachable_code(path):
                    continue
                nxt.update(IDENT.findall(body))
        frontier = {n for n in nxt if n in bodies and n not in reachable}
        if not frontier:
            break

    # per-file text of every reachable function defined in that file
    reachable_text_by_file: dict[Path, str] = {}
    for name in reachable:
        allowed = seed_files.get(name)
        for path, body in bodies.get(name, []):
            if allowed is not None and path not in allowed:
                continue
            if allowed is None and not crate_hosts_reachable_code(path):
                continue
            reachable_text_by_file[path] = reachable_text_by_file.get(path, "") + "\n" + body

    # 3. classify declarations
    holders: list[dict] = []
    for path, text in texts.items():
        rel = repo_relative(path, root)
        tier = tier_of[path]
        perry_tls_declarations = (
            declarations_in_perry_tls(text) if tier == "core" else set()
        )
        bodies_here = function_bodies(text)
        covered_text = reachable_text_by_file.get(path, "")
        if tier == "core":
            allocating_context = "\n".join(
                body
                for _name, body in bodies_here.items()
                if any(token in body for token in ALLOCATOR_TOKENS)
            )
        else:
            crate_key = rel.split("/src/", 1)[0]
            type_index = type_index_by_crate.get(crate_key, {})
            closure_context = "\n".join(
                body
                for _name, body in bodies_here.items()
                if any(token in body for token in FFI_CLOSURE_CONTEXT_TOKENS)
            )
        for name, lineno, type_text in declarations(rel, text):
            if tier == "core":
                rule = holder_is_candidate(name, type_text, allocating_context)
                if rule is None and (name, lineno) in perry_tls_declarations:
                    rule = "T"
            else:
                rule = ffi_holder_is_candidate(name, type_text, type_index, closure_context)
            if rule is None:
                continue
            covered = re.search(r"\b%s\b" % re.escape(name), covered_text) is not None
            holders.append(
                {
                    "file": rel,
                    "line": lineno,
                    "name": name,
                    "type": type_text[:160],
                    "rule": rule,
                    "tier": tier,
                    "ratchet": tier == "frontier" or rule == "T",
                    "covered": covered,
                }
            )
    holders.sort(key=lambda h: (h["file"], h["line"]))
    return holders, len(seeds), seeds


# The verdict vocabulary. An entry outside it is a typo or an invention, and
# either way `apply_inventory` must not accept it as a classification.
VERDICTS = {
    "covered_elsewhere",  # a registered scanner in ANOTHER file visits it
    "not_a_gc_pointer",  # id, counter, epoch, code address, .rodata, Rust-owned
    "test_only",  # #[cfg(test)] storage
    "open_gap",  # a real unrooted GC pointer, with an issue
    "unverified",  # enumerated, verdict not established — a dated TODO
}

# `unverified` is the one verdict that classifies nothing. It exists so a hole
# the gate CAN see is named rather than silent, and it is capped so the list
# cannot quietly become the whole inventory — at which point the gate would be a
# directory of unanswered questions rather than a decision record.
MAX_UNVERIFIED = 2


def load_inventory(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return json.loads(path.read_text(encoding="utf-8"))["holders"]


def load_frontier(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return json.loads(path.read_text(encoding="utf-8")).get("frontier", [])


def apply_frontier(
    holders: list[dict],
    frontier: list[dict],
    registered_scanners: set[str] | None = None,
) -> tuple[list[dict], list[dict]]:
    """(new_unpinned, stale_entries) for the frontier ratchet.

    A COVERED frontier holder must not be pinned either — coverage is the fix,
    and the deletion of its entry is the receipt (same rule as the gated
    inventory). An uncovered declaration whose scanner lives in another file
    may pin that scanner's exact registered name; deleting the registration
    then invalidates the pin instead of silently leaving it matched.
    """
    pinned = {(entry["file"], entry["name"]): entry for entry in frontier}
    used: set[tuple[str, str]] = set()
    unpinned: list[dict] = []
    for holder in holders:
        if not holder.get("ratchet", holder.get("tier") == "frontier"):
            continue
        key = (holder["file"], holder["name"])
        if holder["covered"]:
            continue
        entry = pinned.get(key)
        required_scanner = (entry or {}).get("scanner")
        scanner_is_live = (
            not required_scanner
            or registered_scanners is None
            or required_scanner in registered_scanners
        )
        if entry is None or not scanner_is_live:
            unpinned.append(holder)
        else:
            used.add(key)
    stale = [e for e in frontier if (e["file"], e["name"]) not in used]
    return unpinned, stale


def inventory_problems(inventory: list[dict]) -> list[str]:
    """Structural checks on the inventory itself.

    Without these, `apply_inventory` accepts any object carrying a matching
    (file, name): an entry with no reason, an invented verdict, or a
    `covered_elsewhere` naming no scanner would each silence a holder. A
    suppression whose justification cannot be read or checked is not a decision
    record, it is a mute button.
    """
    problems: list[str] = []
    unverified = 0
    seen: set[tuple[str, str]] = set()
    for entry in inventory:
        label = f"{entry.get('file', '?')}:{entry.get('name', '?')}"
        key = (entry.get("file", ""), entry.get("name", ""))
        if key in seen:
            problems.append(f"{label}: duplicate entry")
        seen.add(key)
        verdict = entry.get("verdict")
        if verdict not in VERDICTS:
            problems.append(f"{label}: verdict {verdict!r} is not one of {sorted(VERDICTS)}")
        why = (entry.get("why") or "").strip()
        if len(why) < 20:
            problems.append(
                f"{label}: `why` is missing or too short to be a reason ({why!r}) — an "
                f"unreadable justification silences a holder just as effectively as no gate"
            )
        if verdict == "covered_elsewhere" and not (entry.get("scanner") or "").strip():
            problems.append(
                f"{label}: covered_elsewhere must name the `scanner` that covers it, or "
                f"the claim cannot be checked or maintained"
            )
        if verdict == "open_gap" and not (entry.get("issue") or "").strip():
            problems.append(f"{label}: open_gap must cite an `issue`")
        if verdict in {"open_gap", "unverified"}:
            problems.append(
                f"{label}: `{verdict}` is not a shippable old-page relocation verdict. "
                "Rewrite/invalidate the holder, prove it cannot hold a movable GC address, "
                "or keep relocation disabled."
            )
        if verdict == "unverified":
            unverified += 1
    if unverified > MAX_UNVERIFIED:
        problems.append(
            f"{unverified} `unverified` entries, cap is {MAX_UNVERIFIED}. `unverified` is "
            f"a dated TODO, not an exemption — take some to a verdict before adding another."
        )
    return problems


def apply_inventory(
    holders: list[dict], inventory: list[dict]
) -> tuple[list[dict], list[dict]]:
    index = {(entry["file"], entry["name"]): entry for entry in inventory}
    used: set[tuple[str, str]] = set()
    unclassified: list[dict] = []
    for holder in holders:
        if holder.get("ratchet", holder.get("tier", "core") == "frontier"):
            continue  # ratcheted by apply_frontier, not verdict-gated
        if holder["covered"]:
            continue
        key = (holder["file"], holder["name"])
        if key in index:
            used.add(key)
        else:
            unclassified.append(holder)
    stale = [e for e in inventory if (e["file"], e["name"]) not in used]
    return unclassified, stale


def state_struct_field_count(root: Path) -> int:
    path = root / STATE_FILE
    if not path.exists():
        return -1
    text = strip_comments(path.read_text(encoding="utf-8", errors="replace"))
    start = text.find(STATE_STRUCT)
    if start < 0:
        return -1
    body = text[start : text.find("\n}", start)]
    return len(re.findall(r"^\s*(?:pub(?:\([^)]*\))?\s+)?\w+\s*:\s*\w", body, re.M))


def report(root: Path, quiet: bool = False) -> int:
    holders, registered_count, registered_scanners = scan(root)
    if len(holders) < MIN_HOLDERS:
        print(
            f"gc_runtime_root_holders: matched only {len(holders)} holder "
            f"declarations, expected at least {MIN_HOLDERS}. The scan is broken "
            f"— a green run here would be vacuous.",
            file=sys.stderr,
        )
        return 2
    if registered_count < MIN_REGISTERED:
        print(
            f"gc_runtime_root_holders: found only {registered_count} registered "
            f"root scanners, expected at least {MIN_REGISTERED}. The registration "
            f"regex is broken, so EVERY holder would read as uncovered.",
            file=sys.stderr,
        )
        return 2

    fields = state_struct_field_count(root)
    if fields >= 0 and fields > STATE_FIELD_FLOOR:
        print(
            f"gc_runtime_root_holders: RuntimeState now has {fields} fields "
            f"(this gate was verified at {STATE_FIELD_FLOOR}). Its fields are NOT "
            f"declarations and are invisible to this scan — read the new field, "
            f"confirm it is covered or add it to the inventory with "
            f'"file": "{STATE_FILE}", then raise STATE_FIELD_FLOOR.',
            file=sys.stderr,
        )
        return 1

    inventory = load_inventory(INVENTORY_PATH)
    unclassified, stale = apply_inventory(holders, inventory)
    malformed = inventory_problems(inventory)
    frontier = load_frontier(INVENTORY_PATH)
    frontier_new, frontier_stale = apply_frontier(
        holders, frontier, registered_scanners
    )

    status = 0
    if frontier_new:
        status = 1
        print(
            "\ngc_runtime_root_holders: NEW identity-ratcheted holders not pinned in\n"
            "the inventory's `frontier` list. This covers perry-ui* callback tables\n"
            "and otherwise-unclassified core `perry_thread_local!` declarations.\n"
            "Register a scanner that reaches the holder, record a researched verdict\n"
            "where the gated rules apply, or pin existing debt deliberately — with the\n"
            "understanding that a pinned entry is not a GC-safety verdict.\n",
            file=sys.stderr,
        )
        for holder in frontier_new:
            print(
                f"  {holder['file']}:{holder['line']}: {holder['name']}: "
                f"{holder['type']}  [rule {holder['rule']}]",
                file=sys.stderr,
            )
    if frontier_stale:
        status = 1
        print(
            "\ngc_runtime_root_holders: these `frontier` entries no longer match an\n"
            "uncovered holder — delete them. (Going stale is what FIXING one looks\n"
            "like: the deletion is the receipt.)\n",
            file=sys.stderr,
        )
        for entry in frontier_stale:
            print(f"  {entry['file']} | {entry['name']}", file=sys.stderr)
    if malformed:
        status = 1
        print(
            "\ngc_runtime_root_holders: the inventory itself is malformed. Each of\n"
            "these entries silences a holder without recording a usable reason.\n",
            file=sys.stderr,
        )
        for problem in malformed:
            print(f"  {problem}", file=sys.stderr)
    if unclassified:
        status = 1
        print(
            "Unclassified runtime GC-pointer holders (#7231).\n"
            "\n"
            "Each of these is a process-global or thread-local whose type can hold a\n"
            "pointer into the GC heap, and NO registered root scanner in its own file\n"
            "mentions it. That is either a missing root — which goes bad at collection\n"
            "#0 and stays bad — or a holder that does not really store a GC pointer.\n"
            "Decide which, and record the decision in\n"
            "scripts/gc_runtime_root_holders.json. A list nobody checks is how this\n"
            "class got here.\n",
            file=sys.stderr,
        )
        for holder in unclassified:
            print(
                f"  {holder['file']}:{holder['line']}: {holder['name']}: "
                f"{holder['type']}  [rule {holder['rule']}]",
                file=sys.stderr,
            )
    if stale:
        status = 1
        print(
            "\ngc_runtime_root_holders: these inventory entries no longer match an\n"
            "uncovered holder. Delete them — a stale exemption is how this gate stops\n"
            "being one. (An entry also goes stale when the holder becomes COVERED,\n"
            "which is exactly what a fix looks like.)\n",
            file=sys.stderr,
        )
        for entry in stale:
            print(f"  {entry['file']} | {entry['name']} | {entry['why']}", file=sys.stderr)

    if status == 0 and not quiet:
        covered = sum(1 for h in holders if h["covered"])
        frontier_count = sum(1 for h in holders if h.get("ratchet"))
        print(
            f"gc_runtime_root_holders: OK — {len(holders)} holder declarations "
            f"scanned ({frontier_count} identity-ratcheted), {covered} reached by a registered "
            f"scanner, {len(inventory)} classified in the inventory, "
            f"{len(frontier)} pinned on the frontier ratchet "
            f"({registered_count} registered scanners)."
        )
        # State the blind spots on every green run rather than letting
        # silence imply coverage (the #8206 pattern; details in the module
        # docstring under "What this gate CANNOT see").
        print(
            "  UNVERIFIED by this gate: RuntimeState struct fields "
            "(growth-floor only); core-crate integer tables in files that "
            f"never call an allocator (rule B's limit); and the "
            f"{len(frontier)} pinned frontier holders, which are ENUMERATED "
            "and RATCHETED but scanned by nothing — a value parked there may "
            "still be invisible to the collector."
        )
    return status


def print_list(root: Path) -> int:
    holders, registered_count, _registered_scanners = scan(root)
    print(f"# {len(holders)} candidate holders, {registered_count} registered scanners")
    for holder in holders:
        flag = "COVERED  " if holder["covered"] else "UNCOVERED"
        print(
            f"{flag} [{holder['tier']}/{holder['rule']}] "
            f"{holder['file']}:{holder['line']} {holder['name']}: {holder['type']}"
        )
    return 0


# --- self-test -------------------------------------------------------------

SELF_TEST_TREE = {
    # A registered scanner that reaches ONE holder directly and another through
    # an accessor. Both must read as covered.
    "crates/perry-runtime/src/gc/mod.rs": """
pub fn gc_init() {
    gc_register_mutable_root_scanner(crate::thing::scan_thing_roots_mut);
    gc_register_mutable_root_scanner(crate::other::scan_other_roots_mut);
    gc_register_mutable_root_scanner(crate::dup_a::scan_dup_roots_mut);
""" + "\n".join(
        f"    gc_register_mutable_root_scanner(crate::pad::scan_pad_{i}_mut);"
        for i in range(MIN_REGISTERED)
    ) + """
}
""",
    "crates/perry-runtime/src/thing.rs": """
static COVERED_DIRECT: RefCell<Vec<*mut ObjectHeader>> = RefCell::new(Vec::new());
static COVERED_VIA_ACCESSOR: Mutex<Option<Vec<*mut ClosureHeader>>> = Mutex::new(None);
struct CoveredOpaqueTls { value: f64 }
crate::perry_thread_local! {
    static COVERED_OPAQUE_TLS: CoveredOpaqueTls = CoveredOpaqueTls { value: 0.0 };
}
fn accessor() -> &'static Mutex<Option<Vec<*mut ClosureHeader>>> { &COVERED_VIA_ACCESSOR }
pub fn scan_thing_roots_mut(v: &mut V) {
    for p in COVERED_DIRECT.borrow_mut().iter_mut() { v.visit(p); }
    for p in accessor().lock().unwrap().iter_mut() { v.visit(p); }
    COVERED_OPAQUE_TLS.with(|slot| v.visit(&mut slot.value));
    let _ = js_object_alloc(0, 0);
}
""",
    # An UNCOVERED holder of each rule, plus a same-name decoy in another file
    # that IS covered — the collision case.
    "crates/perry-runtime/src/leak.rs": """
static UNCOVERED_TYPED: Cell<*mut ArrayHeader> = Cell::new(std::ptr::null_mut());
static UNCOVERED_INT: Cell<f64> = Cell::new(0.0);
struct OpaqueTls { value: String }
perry_thread_local! {
    static UNCOVERED_OPAQUE_TLS: OpaqueTls = OpaqueTls { value: String::new() };
}
fn populate() { let o = js_object_alloc(0, 0); UNCOVERED_INT.set(o as f64); }
""",
    "crates/perry-runtime/src/other.rs": """
static REGISTRY: RefCell<Vec<*mut ObjectHeader>> = RefCell::new(Vec::new());
pub fn scan_other_roots_mut(v: &mut V) { for p in REGISTRY.borrow_mut().iter_mut() { v.visit(p); } }
""",
    "crates/perry-runtime/src/collide.rs": """
static REGISTRY: RefCell<Vec<*mut StringHeader>> = RefCell::new(Vec::new());
fn use_it() { let _ = js_string_from_bytes(std::ptr::null(), 0); }
""",
    # Two modules defining the SAME scanner name; only dup_a's is registered.
    # dup_b's holder must stay uncovered — this is the shape a bare-name call
    # graph gets wrong, and it exists for real (`scan_tls_roots_mut` is defined
    # in both perry-runtime and perry-stdlib).
    "crates/perry-runtime/src/dup_a.rs": """
static DUP_A_TABLE: RefCell<Vec<*mut ObjectHeader>> = RefCell::new(Vec::new());
pub fn scan_dup_roots_mut(v: &mut V) { for p in DUP_A_TABLE.borrow_mut().iter_mut() { v.visit(p); } }
""",
    "crates/perry-runtime/src/dup_b.rs": """
static DUP_B_TABLE: RefCell<Vec<*mut ObjectHeader>> = RefCell::new(Vec::new());
pub fn scan_dup_roots_mut(v: &mut V) { for p in DUP_B_TABLE.borrow_mut().iter_mut() { v.visit(p); } }
""",
    # An ffi-side crate registering through the C-ABI trampoline. Exercises:
    # nested parens in the registration args (`SOURCE.as_ptr()`), an
    # `extern "C" fn` seed body (invisible to FN_DEF before the stripped-
    # literal fix), a function-body `static _: OnceLock<..>` reached through
    # its accessor, a multi-line rule-V declaration, rule E (`dyn` payload),
    # and rule F both ways (a scalar in closure context vs a plain counter).
    "crates/perry-ext-fake/src/lib.rs": """
struct FakeParked { cb: i64, name: String }
fn parked() -> &'static Mutex<HashMap<u64, FakeParked>> {
    static PARKED: OnceLock<Mutex<HashMap<u64, FakeParked>>> = OnceLock::new();
    PARKED.get_or_init(|| Mutex::new(HashMap::new()))
}
static EXT_LISTENERS: Mutex<HashMap<usize,
    Vec<i64>>> =
    Mutex::new(HashMap::new());
static EXT_ERASED: Lazy<DashMap<Handle, Box<dyn Any + Send + Sync>>> = Lazy::new(DashMap::new);
static EXT_LAST_CB: AtomicI64 = AtomicI64::new(0);
static EXT_COUNTER: AtomicI64 = AtomicI64::new(0);
static EXT_IDS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
fn stash_cb(value: f64) {
    let closure = value.to_bits() as i64;
    EXT_LAST_CB.store(closure, Ordering::SeqCst);
}
fn bump() { EXT_COUNTER.fetch_add(1, Ordering::SeqCst); }
fn ensure_registered() {
    const SOURCE: &str = "perry-ext-fake";
    unsafe {
        perry_ffi_gc_register_mutable_root_scanner_named(
            SOURCE.as_ptr(),
            SOURCE.len(),
            0,
            scan_fake_trampoline,
        );
    }
}
extern "C" fn scan_fake_trampoline(_id: usize, visit: Visit, ctx: Ctx) {
    scan_fake_roots(visit, ctx);
}
fn scan_fake_roots(visit: Visit, ctx: Ctx) {
    for entry in parked().lock().unwrap().values_mut() { visit(&mut entry.cb, ctx); }
    for cb_vec in EXT_LISTENERS.lock().unwrap().values_mut() {
        for cb in cb_vec.iter_mut() { visit(cb, ctx); }
    }
}
""",
    # An ffi-side crate that registers NOTHING: the planted catches. `TABLE`
    # is the exact shape the census could not see before this extension —
    # `static … OnceLock<…>` inside a function body whose value type is a
    # bare `i64` carrying a NaN-boxed JS value.
    "crates/perry-ext-leaky/src/lib.rs": """
struct LeakyListeners { cbs: Vec<i64> }
lazy_static! {
    static ref LEAKY_LISTENERS: Mutex<HashMap<usize, LeakyListeners>> = Mutex::new(HashMap::new());
}
fn table() -> &'static Mutex<HashMap<u64, i64>> {
    static TABLE: OnceLock<Mutex<HashMap<u64, i64>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}
static LEAKY_DEFERRED: Mutex<VecDeque<(u64,
    LeakyListeners)>> =
    Mutex::new(VecDeque::new());
""",
    # Frontier (perry-ui*) crate: enumerated + ratcheted, never verdict-gated.
    "crates/perry-ui-fake/src/widgets/button.rs": """
thread_local! {
    static BUTTON_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}
pub fn set_callback(key: usize, callback: f64) {
    BUTTON_CALLBACKS.with(|c| c.borrow_mut().insert(key, callback));
}
""",
    # The fence case: `accessor` is ALSO the name of a genuinely reachable
    # helper in thing.rs. perry-ui-fake registers nothing, so its same-named
    # body must NOT be attributed — without the fence NAV_TABLE reads
    # covered, which is exactly the 27-holder false-coverage bug this
    # extension hit on the real tree.
    "crates/perry-ui-fake/src/media.rs": """
static NAV_TABLE: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
fn accessor() -> u32 { let _ = NAV_TABLE.borrow(); 0 }
""",
}


def _scan_tree(extra: dict[str, str] | None = None) -> list[dict]:
    tree = dict(SELF_TEST_TREE)
    if extra:
        tree.update(extra)
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        # Pad the holder population past MIN_HOLDERS so the floor does not fire.
        pad = "\n".join(
            f"static PAD_{i}: Cell<*mut ObjectHeader> = Cell::new(std::ptr::null_mut());"
            for i in range(MIN_HOLDERS + 10)
        )
        pad_scan = "\n".join(
            f"pub fn scan_pad_{i}_mut(v: &mut V) {{ v.visit(&mut PAD_{i}); }}"
            for i in range(MIN_HOLDERS + 10)
        )
        tree["crates/perry-runtime/src/pad.rs"] = pad + "\n" + pad_scan + "\n"
        tree["crates/perry-runtime/src/gc/mod.rs"] = tree[
            "crates/perry-runtime/src/gc/mod.rs"
        ].replace(
            "}\n",
            "\n".join(
                f"    gc_register_mutable_root_scanner(crate::pad::scan_pad_{i}_mut);"
                for i in range(MIN_HOLDERS + 10)
            )
            + "\n}\n",
            1,
        )
        for rel, body in tree.items():
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            # Explicit UTF-8: a bare write_text() encodes with the host locale
            # (cp1252 on a Windows runner), which is the #7977 class. The
            # fixtures are ASCII today, so this is the latent half — the read
            # side of the same shape is what took `windows-build` down.
            path.write_text(body, encoding="utf-8", newline="")
        holders, _registered_count, _registered_scanners = scan(root)
        return holders


def self_test() -> int:
    failures: list[str] = []
    windows_root = PureWindowsPath(r"C:\perry")
    if repo_relative(windows_root / "crates" / "runtime.rs", windows_root) != "crates/runtime.rs":
        failures.append("repository-relative keys are not normalized on Windows")
    holders = _scan_tree()
    by_key = {(h["file"], h["name"]): h for h in holders}

    def expect(
        rel: str, name: str, covered: bool, why: str, tier: str | None = None
    ) -> None:
        key = (rel, name)
        if key not in by_key:
            failures.append(f"scanner MISSED the declaration {rel}:{name} ({why})")
            return
        if by_key[key]["covered"] != covered:
            failures.append(
                f"{rel}:{name} read as covered={by_key[key]['covered']}, "
                f"expected {covered} ({why})"
            )
        if tier is not None and by_key[key]["tier"] != tier:
            failures.append(
                f"{rel}:{name} classified in tier {by_key[key]['tier']!r}, "
                f"expected {tier!r} ({why})"
            )

    def expect_absent(rel: str, name: str, why: str) -> None:
        if (rel, name) in by_key:
            failures.append(f"{rel}:{name} should NOT be a candidate ({why})")

    expect(
        "crates/perry-runtime/src/thing.rs",
        "COVERED_DIRECT",
        True,
        "named directly in a registered scanner body",
    )
    expect(
        "crates/perry-runtime/src/thing.rs",
        "COVERED_VIA_ACCESSOR",
        True,
        "reached through one hop of accessor indirection — the cp_live_lock() shape",
    )
    expect(
        "crates/perry-runtime/src/thing.rs",
        "COVERED_OPAQUE_TLS",
        True,
        "crate::perry_thread_local! declaration with a type opaque to rules A/B",
    )
    expect(
        "crates/perry-runtime/src/leak.rs",
        "UNCOVERED_TYPED",
        False,
        "rule A: type names a heap header and nothing scans it",
    )
    expect(
        "crates/perry-runtime/src/leak.rs",
        "UNCOVERED_INT",
        False,
        "rule B: Cell<f64> in a file that allocates — the CACHED_ENV shape",
    )
    expect(
        "crates/perry-runtime/src/leak.rs",
        "UNCOVERED_OPAQUE_TLS",
        False,
        "unqualified perry_thread_local! declaration with an opaque type",
    )
    if by_key.get(
        ("crates/perry-runtime/src/thing.rs", "COVERED_OPAQUE_TLS"), {}
    ).get("rule") != "T":
        failures.append("qualified Perry TLS declaration was not classified by rule T")
    if by_key.get(
        ("crates/perry-runtime/src/leak.rs", "UNCOVERED_OPAQUE_TLS"), {}
    ).get("rule") != "T":
        failures.append("unqualified Perry TLS declaration was not classified by rule T")
    expect(
        "crates/perry-runtime/src/collide.rs",
        "REGISTRY",
        False,
        "SAME NAME as a covered holder in another file; a name-only match would "
        "certify the wrong one",
    )
    expect(
        "crates/perry-runtime/src/dup_a.rs",
        "DUP_A_TABLE",
        True,
        "its scanner IS the registered one",
    )
    expect(
        "crates/perry-runtime/src/dup_b.rs",
        "DUP_B_TABLE",
        False,
        "its scanner shares a NAME with the registered one but is a different "
        "function in a different module; registering dup_a's must not certify "
        "dup_b's holder",
    )

    # --- ffi-side shapes: the 2026-08 blind spot, planted -------------------
    expect(
        "crates/perry-ext-fake/src/lib.rs",
        "PARKED",
        True,
        "function-body OnceLock table, covered through accessor + C-ABI "
        "trampoline registration (nested parens + extern-C seed body)",
        tier="ffi",
    )
    expect(
        "crates/perry-ext-fake/src/lib.rs",
        "EXT_LISTENERS",
        True,
        "multi-line rule-V declaration named directly in the scanner",
        tier="ffi",
    )
    expect(
        "crates/perry-ext-fake/src/lib.rs",
        "EXT_ERASED",
        False,
        "rule E: dyn-erased payload in value position, nothing scans it",
        tier="ffi",
    )
    expect(
        "crates/perry-ext-fake/src/lib.rs",
        "EXT_LAST_CB",
        False,
        "rule F: bare scalar named by a closure-handling function",
        tier="ffi",
    )
    expect_absent(
        "crates/perry-ext-fake/src/lib.rs",
        "EXT_COUNTER",
        "a bare counter with no closure-machinery context must not be a "
        "candidate — rule F's qualifier is what keeps this census readable",
    )
    expect_absent(
        "crates/perry-ext-fake/src/lib.rs",
        "EXT_IDS",
        "Vec<usize> is an id list; usize is deliberately not a value primitive",
    )
    expect(
        "crates/perry-ext-leaky/src/lib.rs",
        "TABLE",
        False,
        "THE previously-invisible shape: static OnceLock<Mutex<HashMap<u64, "
        "i64>>> inside a function body, i64 value position, no scanner",
        tier="ffi",
    )
    expect(
        "crates/perry-ext-leaky/src/lib.rs",
        "LEAKY_LISTENERS",
        False,
        "lazy_static `static ref` + rule S through a crate-local struct's "
        "Vec<i64> field",
        tier="ffi",
    )
    expect(
        "crates/perry-ext-leaky/src/lib.rs",
        "LEAKY_DEFERRED",
        False,
        "multi-line declaration + rule S through a tuple payload",
        tier="ffi",
    )
    expect(
        "crates/perry-ui-fake/src/widgets/button.rs",
        "BUTTON_CALLBACKS",
        False,
        "the canonical UI callback table lands in the frontier tier",
        tier="frontier",
    )
    expect(
        "crates/perry-ui-fake/src/media.rs",
        "NAV_TABLE",
        False,
        "fence: `accessor` is also a reachable helper name in thing.rs, but "
        "perry-ui-fake registers nothing, so its body must not be attributed "
        "— without the fence this reads covered",
        tier="frontier",
    )

    # --- frontier ratchet red paths ----------------------------------------
    planted_frontier = [h for h in holders if h.get("ratchet") and not h["covered"]]
    if len(planted_frontier) < 3:
        failures.append("expected planted UI and Perry-TLS frontier holders")
    unpinned, _stale = apply_frontier(holders, [])
    if len(unpinned) != len(planted_frontier):
        failures.append(
            f"apply_frontier with an EMPTY baseline reported {len(unpinned)} "
            f"unpinned of {len(planted_frontier)} frontier holders — new UI "
            f"or Perry-TLS debt could land silently"
        )
    exact_baseline = [
        {"file": h["file"], "name": h["name"]} for h in planted_frontier
    ]
    unpinned, stale = apply_frontier(holders, exact_baseline)
    if unpinned or stale:
        failures.append(
            "an exact frontier baseline still reported "
            f"{len(unpinned)} unpinned / {len(stale)} stale"
        )
    _unpinned, stale = apply_frontier(
        holders,
        exact_baseline + [{"file": "crates/perry-ui-fake/src/gone.rs", "name": "GONE"}],
    )
    if not stale:
        failures.append(
            "a frontier entry matching no holder was NOT reported stale — "
            "the ratchet could not shrink"
        )
    scanner_pinned = [
        dict(entry, scanner="scan_external_tls_roots_mut")
        if entry["name"] == "UNCOVERED_OPAQUE_TLS"
        else entry
        for entry in exact_baseline
    ]
    live, stale = apply_frontier(
        holders, scanner_pinned, {"scan_external_tls_roots_mut"}
    )
    if live or stale:
        failures.append("a live cross-file scanner did not satisfy its frontier pin")
    dead, stale = apply_frontier(holders, scanner_pinned, set())
    if not any(h["name"] == "UNCOVERED_OPAQUE_TLS" for h in dead) or not any(
        e["name"] == "UNCOVERED_OPAQUE_TLS" for e in stale
    ):
        failures.append("deleting a cross-file scanner did not invalidate its pin")

    # A covered rule-T holder is deliberately absent from the baseline. If its
    # scanner registration disappears, it becomes new debt and MUST go red —
    # this is issue #8544's MODULE_PATH_REGISTRY failure direction.
    covered_tls = next(
        (
            h
            for h in holders
            if h.get("ratchet") and h["covered"] and h["rule"] == "T"
        ),
        None,
    )
    if covered_tls is None:
        failures.append("expected a covered rule-T holder in the self-test tree")
    else:
        scanner_deleted = [dict(h) for h in holders]
        for holder in scanner_deleted:
            if (
                holder["file"] == covered_tls["file"]
                and holder["name"] == covered_tls["name"]
            ):
                holder["covered"] = False
        newly_uncovered, _stale = apply_frontier(scanner_deleted, exact_baseline)
        if not any(
            h["file"] == covered_tls["file"] and h["name"] == covered_tls["name"]
            for h in newly_uncovered
        ):
            failures.append(
                "deleting a rule-T holder's scanner did not turn the ratchet red"
            )

    # Classification is only half of it — the VERDICT machinery has to go red.
    # An empty inventory must leave every uncovered GATED holder unclassified
    # (frontier/rule-T holders are the ratchet's subject, not the inventory's)…
    unclassified, _stale = apply_inventory(holders, [])
    uncovered = [h for h in holders if not h["covered"] and not h.get("ratchet")]
    if len(unclassified) != len(uncovered) or not uncovered:
        failures.append(
            f"apply_inventory with an EMPTY inventory reported {len(unclassified)} "
            f"unclassified of {len(uncovered)} uncovered — the gate cannot go red"
        )
    # …and an entry that matches nothing must be reported stale.
    _unclassified, stale_planted = apply_inventory(
        holders,
        [
            {
                "file": "crates/perry-runtime/src/does_not_exist.rs",
                "name": "GONE",
                "verdict": "not_a_gc_pointer",
                "why": "planted",
            }
        ],
    )
    if not stale_planted:
        failures.append(
            "a planted inventory entry matching no holder was NOT reported stale — "
            "a fix could then land without deleting its own exemption"
        )
    # A COVERED holder with an inventory entry is also stale: that is what makes
    # a fix delete its entry.
    covered_sample = next((h for h in holders if h["covered"]), None)
    if covered_sample is not None:
        _u, stale_covered = apply_inventory(
            holders,
            [
                {
                    "file": covered_sample["file"],
                    "name": covered_sample["name"],
                    "verdict": "open_gap",
                    "why": "planted: this holder is covered, so the entry must go stale",
                }
            ],
        )
        if not stale_covered:
            failures.append(
                "an inventory entry for a COVERED holder was not reported stale — "
                "fixing a gap would not force its entry to be deleted"
            )

    # And the inventory itself must be honest about the real tree.
    inventory = load_inventory(INVENTORY_PATH)
    real_holders, _registered_count, real_registered_scanners = scan(REPO_ROOT)
    _unclassified, stale = apply_inventory(real_holders, inventory)
    if stale:
        failures.append(
            "inventory has %d stale entr(y|ies): %s"
            % (len(stale), ", ".join(f"{e['file']}:{e['name']}" for e in stale))
        )
    _unpinned, frontier_stale = apply_frontier(
        real_holders,
        load_frontier(INVENTORY_PATH),
        real_registered_scanners,
    )
    if frontier_stale:
        failures.append(
            "frontier list has %d stale entr(y|ies): %s"
            % (
                len(frontier_stale),
                ", ".join(f"{e['file']}:{e['name']}" for e in frontier_stale[:5]),
            )
        )
    failures.extend(inventory_problems(inventory))
    # …and the structural checker must itself be able to fail.
    long_why = "x" * 30
    for bad, expect in (
        ({"file": "f", "name": "N", "verdict": "invented", "why": long_why}, "verdict"),
        ({"file": "f", "name": "N", "verdict": "not_a_gc_pointer", "why": "short"}, "why"),
        ({"file": "f", "name": "N", "verdict": "covered_elsewhere", "why": long_why}, "scanner"),
        ({"file": "f", "name": "N", "verdict": "open_gap", "why": long_why}, "issue"),
    ):
        if not any(expect in problem for problem in inventory_problems([bad])):
            failures.append(
                f"inventory_problems did not reject the malformed {expect!r} entry: {bad}"
            )
    over_cap = [
        {"file": f"f{i}", "name": "N", "verdict": "unverified", "why": long_why}
        for i in range(MAX_UNVERIFIED + 1)
    ]
    if not any("cap is" in problem for problem in inventory_problems(over_cap)):
        failures.append("the `unverified` cap does not fire")

    if failures:
        print("gc_runtime_root_holders self-test FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print(
        "gc_runtime_root_holders self-test: OK "
        f"({len(by_key)} planted declarations classified, "
        f"{len(inventory)} inventory entries checked)"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true", help="check the checker")
    parser.add_argument("--list", action="store_true", help="print every holder + verdict")
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.list:
        return print_list(REPO_ROOT)
    return report(REPO_ROOT, quiet=args.quiet)


if __name__ == "__main__":
    sys.exit(main())
