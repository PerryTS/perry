#!/usr/bin/env python3
"""Inventory codegen reads of local-binding type hints.

TypeScript annotations are erased, and initializer-derived types become stale
after assignment. Codegen may therefore use a local type only when its runtime
provenance and write stability are explicit, as a dispatch hint for a
runtime-validated path, or alongside an independent representation proof. This
audit keeps that choice explicit:

* ordinary consumers call ``stable_local_type_proof``; its whole-region write
  set conservatively invalidates runtime-derived evidence after any assignment;
* exceptional consumers call ``local_type_hint`` and must explain the runtime
  guard or independent proof;
* branch-scoped narrowings snapshot the prior proof with
  ``snapshot_guarded_proof`` so the narrowing can be undone exactly; that value
  is restore bookkeeping and is never consumed as a type fact;
* every accessor group needs an allowlist classification and reason, including
  ``stable_local_type_proof`` groups; a missing entry fails as an unclassified
  local-type read;
* remaining raw reads in pre-codegen collectors are inventoried as well; and
* a direct read of ``ctx.local_types`` or ``ctx.proven_local_types`` outside the
  accessors is always an error.

The allowlist is count-exact. A new use, a removed use, and a renamed/moved use
all fail until the inventory is reviewed, and an entry matching nothing fails
too. ``--self-test`` plants each failure mode so the gate cannot go vacuously
green.

Usage:
    python3 scripts/local_binding_type_audit.py
    python3 scripts/local_binding_type_audit.py --self-test
    python3 scripts/local_binding_type_audit.py --list
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ALLOWLIST = REPO_ROOT / "scripts" / "local_binding_type_allowlist.json"
SCAN_ROOTS = (
    REPO_ROOT / "crates" / "perry-codegen" / "src",
    REPO_ROOT / "crates" / "perry-hir" / "src",
)
MINIMUM_SITES = 40

SKIP_PARTS = {"tests"}
SKIP_SUFFIXES = ("_tests.rs",)

FN_RE = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")
ACCESS_RE = re.compile(
    r"\b(?P<receiver>[A-Za-z_][A-Za-z0-9_]*|self)\s*\.\s*"
    r"(?P<api>stable_local_type_proof|local_type_hint|snapshot_guarded_proof)\s*\("
)
RAW_RE = re.compile(
    r"\b(?P<receiver>(?:ctx|self)\s*\.\s*(?:local_types|proven_local_types)|self\s*\.\s*locals|module_local_types|"
    r"module_receiver_types|binding_types|proven_types|local_types)\s*"
    r"(?:\.\s*(?:get|get_key_value|get_mut|contains_key|entry|iter|iter_mut|keys|values|values_mut)\s*\(|\[)"
)
GENERIC_LOCAL_FACT_RE = re.compile(r"\bself\s*\.\s*get\s*\(")

CLASSIFICATIONS = {
    "runtime-validated",
    "representation-proven",
    "metadata-only",
}


@dataclass(frozen=True, order=True)
class SiteKey:
    path: str
    function: str
    access: str


@dataclass(frozen=True)
class Site:
    key: SiteKey
    line: int
    text: str


@dataclass(frozen=True)
class Entry:
    key: SiteKey
    count: int
    classification: str
    reason: str


def repo_rel(path: Path) -> str:
    try:
        return path.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def production_rust_files() -> Iterable[Path]:
    for root in SCAN_ROOTS:
        for path in sorted(root.rglob("*.rs")):
            rel_parts = path.relative_to(root).parts
            if any(part in SKIP_PARTS for part in rel_parts):
                continue
            if path.name == "tests.rs" or path.name.endswith(SKIP_SUFFIXES):
                continue
            yield path


def strip_rust_comments(text: str) -> str:
    """Remove line/block comments while preserving newlines and strings."""
    out: list[str] = []
    i = 0
    block_depth = 0
    in_string = False
    in_char = False
    escaped = False
    while i < len(text):
        ch = text[i]
        nxt = text[i + 1] if i + 1 < len(text) else ""
        if block_depth:
            if ch == "/" and nxt == "*":
                block_depth += 1
                out.extend((" ", " "))
                i += 2
                continue
            if ch == "*" and nxt == "/":
                block_depth -= 1
                out.extend((" ", " "))
                i += 2
                continue
            out.append("\n" if ch == "\n" else " ")
            i += 1
            continue
        if not in_string and not in_char and ch == "/" and nxt == "*":
            block_depth = 1
            out.extend((" ", " "))
            i += 2
            continue
        if not in_string and not in_char and ch == "/" and nxt == "/":
            while i < len(text) and text[i] != "\n":
                out.append(" ")
                i += 1
            continue
        out.append(ch)
        if escaped:
            escaped = False
        elif (in_string or in_char) and ch == "\\":
            escaped = True
        elif not in_char and ch == '"':
            in_string = not in_string
        elif not in_string and ch == "'":
            # Lifetimes (`'a`) are common. Treat only a quote followed by a
            # single character and another quote as a character literal.
            if in_char:
                in_char = False
            elif i + 2 < len(text) and text[i + 2] == "'":
                in_char = True
        i += 1
    return "".join(out)


def scan_text(path: Path, text: str) -> tuple[list[Site], list[str]]:
    clean = strip_rust_comments(text)
    raw_lines = text.splitlines()
    sites: list[Site] = []
    errors: list[str] = []
    rel = repo_rel(path)

    functions = [(match.start(), match.group(1)) for match in FN_RE.finditer(clean)]

    def location(offset: int) -> tuple[int, str, str]:
        line_no = clean.count("\n", 0, offset) + 1
        current_fn = "<module>"
        for fn_offset, fn_name in functions:
            if fn_offset > offset:
                break
            current_fn = fn_name
        line_text = raw_lines[line_no - 1].strip() if raw_lines else ""
        return line_no, current_fn, line_text

    for match in ACCESS_RE.finditer(clean):
        line_no, current_fn, line_text = location(match.start())
        sites.append(
            Site(
                SiteKey(rel, current_fn, match.group("api")),
                line_no,
                line_text,
            )
        )

    for match in RAW_RE.finditer(clean):
        line_no, current_fn, line_text = location(match.start())
        receiver = re.sub(r"\s*\.\s*", ".", match.group("receiver"))
        if receiver == "self.local_types" and (
            rel == "crates/perry-codegen/src/expr/mod.rs"
            and current_fn == "local_type_hint"
        ):
            continue
        if receiver == "self.proven_local_types" and (
            (
                rel == "crates/perry-codegen/src/expr/mod.rs"
                and current_fn in {"stable_local_type_proof", "snapshot_guarded_proof"}
            )
            or (
                rel == "crates/perry-codegen/src/type_analysis_facts.rs"
                and current_fn == "local_type"
            )
        ):
            continue
        if receiver in {"ctx.local_types", "ctx.proven_local_types"}:
            errors.append(
                f"{rel}:{line_no}: direct {receiver} read bypasses the "
                "proof API; use stable_local_type_proof or the audited "
                "local_type_hint escape hatch"
            )
            continue
        sites.append(
            Site(
                SiteKey(rel, current_fn, f"raw:{receiver}"),
                line_no,
                line_text,
            )
        )

    if rel == "crates/perry-hir/src/analysis/value_types.rs":
        for match in GENERIC_LOCAL_FACT_RE.finditer(clean):
            line_no, current_fn, line_text = location(match.start())
            if current_fn == "local_type":
                sites.append(
                    Site(
                        SiteKey(rel, current_fn, "raw:HashMapLocalTypeFacts"),
                        line_no,
                        line_text,
                    )
                )
    return sites, errors


def scan_repo() -> tuple[list[Site], list[str]]:
    sites: list[Site] = []
    errors: list[str] = []
    for path in production_rust_files():
        found, bad = scan_text(path, path.read_text(encoding="utf-8"))
        sites.extend(found)
        errors.extend(bad)
    return sites, errors


def load_allowlist(path: Path) -> tuple[list[Entry], list[str]]:
    errors: list[str] = []
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return [], [f"cannot read allowlist {path}: {exc}"]
    if payload.get("schema") != 1:
        errors.append(f"{path}: schema must be 1")
    entries: list[Entry] = []
    seen: set[SiteKey] = set()
    for index, raw in enumerate(payload.get("entries", []), 1):
        where = f"{path}: entry {index}"
        try:
            key = SiteKey(raw["path"], raw["function"], raw["access"])
            count = raw["count"]
            classification = raw["classification"]
            reason = raw["reason"].strip()
        except (KeyError, AttributeError) as exc:
            errors.append(f"{where}: malformed entry ({exc})")
            continue
        if key in seen:
            errors.append(f"{where}: duplicate key {key}")
            continue
        seen.add(key)
        if not isinstance(count, int) or count < 1:
            errors.append(f"{where}: count must be a positive integer")
        if classification not in CLASSIFICATIONS:
            errors.append(
                f"{where}: classification must be one of "
                f"{sorted(CLASSIFICATIONS)}"
            )
        if len(reason) < 24:
            errors.append(f"{where}: reason must explain the validating fact")
        entries.append(Entry(key, count, classification, reason))
    return entries, errors


def audit(
    sites: list[Site],
    raw_errors: list[str],
    entries: list[Entry],
    allowlist_errors: list[str],
    *,
    minimum_sites: int = MINIMUM_SITES,
) -> list[str]:
    errors = [*raw_errors, *allowlist_errors]
    grouped: dict[SiteKey, list[Site]] = defaultdict(list)
    for site in sites:
        grouped[site.key].append(site)

    if len(sites) < minimum_sites:
        errors.append(
            f"candidate floor failed: found {len(sites)} local-type reads, "
            f"expected at least {minimum_sites}; the scan scope may be stale"
        )

    allowed = {entry.key: entry for entry in entries}
    for key, found in sorted(grouped.items()):
        entry = allowed.get(key)
        rendered = f"{key.path}::{key.function} [{key.access}]"
        if entry is None:
            lines = ", ".join(str(site.line) for site in found)
            errors.append(f"unclassified local-type read: {rendered} at lines {lines}")
        elif entry.count != len(found):
            lines = ", ".join(str(site.line) for site in found)
            errors.append(
                f"count drift for {rendered}: allowlist={entry.count}, "
                f"source={len(found)} at lines {lines}"
            )

    for key in sorted(set(allowed) - set(grouped)):
        errors.append(
            f"stale allowlist entry matches nothing: "
            f"{key.path}::{key.function} [{key.access}]"
        )
    return errors


def print_inventory(sites: list[Site], entries: list[Entry]) -> None:
    grouped: dict[SiteKey, int] = Counter(site.key for site in sites)
    allowed = {entry.key: entry for entry in entries}
    for key, count in sorted(grouped.items()):
        entry = allowed.get(key)
        classification = entry.classification if entry else "UNCLASSIFIED"
        print(
            f"{key.path}::{key.function} [{key.access}] "
            f"count={count} class={classification}"
        )
        if entry:
            print(f"  {entry.reason}")
    counts = Counter(
        allowed[key].classification if key in allowed else "UNCLASSIFIED"
        for key in grouped
    )
    print(f"sites={len(sites)} groups={len(grouped)} classes={dict(sorted(counts.items()))}")


def self_test() -> int:
    fixture_path = REPO_ROOT / "crates" / "perry-codegen" / "src" / "fixture.rs"
    fixture = """
fn stable(ctx: &FnCtx<'_>, id: &u32) {
    let _ = ctx.stable_local_type_proof(id);
}
fn guarded(ctx: &FnCtx<'_>, id: &u32) {
    let _ = ctx.local_type_hint(id);
}
fn collector(local_types: &Map, id: &u32) {
    let _ = local_types.get(id);
}
"""
    sites, raw = scan_text(fixture_path, fixture)
    entries = [
        Entry(
            SiteKey(repo_rel(fixture_path), "stable", "stable_local_type_proof"),
            1,
            "representation-proven",
            "Fixture reads runtime-derived evidence invalidated by every write.",
        ),
        Entry(
            SiteKey(repo_rel(fixture_path), "guarded", "local_type_hint"),
            1,
            "runtime-validated",
            "Fixture emits a runtime tag guard before specialization.",
        ),
        Entry(
            SiteKey(repo_rel(fixture_path), "collector", "raw:local_types"),
            1,
            "representation-proven",
            "Fixture collector scans every write before selecting a rep.",
        ),
    ]
    cases = 0

    def expect(label: str, errors: list[str], needle: str) -> None:
        nonlocal cases
        cases += 1
        if not any(needle in error for error in errors):
            raise AssertionError(f"{label}: expected {needle!r}, got {errors!r}")

    clean = audit(sites, raw, entries, [], minimum_sites=3)
    cases += 1
    if clean:
        raise AssertionError(f"clean fixture failed: {clean}")

    added_sites, added_raw = scan_text(
        fixture_path,
        fixture
        + "\nfn new_consumer(ctx: &FnCtx<'_>, id: &u32) {"
        + " let _ = ctx.stable_local_type_proof(id); }\n",
    )
    expect(
        "new consumer",
        audit(added_sites, added_raw, entries, [], minimum_sites=3),
        "unclassified local-type read",
    )

    expect(
        "removed consumer",
        audit(sites[:-1], raw, entries, [], minimum_sites=2),
        "stale allowlist entry",
    )

    expect(
        "count drift",
        audit([*sites, sites[0]], raw, entries, [], minimum_sites=3),
        "count drift",
    )

    bypass = "fn bad(ctx: &FnCtx<'_>, id: &u32) { let _ = ctx.local_types.get(id); }"
    bypass_sites, bypass_raw = scan_text(fixture_path, bypass)
    expect(
        "raw bypass",
        audit(bypass_sites, bypass_raw, [], [], minimum_sites=0),
        "direct ctx.local_types read bypasses the proof API",
    )

    proof_bypass = (
        "fn bad(ctx: &FnCtx<'_>, id: &u32) { "
        "let _ = ctx.proven_local_types.get(id); }"
    )
    proof_bypass_sites, proof_bypass_raw = scan_text(fixture_path, proof_bypass)
    expect(
        "proof bypass",
        audit(proof_bypass_sites, proof_bypass_raw, [], [], minimum_sites=0),
        "direct ctx.proven_local_types read bypasses the proof API",
    )

    for label, bypass in [
        (
            "contains-key bypass",
            "fn bad(ctx: &FnCtx<'_>, id: &u32) { "
            "let _ = ctx.local_types.contains_key(id); }",
        ),
        (
            "index bypass",
            "fn bad(ctx: &FnCtx<'_>, id: &u32) { let _ = ctx.local_types[id]; }",
        ),
    ]:
        bypass_sites, bypass_raw = scan_text(fixture_path, bypass)
        expect(
            label,
            audit(bypass_sites, bypass_raw, [], [], minimum_sites=0),
            "direct ctx.local_types read bypasses the proof API",
        )

    expect(
        "candidate floor",
        audit([], [], [], [], minimum_sites=1),
        "candidate floor failed",
    )

    print(f"local_binding_type_audit self-test: OK ({cases} cases)")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--list", action="store_true", help="print the classified inventory")
    parser.add_argument("--allowlist", type=Path, default=DEFAULT_ALLOWLIST)
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    sites, raw_errors = scan_repo()
    entries, allowlist_errors = load_allowlist(args.allowlist)
    errors = audit(sites, raw_errors, entries, allowlist_errors)
    if args.list:
        print_inventory(sites, entries)
    if errors:
        for error in errors:
            print(f"local-binding-type audit: {error}", file=sys.stderr)
        return 1
    classes = Counter(entry.classification for entry in entries)
    print(
        "local-binding-type audit: OK "
        f"({len(sites)} sites, {len(entries)} groups; "
        + ", ".join(f"{name}={count}" for name, count in sorted(classes.items()))
        + ")"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
