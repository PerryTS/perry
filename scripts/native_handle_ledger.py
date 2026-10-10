#!/usr/bin/env python3
"""Native-handle registry and producer ratchet (#11919, L1/L3).

This is a source census, not a grep counter.  It walks the shipped Rust source
in perry-runtime, perry-stdlib, perry-ffi and every perry-ext-* crate, removes
test-only items and comments, then records two kinds of migration debt:

* ``tables``: static/thread-local map- or set-like tables whose key is an
  integer handle/id, a pointer, or an address-shaped tuple.  A short explicit
  list covers slab/vector registries and the ext-zlib state bundle, whose map
  fields are hidden behind a named struct.
* ``producers``: production calls to register_handle /
  register_reclaimable_handle plus every private handle-family minting shape
  (fetch, streams, zlib, net, TLS, Proxy, EventEmitter, container, PDF,
  parcel-watcher and media).

Precision matters.  Comments, tests/ directories, *tests.rs files,
``#[cfg(test)]`` items and ``#[test]`` functions are removed before matching.
String/content-keyed maps are not handle tables.  Nor are port/config hashes,
class-id metadata, GC page bookkeeping, code-address metadata or diagnostic
hash sets; those stable exclusions are named below instead of being hidden in
a broad negative regexp.  See the changelog fragment for the audit boundary.

The checked-in ledger is a per-file ceiling, with both categories on every
line.  A file not listed is locked at zero, and a stale line for a now-clean
file fails so cleanup has to delete its permission.  ``--update`` may only
lower existing per-file and aggregate ceilings.  ``--no-raise-vs`` compares
the recorded ledger with a git revision, closing the usual loophole where the
same pull request raises both the source count and its baseline. A precise
``# moved-from: PATH`` annotation may transfer TABLE counts only when the move
is verified against the base tree by identity: the destination's recorded
count may not exceed its handle-keyed declarations that already existed by
name at the base (in the destination or the source) and that the source no
longer declares, so no table born in this diff is ever credited. Credit is also
bounded by what the source surrendered in the ledger; split destinations share
it, aggregate counts cannot rise, and landed annotations grant no new credit.
Producer counts are anonymous call sites, so they never move by annotation.

Usage:
    python3 scripts/native_handle_ledger.py
    python3 scripts/native_handle_ledger.py --update
    python3 scripts/native_handle_ledger.py --no-raise-vs <ref>
    python3 scripts/native_handle_ledger.py --self-test
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

from gc_runtime_root_holders import crate_source_files, declarations, repo_relative, strip_comments
from registry_lifetime_check import gated_crates, strip_cfg_test


ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "scripts" / "native_handle_ledger.txt"

MAP_TYPES = (
    "HashMap", "BTreeMap", "DashMap", "IndexMap", "FxHashMap", "AHashMap",
    "PtrHashMap", "StdHashMap", "HashSet", "BTreeSet", "DashSet",
    "IndexSet", "FxHashSet", "AHashSet", "PtrHashSet", "StdHashSet", "Slab",
)
MAP_START = re.compile(r"\b(" + "|".join(MAP_TYPES) + r")\s*<")
ADDRESS_KEY = re.compile(
    r"(?:^|[^A-Za-z0-9_])(?:[iu](?:8|16|32|64|128|size)|Handle|AgentId)\b"
    r"|\*(?:mut|const)\b|\bNonNull\s*<"
)

# These declarations have an integer-ish key, but the key is neither a native
# handle nor a GC-object heap address.  Keep this list exact: a stale entry is
# caught by --self-test's exclusion validation in the real tree scan.
NON_HANDLE_TABLES = {
    # GC page/card/diagnostic bookkeeping.
    ("crates/perry-runtime/src/arena/region.rs", "REGIONS"),
    ("crates/perry-runtime/src/arena/page_meta/mod.rs", "OLD_GEN_PAGE_PROMOTED_RUNS"),
    ("crates/perry-runtime/src/gc/barrier/mod.rs", "DIRTY_OLD_PAGES"),
    ("crates/perry-runtime/src/gc/barrier/mod.rs", "EXTERNAL_DIRTY_SLOT_PAGES"),
    ("crates/perry-runtime/src/gc/barrier/mod.rs", "REMEMBERED_SET"),
    ("crates/perry-runtime/src/gc/barrier/mod.rs", "CONS_PINNED"),
    ("crates/perry-runtime/src/gc/barrier/mod.rs", "EVER_DIRTY_OLD_PAGES"),
    ("crates/perry-runtime/src/gc/fromspace_scan.rs", "SNAPSHOT_PAGES"),
    ("crates/perry-runtime/src/gc/diag_sites.rs", "CHARGES"),
    # Compiled code/literal/site metadata, not heap object identity.
    ("crates/perry-runtime/src/builtins/fn_metadata.rs", "REGISTRY"),
    ("crates/perry-runtime/src/builtins/fn_metadata.rs", "OVERRIDES"),
    ("crates/perry-runtime/src/object/native_module/callable_exports/builtin_closure_metadata.rs", "BUILTIN_CLOSURE_NON_CONSTRUCTABLE"),
    # Class ids are compiler metadata ids, not native-resource handles.
    ("crates/perry-runtime/src/object/class_constructors.rs", "CLASS_CAPTURE_VALUES"),
    ("crates/perry-runtime/src/object/class_env.rs", "CLASS_ENVS"),
    ("crates/perry-runtime/src/object/data_view_registry.rs", "EXTENDS_DATA_VIEW_REGISTRY"),
    ("crates/perry-runtime/src/object/data_view_registry.rs", "EXTENDS_TYPED_ARRAY_REGISTRY"),
    # A TLS option hash and a TCP port respectively.
    ("crates/perry-ext-http/src/client_turnloop/tls.rs", "CONFIGS"),
    ("crates/perry-ext-http/src/tls_client.rs", "INTERNAL_HTTPS_SERVERS"),
    # Keyed by agent id: one state bundle per thread agent, not per resource.
}

# Numeric class registries are concentrated in these modules.  The one
# address-keyed CLASS_PROTOTYPE_ADDR_COUNTS table is intentionally retained.
NON_HANDLE_CLASS_PATHS = (
    "crates/perry-runtime/src/object/class_meta_registry.rs",
    "crates/perry-runtime/src/object/class_registry/",
)

# Slabs/vectors used as id-indexed tables, plus an opaque state bundle.  These
# are real L1 tables but cannot be inferred from a HashMap-shaped declaration.
EXTRA_TABLES: dict[tuple[str, str], int] = {
    ("crates/perry-ffi/src/handle.rs", "INDEXED_IDS"): 1,
    ("crates/perry-runtime/src/proxy.rs", "PROXIES"): 1,
    # Custom key wrappers whose fields contain NaN-box/heap-address bits.
    ("crates/perry-runtime/src/proxy.rs", "REFLECT_METADATA"): 1,
    ("crates/perry-runtime/src/node_submodules/diagnostics.rs", "DIAG_CHANNEL_BY_KEY"): 1,
    ("crates/perry-runtime/src/geisterhand_registry.rs", "REGISTRY"): 1,
    ("crates/perry-runtime/src/tui/hooks.rs", "SLOTS"): 1,
    ("crates/perry-runtime/src/tui/state.rs", "SLOTS"): 1,
    ("crates/perry-runtime/src/tui/tree.rs", "REGISTRY"): 1,
    ("crates/perry-stdlib/src/common/handle_lifecycle.rs", "ORPHANS"): 1,
    ("crates/perry-stdlib/src/readline/mod.rs", "READLINE_INTERFACES"): 1,
}

REGISTER_CALL = re.compile(
    r"(?<![A-Za-z0-9_])(?:[A-Za-z_]\w*::)*register_(?:reclaimable_)?handle\s*\("
)
RESERVE_CALL = re.compile(
    r"(?<![A-Za-z0-9_])(?:[A-Za-z_]\w*::)*reserve_handle_id_in_domain\s*\("
)

# Path-scoped recognizers for non-register handle producers.  These are the
# outward producer sites, not the allocator helper's implementation call, so a
# zlib factory counts once and register_handle's internal reserve does not
# double-count all 115 callers.
PRODUCER_RULES: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("crates/perry-stdlib/src/fetch/", re.compile(r"\balloc_fetch_handle_id\s*\(")),
    ("crates/perry-stdlib/src/streams", re.compile(r"\bnext_stream_id\s*\(")),
    ("crates/perry-ext-streams/src/lib.rs", re.compile(r"\bnext_id\s*\(\s*&NEXT_")),
    ("crates/perry-stdlib/src/zlib.rs", re.compile(r"\bcreate_zlib_stream\s*\(")),
    ("crates/perry-stdlib/src/tls", re.compile(r"\bnext_tls_handle_id\s*\(")),
    ("crates/perry-ext-net/src/", re.compile(r"\bnext_id_or_throw\s*\(")),
    # Background/adoption paths cannot throw, and use next_id directly.
    ("crates/perry-ext-net/src/adopt.rs", re.compile(r"\bnext_id\s*\(")),
    ("crates/perry-ext-net/src/turnloop_io.rs", re.compile(r"\b(?:crate::)?next_id\s*\(")),
    ("crates/perry-runtime/src/proxy.rs", re.compile(r"\breserve_proxy_id\s*\(")),
    # Proxy.revocable is a second producer surface which delegates to new.
    ("crates/perry-runtime/src/proxy.rs", re.compile(r"(?m)^\s*let proxy = js_proxy_new\s*\(")),
    ("crates/perry-stdlib/src/container/types.rs", re.compile(r"NEXT_HANDLE_ID\.fetch_add\s*\(")),
    ("crates/perry-ext-pdf/src/lib.rs", re.compile(r"\bnext_handle\s*\(")),
    ("crates/perry-ext-parcel-watcher/src/lib.rs", re.compile(r"NEXT_ID\.fetch_add\s*\(")),
    ("crates/perry-runtime/src/media_playback.rs", re.compile(r"HANDLE_SEQ\.fetch_add\s*\(")),
)

TEST_ATTR = re.compile(r"^\s*#\[(?:test|tokio::test|async_std::test)(?:\([^]]*\))?\]\s*$")


def strip_test_items(text: str) -> str:
    """Blank test-attributed functions after registry_lifetime's cfg stripper."""
    code = strip_cfg_test(text)
    lines = strip_comments(code).splitlines()
    out = list(lines)
    i = 0
    while i < len(lines):
        if not TEST_ATTR.match(lines[i]):
            i += 1
            continue
        start = i
        i += 1
        depth = 0
        opened = False
        while i < len(lines):
            depth += lines[i].count("{") - lines[i].count("}")
            opened |= "{" in lines[i]
            i += 1
            if opened and depth <= 0:
                break
            if not opened and lines[i - 1].rstrip().endswith(";"):
                break
        for j in range(start, i):
            out[j] = ""
    return "\n".join(out)


def first_generic_arg(type_text: str, start: int) -> str:
    depth = 0
    for i in range(start, len(type_text)):
        char = type_text[i]
        if char == "<":
            depth += 1
        elif char == ">":
            if depth == 0:
                return type_text[start:i].strip()
            depth -= 1
        elif char == "," and depth == 0:
            return type_text[start:i].strip()
    return type_text[start:].strip()


def handle_keyed(type_text: str) -> bool:
    match = MAP_START.search(type_text)
    if not match:
        return False
    key = first_generic_arg(type_text, match.end())
    # Content containers happen to contain u8/usize tokens, but are not ids.
    if re.match(r"^(?:Vec\s*<|String\b|str\b|&\s*str\b|\[\s*u8\b)", key):
        return False
    return bool(ADDRESS_KEY.search(key)) or match.group(1) == "Slab"


def excluded_table(rel: str, name: str) -> bool:
    if (rel, name) in NON_HANDLE_TABLES:
        return True
    if rel == "crates/perry-runtime/src/object/class_registry/state.rs" and name == "CLASS_PROTOTYPE_ADDR_COUNTS":
        return False
    return any(rel == prefix or rel.startswith(prefix) for prefix in NON_HANDLE_CLASS_PATHS)


def call_count(code: str, pattern: re.Pattern[str]) -> int:
    """Count calls/macro invocations, not a same-named function declaration."""
    count = 0
    for match in pattern.finditer(code):
        line_start = code.rfind("\n", 0, match.start()) + 1
        if re.search(r"\bfn\s*$", code[line_start:match.start()]):
            continue
        count += 1
    return count


def scan(root: Path = ROOT) -> tuple[dict[str, int], dict[str, int], set[tuple[str, str]]]:
    tables: dict[str, int] = defaultdict(int)
    producers: dict[str, int] = defaultdict(int)
    seen_decls: set[tuple[str, str]] = set()
    files = crate_source_files(root, gated_crates(root))
    for path in files:
        rel = repo_relative(path, root)
        code = strip_test_items(path.read_text(encoding="utf-8", errors="replace"))
        for name, _line, type_text in declarations(rel, code):
            seen_decls.add((rel, name))
            if handle_keyed(type_text) and not excluded_table(rel, name):
                tables[rel] += 1
            if (rel, name) in EXTRA_TABLES:
                tables[rel] += EXTRA_TABLES[(rel, name)]
        register_count = call_count(code, REGISTER_CALL) + call_count(code, RESERVE_CALL)
        if register_count:
            producers[rel] += register_count
        for prefix, pattern in PRODUCER_RULES:
            if rel == prefix or rel.startswith(prefix):
                count = call_count(code, pattern)
                if count:
                    producers[rel] += count

    return dict(tables), dict(producers), seen_decls


def combined(tables: dict[str, int], producers: dict[str, int]) -> dict[str, tuple[int, int]]:
    return {path: (tables.get(path, 0), producers.get(path, 0)) for path in sorted(set(tables) | set(producers))}


def parse_ledger(text: str) -> dict[str, tuple[int, int]]:
    out: dict[str, tuple[int, int]] = {}
    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        row, _, annotation = line.partition("#")
        if annotation and not re.fullmatch(r"\s*moved-from:\s*\S+\s*", annotation):
            raise SystemExit(f"{LEDGER.name}:{lineno}: malformed relocation annotation")
        parts = row.split()
        if len(parts) != 3:
            raise SystemExit(f"{LEDGER.name}:{lineno}: expected `TABLES PRODUCERS PATH`")
        tables, producers, path = parts
        if path in out:
            raise SystemExit(f"{LEDGER.name}:{lineno}: duplicate path {path}")
        out[path] = (int(tables), int(producers))
    return out


def ledger_moves(text: str) -> dict[str, str]:
    # Validate every row before interpreting annotations; typos must not vanish
    # into comments and silently revoke or invent relocation credit.
    rows = parse_ledger(text)
    moves = {}
    for raw in text.splitlines():
        row, _, annotation = raw.partition("#")
        if not row.strip() or not annotation:
            continue
        path = row.split()[2]
        source = annotation.split(":", 1)[1].strip()
        if source == path:
            raise SystemExit(f"{LEDGER.name}: {path}: relocation names its own path")
        moves[path] = source
    assert set(moves) <= set(rows)
    return moves


def render(rows: dict[str, tuple[int, int]], moves: dict[str, str] | None = None) -> str:
    moves = moves or {}
    header = (
        "# Native-handle conversion ledger (#11919, L1 tables / L3 producers).\n"
        "# Columns: TABLES PRODUCERS REPOSITORY_RELATIVE_PATH\n"
        "# Generated by: python3 scripts/native_handle_ledger.py --update\n"
        "# Counts and per-file ceilings may only go down.\n"
        "# A moved-from: PATH annotation spends only that source's surrendered counts.\n"
    )
    return header + "".join(
        f"{t} {p} {path}" + (f"  # moved-from: {moves[path]}" if path in moves else "") + "\n"
        for path, (t, p) in sorted(rows.items())
    )


def totals(rows: dict[str, tuple[int, int]]) -> tuple[int, int]:
    return sum(v[0] for v in rows.values()), sum(v[1] for v in rows.values())


def compare_ceiling(
    actual: dict[str, tuple[int, int]],
    ceiling: dict[str, tuple[int, int]],
    *,
    require_no_stale: bool = True,
) -> list[str]:
    bad: list[str] = []
    for path, (tables, producers) in actual.items():
        old_tables, old_producers = ceiling.get(path, (0, 0))
        if tables > old_tables:
            bad.append(f"{path}: tables {tables} exceeds ceiling {old_tables}")
        if producers > old_producers:
            bad.append(f"{path}: producers {producers} exceeds ceiling {old_producers}")
    if require_no_stale:
        for path, pair in ceiling.items():
            if path not in actual:
                bad.append(f"{path}: stale ledger entry {pair}; delete it to lock the cleanup in")
    return bad


def git_show(ref: str) -> str | None:
    resolved = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}"],
        cwd=ROOT, capture_output=True, text=True,
    )
    if resolved.returncode:
        raise SystemExit(
            f"::error::cannot resolve {ref}; fetch the merge-base commit before running the native-handle ratchet"
        )
    shown = subprocess.run(
        ["git", "show", f"{ref}:scripts/native_handle_ledger.txt"],
        cwd=ROOT, capture_output=True, text=True,
    )
    return shown.stdout if shown.returncode == 0 else None


def no_raise_vs(ref: str) -> int:
    base_text = git_show(ref)
    if base_text is None:
        print(f"native-handle ledger did not exist at {ref}; nothing to compare")
        return 0
    base = parse_ledger(base_text)
    head_text = LEDGER.read_text(encoding="utf-8")
    head = parse_ledger(head_text)
    moves = ledger_moves(head_text)
    bad = compare_relocated_ceiling(head, base, moves, verified_moves(ref, moves))
    bt, bp = totals(base)
    ht, hp = totals(head)
    if ht > bt:
        bad.append(f"recorded table total rose {bt} -> {ht}")
    if hp > bp:
        bad.append(f"recorded producer total rose {bp} -> {hp}")
    if bad:
        print(f"::error::native-handle ledger rose vs. {ref}: {len(bad)} violation(s)")
        for problem in bad:
            print(f"  {problem}")
        return 1
    print(f"native-handle ledger vs. {ref}: tables {bt}->{ht}, producers {bp}->{hp}; none raised")
    return 0


def handle_table_names(rel: str, text: str | None) -> set[str]:
    """Names of the shipped handle-keyed tables `rel` declares in `text`."""
    if text is None:
        return set()
    code = strip_test_items(text)
    return {
        name
        for name, _line, type_text in declarations(rel, code)
        if handle_keyed(type_text) and not excluded_table(rel, name)
    }


def moved_table_names(
    dest: str, source: str, base_files: dict[str, str | None], head_files: dict[str, str | None]
) -> set[str]:
    """Tables of `dest` that verifiably came from `source`, or were already there.

    A head declaration of `dest` counts only if a table of that exact name
    existed at the base in `dest` or in `source`, and `source` no longer
    declares it. A table born in this diff never counts, and a copy that left
    the source in place never counts. The base side covers a move that landed
    before the base while the base ledger still charged the source. This
    needs only the two trees (CI checks out at depth 1), not history.
    """
    existed = handle_table_names(dest, base_files.get(dest)) | handle_table_names(
        source, base_files.get(source)
    )
    return (handle_table_names(dest, head_files.get(dest)) & existed) - handle_table_names(
        source, head_files.get(source)
    )


def git_show_file(ref: str, path: str) -> str | None:
    shown = subprocess.run(["git", "show", f"{ref}:{path}"], cwd=ROOT, capture_output=True, text=True)
    return shown.stdout if shown.returncode == 0 else None


def verified_moves(ref: str, moves: dict[str, str]) -> dict[str, int]:
    """Per destination: the table count the base and head trees can account for."""
    paths = set(moves) | set(moves.values())
    base_files = {path: git_show_file(ref, path) for path in paths}
    head_files = {
        path: (ROOT / path).read_text(encoding="utf-8", errors="replace") if (ROOT / path).exists() else None
        for path in paths
    }
    return {dest: len(moved_table_names(dest, source, base_files, head_files)) for dest, source in moves.items()}


def compare_relocated_ceiling(head, base, moves, verified):
    """Each source supplies one shared table pool, bounded by real cleanup.

    Only tables move, and a destination's head count may not exceed what
    `verified` accounts for by identity. Relocations cannot increase either total, spend a source
    twice, or borrow the source's retained debt. A landed annotation is inert
    once base and head agree about both paths.
    """
    pools = {source: max(0, base.get(source, (0, 0))[0] - head.get(source, (0, 0))[0])
             for source in set(moves.values())}
    bad = []
    for path, counts in sorted(head.items()):
        for i, axis in enumerate(("tables", "producers")):
            need = counts[i] - base.get(path, (0, 0))[i]
            if need <= 0:
                continue
            source = moves.get(path)
            if (
                i != 0
                or source is None
                or source == path
                or pools[source] < need
                or verified.get(path, 0) < counts[0]
            ):
                bad.append(f"{path}: {axis} increased by {need} without sufficient surrendered relocation credit")
            else:
                pools[source] -= need
    return bad


def self_test() -> int:
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        src = root / "crates/perry-runtime/src"
        src.mkdir(parents=True)
        (src / "lib.rs").write_text(
            """
use std::collections::HashMap;
static LIVE: std::sync::LazyLock<HashMap<i64, u8>> = todo!();
static WORDS: std::sync::LazyLock<HashMap<String, u8>> = todo!();
// static COMMENT: HashMap<i64, u8> = todo!();
fn make() { register_handle(1_u8); reserve_handle_id_in_domain(domain()); }
#[cfg(test)] static TEST_TABLE: std::sync::LazyLock<HashMap<i64, u8>> = todo!();
#[test]
fn test_only() { register_handle(2_u8); }
""",
            encoding="utf-8",
        )
        t, p, _ = scan(root)
        rel = "crates/perry-runtime/src/lib.rs"
        if t != {rel: 1} or p != {rel: 2}:
            print(f"native_handle_ledger self-test FAILED: precision fixture got tables={t}, producers={p}")
            return 1

    base = {"a.rs": (2, 3), "gone.rs": (1, 0)}
    if compare_ceiling({"a.rs": (2, 3), "gone.rs": (1, 0)}, base):
        print("native_handle_ledger self-test FAILED: clean ceiling comparison failed")
        return 1
    planted = [
        ({"a.rs": (3, 3), "gone.rs": (1, 0)}, "tables 3 exceeds"),
        ({"a.rs": (2, 4), "gone.rs": (1, 0)}, "producers 4 exceeds"),
        ({"a.rs": (2, 3), "new.rs": (1, 0), "gone.rs": (1, 0)}, "new.rs"),
        ({"a.rs": (2, 3)}, "stale ledger entry"),
    ]
    for actual, needle in planted:
        if not any(needle in problem for problem in compare_ceiling(actual, base)):
            print(f"native_handle_ledger self-test FAILED: planted rule did not fire: {needle}")
            return 1
    if compare_ceiling({"a.rs": (1, 2)}, base, require_no_stale=False):
        print("native_handle_ledger self-test FAILED: legal cleanup was rejected by update/base comparison")
        return 1

    relocation_base = {"source.rs": (4, 2)}
    proven = {"dest.rs": 2, "left.rs": 2, "right.rs": 2}
    relocation_cases = [
        ({"source.rs": (2, 2), "dest.rs": (2, 0)}, {"dest.rs": "source.rs"}, proven, False),
        ({"source.rs": (2, 2), "dest.rs": (2, 0)}, {}, proven, True),
        # The ledger alone is not proof: no verified table identity, no credit.
        ({"source.rs": (2, 2), "dest.rs": (2, 0)}, {"dest.rs": "source.rs"}, {}, True),
        ({"source.rs": (2, 2), "dest.rs": (2, 0)}, {"dest.rs": "source.rs"}, {"dest.rs": 1}, True),
        ({"source.rs": (4, 2), "dest.rs": (1, 0)}, {"dest.rs": "source.rs"}, proven, True),
        ({"source.rs": (2, 2), "dest.rs": (3, 0)}, {"dest.rs": "source.rs"}, {"dest.rs": 3}, True),
        # Producers are anonymous call sites: they never move by annotation.
        ({"source.rs": (2, 1), "dest.rs": (2, 1)}, {"dest.rs": "source.rs"}, proven, True),
        ({"left.rs": (2, 0), "right.rs": (2, 0), "source.rs": (0, 2)},
         {"left.rs": "source.rs", "right.rs": "source.rs"}, proven, False),
        ({"left.rs": (3, 0), "right.rs": (2, 0), "source.rs": (0, 2)},
         {"left.rs": "source.rs", "right.rs": "source.rs"}, {"left.rs": 3, "right.rs": 2}, True),
    ]
    for head, moves, verified, rejected in relocation_cases:
        if bool(compare_relocated_ceiling(head, relocation_base, moves, verified)) != rejected:
            print(f"native_handle_ledger self-test FAILED: relocation {head}, {moves}, {verified}")
            return 1
    table = "static {}: std::sync::LazyLock<std::collections::HashMap<usize, u8>> = todo!();\n"
    base_files = {"src.rs": table.format("MOVED") + table.format("KEPT"), "dst.rs": ""}
    head_files = {"src.rs": table.format("KEPT"), "dst.rs": table.format("MOVED") + table.format("FRESH")}
    if moved_table_names("dst.rs", "src.rs", base_files, head_files) != {"MOVED"}:
        print("native_handle_ledger self-test FAILED: identity check credited a table that did not move")
        return 1
    copied = {"src.rs": base_files["src.rs"], "dst.rs": head_files["dst.rs"]}
    if moved_table_names("dst.rs", "src.rs", base_files, copied):
        print("native_handle_ledger self-test FAILED: a copy that left the source in place was credited")
        return 1
    landed_early = {"src.rs": table.format("KEPT"), "dst.rs": table.format("MOVED")}
    if moved_table_names("dst.rs", "src.rs", landed_early, head_files) != {"MOVED"}:
        print("native_handle_ledger self-test FAILED: a move that landed before the base was not accounted")
        return 1
    landed = {"dest.rs": (4, 2)}
    if not compare_relocated_ceiling({"dest.rs": (5, 2)}, landed, {"dest.rs": "source.rs"}, {"dest.rs": 5}):
        print("native_handle_ledger self-test FAILED: landed annotation granted fresh credit")
        return 1
    encoded = render(landed, {"dest.rs": "source.rs"})
    if parse_ledger(encoded) != landed or ledger_moves(encoded) != {"dest.rs": "source.rs"}:
        print("native_handle_ledger self-test FAILED: relocation writer lost its annotation")
        return 1
    for broken in ("1 0 a.rs # moved-from a.rs", "1 0 a.rs # moved-from: a.rs"):
        try:
            ledger_moves(broken)
        except SystemExit:
            pass
        else:
            print("native_handle_ledger self-test FAILED: malformed/self relocation accepted")
            return 1

    tables, producers, seen = scan(ROOT)
    missing = sorted(key for key in NON_HANDLE_TABLES if key not in seen)
    missing += sorted(key for key in EXTRA_TABLES if key not in seen)
    if missing:
        print(f"native_handle_ledger self-test FAILED: stale classification entries: {missing}")
        return 1
    if sum(tables.values()) < 150 or sum(producers.values()) < 100:
        print("native_handle_ledger self-test FAILED: implausibly small real-tree census")
        return 1
    print(
        f"native_handle_ledger self-test: OK ({sum(tables.values())} tables, "
        f"{sum(producers.values())} producers; comments/tests/content maps excluded; "
        "table, producer, new-file and stale-entry raises all caught)"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--update", action="store_true")
    parser.add_argument("--no-raise-vs", metavar="REF")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.no_raise_vs:
        return no_raise_vs(args.no_raise_vs)

    tables, producers, _ = scan(ROOT)
    actual = combined(tables, producers)
    at, ap = totals(actual)
    if args.update:
        if LEDGER.exists():
            old = parse_ledger(LEDGER.read_text(encoding="utf-8"))
            bad = compare_ceiling(actual, old, require_no_stale=False)
            ot, op = totals(old)
            if at > ot or ap > op or bad:
                print(f"refusing to raise native-handle ledger: tables {ot}->{at}, producers {op}->{ap}")
                for problem in bad:
                    print(f"  {problem}")
                return 1
        moves = ledger_moves(LEDGER.read_text(encoding="utf-8")) if LEDGER.exists() else {}
        LEDGER.write_text(render(actual, moves), encoding="utf-8")
        print(f"native-handle ledger updated: {at} tables, {ap} producers across {len(actual)} files")
        return 0
    if not LEDGER.exists():
        print("native-handle ledger missing; run --update")
        return 1
    ceiling = parse_ledger(LEDGER.read_text(encoding="utf-8"))
    bad = compare_ceiling(actual, ceiling)
    ct, cp = totals(ceiling)
    print(f"native-handle ledger: {at} tables (ceiling {ct}), {ap} producers (ceiling {cp})")
    if bad:
        print(f"::error::native-handle ledger has {len(bad)} violation(s)")
        for problem in bad:
            print(f"  {problem}")
        return 1
    if (at, ap) != (ct, cp):
        print("debt fell; run --update to lock the lower count in")
    print(f"per-file ceilings hold across {len(actual)} file(s); every unlisted file is locked at zero")
    return 0


if __name__ == "__main__":
    sys.exit(main())
