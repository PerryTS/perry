#!/usr/bin/env python3
"""Thread-exit custody of process-global address-keyed state (#11471).

The bug class
-------------

A process-global table keyed by (or holding) a heap address outlives the
thread whose arena owned that address. When the thread exits, `Arena::drop`
hands its blocks back to the allocator; another thread's arena reuses the
same addresses; the stale entry now describes a DIFFERENT object. Three
instances shipped before this gate existed, each found by chasing a flake:

* #11319 / #11422 — the closure side tables (`CLOSURE_PROPS` and friends)
  kept a dead thread's entries, so a closure allocated at a reused address
  inherited them. Fixed by `release_closure_side_table_owners_in_ranges`,
  called from `Arena::drop`.
* #11462 — `tls.rs` parked `tls.rootCertificates` in process-global
  `AtomicU64` GC roots, which pinned whatever later occupied the address.
  Fixed by making the roots per-thread.
* #11470 — `PERRY_TA_KIND_CACHE` / `INLINE_OWNING_U32_CACHE` kept "this
  address is a typed array" after the owning thread exited, so a Promise at
  the reused address read as a `Uint8Array`. Fixed by
  `invalidate_kind_caches_in_ranges`, called from `Arena::drop`.

`scripts/gc_runtime_root_holders.py` enumerates statics that hold heap
pointers as GC ROOTS. A lookup cache keyed by an address is not a root — the
collector never needs to see it — so that census is blind to this class by
construction. This gate is the enumeration for the other half.

What it does
------------

1. **Enumerate** every process-global `static` in `perry-runtime` and
   `perry-stdlib` that (i) can change at run time — `static mut`, or a type
   that names an atomic, a lock, a once-cell, a lazy, a `DashMap`, or a
   crate-local wrapper that does — and (ii) can hold a 64-bit word: `u64`,
   `i64`, `usize`, `isize`, `f64`, an atomic of those, `AtomicPtr`, a raw
   pointer, `NonNull`, `JSValue`, or a `dyn` payload. Map KEYS count: an
   address-keyed table is exactly the shape of #11319 and #11470. Type aliases
   and crate-local struct/enum fields are resolved (bounded depth), so a
   `Mutex<HashMap<String, Entry>>` whose `Entry` has a `ptr: usize` field is
   found.

   Declarations inside `thread_local!` / `perry_thread_local!` are NOT
   enumerated: they die with their thread. Declarations inside
   `per_test_global!` ARE: that macro is a plain process-global `static` in
   every non-test build, and the closure side tables of #11319 were exactly
   such statics.

2. **Discharge counter-only atomics mechanically.** A scalar
   `AtomicU64`/`AtomicI64`/`AtomicUsize`/`AtomicIsize` whose every mention in
   the two crates is a `load`, an arithmetic `fetch_*`, or a `store`/`swap` of
   an integer literal cannot hold an address. Any other mention — a computed
   `store`, a `compare_exchange`, a borrow (`&NAME`), an exported symbol
   (`#[no_mangle]` / `export_name`, which codegen may write directly) — makes
   it a manual candidate. The rule reads the code; it does not read names.

3. **Require a verdict** for everything else, in
   `scripts/thread_exit_address_globals.json`:

   * `type_verdicts` — one verdict for every static whose OUTER type is a
     named wrapper that resolves per thread (`RealmAtomicI64`, `ImageTable`).
   * `entries` — `{file, names, verdict, why}` groups. Verdicts:
       - `no_heap_address`: counters, flags, config, code addresses, handle
         ids from a process-global counter, Rust-owned data.
       - `process_global_allocation`: holds only addresses of allocations that
         no thread's arena owns (leaked `Box`, `shared_sab`, malloc outside
         the arena), so thread exit cannot free them.
       - `per_thread`: the static is a handle whose storage resolves per
         thread.
       - `conservative_filter`: address bounds/bits whose staleness can only
         turn a definite "no" into a "maybe" that an authoritative table
         (itself per-thread or invalidated) then answers.
       - `main_thread_only`: holds arena addresses, but every writer is
         enforced to run on the main thread, whose arena is never dropped
         before process exit. The `why` must cite the enforcement.
       - `thread_exit_invalidated`: cleared over the freed ranges on thread
         exit. Must name its `hook`; the gate checks that the hook is called
         in `impl Drop for Arena`'s body AND is defined in the static's own
         file, so deleting the call reddens the gate.
       - `pending_fix`: a known instance with an open fix PR (`pr`). Allowed
         so the gate can land before the fix, but it cannot outlive the fix:
         with a `hook`, it fails the moment the hook appears in `Arena::drop`
         (promote it to `thread_exit_invalidated`); a fix that makes the
         static per-thread deletes the declaration, so the entry goes stale.
       - `open_bug`: a stale-capable static with an `issue`. Always fails.

How it fails
------------

* a candidate with no verdict -> exit 1
* an entry name (or a `type_verdicts` type) matching no candidate -> exit 1
* `open_bug`, or a `thread_exit_invalidated` whose hook is not called by
  `Arena::drop` or not defined in the static's file -> exit 1
* a `pending_fix` whose hook IS now called by `Arena::drop` -> exit 1
* fewer than MIN_CANDIDATES candidates or MIN_COUNTER_ONLY counter-only
  discharges -> exit 2 (the regexes stopped matching; an empty green run
  would otherwise look like success)

`--self-test` plants each shape into a temp tree and requires the scanner to
reject it; run it before trusting a green scan.

What this gate CANNOT see
-------------------------

* `RuntimeState` fields (`crates/perry-runtime/src/state.rs`) — they are
  per-thread (reached through `state()`), so out of subject anyway.
* A static whose TYPE cannot hold a 64-bit word but whose meaning is an
  address-derived value (a `u32` hash of an address). None is known.
* Whether a `thread_exit_invalidated` hook clears the RIGHT entries. The
  gate proves the call exists; the regression tests in
  `crates/perry-stdlib/src/runtime_thread_exit_tests.rs` prove what it does.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from gc_runtime_root_holders import (  # noqa: E402
    crate_source_files,
    crate_type_index,
    declarations,
    declarations_in_tls,
    function_bodies,
    strip_comments,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
INVENTORY_PATH = REPO_ROOT / "scripts" / "thread_exit_address_globals.json"
CRATES = ("crates/perry-runtime/src", "crates/perry-stdlib/src")
ARENA_FILE = "crates/perry-runtime/src/arena/block.rs"
ARENA_DROP = re.compile(r"impl\s+Drop\s+for\s+Arena\s*\{")
THREAD_EXIT_FILE = "crates/perry-runtime/src/arena/thread_exit.rs"
AGENT_FILE = "crates/perry-runtime/src/agent.rs"

MIN_CANDIDATES = 300
MIN_COUNTER_ONLY = 100

MUTABLE = re.compile(
    r"\b(?:Atomic\w*|Mutex|RwLock|OnceLock|OnceCell|LazyLock|Lazy|Cell|RefCell|"
    r"UnsafeCell|DashMap|DashSet|ArcSwap\w*)\b"
)
WORD = re.compile(
    r"\b(?:u64|i64|usize|isize|f64|AtomicU64|AtomicI64|AtomicUsize|AtomicIsize|"
    r"AtomicPtr|NonNull|JSValue)\b|\*\s*(?:mut|const)\b|\bdyn\b"
)
FN_POINTER_TYPE = re.compile(r'^(?:unsafe\s+)?(?:extern\s*(?:"[^"]*"\s*)?)?fn\s*\(')
SCALAR_ATOMIC = re.compile(r"^(?:(?:std|core)::sync::atomic::)?Atomic(?:U64|I64|Usize|Isize)$")
COUNTER_USE = re.compile(
    r"\s*\.\s*(?:load|fetch_add|fetch_sub|fetch_max|fetch_min|fetch_or|fetch_and|fetch_xor)\s*\("
    r"|\s*\.\s*(?:store|swap)\s*\(\s*(?:-?\d[\w_]*|true|false)\s*,"
)
EXPORTED = re.compile(r"#\[\s*(?:unsafe\s*\(\s*)?(?:no_mangle|export_name)")
TYPE_ALIAS = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?type\s+(\w+)(?:<[^=]*>)?\s*=\s*(.+?);", re.M | re.S
)
TYPE_IDENT = re.compile(r"\b[A-Z]\w*\b")
OUTER_NAME = re.compile(r"^(?:[\w$]+::)*(\w+)")
MAX_EXPANSION_DEPTH = 4

VERDICTS = {
    "no_heap_address",
    "process_global_allocation",
    "per_thread",
    "conservative_filter",
    "main_thread_only",
    "thread_exit_invalidated",
    "pending_fix",
    "open_bug",
}


def _expand(type_text: str, aliases: dict, types: dict, depth: int = 0, seen=frozenset()) -> str:
    """The type text plus every alias target / crate-local field type it names."""
    parts = [type_text]
    if depth >= MAX_EXPANSION_DEPTH:
        return type_text
    for ident in set(TYPE_IDENT.findall(type_text)):
        if ident in seen:
            continue
        if ident in aliases:
            parts.append(_expand(aliases[ident], aliases, types, depth + 1, seen | {ident}))
        for field in types.get(ident, ()):
            parts.append(_expand(field, aliases, types, depth + 1, seen | {ident}))
    return " ".join(parts)


def scan(root: Path) -> list[dict]:
    """Every candidate static, with `counter_only` set where rule 2 discharges it."""
    files = crate_source_files(root, CRATES)
    texts = {path: path.read_text(encoding="utf-8", errors="replace") for path in files}
    codes = {path: strip_comments(text) for path, text in texts.items()}
    types = crate_type_index(list(texts.values()))
    aliases: dict[str, str] = {}
    for code in codes.values():
        for match in TYPE_ALIAS.finditer(code):
            aliases.setdefault(match.group(1), match.group(2))
    all_code = "\n".join(codes.values())

    candidates: list[dict] = []
    for path, text in texts.items():
        rel = path.relative_to(root).as_posix()
        in_tls = declarations_in_tls(text)
        lines = codes[path].splitlines()
        raw_lines = text.splitlines()
        for name, line, type_text in declarations(rel, text):
            if (name, line) in in_tls:
                continue
            type_text = type_text.strip()
            if FN_POINTER_TYPE.match(type_text):
                continue
            decl_line = lines[line - 1]
            expanded = _expand(type_text, aliases, types)
            if not (re.search(r"\bstatic\s+mut\b", decl_line) or MUTABLE.search(expanded)):
                continue
            if not WORD.search(expanded):
                continue
            attrs = " ".join(raw_lines[max(0, line - 4) : line])
            counter_only = (
                bool(SCALAR_ATOMIC.match(type_text))
                and not EXPORTED.search(attrs)
                and _uses_are_counter_only(name, all_code)
            )
            outer = OUTER_NAME.match(type_text)
            candidates.append(
                {
                    "file": rel,
                    "name": name,
                    "line": line,
                    "type": type_text,
                    "outer": outer.group(1) if outer else type_text,
                    "counter_only": counter_only,
                }
            )
    return candidates


def _uses_are_counter_only(name: str, all_code: str) -> bool:
    uses = 0
    for match in re.finditer(r"\b%s\b" % re.escape(name), all_code):
        start = all_code.rfind("\n", 0, match.start()) + 1
        end = all_code.find("\n", match.end())
        line = all_code[start : end if end >= 0 else len(all_code)]
        if re.search(r"\bstatic\s+(?:mut\s+)?%s\s*:" % re.escape(name), line):
            continue
        if re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?use\b", line):
            continue
        uses += 1
        if not COUNTER_USE.match(all_code[match.end() : match.end() + 120]):
            return False
    return uses > 0


def _brace_body(code: str, open_at: int) -> str:
    """Text between the `{` at `open_at` and its matching `}`."""
    depth = 0
    for index in range(open_at, len(code)):
        if code[index] == "{":
            depth += 1
        elif code[index] == "}":
            depth -= 1
            if depth == 0:
                return code[open_at + 1 : index]
    return ""


CALL = re.compile(r"\b(\w+)\s*\(")
REGISTERED_HOOK = re.compile(
    r"\b(?:register_thread_exit_range_hook|register_retire_hook)\s*\(\s*(?:[\w:]*::)?(\w+)\s*,?\s*\)"
)
DROP_IMPL = re.compile(r"impl\s+Drop\s+for\s+(\w+)\s*\{")


def exit_hooks(root: Path) -> set[str]:
    """Function names that provably run when a thread exits.

    * called in `impl Drop for Arena` (the arena's own TLS destructor);
    * called in `arena::thread_exit::release_freed_ranges`, which that drop
      calls with the freed ranges;
    * passed to `register_thread_exit_range_hook` (run by the same dispatcher)
      or to `agent::register_retire_hook` / called in `retire_agent` (run when
      a worker agent retires at thread exit);
    * called in an `impl Drop for T` whose `T` is the type of a
      `thread_local!` in the same file (a TLS destructor of its own).
    """
    hooks: set[str] = set()
    for path in crate_source_files(root, CRATES):
        rel = path.relative_to(root).as_posix()
        code = strip_comments(path.read_text(encoding="utf-8", errors="replace"))
        hooks.update(REGISTERED_HOOK.findall(code))
        if rel == ARENA_FILE:
            match = ARENA_DROP.search(code)
            if match:
                hooks.update(CALL.findall(_brace_body(code, match.end() - 1)))
        for fn_name in ("release_freed_ranges", "retire_agent"):
            match = re.search(r"\bfn\s+%s\s*\([^{]*\{" % fn_name, code)
            if match and rel in (THREAD_EXIT_FILE, AGENT_FILE):
                hooks.update(CALL.findall(_brace_body(code, match.end() - 1)))
        tls_types = {
            decl_type
            for name, line, decl_type in declarations(rel, code)
            if (name, line) in declarations_in_tls(code)
        }
        for match in DROP_IMPL.finditer(code):
            owner = match.group(1)
            if owner != "Arena" and any(re.search(r"\b%s\b" % owner, t) for t in tls_types):
                hooks.update(CALL.findall(_brace_body(code, match.end() - 1)))
    return hooks


def load_inventory(path: Path) -> dict:
    if not path.exists():
        return {"type_verdicts": [], "entries": []}
    return json.loads(path.read_text(encoding="utf-8"))


def check(root: Path, inventory: dict) -> tuple[list[dict], list[str], dict]:
    """(unclassified candidates, problems, stats)."""
    candidates = scan(root)
    drop_calls = exit_hooks(root)
    problems: list[str] = []
    type_verdicts = {tv["type"]: tv for tv in inventory.get("type_verdicts", [])}
    for tv in inventory.get("type_verdicts", []):
        if tv.get("verdict") not in {"per_thread", "no_heap_address"}:
            problems.append(f"type_verdicts[{tv.get('type')}]: verdict must be per_thread or no_heap_address")
        if len((tv.get("why") or "").strip()) < 20:
            problems.append(f"type_verdicts[{tv.get('type')}]: `why` missing or too short")

    index: dict[tuple[str, str], dict] = {}
    for entry in inventory.get("entries", []):
        label = f"{entry.get('file', '?')}:{','.join(entry.get('names', []))}"
        verdict = entry.get("verdict")
        if verdict not in VERDICTS:
            problems.append(f"{label}: verdict {verdict!r} is not one of {sorted(VERDICTS)}")
        if len((entry.get("why") or "").strip()) < 20:
            problems.append(f"{label}: `why` missing or too short to be a reason")
        if not entry.get("names"):
            problems.append(f"{label}: entry names no static")
        for name in entry.get("names", []):
            key = (entry.get("file", ""), name)
            if key in index:
                problems.append(f"{entry.get('file')}:{name}: classified twice")
            index[key] = entry
        hook = (entry.get("hook") or "").strip()
        if verdict == "open_bug":
            problems.append(
                f"{label}: open_bug ({entry.get('issue', 'no issue')}) — a static that can "
                f"describe a dead thread's memory; invalidate it in Arena::drop or make it per-thread"
            )
        if verdict == "thread_exit_invalidated" and not hook:
            problems.append(f"{label}: {verdict} must name the `hook` that runs at thread exit")
        if verdict == "thread_exit_invalidated" and hook:
            if hook not in drop_calls:
                problems.append(
                    f"{label}: hook `{hook}` does not run at thread exit (not called by "
                    f"`impl Drop for Arena` / `release_freed_ranges` / `retire_agent` / a TLS "
                    f"destructor, nor registered); the statics it covers now outlive their thread"
                )
            else:
                hook_file = entry.get("hook_file") or entry.get("file", "")
                if not _defines_fn(root / hook_file, hook):
                    problems.append(f"{label}: hook `{hook}` is not defined in {hook_file}")
        if verdict == "pending_fix":
            if not (entry.get("pr") or "").strip():
                problems.append(f"{label}: pending_fix must cite the fix `pr`")
            if hook and hook in drop_calls:
                problems.append(
                    f"{label}: its fix landed (`{hook}` now runs at thread exit) — "
                    f"promote the entry to thread_exit_invalidated"
                )

    unclassified: list[dict] = []
    used_keys: set[tuple[str, str]] = set()
    used_types: set[str] = set()
    counter_only = 0
    for cand in candidates:
        key = (cand["file"], cand["name"])
        if key in index:
            used_keys.add(key)
            if cand["counter_only"]:
                problems.append(
                    f"{cand['file']}:{cand['name']}: counter-only (discharged mechanically); "
                    f"delete its inventory entry"
                )
            continue
        if cand["counter_only"]:
            counter_only += 1
            continue
        if cand["outer"] in type_verdicts:
            used_types.add(cand["outer"])
            continue
        unclassified.append(cand)
    for key, entry in index.items():
        if key not in used_keys:
            problems.append(f"{key[0]}:{key[1]}: stale entry — no such candidate static")
    for name in type_verdicts:
        if name not in used_types:
            problems.append(f"type_verdicts[{name}]: stale — no candidate has this outer type")
    stats = {"candidates": len(candidates), "counter_only": counter_only, "entries": len(index)}
    return unclassified, problems, stats


def _defines_fn(path: Path, name: str) -> bool:
    if not path.exists():
        return False
    return name in function_bodies(path.read_text(encoding="utf-8", errors="replace"))


def report(root: Path, inventory_path: Path, quiet: bool = False) -> int:
    inventory = load_inventory(inventory_path)
    unclassified, problems, stats = check(root, inventory)
    if stats["candidates"] < MIN_CANDIDATES or stats["counter_only"] < MIN_COUNTER_ONLY:
        print(
            f"thread_exit_address_globals: only {stats['candidates']} candidates / "
            f"{stats['counter_only']} counter-only (floors {MIN_CANDIDATES}/{MIN_COUNTER_ONLY}) — "
            f"the extraction regexes stopped matching; this run checked nothing",
            file=sys.stderr,
        )
        return 2
    for cand in unclassified:
        problems.append(
            f"{cand['file']}:{cand['line']}: `{cand['name']}: {cand['type']}` is a process-global "
            f"static that can hold a heap address and has no thread-exit verdict in "
            f"{inventory_path.name}. After its thread exits, Arena::drop frees the memory and "
            f"another thread reuses it (#11471): invalidate it there, make it per-thread, or "
            f"record why it cannot hold an arena address."
        )
    for problem in problems:
        print(f"thread_exit_address_globals: {problem}", file=sys.stderr)
    pending = [e for e in inventory.get("entries", []) if e.get("verdict") == "pending_fix"]
    if not quiet:
        print(
            f"thread_exit_address_globals: {stats['candidates']} candidates, "
            f"{stats['counter_only']} counter-only, {stats['entries']} classified by entry, "
            f"{len(pending)} pending_fix group(s), {len(problems)} problem(s)"
        )
    return 1 if problems else 0


def print_list(root: Path, inventory_path: Path) -> int:
    inventory = load_inventory(inventory_path)
    index = {(e["file"], n): e for e in inventory.get("entries", []) for n in e.get("names", [])}
    types = {tv["type"]: tv for tv in inventory.get("type_verdicts", [])}
    for cand in scan(root):
        if cand["counter_only"]:
            verdict = "counter_only"
        elif (cand["file"], cand["name"]) in index:
            verdict = index[(cand["file"], cand["name"])]["verdict"]
        elif cand["outer"] in types:
            verdict = types[cand["outer"]]["verdict"] + " (type)"
        else:
            verdict = "UNCLASSIFIED"
        print(f"{verdict:26} {cand['file']}:{cand['line']} {cand['name']}: {cand['type']}")
    return 0


# ---------------------------------------------------------------- self-test

SELF_TEST_TREE = {
    ARENA_FILE: """
pub(crate) struct Arena { blocks: Vec<u8> }
impl Drop for Arena {
    fn drop(&mut self) {
        crate::cache::forget_in_ranges(&[]);
    }
}
""",
    "crates/perry-runtime/src/cache.rs": """
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
static ADDR_CACHE: Mutex<Vec<usize>> = Mutex::new(Vec::new());
pub fn forget_in_ranges(r: &[(usize, usize)]) { let _ = r; }
static HITS: AtomicU64 = AtomicU64::new(0);
pub fn hit() { HITS.fetch_add(1, Ordering::Relaxed); HITS.store(0, Ordering::Relaxed); }
static NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());
thread_local! {
    static LOCAL_TABLE: std::cell::RefCell<Vec<usize>> = std::cell::RefCell::new(Vec::new());
}
""",
}

SELF_TEST_INVENTORY = {
    "type_verdicts": [],
    "entries": [
        {
            "file": "crates/perry-runtime/src/cache.rs",
            "names": ["ADDR_CACHE"],
            "verdict": "thread_exit_invalidated",
            "hook": "forget_in_ranges",
            "why": "self-test fixture: cleared by forget_in_ranges from Arena::drop",
        }
    ],
}


def _run_fixture(extra: dict[str, str] | None, inventory: dict, drop: tuple[str, str] | None = None):
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        tree = dict(SELF_TEST_TREE)
        tree.update(extra or {})
        for rel, body in tree.items():
            if drop and rel == drop[0]:
                body = body.replace(drop[1], "")
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body, encoding="utf-8")
        return check(root, inventory), scan(root)


def self_test() -> int:
    failures: list[str] = []

    def expect(cond: bool, what: str) -> None:
        if not cond:
            failures.append(what)

    (unclassified, problems, _), cands = _run_fixture(None, SELF_TEST_INVENTORY)
    names = {c["name"]: c for c in cands}
    expect(not unclassified and not problems, f"baseline fixture must be clean: {unclassified} {problems}")
    expect("LOCAL_TABLE" not in names, "a thread_local! static must not be enumerated")
    expect("NAMES" not in names, "a Mutex<Vec<String>> holds no word and must not be enumerated")
    expect(names.get("HITS", {}).get("counter_only"), "a fetch_add/literal-store atomic is counter-only")

    # 1. A new address-keyed table with no verdict fails.
    (unclassified, _, _), _ = _run_fixture(
        {"crates/perry-stdlib/src/new.rs": "static BY_ADDR: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();\n"},
        SELF_TEST_INVENTORY,
    )
    expect(any(c["name"] == "BY_ADDR" for c in unclassified), "an unclassified address-keyed map must fail")

    # 2. An address stored into an atomic is NOT counter-only, whatever its name says.
    (unclassified, _, _), _ = _run_fixture(
        {
            "crates/perry-runtime/src/cnt.rs": (
                "static OBJECT_COUNT: AtomicU64 = AtomicU64::new(0);\n"
                "pub fn f(p: *mut u8) { OBJECT_COUNT.store(p as u64, Ordering::Relaxed); }\n"
            )
        },
        SELF_TEST_INVENTORY,
    )
    expect(any(c["name"] == "OBJECT_COUNT" for c in unclassified), "a computed store must defeat counter-only")

    # 3. per_test_global! is process-global in production and IS enumerated.
    (unclassified, _, _), _ = _run_fixture(
        {"crates/perry-runtime/src/ptg.rs": "per_test_global! {\n    static SIDE: OnceLock<Mutex<PtrHashMap<usize, u64>>> = OnceLock::new();\n}\n"},
        SELF_TEST_INVENTORY,
    )
    expect(any(c["name"] == "SIDE" for c in unclassified), "per_test_global! statics must be enumerated")

    # 4. A crate-local struct holding a usize is resolved.
    (unclassified, _, _), _ = _run_fixture(
        {"crates/perry-runtime/src/st.rs": "struct Slot { ptr: usize }\nstatic SLOTS: Mutex<Vec<Slot>> = Mutex::new(Vec::new());\n"},
        SELF_TEST_INVENTORY,
    )
    expect(any(c["name"] == "SLOTS" for c in unclassified), "a struct field holding usize must be resolved")

    # 5. Deleting the Arena::drop call reddens a thread_exit_invalidated entry.
    (_, problems, _), _ = _run_fixture(None, SELF_TEST_INVENTORY, drop=(ARENA_FILE, "crate::cache::forget_in_ranges(&[]);"))
    expect(any("does not run at thread exit" in p for p in problems), "a removed hook call must fail")

    # 5b. A downstream table registered through the range-hook registry passes;
    #     deleting the registration fails; a hook defined elsewhere fails.
    stdlib_table = {
        "crates/perry-stdlib/src/lst.rs": (
            "static LISTENERS: Mutex<Vec<i64>> = Mutex::new(Vec::new());\n"
            "fn release_listeners(f: &FreedRanges) { let _ = f; }\n"
            "pub fn on(cb: i64) { REGISTER.call_once(|| "
            "perry_runtime::arena::thread_exit::register_thread_exit_range_hook(\n        release_listeners,\n    )); }\n"
        )
    }
    reg = json.loads(json.dumps(SELF_TEST_INVENTORY))
    reg["entries"].append({"file": "crates/perry-stdlib/src/lst.rs", "names": ["LISTENERS"], "verdict": "thread_exit_invalidated", "hook": "release_listeners", "why": "self-test fixture: registered range hook"})
    (unclassified, problems, _), _ = _run_fixture(stdlib_table, reg)
    expect(not unclassified and not problems, f"a registered range hook must be accepted: {problems}")
    (_, problems, _), _ = _run_fixture(stdlib_table, reg, drop=("crates/perry-stdlib/src/lst.rs", "register_thread_exit_range_hook(\n        release_listeners,\n    )"))
    expect(any("does not run at thread exit" in p for p in problems), "a deleted registration must fail")
    moved = json.loads(json.dumps(reg))
    moved["entries"][-1]["hook_file"] = "crates/perry-runtime/src/cache.rs"
    (_, problems, _), _ = _run_fixture(stdlib_table, moved)
    expect(any("is not defined in" in p for p in problems), "a hook defined in another file must fail")

    # 5c. A TLS destructor path counts: `impl Drop for T` where T lives in thread_local!.
    tls_drop = {
        "crates/perry-stdlib/src/own.rs": (
            "static RECORDS: Mutex<HashMap<u64, f64>> = Mutex::new(HashMap::new());\n"
            "thread_local! {\n    static OWNED: RefCell<Owned> = RefCell::new(Owned::default());\n}\n"
            "impl Drop for Owned {\n    fn drop(&mut self) {\n        release_owned(&self.0);\n    }\n}\n"
            "fn release_owned(ids: &[u64]) { let _ = ids; }\n"
        )
    }
    own = json.loads(json.dumps(SELF_TEST_INVENTORY))
    own["entries"].append({"file": "crates/perry-stdlib/src/own.rs", "names": ["RECORDS"], "verdict": "thread_exit_invalidated", "hook": "release_owned", "why": "self-test fixture: TLS destructor path"})
    (unclassified, problems, _), _ = _run_fixture(tls_drop, own)
    expect(not unclassified and not problems, f"a TLS-destructor hook must be accepted: {problems}")

    # 6. A stale entry fails.
    stale = json.loads(json.dumps(SELF_TEST_INVENTORY))
    stale["entries"].append({"file": "crates/perry-runtime/src/cache.rs", "names": ["GONE"], "verdict": "no_heap_address", "why": "self-test fixture for a stale entry"})
    (_, problems, _), _ = _run_fixture(None, stale)
    expect(any("stale entry" in p for p in problems), "a stale entry must fail")

    # 7. open_bug always fails; pending_fix fails once its hook lands.
    bug = json.loads(json.dumps(SELF_TEST_INVENTORY))
    bug["entries"][0].update(verdict="open_bug", issue="#1")
    (_, problems, _), _ = _run_fixture(None, bug)
    expect(any("open_bug" in p for p in problems), "open_bug must fail")
    pend = json.loads(json.dumps(SELF_TEST_INVENTORY))
    pend["entries"][0].update(verdict="pending_fix", pr="#2")
    (_, problems, _), _ = _run_fixture(None, pend)
    expect(any("promote the entry" in p for p in problems), "a landed pending_fix must fail")

    # 8. A stale type verdict fails.
    tv = json.loads(json.dumps(SELF_TEST_INVENTORY))
    tv["type_verdicts"] = [{"type": "NoSuchWrapper", "verdict": "per_thread", "why": "self-test fixture for a stale type verdict"}]
    (_, problems, _), _ = _run_fixture(None, tv)
    expect(any("type_verdicts[NoSuchWrapper]: stale" in p for p in problems), "a stale type verdict must fail")

    # 8b. A type alias spanning several lines (rustfmt's shape for a long
    #     `Arc<dyn Fn(..)>`) is still resolved.
    (unclassified, _, _), _ = _run_fixture(
        {"crates/perry-stdlib/src/alias.rs": "type Releaser = std::sync::Arc<\n    dyn Fn(&mut u8) -> bool\n        + Send,\n>;\nstatic RELEASERS: Mutex<Vec<Releaser>> = Mutex::new(Vec::new());\n"},
        SELF_TEST_INVENTORY,
    )
    expect(any(c["name"] == "RELEASERS" for c in unclassified), "a multi-line type alias must be resolved")

    # 9. An exported atomic is never counter-only (codegen may write it).
    (unclassified, _, _), _ = _run_fixture(
        {"crates/perry-runtime/src/exp.rs": "#[no_mangle]\npub static EXPORTED_WORD: AtomicU64 = AtomicU64::new(0);\npub fn g() { EXPORTED_WORD.load(Ordering::Relaxed); }\n"},
        SELF_TEST_INVENTORY,
    )
    expect(any(c["name"] == "EXPORTED_WORD" for c in unclassified), "an exported atomic must not be counter-only")

    if failures:
        for failure in failures:
            print(f"thread_exit_address_globals self-test FAILED: {failure}", file=sys.stderr)
        return 1
    print("thread_exit_address_globals self-test: every planted shape caught")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true", help="check the checker")
    parser.add_argument("--list", action="store_true", help="print every candidate + verdict")
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.list:
        return print_list(REPO_ROOT, INVENTORY_PATH)
    return report(REPO_ROOT, INVENTORY_PATH, quiet=args.quiet)


if __name__ == "__main__":
    sys.exit(main())
