#!/usr/bin/env python3
"""Runtime-side GC-pointer holder custody gate (#7231).

A `thread_local!` or `static` in `perry-runtime` / `perry-stdlib` that stores a
pointer into the GC heap **is a GC root**, and the collector only knows that if
something registers it via `gc_register_*root_scanner*`. An unregistered holder
is not an intermittent bug: it goes bad at collection #0 and stays bad, so the
symptom is a *perfectly reproducible* use-after-free — the opposite tell from
the #7154 stale-register class.

Nothing static could find this class before. `scripts/gc_root_dominance_check.py`
reads emitted LLVM IR, and a runtime table is not in it — that is not a gap in
that tool, it is outside its subject. #7226, #7239, #7268 and #7274 were all
found by hand. This script is the enumeration those fixes kept re-deriving,
turned into something that can fail.

What it does
------------

1. **Enumerate** every `static` / `thread_local!` declaration in the two crates
   whose stored type can hold a GC heap pointer (rule A: the type names a heap
   header type or `JSValue`; rule B: the type is an integer/`f64` cell that
   some function in its own file both names and allocates in, which is how
   `CACHED_ENV: Cell<f64>` — the highest-impact holder in #7231's original
   report — has to be caught).

2. **Compute coverage** rather than trusting names. The registered scanner set
   is read from every `gc_register_*root_scanner*(...)` call site; a call graph
   over all `fn` bodies in both crates is walked from those roots to
   `MAX_SCANNER_DEPTH`, and a holder counts as covered when its identifier
   appears in a reachable function **defined in the same file as the
   declaration**. The same-file requirement is not incidental: `REGISTRY`,
   `SLOTS`, `ROOTS`, `STATES`, `CACHED` and `CLOSE_CALLBACK` each name several
   different holders in this tree, and a name-only match certifies the wrong
   one. The call-graph walk is what finds the holders a scanner reaches through
   an accessor (`cp_live_lock()`, `get_closure_props()`, `buffer_props()`)
   rather than by name.

3. **Require a verdict** for everything left over. Each uncovered holder must
   appear in `scripts/gc_runtime_root_holders.json` with a `verdict` and a
   `why`. A holder with no entry fails; an entry that matches no holder fails
   (a stale exemption is how these gates rot — same rule as
   `scripts/gc_root_dominance_allowlist.json`).

How it fails
------------

* a new uncovered holder with no inventory entry -> exit 1
* an inventory entry that no longer matches a declaration -> exit 1
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
  `field_lookup`, `shapes`). They are struct FIELDS, reached through `state()`,
  so no declaration-site scan sees them. All are covered today; a new field
  added there is invisible here. `STATE_FIELD_FLOOR` below asserts the struct
  has not grown past the field count this was checked at, so growth is at least
  *loud*.
* **An integer-typed holder whose own file never calls an allocator.** Rule B
  needs a function that both names the holder and allocates; a cell written
  purely from a value handed in across a module boundary has neither, and is
  invisible.
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
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
INVENTORY_PATH = REPO_ROOT / "scripts" / "gc_runtime_root_holders.json"

CRATES = ("crates/perry-runtime/src", "crates/perry-stdlib/src")

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
REGISTER_CALL = re.compile(r"gc_register_\w*root_scanner\w*\s*\((?P<args>[^;()]*)\)", re.S)
FN_DEF = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+|extern\s+\"C\"\s+)*fn\s+(\w+)")
IDENT = re.compile(r"\b[A-Za-z_]\w*\b")

# A declaration: `static NAME: TYPE =` — covers `pub(crate) static`, the bodies
# of `thread_local!` blocks (which use the same syntax), and `static NAME:
# Lazy<...>`.
DECL = re.compile(
    r"^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?static\s+(?P<name>[A-Z][A-Z0-9_]*)\s*:\s*(?P<type>[^=]+?)\s*="
)

MAX_SCANNER_DEPTH = 3

# Floors. Each is a "the extraction still works" assertion, not a budget.
MIN_HOLDERS = 60
MIN_REGISTERED = 60

# See "What this gate CANNOT see". `RuntimeState`'s fields are not declarations
# and are invisible to DECL; this makes the struct growing at least loud.
STATE_FILE = "crates/perry-runtime/src/state.rs"
STATE_STRUCT = "struct RuntimeState"
STATE_FIELD_FLOOR = 4


def source_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for crate in CRATES:
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
        while index < len(lines):
            line = lines[index]
            chunk.append(line)
            depth += line.count("{") - line.count("}")
            if "{" in line:
                started = True
            index += 1
            if started and depth <= 0:
                break
        bodies.setdefault(name, "")
        bodies[name] += "\n".join(chunk)
    return bodies


def declarations(rel: str, text: str) -> list[tuple[str, int, str]]:
    """(name, line, type) for every static-shaped declaration in the file."""
    out: list[tuple[str, int, str]] = []
    for lineno, line in enumerate(strip_comments(text).splitlines(), start=1):
        match = DECL.match(line)
        if not match:
            continue
        out.append((match.group("name"), lineno, " ".join(match.group("type").split())))
    return out


def holder_is_candidate(name: str, type_text: str, allocating_context: str) -> str | None:
    """Return the rule that makes this a candidate, or None.

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


def scan(root: Path) -> tuple[list[dict], int]:
    files = source_files(root)
    texts: dict[Path, str] = {}
    for path in files:
        try:
            texts[path] = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue

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
    registered_paths: list[tuple[str, list[str]]] = []
    for text in texts.values():
        for match in REGISTER_CALL.finditer(strip_comments(text)):
            for ident in re.findall(r"[A-Za-z_][\w:]*", match.group("args")):
                segments = ident.split("::")
                registered_paths.append((segments[-1], segments[:-1]))

    # 2. call graph over every fn in both crates
    bodies: dict[str, list[tuple[Path, str]]] = {}
    for path, text in texts.items():
        for name, body in function_bodies(text).items():
            bodies.setdefault(name, []).append((path, body))

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
            reachable_text_by_file[path] = reachable_text_by_file.get(path, "") + "\n" + body

    # 3. classify declarations
    holders: list[dict] = []
    for path, text in texts.items():
        rel = str(path.relative_to(root))
        allocating_context = "\n".join(
            body
            for _name, body in function_bodies(text).items()
            if any(token in body for token in ALLOCATOR_TOKENS)
        )
        covered_text = reachable_text_by_file.get(path, "")
        for name, lineno, type_text in declarations(rel, text):
            rule = holder_is_candidate(name, type_text, allocating_context)
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
                    "covered": covered,
                }
            )
    holders.sort(key=lambda h: (h["file"], h["line"]))
    return holders, len(seeds)


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
    holders, registered_count = scan(root)
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

    status = 0
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
        print(
            f"gc_runtime_root_holders: OK — {len(holders)} holder declarations "
            f"scanned, {covered} reached by a registered scanner, "
            f"{len(inventory)} classified in the inventory "
            f"({registered_count} registered scanners)."
        )
    return status


def print_list(root: Path) -> int:
    holders, registered_count = scan(root)
    print(f"# {len(holders)} candidate holders, {registered_count} registered scanners")
    for holder in holders:
        flag = "COVERED  " if holder["covered"] else "UNCOVERED"
        print(f"{flag} {holder['file']}:{holder['line']} {holder['name']}: {holder['type']}")
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
fn accessor() -> &'static Mutex<Option<Vec<*mut ClosureHeader>>> { &COVERED_VIA_ACCESSOR }
pub fn scan_thing_roots_mut(v: &mut V) {
    for p in COVERED_DIRECT.borrow_mut().iter_mut() { v.visit(p); }
    for p in accessor().lock().unwrap().iter_mut() { v.visit(p); }
    let _ = js_object_alloc(0, 0);
}
""",
    # An UNCOVERED holder of each rule, plus a same-name decoy in another file
    # that IS covered — the collision case.
    "crates/perry-runtime/src/leak.rs": """
static UNCOVERED_TYPED: Cell<*mut ArrayHeader> = Cell::new(std::ptr::null_mut());
static UNCOVERED_INT: Cell<f64> = Cell::new(0.0);
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
            path.write_text(body)
        holders, _ = scan(root)
        return holders


def self_test() -> int:
    failures: list[str] = []
    holders = _scan_tree()
    by_key = {(h["file"], h["name"]): h for h in holders}

    def expect(rel: str, name: str, covered: bool, why: str) -> None:
        key = (rel, name)
        if key not in by_key:
            failures.append(f"scanner MISSED the declaration {rel}:{name} ({why})")
            return
        if by_key[key]["covered"] != covered:
            failures.append(
                f"{rel}:{name} read as covered={by_key[key]['covered']}, "
                f"expected {covered} ({why})"
            )

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

    # Classification is only half of it — the VERDICT machinery has to go red.
    # An empty inventory must leave every uncovered holder unclassified…
    unclassified, _stale = apply_inventory(holders, [])
    uncovered = [h for h in holders if not h["covered"]]
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
    real_holders, _ = scan(REPO_ROOT)
    _unclassified, stale = apply_inventory(real_holders, inventory)
    if stale:
        failures.append(
            "inventory has %d stale entr(y|ies): %s"
            % (len(stale), ", ".join(f"{e['file']}:{e['name']}" for e in stale))
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
