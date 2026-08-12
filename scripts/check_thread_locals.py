#!/usr/bin/env python3
"""Keep `thread_local!` from silently reintroducing `_tlv_get_addr` cost (#7469).

WHY THIS EXISTS
===============

On Darwin every `thread_local!` access is an out-of-line call to
`_tlv_get_addr` in libdyld, and the runtime resolves thread-locals often enough
that the call was the single largest symbol in the worst-performing program in
the corpus (20.5% of `asyncpipe`). `crates/perry-runtime/src/tls_hot.rs` fixes
the *mechanism*; this script fixes the *policy*.

The mechanism has been fixed three times and regressed three times, because
coverage was an opt-in allowlist of sixteen hand-wired fields curated against
whichever workload was profiled last. `churn` — the profiled one — read 0%
forever. Every path the list did not anticipate paid full price, silently,
because forgetting to add a field produces a *working slow path* rather than a
build error.

`crate::perry_thread_local!` inverts that: a declaration written with it is on
the fast path with no wiring, no hand-written test and no list. This script is
what makes that the default rather than a suggestion — a new raw `thread_local!`
is a build error unless it is deliberately recorded here as cold.

THIS CHECK IS DESIGNED TO BE ABLE TO FAIL
=========================================

* A file gaining a raw `thread_local!` fails (the regression this exists for).
* A file *losing* one also fails: a stale allowlist entry is an entry nobody
  has to justify any more, and the `gc_root_dominance_allowlist.json` rule
  applies — an entry that matches nothing must be deleted, not left to rot.
* `--self-test` runs both directions against synthetic trees, so the checker
  itself cannot quietly stop being able to say no.

`#[cfg(test)]` DECLARATIONS ARE OUT OF SCOPE BY CONSTRUCTION
===========================================================

A `thread_local!` that only exists under `#[cfg(test)]` cannot cost
`_tlv_get_addr` in a shipping build, because it is not in one. Recording such a
declaration as "cold" would be recording the wrong fact — it is not cold, it is
*absent* — and it would spend the allowlist's credibility on entries no one can
ever act on. Three gated shapes are therefore not counted at all:

* `#[cfg(test)]` immediately above the block,
* the block nested inside a `#[cfg(test)] mod … { … }` in the same file,
* the block's whole FILE being a test module, i.e. some `lib.rs`/`mod.rs`
  declares it `#[cfg(test)] mod <stem>;`.

This is a *static* fact the scan can see, unlike hot-vs-cold. It is also the
direction that can go wrong quietly, so `--self-test` checks that removing the
gate makes the same declaration fail again.

WHAT IT DOES NOT DO
===================

It cannot tell a hot cold-listed declaration from a genuinely cold one. That is
what the runtime budget gate (`scripts/tls_budget_gate.sh`, measuring
`_tlv_get_addr`'s share of two programs whose paths are deliberately *not* in
the sixteen named slots) is for. This one is the fast, hermetic half.

USAGE
=====

    scripts/check_thread_locals.py            # verify (CI)
    scripts/check_thread_locals.py --update   # rewrite the allowlist
    scripts/check_thread_locals.py --self-test
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import tempfile
from pathlib import Path, PurePath, PureWindowsPath

REPO = Path(__file__).resolve().parent.parent
ALLOWLIST = REPO / "scripts" / "thread_local_cold_allowlist.json"
CRATES = ["crates/perry-runtime/src"]

# A raw declaration: `thread_local!` or `std::thread_local!` at the start of a
# statement. `crate::perry_thread_local! {` does not match, and neither does the
# one inside the macro's own expansion in tls_hot.rs (excluded by path below).
RAW_RE = re.compile(r"(?m)^[ \t]*(?:std::)?thread_local!\s*\{")
HOT_RE = re.compile(r"(?m)^[ \t]*(?:crate::)?perry_thread_local!\s*\{")
# One `static NAME: Ty = …;` inside a block.
DECL_RE = re.compile(
    r"(?m)^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?static\s+[A-Za-z_0-9]+\s*:"
)

# `tls_hot.rs` defines the mechanism: its own `thread_local!`s are the cache
# itself and the macro's expansion target, so they cannot be written with the
# macro without infinite regress.
EXCLUDED = {"crates/perry-runtime/src/tls_hot.rs"}


def repo_relative(path: PurePath, root: PurePath) -> str:
    """Return a stable repository-relative key on every host platform."""
    return path.relative_to(root).as_posix()

# `#[cfg(test)] mod <stem>;` — the whole file is a test module.
CFG_TEST_MOD_RE = re.compile(
    r"(?m)^[ \t]*#\[cfg\(test\)\]\s*\n[ \t]*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_0-9]+)\s*;"
)
# Any out-of-line `mod <stem>;`, gated or not — the edges of the module tree.
ANY_MOD_RE = re.compile(r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_0-9]+)\s*;")
# `#[cfg(test)] mod <name> {` — an inline test module, whose body is skipped.
CFG_TEST_INLINE_MOD_RE = re.compile(
    r"(?m)^[ \t]*#\[cfg\(test\)\]\s*\n[ \t]*(?:pub(?:\([^)]*\))?\s+)?mod\s+[A-Za-z_0-9]+\s*\{"
)


def block_bodies(src: str, pattern: re.Pattern[str]) -> list[str]:
    """Bodies of every macro block `pattern` starts, brace-matched."""
    bodies = []
    for m in pattern.finditer(src):
        i = src.index("{", m.start())
        depth = 0
        j = i
        while j < len(src):
            if src[j] == "{":
                depth += 1
            elif src[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        bodies.append(src[i + 1 : j])
    return bodies


def brace_span(src: str, start: int) -> tuple[int, int]:
    """`(open, close)` offsets of the brace-matched block opening at/after `start`."""
    i = src.index("{", start)
    depth = 0
    j = i
    while j < len(src):
        if src[j] == "{":
            depth += 1
        elif src[j] == "}":
            depth -= 1
            if depth == 0:
                break
        j += 1
    return i, j


def cfg_test_module_files(root: Path, crates: list[str]) -> set[str]:
    """Files whose entire module cannot exist in a shipping build.

    Seeded from every `#[cfg(test)] mod <stem>;`, then closed transitively: a
    plain `mod y;` inside an already-test-only file is test-only too, which is
    what makes `gc/tests/mod.rs` carry its whole subtree. A `mod x;` in `a/b.rs`
    (or `a/b/mod.rs`) resolves to `a/b/x.rs` or `a/b/x/mod.rs`; both spellings
    are recorded, and a miss is simply a file that stays in scope.
    """
    declares: dict[str, list[tuple[str, bool]]] = {}
    for crate in crates:
        base = root / crate
        for dirpath, _dirs, files in os.walk(base):
            for name in sorted(files):
                if not name.endswith(".rs"):
                    continue
                path = Path(dirpath) / name
                rel = repo_relative(path, root)
                src = path.read_text()
                parent = path.parent if name in ("lib.rs", "mod.rs") else path.with_suffix("")
                gated = set(CFG_TEST_MOD_RE.findall(src))
                edges = []
                for stem in set(ANY_MOD_RE.findall(src)):
                    for candidate in (parent / f"{stem}.rs", parent / stem / "mod.rs"):
                        if candidate.exists():
                            edges.append((repo_relative(candidate, root), stem in gated))
                declares[rel] = edges

    test_only = {child for edges in declares.values() for child, gated in edges if gated}
    changed = True
    while changed:
        changed = False
        for rel in test_only & declares.keys():
            for child, _gated in declares[rel]:
                if child not in test_only:
                    test_only.add(child)
                    changed = True
    return test_only


def shipping_raw_blocks(src: str) -> int:
    """Raw `thread_local!` blocks that survive into a non-test build.

    Skips a block carrying `#[cfg(test)]` directly above it, and any block
    inside an inline `#[cfg(test)] mod … { … }`.
    """
    gated_spans = [brace_span(src, m.start()) for m in CFG_TEST_INLINE_MOD_RE.finditer(src)]
    count = 0
    for m in RAW_RE.finditer(src):
        if any(open_at < m.start() < close_at for open_at, close_at in gated_spans):
            continue
        preceding = src[: m.start()].rstrip()
        line = preceding[preceding.rfind("\n") + 1 :].strip()
        if line == "#[cfg(test)]":
            continue
        count += 1
    return count


def scan(root: Path, crates: list[str]) -> tuple[dict[str, int], int]:
    """Raw `thread_local!` blocks per file, and total hot declarations."""
    raw: dict[str, int] = {}
    hot_declarations = 0
    test_only_files = cfg_test_module_files(root, crates)
    for crate in crates:
        base = root / crate
        for dirpath, _dirs, files in os.walk(base):
            for name in sorted(files):
                if not name.endswith(".rs"):
                    continue
                path = Path(dirpath) / name
                rel = repo_relative(path, root)
                src = path.read_text()
                hot_declarations += sum(
                    len(DECL_RE.findall(body)) for body in block_bodies(src, HOT_RE)
                )
                if rel in EXCLUDED or rel in test_only_files:
                    continue
                count = shipping_raw_blocks(src)
                if count:
                    raw[rel] = count
    return raw, hot_declarations


def hot_slot_capacity(root: Path) -> int:
    src = (root / "crates/perry-runtime/src/tls_hot.rs").read_text()
    m = re.search(r"pub const HOT_SLOT_CAPACITY: usize = (\d+);", src)
    if not m:
        raise SystemExit("could not read HOT_SLOT_CAPACITY from tls_hot.rs")
    return int(m.group(1))


def verify(root: Path, crates: list[str], allowlist_path: Path) -> list[str]:
    raw, hot_declarations = scan(root, crates)
    recorded = json.loads(allowlist_path.read_text())["files"]
    problems = []

    for rel, count in sorted(raw.items()):
        if rel not in recorded:
            problems.append(
                f"{rel}: {count} raw `thread_local!` block(s), none allowed.\n"
                f"    Use `crate::perry_thread_local!` — same syntax, same "
                f"`.with()` at every call site, and the address lands in this "
                f"thread's hot cache instead of costing a `_tlv_get_addr` call "
                f"(#7469). If the declaration really is cold, record it:\n"
                f"      scripts/check_thread_locals.py --update"
            )
        elif recorded[rel] != count:
            direction = "gained" if count > recorded[rel] else "lost"
            problems.append(
                f"{rel}: {direction} raw `thread_local!` blocks "
                f"({recorded[rel]} recorded, {count} found). "
                f"Convert it, or run --update to re-record."
            )

    for rel in sorted(recorded):
        if rel not in raw:
            problems.append(
                f"{rel}: recorded as having {recorded[rel]} cold "
                f"`thread_local!` block(s), but has none. A stale entry is one "
                f"nobody has to justify — delete it with --update."
            )

    capacity = hot_slot_capacity(root)
    if hot_declarations >= capacity:
        problems.append(
            f"{hot_declarations} `perry_thread_local!` declarations vs "
            f"HOT_SLOT_CAPACITY {capacity}. Slot exhaustion is *correct* but "
            f"silent — the declarations past the ceiling fall back to "
            f"`_tlv_get_addr` forever. Raise HOT_SLOT_CAPACITY."
        )
    return problems


def write_allowlist(root: Path, crates: list[str], allowlist_path: Path) -> None:
    raw, hot_declarations = scan(root, crates)
    allowlist_path.write_text(
        json.dumps(
            {
                "_comment": (
                    "Files still declaring raw `thread_local!`. Every entry is a "
                    "declaration that pays `_tlv_get_addr` on Darwin; the count is "
                    "a ratchet, so adding one to an already-listed file fails too. "
                    "New code should use `crate::perry_thread_local!` — see "
                    "crates/perry-runtime/src/tls_hot.rs. Regenerate with "
                    "scripts/check_thread_locals.py --update."
                ),
                "_hot_declarations": hot_declarations,
                "files": dict(sorted(raw.items())),
            },
            indent=2,
        )
        + "\n"
    )


def self_test() -> int:
    """Prove the checker can say no, in both directions."""
    failures = []
    windows_root = PureWindowsPath(r"C:\perry")
    if repo_relative(windows_root / "crates" / "runtime.rs", windows_root) != "crates/runtime.rs":
        failures.append("repository-relative keys are not normalized on Windows")
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        src_dir = root / "crates/perry-runtime/src"
        src_dir.mkdir(parents=True)
        (src_dir / "tls_hot.rs").write_text(
            "pub const HOT_SLOT_CAPACITY: usize = 768;\n"
            "thread_local! { static HOT: u8 = const { 0 }; }\n"
        )
        (src_dir / "cold.rs").write_text("thread_local! { static A: u8 = const { 0 }; }\n")
        (src_dir / "hot.rs").write_text(
            "crate::perry_thread_local! { static B: u8 = const { 0 }; }\n"
        )
        allowlist = root / "allow.json"

        write_allowlist(root, CRATES, allowlist)
        recorded = json.loads(allowlist.read_text())
        if "crates/perry-runtime/src/tls_hot.rs" in recorded["files"]:
            failures.append("tls_hot.rs must be excluded from the allowlist")
        if recorded["_hot_declarations"] != 1:
            failures.append(
                f"expected 1 hot declaration, counted {recorded['_hot_declarations']}"
            )
        if verify(root, CRATES, allowlist):
            failures.append("a freshly written allowlist must verify clean")

        # 1. A new raw declaration in an unlisted file must fail.
        (src_dir / "new.rs").write_text("thread_local! { static C: u8 = const { 0 }; }\n")
        if not verify(root, CRATES, allowlist):
            failures.append("a new raw `thread_local!` in an unlisted file passed")
        (src_dir / "new.rs").unlink()

        # 2. A second raw declaration in an already-listed file must fail.
        (src_dir / "cold.rs").write_text(
            "thread_local! { static A: u8 = const { 0 }; }\n"
            "thread_local! { static D: u8 = const { 0 }; }\n"
        )
        if not verify(root, CRATES, allowlist):
            failures.append("a raw `thread_local!` added to a listed file passed")

        # 3. A stale entry must fail.
        (src_dir / "cold.rs").write_text(
            "crate::perry_thread_local! { static A: u8 = const { 0 }; }\n"
        )
        if not verify(root, CRATES, allowlist):
            failures.append("a stale allowlist entry passed")

        # 4. Blowing the slot ceiling must fail.
        write_allowlist(root, CRATES, allowlist)
        (src_dir / "hot.rs").write_text(
            "crate::perry_thread_local! {\n"
            + "".join(f"    static H{i}: u8 = const {{ 0 }};\n" for i in range(800))
            + "}\n"
        )
        if not any("HOT_SLOT_CAPACITY" in p for p in verify(root, CRATES, allowlist)):
            failures.append("exceeding HOT_SLOT_CAPACITY passed")
        (src_dir / "hot.rs").write_text(
            "crate::perry_thread_local! { static B: u8 = const { 0 }; }\n"
        )

        # 5. The three `#[cfg(test)]` shapes are out of scope — and, the half
        #    that can go wrong quietly, REMOVING the gate puts them back in it.
        (src_dir / "cold.rs").write_text("thread_local! { static A: u8 = const { 0 }; }\n")
        write_allowlist(root, CRATES, allowlist)
        gated = {
            "attribute": "#[cfg(test)]\nthread_local! { static G: u8 = const { 0 }; }\n",
            "inline mod": (
                "#[cfg(test)]\nmod t {\n"
                "    thread_local! { static G: u8 = const { 0 }; }\n"
                "}\n"
            ),
        }
        for shape, body in gated.items():
            (src_dir / "gated.rs").write_text(body)
            if verify(root, CRATES, allowlist):
                failures.append(f"a `#[cfg(test)]` {shape} declaration was counted")
            (src_dir / "gated.rs").write_text(body.replace("#[cfg(test)]\n", ""))
            if not verify(root, CRATES, allowlist):
                failures.append(f"an UNGATED {shape} declaration passed")
        (src_dir / "gated.rs").unlink()

        # 6. A whole file declared `#[cfg(test)] mod <stem>;` is out of scope,
        #    and drops back in when the parent stops gating it.
        (src_dir / "probes.rs").write_text(
            "thread_local! { static G: u8 = const { 0 }; }\n"
        )
        (src_dir / "lib.rs").write_text("#[cfg(test)]\nmod probes;\n")
        if verify(root, CRATES, allowlist):
            failures.append("a `#[cfg(test)] mod <stem>;` file was counted")
        (src_dir / "lib.rs").write_text("mod probes;\n")
        if not verify(root, CRATES, allowlist):
            failures.append("an ungated `mod <stem>;` file passed")

    for f in failures:
        print(f"SELF-TEST FAILED: {f}", file=sys.stderr)
    if failures:
        return 1
    print("self-test: the checker can fail in all six directions")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--update", action="store_true", help="rewrite the allowlist")
    ap.add_argument("--self-test", action="store_true", help="prove the checker can fail")
    args = ap.parse_args()

    if args.self_test:
        return self_test()
    if args.update:
        write_allowlist(REPO, CRATES, ALLOWLIST)
        print(f"wrote {ALLOWLIST.relative_to(REPO).as_posix()}")
        return 0

    problems = verify(REPO, CRATES, ALLOWLIST)
    if problems:
        print("thread-local policy check FAILED:\n", file=sys.stderr)
        for p in problems:
            print(f"  {p}\n", file=sys.stderr)
        return 1
    raw, hot_declarations = scan(REPO, CRATES)
    print(
        f"thread-local policy OK: {hot_declarations} hot declarations, "
        f"{sum(raw.values())} raw blocks in {len(raw)} recorded cold files, "
        f"capacity {hot_slot_capacity(REPO)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
