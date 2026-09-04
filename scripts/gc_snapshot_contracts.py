"""Source-pinned contracts for deliberately untraced, non-moving GC snapshots.

These pins require a renewed review when the holder, its boundaries, or its
collector driver changes. They do not infer GC safety from function names or
prove arbitrary callees safe. The inventory must explain the reviewed window.
"""

from __future__ import annotations

import hashlib
import re
import tempfile
from pathlib import Path, PurePosixPath

from check_gc_scanner_latches import mask_non_code


def source_digest(path: Path) -> str:
    # A Windows checkout's CRLF conversion must not invalidate a reviewed pin.
    return hashlib.sha256(path.read_text(encoding="utf-8").encode("utf-8")).hexdigest()


def snapshot_contract_problems(entry: dict, root: Path | None) -> list[str]:
    label = f"{entry.get('file', '?')}:{entry.get('name', '?')}"
    problems: list[str] = []

    def fail(message: str) -> None:
        problems.append(f"{label}: non_moving_snapshot {message}")

    name = entry.get("name")
    if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z_]\w*", name):
        fail("requires a holder name")
        return problems
    window = entry.get("window")
    if not isinstance(window, dict):
        fail("requires a window with start, end, owner and source pins")
        return problems
    sources = window.get("sources")
    if not isinstance(sources, dict) or not sources:
        fail("requires nonempty window.sources SHA-256 pins")
        return problems
    if root is None:
        fail("requires a source root to verify its window")
        return problems

    pinned: dict[str, str] = {}
    for rel, digest in sources.items():
        if not isinstance(rel, str):
            fail("source paths must be repository-relative Rust files")
            continue
        path = PurePosixPath(rel)
        if (path.is_absolute() or ".." in path.parts or "\\" in rel
                or not rel.startswith("crates/") or path.suffix != ".rs"):
            fail(f"invalid source path {rel!r}")
            continue
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            fail(f"invalid SHA-256 pin for {rel}")
            continue
        target = root / rel
        if not target.is_file() or not target.resolve().is_relative_to(root.resolve()):
            fail(f"source missing or outside repository: {rel}")
            continue
        pinned[rel] = mask_non_code(target.read_text(encoding="utf-8"))
        if source_digest(target) != digest:
            fail(f"source changed: {rel}; re-audit the window before updating its pin")

    holder_file = entry.get("file")
    if not isinstance(holder_file, str) or holder_file not in pinned:
        fail("must pin the holder's declaration and accesses")
    symbols = {name}
    boundaries = []
    for role in ("start", "end", "owner"):
        boundary = window.get(role)
        if not isinstance(boundary, dict):
            fail(f"requires a {role} boundary with file and function")
            continue
        rel, name = boundary.get("file"), boundary.get("function")
        if not isinstance(rel, str) or rel not in pinned:
            fail(f"{role} boundary must be in a pinned source file")
            continue
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z_]\w*", name):
            fail(f"{role} function must be a Rust identifier")
            continue
        if not re.search(r"\bfn\s+" + re.escape(name) + r"\s*(?:<|\()", pinned[rel]):
            fail(f"{role} function {name} is missing from {rel}")
        if role != "owner":
            symbols.add(name)
            boundaries.append((rel, name))
    if len(boundaries) == 2 and boundaries[0] == boundaries[1]:
        fail("start and end must identify distinct boundaries")

    # A new access/caller outside the reviewed files must not escape merely
    # because none of their hashes changed. Names in comments/strings do not
    # count. Include test code too: an extra caller still deserves review.
    symbols.discard("")
    if symbols:
        references = re.compile(r"\b(?:" + "|".join(map(re.escape, sorted(symbols))) + r")\b")
        for source in sorted((root / "crates").glob("*/src/**/*.rs")):
            rel = source.relative_to(root).as_posix()
            if rel in pinned:
                continue
            text = source.read_text(encoding="utf-8")
            if any(symbol in text for symbol in symbols) and references.search(mask_non_code(text)):
                fail(f"unreviewed holder access or boundary reference in {rel}")
    return problems


def snapshot_contract_self_test() -> list[str]:
    failures = []
    with tempfile.TemporaryDirectory(prefix="perry-snapshot-contract-") as temporary:
        root = Path(temporary)
        rel = "crates/perry-runtime/src/snapshot.rs"
        source = root / rel
        source.parent.mkdir(parents=True)
        original = """static SNAPSHOT: Cell<usize> = Cell::new(0);
fn begin() { SNAPSHOT.set(1); }
fn end() { SNAPSHOT.set(0); }
fn collect() { begin(); end(); }
"""
        source.write_text(original)
        entry = {
            "file": rel, "name": "SNAPSHOT", "verdict": "non_moving_snapshot",
            "window": {
                **{role: {"file": rel, "function": name}
                   for role, name in (("start", "begin"), ("end", "end"), ("owner", "collect"))},
                "sources": {rel: source_digest(source)},
            },
        }
        if snapshot_contract_problems(entry, root):
            failures.append("valid snapshot contract rejected")
        source.write_bytes(original.replace("\n", "\r\n").encode("utf-8"))
        if snapshot_contract_problems(entry, root):
            failures.append("CRLF checkout changed the snapshot source pin")
        source.write_text(original)
        for role in ("start", "end", "owner", "sources"):
            bad = {**entry, "window": {k: v for k, v in entry["window"].items() if k != role}}
            if not snapshot_contract_problems(bad, root):
                failures.append(f"snapshot contract accepted missing {role}")
        for bad in ({**entry, "window": None}, {**entry, "window": {}},
                    {**entry, "window": {**entry["window"], "sources": {"../escape.rs": "0" * 64}}},
                    {**entry, "window": {**entry["window"], "sources": {rel: "bad"}}},
                    {**entry, "window": {**entry["window"], "end": {"file": rel, "function": "missing"}}}):
            if not snapshot_contract_problems(bad, root):
                failures.append("malformed snapshot contract accepted")
        for replacement in (
            original.replace("end();", "resume_mutator(); end();"),
            original.replace("Cell::new(0)", "Cell::new(1)"),
            original.replace("SNAPSHOT.set(0);", ""),
        ):
            source.write_text(replacement)
            if not any("source changed" in p for p in snapshot_contract_problems(entry, root)):
                failures.append("changed snapshot lifetime/holder escaped source pin")
        source.write_text(original)
        caller = source.with_name("new_caller.rs")
        for access in ("begin();", "end();", "SNAPSHOT.set(2);"):
            caller.write_text(f"fn caller() {{ {access} }}")
            if not any("unreviewed" in p for p in snapshot_contract_problems(entry, root)):
                failures.append(f"new snapshot reference {access} escaped the closed reference set")
        caller.write_text('// begin();\nconst TEXT: &str = "SNAPSHOT end()";')
        if snapshot_contract_problems(entry, root):
            failures.append("comments/literals were treated as snapshot references")
    return failures
