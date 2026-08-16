#!/usr/bin/env python3
"""Custody gate for REKEYED address-keyed side tables (#8174).

WHY THIS EXISTS
---------------

`RuntimeRootVisitor::visit_metadata_usize_slot` (and its `i64` / raw-pointer
siblings) rewrite a recorded raw heap address if a moving collection forwarded
it, and deliberately do NOT mark it. That is the whole point: the address is a
side table's KEY, not a reference the program can reach, so keeping it alive
would leak. The cost of that choice is that the key's object CAN die, and the
arena can then recycle the address under it.

When that happens the copying minor's rewrite pass reads the recycled bytes as
a `GcHeader`. #8040 is what that looks like: recycled payload bytes with
`gc_flags = 0x86` — `GC_FLAG_FORWARDED` set by coincidence — `obj_type = 104`,
and a "forwarding pointer" that was really a NaN-boxed value. The walk followed
it, the caller masked it to 48 bits, and a synthetic class's id was bound to an
interned string. The program died several collections later in an unrelated
function with `TypeError: value is not a function`, and tracing it back took
days.

`gc::dead_owner` is the fix for that class: it drops entries whose key object is
provably dead, so no dead key survives to be walked. #8168 wired up the one
table that had been missed. Nothing was checking, and nothing would catch the
next one — the invariant was maintained by a hand-written list happening to be
complete. This script is that check.

WHAT IT DOES
------------

1. **Enumerate** every `visit_metadata_*` call site in `perry-runtime` /
   `perry-stdlib`, attributed to its enclosing `fn`. That set IS the population
   of rekeyed keys — a slot rewritten without being marked.

2. **Read the runtime registry**: `DEAD_KEY_PRUNES` in
   `crates/perry-runtime/src/gc/dead_owner.rs`, the array `fan_out()` iterates.
   A `death: "dead_owner:<fn>"` verdict must name a prune that is actually in
   it, so deleting a prune from the registry fails here rather than silently
   reopening #8040.

3. **Require a written verdict** for every site, in
   `scripts/gc_rekeyed_key_tables.json`. A new site with no entry fails; an
   entry that matches no site fails (a stale exemption is how these gates rot —
   same rule as `scripts/gc_root_dominance_allowlist.json` and
   `scripts/gc_runtime_root_holders.json`).

VERDICTS
--------

* `dead_owner:<fn>`   — pruned by a registered `DEAD_KEY_PRUNES` entry.
* `self_pruned:<fn>`  — pruned by the table's own GC pass, not via
                        `dead_owner`. `<fn>` must exist in the tree.
* `key_is_rooted`     — some scanner strongly MARKS the same address, so the
                        key cannot die while the entry lives. `why` must name
                        the marking site.
* `not_a_gc_address`  — the key is a handle id, an fd, or a `Box::leak`'d
                        pseudo-object with no `GcHeader`, so it is never
                        forwardable. `why` must say which.
* `open_gap:#<issue>` — no prune and no rooting, tracked. Permitted, CAPPED,
                        and never allowed to grow: `MAX_OPEN_GAPS` is the count
                        at landing time.

HOW IT FAILS
------------

* a rekey site with no manifest entry               -> exit 1
* a manifest entry matching no rekey site           -> exit 1
* `dead_owner:<fn>` not in the runtime registry     -> exit 1
* `self_pruned:<fn>` not defined anywhere           -> exit 1
* a verdict with no `why`                           -> exit 1
* more `open_gap` entries than `MAX_OPEN_GAPS`      -> exit 1
* fewer than `MIN_SITES` sites matched              -> exit 2 (regex rot: a
  scan that stopped matching would otherwise report a clean, empty, green run)
* fewer than `MIN_REGISTRY_PRUNES` registry entries -> exit 2, same reason

WHAT THIS GATE CANNOT SEE
-------------------------

Named, because an unstated limit is how a gate gets trusted past its subject.

* **Whether a prune is CORRECT.** It reads the wiring, not the predicate. A
  prune that narrows on the wrong `obj_type` reads as covered here.
* **A key rekeyed by hand**, outside `visit_metadata_*` — e.g. a table walked
  by a per-object move hook (`gc_type_after_payload_move`). Those are not in
  the population and need no entry; three registry prunes
  (`ELEMENT_SHAPES`, `OBJECT_PROTOTYPES`, `exotic_expando`) are exactly that
  shape, which is why registry entries are NOT required to have a site.
* **Timing.** On the copying minor the rewrite pass runs BEFORE the prune
  (`copying.rs`: rewrite, then `finalize_dead_copied_minor_from_space_side_
  allocations`). A prune therefore protects the NEXT cycle, which is sound only
  because the address is not recycled until the later from-space flip. This
  gate does not check that ordering.

Usage:
    python3 scripts/gc_rekeyed_key_tables.py             # check the repo
    python3 scripts/gc_rekeyed_key_tables.py --list      # print the population
    python3 scripts/gc_rekeyed_key_tables.py --self-test # check the checker
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST_NAME = "scripts/gc_rekeyed_key_tables.json"
REGISTRY_PATH = "crates/perry-runtime/src/gc/dead_owner.rs"
CRATES = ("crates/perry-runtime/src", "crates/perry-stdlib/src")

# `gc/roots.rs` DEFINES the three methods; its own bodies are not call sites.
EXCLUDED_FILES = ("crates/perry-runtime/src/gc/roots.rs",)

# Floors. A regex that stops matching must fail loudly, not report a clean run.
MIN_SITES = 30
MIN_REGISTRY_PRUNES = 16
# Declared, tracked `open_gap` verdicts allowed. ZERO, and it stays zero: the
# audit that came with this gate found six rekeyed tables with no death story
# (#8190-#8195) and all six were fixed rather than exempted, so there is no
# precedent here for declaring one. Raising this is a decision to ship a known
# #8040 exposure, not a fix.
MAX_OPEN_GAPS = 0

REKEY_CALL = re.compile(
    r"\.visit_metadata_(?:usize_slot|i64_slot|usize_raw_slot)\s*\("
)
FN_DECL = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?"
    r"(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?fn\s+([A-Za-z0-9_]+)"
)
REGISTRY_PRUNE = re.compile(r"prune:\s*crate::(?:[A-Za-z0-9_]+::)*([A-Za-z0-9_]+)\s*,")
ANY_FN_DEF = re.compile(r"\bfn\s+([A-Za-z0-9_]+)")

VERDICT_KINDS = (
    "dead_owner",
    "self_pruned",
    "key_is_rooted",
    "not_a_gc_address",
    "open_gap",
)


def is_test_path(rel: str) -> bool:
    return (
        "/tests/" in rel
        or rel.endswith("/tests.rs")
        or rel.endswith("_tests.rs")
        or "/benches/" in rel
    )


def collect_sites(root: Path) -> list[tuple[str, str, int]]:
    """Every rekey call site as (repo-relative file, enclosing fn, line)."""
    sites: list[tuple[str, str, int]] = []
    for crate in CRATES:
        base = root / crate
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if is_test_path(rel) or rel in EXCLUDED_FILES:
                continue
            enclosing = "<file scope>"
            for lineno, line in enumerate(path.read_text().splitlines(), 1):
                decl = FN_DECL.match(line)
                if decl:
                    enclosing = decl.group(1)
                stripped = line.lstrip()
                # A doc comment showing the call is documentation, not a site.
                if stripped.startswith("//"):
                    continue
                if REKEY_CALL.search(line):
                    sites.append((rel, enclosing, lineno))
    return sites


def registry_prunes(root: Path) -> set[str]:
    path = root / REGISTRY_PATH
    if not path.is_file():
        return set()
    text = path.read_text()
    start = text.find("DEAD_KEY_PRUNES")
    if start < 0:
        return set()
    return set(REGISTRY_PRUNE.findall(text[start:]))


def defined_fns(root: Path) -> set[str]:
    names: set[str] = set()
    for crate in CRATES:
        base = root / crate
        if not base.is_dir():
            continue
        for path in base.rglob("*.rs"):
            names.update(ANY_FN_DEF.findall(path.read_text()))
    return names


def load_manifest(root: Path) -> list[dict]:
    path = root / MANIFEST_NAME
    if not path.is_file():
        return []
    data = json.loads(path.read_text())
    return data.get("sites", data) if isinstance(data, dict) else data


def check(root: Path, *, min_sites: int = MIN_SITES,
          min_registry: int = MIN_REGISTRY_PRUNES,
          max_open_gaps: int = MAX_OPEN_GAPS) -> tuple[int, list[str]]:
    out: list[str] = []
    sites = collect_sites(root)
    keys = {f"{rel}::{fn}" for rel, fn, _ in sites}
    prunes = registry_prunes(root)

    if len(keys) < min_sites:
        return 2, [
            f"only {len(keys)} rekey sites matched (floor {min_sites}). The scan "
            "regex or the crate list is broken; an empty scan reads as a clean "
            "run, so this is a hard failure rather than a pass.",
        ]
    if len(prunes) < min_registry:
        return 2, [
            f"only {len(prunes)} entries parsed out of DEAD_KEY_PRUNES in "
            f"{REGISTRY_PATH} (floor {min_registry}). Every `dead_owner:` "
            "verdict would read as unregistered; refusing to adjudicate.",
        ]

    manifest = load_manifest(root)
    by_site: dict[str, dict] = {}
    for entry in manifest:
        site = entry.get("site", "")
        if site in by_site:
            out.append(f"duplicate manifest entry for {site}")
        by_site[site] = entry

    open_gaps = 0
    for site in sorted(keys):
        entry = by_site.get(site)
        if entry is None:
            out.append(
                f"UNCLASSIFIED rekey site {site}: a metadata key is rewritten "
                "here without being marked, so its object can die and the "
                "recycled address can be walked as a forwarding header "
                f"(#8174). Add an entry to {MANIFEST_NAME}."
            )
            continue
        death = str(entry.get("death", ""))
        why = str(entry.get("why", "")).strip()
        kind, _, arg = death.partition(":")
        if kind not in VERDICT_KINDS:
            out.append(f"{site}: unknown death verdict {death!r}")
            continue
        if len(why) < 20:
            out.append(f"{site}: verdict {death!r} needs a `why` that says how")
        if kind == "dead_owner":
            if arg not in prunes:
                out.append(
                    f"{site}: death names `{arg}`, which is not in "
                    f"DEAD_KEY_PRUNES ({REGISTRY_PATH}). Either the prune was "
                    "removed from the registry or the verdict is wrong."
                )
        elif kind == "self_pruned":
            if arg not in defined_fns(root):
                out.append(f"{site}: self_pruned names `{arg}`, which is not defined")
        elif kind == "open_gap":
            open_gaps += 1
            if not re.fullmatch(r"#\d+", arg):
                out.append(f"{site}: open_gap must name an issue, got {arg!r}")

    for site in sorted(by_site):
        if site not in keys:
            out.append(
                f"STALE manifest entry {site}: no rekey site matches it any "
                "more. A fix must delete its own exemption."
            )

    if open_gaps > max_open_gaps:
        out.append(
            f"{open_gaps} open_gap verdicts, cap is {max_open_gaps}. A new "
            "rekeyed table without a prune is #8040 again; wire it into "
            "DEAD_KEY_PRUNES instead of raising the cap."
        )

    if out:
        return 1, out
    return 0, [
        f"{len(keys)} rekey sites, {len(prunes)} registered prunes, "
        f"{open_gaps}/{max_open_gaps} declared gaps — all classified."
    ]


# ---------------------------------------------------------------------------
# Self-test: plant each failure shape and require the checker to reject it.
# A gate nobody has watched fail is a gate nobody has tested.
# ---------------------------------------------------------------------------
GOOD_REGISTRY = """
pub(super) const DEAD_KEY_PRUNES: &[DeadKeyPrune] = &[
    DeadKeyPrune { table: "T1", owner: DeadKeyOwner::Any, prune: crate::a::prune_one, },
    DeadKeyPrune { table: "T2", owner: DeadKeyOwner::Closure, prune: crate::b::c::prune_two, },
];
"""

GOOD_SOURCE = """
fn scan_one_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    visitor.visit_metadata_usize_slot(&mut owner);
}
fn scan_two_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    visitor.visit_metadata_i64_slot(&mut key);
}
fn prune_one(_d: &dyn Fn(usize) -> bool) {}
fn prune_two(_d: &dyn Fn(usize) -> bool) {}
fn its_own_pass(_d: &dyn Fn(usize) -> bool) {}
"""

GOOD_MANIFEST = {
    "sites": [
        {
            "site": "crates/perry-runtime/src/one.rs::scan_one_mut",
            "table": "T1",
            "death": "dead_owner:prune_one",
            "why": "pruned by the registered T1 prune in the fan-out",
        },
        {
            "site": "crates/perry-runtime/src/one.rs::scan_two_mut",
            "table": "T2",
            "death": "self_pruned:its_own_pass",
            "why": "pruned by its own registry pass at collection time",
        },
    ]
}

SELF_TEST_KWARGS = {"min_sites": 2, "min_registry": 2, "max_open_gaps": 0}


def _plant(root: Path, registry: str, source: str, manifest: dict) -> None:
    (root / "crates/perry-runtime/src/gc").mkdir(parents=True, exist_ok=True)
    (root / "scripts").mkdir(parents=True, exist_ok=True)
    (root / REGISTRY_PATH).write_text(registry)
    (root / "crates/perry-runtime/src/one.rs").write_text(source)
    (root / MANIFEST_NAME).write_text(json.dumps(manifest))


def _run(registry: str, source: str, manifest: dict, **kw) -> tuple[int, list[str]]:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _plant(root, registry, source, manifest)
        return check(root, **{**SELF_TEST_KWARGS, **kw})


def self_test() -> int:
    import copy

    failures: list[str] = []

    def expect(label: str, want: int, got: tuple[int, list[str]]) -> None:
        if got[0] != want:
            failures.append(f"{label}: wanted exit {want}, got {got[0]} {got[1]}")

    expect("a correctly classified tree passes", 0,
           _run(GOOD_REGISTRY, GOOD_SOURCE, GOOD_MANIFEST))

    # 1. A NEW rekey site with no verdict.
    expect("new unclassified rekey site", 1, _run(
        GOOD_REGISTRY,
        GOOD_SOURCE + "\nfn scan_three_mut(v: &mut V) { v.visit_metadata_usize_slot(&mut k); }\n",
        GOOD_MANIFEST))

    # 2. A stale entry that matches nothing.
    stale = copy.deepcopy(GOOD_MANIFEST)
    stale["sites"].append({
        "site": "crates/perry-runtime/src/one.rs::scan_gone_mut",
        "table": "T9", "death": "dead_owner:prune_one",
        "why": "a table that was deleted, whose exemption was not",
    })
    expect("stale manifest entry", 1, _run(GOOD_REGISTRY, GOOD_SOURCE, stale))

    # 3. The prune is dropped from the runtime registry.
    expect("prune removed from DEAD_KEY_PRUNES", 1, _run(
        GOOD_REGISTRY.replace(
            'DeadKeyPrune { table: "T1", owner: DeadKeyOwner::Any, prune: crate::a::prune_one, },',
            'DeadKeyPrune { table: "T1", owner: DeadKeyOwner::Any, prune: crate::a::prune_other, },'),
        GOOD_SOURCE, GOOD_MANIFEST))

    # 4. `self_pruned` naming a function that does not exist.
    ghost = copy.deepcopy(GOOD_MANIFEST)
    ghost["sites"][1]["death"] = "self_pruned:no_such_pass"
    expect("self_pruned names a missing fn", 1, _run(GOOD_REGISTRY, GOOD_SOURCE, ghost))

    # 5. A verdict with no reasoning.
    bare = copy.deepcopy(GOOD_MANIFEST)
    bare["sites"][0]["why"] = "because"
    expect("verdict without a why", 1, _run(GOOD_REGISTRY, GOOD_SOURCE, bare))

    # 6. An open_gap over the cap.
    gap = copy.deepcopy(GOOD_MANIFEST)
    gap["sites"][1]["death"] = "open_gap:#8174"
    expect("open_gap over the cap", 1, _run(GOOD_REGISTRY, GOOD_SOURCE, gap))
    expect("open_gap under the cap passes", 0,
           _run(GOOD_REGISTRY, GOOD_SOURCE, gap, max_open_gaps=1))
    nonum = copy.deepcopy(gap)
    nonum["sites"][1]["death"] = "open_gap:soon"
    expect("open_gap without an issue number", 1,
           _run(GOOD_REGISTRY, GOOD_SOURCE, nonum, max_open_gaps=1))

    # 7. Floor rot: the scan finds nothing / the registry parse finds nothing.
    expect("scan regex rot", 2, _run(GOOD_REGISTRY, "fn nothing() {}\n", {"sites": []}))
    expect("registry parse rot", 2,
           _run("pub(super) const DEAD_KEY_PRUNES: &[DeadKeyPrune] = &[];\n",
                GOOD_SOURCE, GOOD_MANIFEST))

    # 8. A doc comment showing the call is not a site.
    expect("a doc comment is not a call site", 0, _run(
        GOOD_REGISTRY,
        GOOD_SOURCE + "\n//! see `visitor.visit_metadata_usize_slot(&mut key)`\n",
        GOOD_MANIFEST))

    for line in failures:
        print(f"self-test FAIL: {line}")
    if failures:
        return 1
    print("self-test: 12 planted shapes, all adjudicated as expected")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--list", action="store_true", help="print the population")
    ap.add_argument("--self-test", action="store_true", help="check the checker")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    if args.list:
        manifest = {e.get("site"): e for e in load_manifest(REPO_ROOT)}
        for rel, fn, lineno in collect_sites(REPO_ROOT):
            entry = manifest.get(f"{rel}::{fn}", {})
            print(f"{rel}:{lineno}\t{fn}\t{entry.get('table', '?')}\t"
                  f"{entry.get('death', 'UNCLASSIFIED')}")
        return 0

    code, messages = check(REPO_ROOT)
    for line in messages:
        print(("gc_rekeyed_key_tables: " if code else "") + line)
    return code


if __name__ == "__main__":
    sys.exit(main())
