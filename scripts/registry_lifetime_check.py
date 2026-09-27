#!/usr/bin/env python3
"""Registry lifetime gate: a handle table must be able to SHRINK (#11511).

The Web Fetch handle registries (`FETCH_RESPONSES`, `HEADERS_REGISTRY`,
`REQUEST_REGISTRY`, `BLOB_REGISTRY`, `FORM_DATA_REGISTRY`) were only ever
`remove`d in tests. A server leaked every request and response body and
eventually panicked when the id band ran out (#11164/#11165). No static check
could have caught it: `scripts/gc_runtime_root_holders.py` proves a heap-pointer
holder is *scanned*, not that it ever *shrinks*.

What it does
------------

1. **Enumerate** every `static` / `thread_local!` / `lazy_static!` declaration
   in `perry-runtime`, `perry-stdlib`, `perry-ffi` and every `perry-ext-*`
   crate (the same file set and declaration parser as
   `gc_runtime_root_holders.py`) whose type names a map-like container
   (`HashMap`, `BTreeMap`, `DashMap`, `IndexMap`, the sets, `Slab`, ...).

2. **Look for a production removal path.** Test code is cut out first:
   `tests/` directories and `*tests.rs` files are never read, and every item
   gated by `#[cfg(test)]` (or `#[cfg(all(test, ...))]`) inside a production
   file is blanked before anything is parsed. A registry then passes when some
   production function that *touches* it also calls a removal method
   (`remove`, `retain`, `drain`, `clear`, `pop_first`, an entry `remove`,
   ...), or swaps the whole table out (`take`/`replace`/`mem::take` on a line
   that names the registry). A function touches a registry
   when it names it, or calls the accessor function the registry is declared
   inside (`fn registry() -> &'static Mutex<..> { static R: .. }`), or calls a
   function that returns a borrow/guard of it.

   Names are resolved within the declaring crate. Several crates declare
   `REGISTRY`, `CALLBACKS`, `HANDLES` more than once, so for a name declared
   in more than one file of a crate, only the declaring file and the files of
   its own module directory can vouch for it; a unique name may be removed
   from anywhere in the crate (`fetch/lifecycle.rs` releasing `fetch/mod.rs`'s
   tables is the case this gate was written for).

3. **Require a verdict** for everything left over, in
   `scripts/registry_lifetime_allowlist.json`: `{file, name, [fn], verdict, why}`
   (verdicts: see `VERDICTS`). A registry
   with no removal path and no entry fails; an entry matching no such
   registry fails too, so a fix (adding the removal path, or deleting the
   table) must delete its own exemption.

How it fails
------------

* a registry with no production removal and no allowlist entry -> exit 1
* an allowlist entry that matches nothing (stale), is duplicated, or has no
  readable `why` -> exit 1
* fewer than MIN_REGISTRIES registries enumerated -> exit 2, because a parser
  that stopped matching would otherwise report a clean, empty, green run
* fewer than MIN_REMOVING registries WITH a removal path -> exit 2, because a
  removal regex that stopped matching would flag everything (noise, not a
  gate) and one that matched everything would flag nothing

`--self-test` plants each shape in a temp tree: a registry removed only in a
`tests.rs` file and only in a `#[cfg(test)]` module must FAIL; production
removal (same file, sibling file, through an accessor) must PASS; a stale
allowlist entry must FAIL.

What this gate CANNOT see
-------------------------

* **Whether the removal path actually RUNS.** It is a function-granularity
  syntactic check: a production function that names the table and calls
  `.remove(` on *something* passes. A removal in dead code, or on a different
  map in the same function, still reads as a path. The gate bounds the
  population of tables that *cannot* shrink; it does not audit the ones that
  can.
* **Registries behind a struct.** `static STATE: Mutex<State>` whose `State`
  holds the maps is not map-typed at the declaration and is not enumerated;
  neither are `RuntimeState` fields (`crates/perry-runtime/src/state.rs`).
* **`Vec`/`VecDeque` tables.** Handle vectors are usually slot-reused
  (`v[i] = None`) rather than removed from, which no method-name rule can
  distinguish from a leak, so they are out of scope rather than exempted
  wholesale.
* **Tables passed by reference** to a helper in another crate that shrinks
  them. The allowlist entry is the correction.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from pathlib import Path, PurePosixPath

from gc_runtime_root_holders import (
    CORE_CRATES,
    FN_DEF,
    crate_source_files,
    declarations,
    gated_ffi_crates,
    repo_relative,
    strip_comments,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
ALLOWLIST_PATH = REPO_ROOT / "scripts" / "registry_lifetime_allowlist.json"

# Outer type names that make a declaration a keyed / set-like registry.
REGISTRY_CONTAINERS = (
    "HashMap",
    "BTreeMap",
    "DashMap",
    "IndexMap",
    "FxHashMap",
    "AHashMap",
    "PtrHashMap",
    "StdHashMap",
    "HashSet",
    "BTreeSet",
    "DashSet",
    "IndexSet",
    "FxHashSet",
    "AHashSet",
    "Slab",
)
IDENT = re.compile(r"\b[A-Za-z_]\w*\b")
CALLED = re.compile(r"(?<![.\w])([A-Za-z_]\w*)\s*(?:::<[^()]*>)?\s*\(")
REGISTRY_TYPE = re.compile(r"\b(?:" + "|".join(REGISTRY_CONTAINERS) + r")\s*<")

# Calls that take entries out of a container.
REMOVAL_CALL = re.compile(
    r"\.\s*(?:remove|remove_entry|try_remove|remove_if|remove_if_mut|swap_remove|"
    r"shift_remove|swap_remove_entry|shift_remove_entry|retain|retain_mut|drain|"
    r"drain_filter|extract_if|clear|pop_first|pop_last|split_off)\s*\("
)
# Whole-table swaps (`TABLE.take()`, `mem::take(&mut *TABLE.lock())`,
# `TABLE.replace(HashMap::new())`). These words are far more often an
# `Option::take` or a `mem::replace` of some FIELD, so they only count on a
# line that names the registry itself; `mem::replace(&mut state.destroyed,
# true)` inside a toucher is not a removal path.
SWAP_CALL = re.compile(r"\.\s*(?:take|replace)\s*\(|\bmem::(?:take|replace|swap)\s*\(")

# `#[cfg(test)]` and `#[cfg(all(test, ...))]` gate an item out of the shipped
# binary. `#[cfg(any(test, ...))]` does NOT: the item also builds under the
# other arm, so it stays in.
CFG_TEST = re.compile(r"^\s*#\[cfg\(\s*(?:test\s*|all\(\s*test\b[^\]]*)\)\]")

# A function whose return type is a borrow or guard, or that hands the table
# to a closure parameter (`fn with_nodes<F: FnOnce(&mut HashMap<..>)>(f: F)`),
# is an accessor: its callers are what actually mutate the table it names.
ACCESSOR_RETURN = re.compile(
    r"->\s*(?:&|[\w:]*(?:Guard|Ref|RefMut)\b|[\w:]*(?:Mutex|RwLock|RefCell)\s*<)"
    r"|\bFn(?:Once|Mut)?\s*\("
)

# Floors: "the extraction still works" assertions, not budgets.
MIN_REGISTRIES = 250
MIN_REMOVING = 170

MIN_WHY = 20

# Why a table that never shrinks is acceptable, or that it is not.
VERDICTS = {
    # Keys come from a set the compiled program fixes: class ids, source
    # symbols, module paths, builtin names. Growth stops once the program's
    # code has run once.
    "bounded_by_program",
    # A tiny fixed key space (an enum, a handful of flags) independent of
    # both the program and its input.
    "bounded_by_constant",
    # No removal, but inserts stop at a hard entry/byte cap (past it the
    # caller falls back to an uncached path), so the table cannot grow
    # without bound.
    "bounded_by_cap",
    # Deliberately process-lifetime by spec or design (`Symbol.for`'s
    # registry, an interner whose ids must stay stable), and growth is
    # proportional to distinct inputs rather than to operations.
    "process_lifetime",
    # Has a production removal path the name-based walk cannot follow (the
    # table is reached through a raw TLS address cache, say). The entry must
    # name that `site` as `file::fn`; the gate checks it still exists outside
    # tests and still calls a removal method, so the claim cannot rot.
    "removed_elsewhere",
    # Only written under a diagnostics knob that is off in shipped runs.
    "diagnostic_only",
    # A real unbounded table: it grows per operation and nothing shrinks it.
    # Named so the debt is visible and counted; fixing it deletes the entry.
    "open_leak",
}


def gated_crates(root: Path) -> tuple[str, ...]:
    return CORE_CRATES + gated_ffi_crates(root)


def strip_cfg_test(text: str) -> str:
    """Blank every `#[cfg(test)]`-gated item, keeping the line count.

    Runs on comment/literal-stripped text so braces inside strings cannot end
    an item early. `#[cfg(test)] mod tests;` (a body in another file) is one
    line; a braced item is blanked to its matching close brace; any other
    one-line item ends at its `;`.
    """
    lines = strip_comments(text).splitlines()
    out = list(lines)
    index = 0
    while index < len(lines):
        if not CFG_TEST.match(lines[index]):
            index += 1
            continue
        start = index
        depth = 0
        opened = False
        while index < len(lines):
            line = lines[index]
            depth += line.count("{") - line.count("}")
            if "{" in line:
                opened = True
            index += 1
            if opened and depth <= 0:
                break
            if not opened and line.rstrip().endswith(";"):
                break
        for blank in range(start, index):
            out[blank] = ""
    return "\n".join(out)


def function_spans(code: str) -> list[tuple[str, int, int, str, str]]:
    """(name, first_line, last_line, signature, body) for each `fn`, 1-based.

    Nested functions are reported inside their parent's span AND on their own,
    which is what lets a declaration inside an accessor fn find that fn.
    """
    lines = code.splitlines()
    spans: list[tuple[str, int, int, str, str]] = []
    for start, line in enumerate(lines):
        match = FN_DEF.match(line)
        if not match:
            continue
        depth = 0
        opened = False
        signature: list[str] = []
        end = start
        body_only = False
        while end < len(lines):
            current = lines[end]
            if not opened:
                if current.rstrip().endswith(";") and "{" not in current:
                    body_only = True  # extern / trait declaration, no body
                    break
                signature.append(current.split("{", 1)[0])
            depth += current.count("{") - current.count("}")
            if "{" in current:
                opened = True
            if opened and depth <= 0:
                break
            end += 1
        if body_only:
            continue
        spans.append(
            (
                match.group(1),
                start + 1,
                end + 1,
                " ".join(" ".join(signature).split()),
                "\n".join(lines[start : end + 1]),
            )
        )
    return spans


def module_dir(rel: str) -> str:
    """The directory holding a file's module and its children."""
    path = PurePosixPath(rel)
    if path.name in {"mod.rs", "lib.rs", "main.rs"}:
        return str(path.parent)
    return str(path.parent / path.stem)


def crate_of(rel: str) -> str:
    return "/".join(rel.split("/")[:2])


def scan(root: Path) -> list[dict]:
    """Every map-like registry with whether a production removal path exists."""
    return scan_with_sites(root)[0]


def scan_with_sites(root: Path) -> tuple[list[dict], dict[str, bool]]:
    """(registries, {"file::fn": removes?} for every production function)."""
    files: dict[str, str] = {}
    for path in crate_source_files(root, gated_crates(root)):
        rel = repo_relative(path, root)
        files[rel] = strip_cfg_test(path.read_text(encoding="utf-8", errors="replace"))

    spans = {rel: function_spans(code) for rel, code in files.items()}

    registries: list[dict] = []
    for rel, code in files.items():
        for name, line, type_text in declarations(rel, code):
            if not REGISTRY_TYPE.search(type_text):
                continue
            enclosing = [
                span for span in spans[rel] if span[1] < line <= span[2]
            ]
            accessor = min(enclosing, key=lambda s: s[2] - s[1])[0] if enclosing else None
            registries.append(
                {"file": rel, "name": name, "line": line, "type": type_text, "accessor": accessor}
            )

    declared_in: dict[tuple[str, str], set[str]] = {}
    for reg in registries:
        declared_in.setdefault((crate_of(reg["file"]), reg["name"]), set()).add(reg["file"])

    # Index every function once: identifiers it names, functions it calls,
    # whether it removes, whether it is a borrow/guard accessor.
    fn_index: dict[str, list[dict]] = {}
    for rel, file_spans in spans.items():
        entries = fn_index.setdefault(crate_of(rel), [])
        for fn_name, first, last, signature, body in file_spans:
            entries.append(
                {
                    "file": rel,
                    "name": fn_name,
                    "first": first,
                    "last": last,
                    "idents": set(IDENT.findall(body)),
                    "calls": set(CALLED.findall(body)),
                    "removes": bool(REMOVAL_CALL.search(body)),
                    "swap_lines": [l for l in body.splitlines() if SWAP_CALL.search(l)],
                    "accessor": bool(ACCESSOR_RETURN.search(signature)),
                }
            )

    fn_defined_in: dict[tuple[str, str], set[str]] = {}
    for crate, entries in fn_index.items():
        for fn in entries:
            fn_defined_in.setdefault((crate, fn["name"]), set()).add(fn["file"])

    for reg in registries:
        crate = crate_of(reg["file"])
        # A function-body static is private to its accessor fn, so it is
        # reachable only by CALLING that fn; name ambiguity is then the
        # accessor's, not the static's.
        if reg["accessor"]:
            own_files = fn_defined_in.get((crate, reg["accessor"]), {reg["file"]})
        else:
            own_files = declared_in[(crate, reg["name"])]
        ambiguous = len(own_files) > 1
        home = module_dir(reg["file"])

        def may_vouch(rel: str) -> bool:
            if rel == reg["file"]:
                return True
            if rel in own_files:
                return False  # that file's own same-named item, not this one
            return not ambiguous or rel.startswith(home + "/")

        candidates = [fn for fn in fn_index.get(crate, []) if may_vouch(fn["file"])]

        def visible(caller: str, def_file: str, fn_name: str) -> bool:
            """Can a call to `fn_name` from `caller` resolve to the one in `def_file`?"""
            if caller == def_file:
                return True
            defs = fn_defined_in.get((crate, fn_name), set())
            if caller in defs:
                return False  # the caller's own same-named fn
            return len(defs) <= 1 or caller.startswith(module_dir(def_file) + "/")

        def touches(fn: dict, via: dict[str, str]) -> bool:
            if not reg["accessor"] and reg["name"] in fn["idents"]:
                return True
            return any(
                name in fn["calls"] and visible(fn["file"], def_file, name)
                for name, def_file in via.items()
            )

        via: dict[str, str] = {reg["accessor"]: reg["file"]} if reg["accessor"] else {}
        removal_site = None
        # Two rounds: direct touchers, then callers of borrow/guard/closure
        # accessors. An accessor's callers are scoped like a static's users,
        # so `with_env` in one module cannot vouch through another module's
        # unrelated `with_env`.
        for _round in range(2):
            grown: dict[str, str] = {}
            for fn in candidates:
                if not touches(fn, via):
                    continue
                swaps = any(
                    re.search(rf"\b{re.escape(reg['name'])}\b", line)
                    for line in fn["swap_lines"]
                ) if not reg["accessor"] else False
                if (fn["removes"] or swaps) and removal_site is None:
                    removal_site = f"{fn['file']}::{fn['name']}"
                if fn["accessor"] and fn["name"] not in via:
                    grown[fn["name"]] = fn["file"]
            if removal_site or not grown:
                break
            via.update(grown)
            if not reg["accessor"]:
                # Callers of an accessor may live anywhere its NAME resolves,
                # not only where the static's name resolves.
                candidates = fn_index.get(crate, [])
        reg["removal"] = removal_site
    sites = {
        f"{fn['file']}::{fn['name']}": fn["removes"]
        for entries in fn_index.values()
        for fn in entries
    }
    # A name defined twice in one file (impl blocks) removes if either does.
    for entries in fn_index.values():
        for fn in entries:
            if fn["removes"]:
                sites[f"{fn['file']}::{fn['name']}"] = True
    return registries, sites


def load_allowlist(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return json.loads(path.read_text(encoding="utf-8"))["registries"]


def allowlist_problems(allowlist: list[dict], sites: dict[str, bool] | None = None) -> list[str]:
    problems: list[str] = []
    seen: set[tuple[str, str, str]] = set()
    for entry in allowlist:
        key = entry_key(entry)
        label = f"{key[0]}:{key[1]}" + (f" (in fn {key[2]})" if key[2] else "")
        if entry.get("verdict") not in VERDICTS:
            problems.append(f"{label}: verdict {entry.get('verdict')!r} is not one of {sorted(VERDICTS)}")
        if key in seen:
            problems.append(f"{label}: duplicate entry")
        seen.add(key)
        if entry.get("verdict") == "removed_elsewhere" and sites is not None:
            site = (entry.get("site") or "").strip()
            if not site:
                problems.append(f"{label}: removed_elsewhere must name the removing `site` (file::fn)")
            elif site not in sites:
                problems.append(
                    f"{label}: removal site {site!r} is not a production function any more; "
                    f"re-establish the removal path or the verdict"
                )
            elif not sites[site]:
                problems.append(f"{label}: removal site {site!r} no longer calls a removal method")
        why = (entry.get("why") or "").strip()
        if len(why) < MIN_WHY:
            problems.append(
                f"{label}: `why` is missing or too short to be a reason ({why!r}); an "
                f"unreadable justification silences a registry as well as no gate"
            )
    return problems


def entry_key(entry: dict) -> tuple[str, str, str]:
    """(file, name, accessor fn). The fn disambiguates function-body statics
    that share a name inside one file (`fn a() { static MAP }`)."""
    return (entry.get("file", ""), entry.get("name", ""), entry.get("fn") or "")


def registry_key(reg: dict) -> tuple[str, str, str]:
    return (reg["file"], reg["name"], reg["accessor"] or "")


def evaluate(registries: list[dict], allowlist: list[dict]) -> tuple[list[dict], list[dict]]:
    """(unexempted leaks, stale allowlist entries)."""
    index = {entry_key(e) for e in allowlist}
    used: set[tuple[str, str, str]] = set()
    leaks: list[dict] = []
    for reg in registries:
        if reg["removal"]:
            continue
        key = registry_key(reg)
        if key in index:
            used.add(key)
        else:
            leaks.append(reg)
    stale = [e for e in allowlist if entry_key(e) not in used]
    return leaks, stale


def report(root: Path, allowlist_path: Path, floors: bool = True, quiet: bool = False) -> int:
    registries, sites = scan_with_sites(root)
    removing = sum(1 for r in registries if r["removal"])
    if floors and len(registries) < MIN_REGISTRIES:
        print(
            f"registry_lifetime_check: only {len(registries)} registries enumerated "
            f"(floor {MIN_REGISTRIES}); the declaration parser has probably stopped "
            f"matching, and an empty scan is not a green one",
            file=sys.stderr,
        )
        return 2
    if floors and removing < MIN_REMOVING:
        print(
            f"registry_lifetime_check: only {removing} registries have a removal path "
            f"(floor {MIN_REMOVING}); the removal/test-stripping rules have probably "
            f"stopped matching",
            file=sys.stderr,
        )
        return 2

    allowlist = load_allowlist(allowlist_path)
    problems = allowlist_problems(allowlist, sites)
    leaks, stale = evaluate(registries, allowlist)
    for problem in problems:
        print(f"registry_lifetime_check: allowlist: {problem}", file=sys.stderr)
    for reg in leaks:
        print(
            f"registry_lifetime_check: {reg['file']}:{reg['line']}: `{reg['name']}`"
            f"{' (in fn ' + reg['accessor'] + ')' if reg['accessor'] else ''} "
            f"({reg['type']}) has no production removal path "
            f"(remove/retain/drain/clear/take outside tests). Add one, or give it a "
            f"verdict in {repo_relative(allowlist_path, root) if allowlist_path.is_relative_to(root) else allowlist_path}",
            file=sys.stderr,
        )
    for entry in stale:
        print(
            f"registry_lifetime_check: stale allowlist entry {entry['file']}:{entry['name']}"
            f"{' (in fn ' + entry['fn'] + ')' if entry.get('fn') else ''}: "
            f"no such registry lacks a removal path any more. Delete the entry.",
            file=sys.stderr,
        )
    if problems or leaks or stale:
        return 1
    if not quiet:
        print(
            f"registry_lifetime_check: OK ({len(registries)} registries; {removing} with a "
            f"production removal path; {len(allowlist)} allowlisted)"
        )
    return 0


def print_list(root: Path) -> int:
    allow = {entry_key(e): e for e in load_allowlist(ALLOWLIST_PATH)}
    for reg in sorted(scan(root), key=lambda r: (r["file"], r["line"])):
        entry = allow.get(registry_key(reg))
        status = reg["removal"] or (
            f"{entry['verdict']}: {entry['why']}" if entry else "NO REMOVAL PATH"
        )
        print(f"{reg['file']}:{reg['line']}\t{reg['name']}\t{reg['accessor'] or ''}\t{status}")
    return 0


# --------------------------------------------------------------------------
# Self-test

SELF_TEST_BASE = {
    # Removed only in a tests.rs file: the #11164 shape. Must FAIL.
    "crates/perry-stdlib/src/fetch/mod.rs": """
use std::collections::HashMap;
use std::sync::Mutex;
static FETCH_RESPONSES: std::sync::LazyLock<Mutex<HashMap<usize, u8>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
pub fn insert(id: usize) {
    FETCH_RESPONSES.lock().unwrap().insert(id, 0);
}
""",
    "crates/perry-stdlib/src/fetch/tests.rs": """
fn cleanup(id: usize) {
    FETCH_RESPONSES.lock().unwrap().remove(&id);
}
""",
    # Removed only inside a #[cfg(test)] module of a production file. Must FAIL.
    "crates/perry-runtime/src/cfgtest.rs": """
thread_local! {
    static CFG_ONLY: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}
pub fn put(k: u64) { CFG_ONLY.with(|m| { m.borrow_mut().insert(k, k); }); }
#[cfg(test)]
mod tests {
    #[test]
    fn t() { CFG_ONLY.with(|m| m.borrow_mut().clear()); }
}
""",
    # Production removal in the declaring file. Must PASS.
    "crates/perry-runtime/src/same.rs": """
thread_local! {
    static SAME_FILE: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}
pub fn put(k: u64) { SAME_FILE.with(|m| { m.borrow_mut().insert(k, k); }); }
pub fn release(k: u64) { SAME_FILE.with(|m| { m.borrow_mut().remove(&k); }); }
""",
    # Production removal from a sibling file (the fetch/lifecycle.rs shape). Must PASS.
    "crates/perry-runtime/src/sib/mod.rs": """
pub(crate) static SIBLING: Mutex<BTreeMap<u64, u64>> = Mutex::new(BTreeMap::new());
""",
    "crates/perry-runtime/src/sib/lifecycle.rs": """
use super::SIBLING;
pub(super) fn release(dead: &[u64]) {
    let mut table = SIBLING.lock().unwrap();
    for id in dead {
        table.remove(id);
    }
}
""",
    # Declared inside an accessor fn; callers remove through it. Must PASS.
    "crates/perry-ext-demo/src/lib.rs": """
fn registry() -> &'static Mutex<HashMap<u64, u64>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, u64>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}
pub extern "C" fn js_demo_close(id: u64) {
    registry().lock().unwrap().remove(&id);
}
""",
    # Handed to a closure by a `with_*` helper; the caller's closure removes.
    # Must PASS.
    "crates/perry-runtime/src/with_helper.rs": """
thread_local! {
    static NODES: RefCell<Option<HashMap<u32, u8>>> = const { RefCell::new(None) };
}
fn with_nodes<R, F: FnOnce(&mut HashMap<u32, u8>) -> R>(f: F) -> R {
    NODES.with(|n| f(n.borrow_mut().get_or_insert_with(HashMap::new)))
}
pub extern "C" fn js_node_free(id: u32) {
    with_nodes(|m| {
        m.remove(&id);
    });
}
""",
    # Two same-named tables in one crate; only one has a removal path, and the
    # other must not borrow its credit. `amb_a` must FAIL, `amb_b` PASS.
    "crates/perry-ext-demo/src/amb_a.rs": """
static HANDLES: Mutex<HashMap<u64, u64>> = Mutex::new(HashMap::new());
pub fn open(id: u64) { HANDLES.lock().unwrap().insert(id, id); }
""",
    "crates/perry-ext-demo/src/amb_b.rs": """
static HANDLES: Mutex<HashMap<u64, u64>> = Mutex::new(HashMap::new());
pub fn open(id: u64) { HANDLES.lock().unwrap().insert(id, id); }
pub fn close(id: u64) { HANDLES.lock().unwrap().remove(&id); }
""",
    # Allowlisted, with no removal. Must PASS via the allowlist.
    "crates/perry-runtime/src/bounded.rs": """
static CLASS_TABLE: Mutex<HashMap<u32, u32>> = Mutex::new(HashMap::new());
pub fn put(k: u32) { CLASS_TABLE.lock().unwrap().insert(k, k); }
""",
}

SELF_TEST_ALLOW = [
    {
        "file": "crates/perry-runtime/src/bounded.rs",
        "name": "CLASS_TABLE",
        "verdict": "bounded_by_program",
        "why": "bounded by program size: keyed by class id",
    }
]


def _run_tree(tree: dict[str, str], allow: list[dict]) -> tuple[int, list[dict]]:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for rel, text in tree.items():
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        allow_path = root / "allow.json"
        allow_path.write_text(json.dumps({"registries": allow}), encoding="utf-8")
        import contextlib
        import io

        with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
            code = report(root, allow_path, floors=False)
        return code, scan(root)


def self_test() -> int:
    failures: list[str] = []

    def check(cond: bool, message: str) -> None:
        if not cond:
            failures.append(message)

    code, regs = _run_tree(SELF_TEST_BASE, SELF_TEST_ALLOW)
    by_key = {(r["file"].split("/src/")[1], r["name"]): r for r in regs}
    expect = {
        ("fetch/mod.rs", "FETCH_RESPONSES"): False,
        ("cfgtest.rs", "CFG_ONLY"): False,
        ("same.rs", "SAME_FILE"): True,
        ("sib/mod.rs", "SIBLING"): True,
        ("lib.rs", "REGISTRY"): True,
        ("amb_a.rs", "HANDLES"): False,
        ("amb_b.rs", "HANDLES"): True,
        ("with_helper.rs", "NODES"): True,
        ("bounded.rs", "CLASS_TABLE"): False,
    }
    for key, has_removal in expect.items():
        reg = by_key.get(key)
        if reg is None:
            failures.append(f"planted registry {key} was not enumerated")
            continue
        check(
            bool(reg["removal"]) == has_removal,
            f"{key}: expected removal={has_removal}, got {reg['removal']!r}",
        )
    check(code == 1, f"the planted leaks must fail the gate (exit {code}, want 1)")

    # With the three leaks removed from the tree, the gate goes green.
    clean = {
        k: v
        for k, v in SELF_TEST_BASE.items()
        if not k.endswith(("fetch/mod.rs", "fetch/tests.rs", "cfgtest.rs", "amb_a.rs"))
    }
    code, _ = _run_tree(clean, SELF_TEST_ALLOW)
    check(code == 0, f"a tree with production removal everywhere must pass (exit {code})")

    # A production removal of the #11164 shape makes it pass.
    fixed = dict(clean)
    fixed["crates/perry-stdlib/src/fetch/mod.rs"] = SELF_TEST_BASE["crates/perry-stdlib/src/fetch/mod.rs"]
    fixed["crates/perry-stdlib/src/fetch/lifecycle.rs"] = """
use super::FETCH_RESPONSES;
fn release(dead: &[usize]) {
    let mut table = FETCH_RESPONSES.lock().unwrap();
    for id in dead { table.remove(id); }
}
"""
    code, _ = _run_tree(fixed, SELF_TEST_ALLOW)
    check(code == 0, f"a production release path must satisfy the gate (exit {code})")

    # `#[cfg(any(test, ...))]` still ships: removal under it counts.
    any_cfg = dict(clean)
    any_cfg["crates/perry-runtime/src/anycfg.rs"] = """
static ANY: Mutex<HashMap<u64, u64>> = Mutex::new(HashMap::new());
#[cfg(any(test, feature = "x"))]
pub fn drop_all() { ANY.lock().unwrap().clear(); }
"""
    code, _ = _run_tree(any_cfg, SELF_TEST_ALLOW)
    check(code == 0, f"#[cfg(any(test, ..))] code is production code (exit {code})")

    # A stale allowlist entry fails.
    stale_allow = SELF_TEST_ALLOW + [
        {
            "file": "crates/perry-runtime/src/same.rs",
            "name": "SAME_FILE",
            "verdict": "open_leak",
            "why": "stale: it has a removal path now",
        }
    ]
    code, _ = _run_tree(clean, stale_allow)
    check(code == 1, f"a stale allowlist entry must fail (exit {code})")

    # An entry with no reason, or an invented verdict, fails.
    code, _ = _run_tree(clean, [dict(SELF_TEST_ALLOW[0], why="bounded")])
    check(code == 1, f"an allowlist entry without a readable `why` must fail (exit {code})")
    code, _ = _run_tree(clean, [dict(SELF_TEST_ALLOW[0], verdict="fine")])
    check(code == 1, f"an allowlist entry with an unknown verdict must fail (exit {code})")

    # A `mem::replace` of a FIELD inside a toucher is not a removal (the
    # CLIENT_REQUEST_SURFACE shape). Must FAIL. A whole-table `take()` on the
    # registry itself is. Must PASS.
    swaps = dict(clean)
    swaps["crates/perry-ext-demo/src/surface.rs"] = """
static SURFACE: Mutex<HashMap<u64, State>> = Mutex::new(HashMap::new());
pub fn destroy(h: u64) {
    let mut g = SURFACE.lock().unwrap();
    let st = g.entry(h).or_default();
    let _ = std::mem::replace(&mut st.destroyed, true);
    let _ = st.pending.take();
}
"""
    swaps["crates/perry-runtime/src/whole.rs"] = """
thread_local! {
    static WHOLE: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}
pub fn put(k: u64) { WHOLE.with(|m| { m.borrow_mut().insert(k, k); }); }
pub fn reset() { WHOLE.with(|m| std::mem::take(&mut *m.borrow_mut())); }
pub fn reset2() { let _ = WHOLE.take(); }
"""
    _, regs = _run_tree(swaps, SELF_TEST_ALLOW)
    got = {r["name"]: r["removal"] for r in regs if r["name"] in {"SURFACE", "WHOLE"}}
    check(not got.get("SURFACE"), f"a field mem::replace must not count as removal ({got})")
    check(bool(got.get("WHOLE")), f"a whole-table take must count as removal ({got})")

    # An accessor's callers are scoped: a same-named helper in another module
    # that removes from ITS table must not vouch for this one. Must FAIL.
    shadow = dict(clean)
    shadow["crates/perry-runtime/src/class_env.rs"] = """
thread_local! {
    static ENVS: RefCell<HashMap<u32, u8>> = RefCell::new(HashMap::new());
}
fn with_env<R>(cid: u32, f: impl FnOnce(&mut u8) -> R) -> R {
    ENVS.with(|e| f(e.borrow_mut().entry(cid).or_default()))
}
pub fn set(cid: u32) { with_env(cid, |v| *v = 1); }
"""
    shadow["crates/perry-runtime/src/napi/host.rs"] = """
fn with_env<R>(f: impl FnOnce(&mut Vec<u8>) -> R) -> R { unimplemented!() }
pub fn thunk() { with_env(|v| { v.clear(); }); }
"""
    _, regs = _run_tree(shadow, SELF_TEST_ALLOW)
    envs = [r for r in regs if r["name"] == "ENVS"]
    check(
        len(envs) == 1 and not envs[0]["removal"],
        f"another module's same-named accessor must not vouch ({envs})",
    )

    # removed_elsewhere is checked: a live removing site passes, a missing or
    # non-removing one fails.
    hidden = dict(clean)
    hidden["crates/perry-runtime/src/hidden.rs"] = """
static HIDDEN: Mutex<HashMap<u64, u64>> = Mutex::new(HashMap::new());
pub fn put(k: u64) { HIDDEN.lock().unwrap().insert(k, k); }
pub fn drop_via_raw(map: *mut HashMap<u64, u64>, k: u64) { unsafe { (*map).remove(&k); } }
pub fn peek_via_raw(map: *mut HashMap<u64, u64>, k: u64) -> bool { unsafe { (*map).contains_key(&k) } }
"""
    elsewhere = {
        "file": "crates/perry-runtime/src/hidden.rs",
        "name": "HIDDEN",
        "verdict": "removed_elsewhere",
        "why": "reached through a raw pointer the walk cannot follow",
    }
    for site, want in (
        ("crates/perry-runtime/src/hidden.rs::drop_via_raw", 0),
        ("crates/perry-runtime/src/hidden.rs::peek_via_raw", 1),
        ("crates/perry-runtime/src/hidden.rs::gone", 1),
    ):
        code, _ = _run_tree(hidden, SELF_TEST_ALLOW + [dict(elsewhere, site=site)])
        check(code == want, f"removed_elsewhere site {site}: exit {code}, want {want}")

    # Two function-body statics sharing a name in one file are two registries:
    # removing through one accessor must not vouch for the other.
    twins = dict(clean)
    twins["crates/perry-ext-demo/src/twins.rs"] = """
fn lists() -> &'static Mutex<HashMap<i64, u8>> {
    static MAP: OnceLock<Mutex<HashMap<i64, u8>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}
fn addrs() -> &'static Mutex<HashMap<i64, u16>> {
    static MAP: OnceLock<Mutex<HashMap<i64, u16>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}
pub fn free_list(id: i64) { lists().lock().unwrap().remove(&id); }
pub fn new_addr(id: i64) { addrs().lock().unwrap().insert(id, 0); }
"""
    _, regs = _run_tree(twins, SELF_TEST_ALLOW)
    by_fn = {r["accessor"]: r["removal"] for r in regs if r["file"].endswith("twins.rs")}
    check(bool(by_fn.get("lists")), f"removal through lists() must count for its MAP ({by_fn})")
    check(not by_fn.get("addrs"), f"removal through lists() must not count for addrs()'s MAP ({by_fn})")

    if failures:
        for failure in failures:
            print(f"registry_lifetime_check self-test FAILED: {failure}", file=sys.stderr)
        return 1
    print(
        "registry_lifetime_check self-test: OK (test-only removal, cfg(test) removal and a "
        "same-named table's removal all rejected; same-file, sibling-file and accessor removal "
        "accepted; stale/unreasoned allowlist entries rejected)"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true", help="check the checker")
    parser.add_argument("--list", action="store_true", help="print every registry and its removal site")
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.list:
        return print_list(REPO_ROOT)
    return report(REPO_ROOT, ALLOWLIST_PATH, quiet=args.quiet)


if __name__ == "__main__":
    sys.exit(main())
