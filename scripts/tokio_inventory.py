#!/usr/bin/env python3
"""The tokio gate: tokio is GONE from Perry's workspace, and this keeps it gone.

Perry migrated off tokio onto turnloop (`docs/turnloop/`). From turnloop P8 to
the final tokio lane this script was a *ratchet*: every remaining manifest edge
to a tokio-family crate, and every tokio-family package in `Cargo.lock`, was
recorded in `scripts/tokio_inventory.json` with the JS surface that reached it
and what blocked its removal, compared strictly in both directions. The final
lane deleted the last edge (perry-stdlib's `async-runtime` feature) and with it
every tokio package in the lock, so the ratchet reached zero.

At zero it is no longer a ratchet — it is a ban, and the baseline can no longer
grant anything. Owner rule: turnloop for everything, including tests; no tokio
dependency anywhere (normal, dev or build), and no `#[tokio::test]`.

WHAT FAILS
----------

Each of these, absolutely (there is no allowlist to add an entry to):

  * a **manifest edge** from any workspace crate to a crate in `TOKIO` — normal,
    dev or build, optional or not, under any target cfg. Read from
    `cargo metadata --no-deps`, so a target-gated edge that `cargo tree` on the
    host would hide still counts;
  * a package in `TOKIO` appearing in **`Cargo.lock`** (whatever pulled it in,
    including a third-party crate turning a `tokio` feature on);
  * a **source line** (not a comment) in `crates/*/src`, `crates/*/tests`,
    `crates/*/benches` or `crates/*/examples` that names a tokio path
    (`tokio::`, `tokio_rustls::`, …) or a `#[tokio::main]` / `#[tokio::test]`
    attribute;
  * a non-empty `edges` or `lockfile` in `scripts/tokio_inventory.json` — the
    fields are kept (empty) so an old tree's reader finds them, but an entry
    there would be a permission this gate no longer honours;
  * a `not_tokio` entry (below) whose package's `Cargo.lock` dependency list
    names a `TOKIO` package, or a `not_tokio` package that has left the lock
    (a stale note fails, so the file cannot describe a tree that is gone).

NOT TOKIO: `tungstenite`, `lettre`
----------------------------------

The old inventory tracked the whole "tokio family" by name, which included two
crates that are not tokio at all. They are listed under `not_tokio` in the JSON
with the reason, and the gate CHECKS that reason against the lock rather than
trusting it:

  * `tungstenite` 0.30 — synchronous WebSocket protocol. perry-ui-android uses
    it over a blocking `std::net::TcpStream` on one std thread per connection,
    and turnloop-websocket uses it as a codec. Its lock entry has no tokio
    dependency.
  * `lettre` — perry-ext-nodemailer uses its `message` builder only; the
    `tokio1` / `tokio1-rustls-tls` / `pool` transport features are off.

USAGE
-----

    python3 scripts/tokio_inventory.py            # the gate
    python3 scripts/tokio_inventory.py --list     # what was checked, and the not-tokio notes
    python3 scripts/tokio_inventory.py --table    # the not-tokio notes as markdown
    python3 scripts/tokio_inventory.py --update   # re-record not_tokio versions (refuses if tokio is back)
    python3 scripts/tokio_inventory.py --self-test
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

# Crates that ARE tokio, or exist only to run on it. An edge to, or a lock entry
# for, any of these is tokio in the graph. Membership is by NAME so it is stable
# under feature unification and target cfgs.
TOKIO = (
    "tokio",
    "tokio-macros",
    "tokio-util",
    "tokio-stream",
    "tokio-rustls",
    "tokio-native-tls",
    "tokio-tungstenite",
    "tokio-io-timeout",
    "hyper-util",
    "hyper-rustls",
    "hyper-tls",
    "h2",
    "reqwest",
    "tower",
    "tower-http",
    "sqlx",
    "sqlx-core",
    "sqlx-mysql",
    "sqlx-postgres",
    "redis",
    "mongodb",
    "async-compat",
)

# A code line naming a tokio path or attribute. Comment lines are skipped by the
# caller: history in a doc comment ("the tokio path used to …") is not a use.
SOURCE_RE = re.compile(
    r"\b(?:tokio|tokio_rustls|tokio_tungstenite|tokio_util|tokio_stream|tokio_macros)::"
)

SOURCE_DIRS = ("src", "tests", "benches", "examples")

BASELINE = Path("scripts/tokio_inventory.json")


# ---------------------------------------------------------------------------
# Fact extraction. Pure given its input, so --self-test needs no cargo.
# ---------------------------------------------------------------------------


def edges_from_metadata(metadata: dict) -> list[dict]:
    """Every (workspace crate -> TOKIO crate) manifest edge, any kind/target."""
    edges = []
    for pkg in metadata.get("packages", []):
        for dep in pkg.get("dependencies", []):
            if dep["name"] not in TOKIO:
                continue
            edges.append(
                {
                    "crate": pkg["name"],
                    "dep": dep["name"],
                    "kind": dep.get("kind") or "normal",
                    "optional": bool(dep.get("optional", False)),
                    "target": dep.get("target"),
                }
            )
    return sorted(edges, key=lambda e: (e["crate"], e["dep"], e["kind"], e["target"] or ""))


def lock_entries(lock_text: str) -> dict[str, list[tuple[str, list[str]]]]:
    """Every `[[package]]` in `Cargo.lock` as name -> [(version, dependency names)].

    Parsed lexically rather than with a TOML reader so the gate still runs on a
    lockfile cargo would refuse — the state a half-finished edit leaves behind.
    """
    out: dict[str, list[tuple[str, list[str]]]] = {}
    for block in lock_text.split("[[package]]")[1:]:
        name = version = None
        deps: list[str] = []
        in_deps = False
        for line in block.splitlines():
            if line.startswith('name = "'):
                name = line[8:].rstrip('"')
            elif line.startswith('version = "'):
                version = line[11:].rstrip('"')
            elif line.startswith("dependencies = ["):
                in_deps = True
            elif in_deps:
                if line.startswith("]"):
                    in_deps = False
                else:
                    item = line.strip().strip(",").strip('"')
                    if item:
                        deps.append(item.split(" ")[0])
        if name and version:
            out.setdefault(name, []).append((version, deps))
    return out


def tokio_in_lock(entries: dict) -> dict[str, list[str]]:
    return {
        name: sorted(v for v, _ in entries[name]) for name in sorted(entries) if name in TOKIO
    }


def source_hits(root: Path) -> list[str]:
    hits = []
    crates_dir = root / "crates"
    if not crates_dir.is_dir():
        return hits
    for crate in sorted(p for p in crates_dir.iterdir() if p.is_dir()):
        for sub in SOURCE_DIRS:
            base = crate / sub
            if not base.is_dir():
                continue
            for path in sorted(base.rglob("*.rs")):
                try:
                    text = path.read_text(encoding="utf-8", errors="replace")
                except OSError:
                    continue
                for n, line in enumerate(text.splitlines(), 1):
                    stripped = line.lstrip()
                    if stripped.startswith("//") or stripped.startswith("*"):
                        continue
                    if SOURCE_RE.search(line):
                        hits.append(f"{path.relative_to(root)}:{n}: {stripped[:120]}")
    return hits


# ---------------------------------------------------------------------------
# The check
# ---------------------------------------------------------------------------


def check(baseline: dict, edges: list[dict], entries: dict, hits: list[str]) -> list[str]:
    problems: list[str] = []
    for e in edges:
        problems.append(
            f"tokio edge: {e['crate']} -> {e['dep']} (kind={e['kind']}, "
            f"optional={e['optional']}, target={e['target']}). tokio was removed "
            "from the workspace; use turnloop (perry_ffi::turnloop_net, the pool, "
            "turnloop-http/-tls/-websocket). There is no allowlist."
        )
    for name, versions in tokio_in_lock(entries).items():
        problems.append(
            f"tokio package in Cargo.lock: {name} {', '.join(versions)}. Find what pulls "
            f"it in with `cargo tree --workspace --all-features --target all -i {name}`."
        )
    for hit in hits:
        problems.append(f"tokio in source: {hit}")
    if baseline.get("edges") or baseline.get("lockfile"):
        problems.append(
            f"{BASELINE} records tokio edges or lock packages; both must stay empty — "
            "an entry there would be a permission this gate no longer grants."
        )
    for note in baseline.get("not_tokio", []):
        name = note.get("name")
        found = entries.get(name)
        if not found:
            problems.append(
                f"STALE not_tokio note: {name} is no longer in Cargo.lock; delete its entry."
            )
            continue
        versions = sorted(v for v, _ in found)
        if versions != sorted(note.get("versions", [])):
            problems.append(
                f"not_tokio {name}: Cargo.lock has {versions}, the note says "
                f"{note.get('versions')}. Re-check the reason, then --update."
            )
        for version, deps in found:
            bad = sorted(d for d in deps if d in TOKIO)
            if bad:
                problems.append(
                    f"not_tokio {name} {version} now depends on {', '.join(bad)} in "
                    "Cargo.lock — the note's reason no longer holds."
                )
    return problems


# ---------------------------------------------------------------------------
# Self-test: the gate must be able to fail
# ---------------------------------------------------------------------------


def self_test() -> int:
    failures: list[str] = []

    metadata = {
        "packages": [
            {"name": "crate-a", "dependencies": [
                {"name": "serde", "kind": None, "optional": False, "target": None}]},
            {"name": "crate-b", "dependencies": [
                {"name": "tokio", "kind": "dev", "optional": False,
                 "target": 'cfg(target_os = "linux")'}]},
            {"name": "crate-c", "dependencies": [
                {"name": "tungstenite", "kind": None, "optional": False, "target": None}]},
        ]
    }
    edges = edges_from_metadata(metadata)
    if [e["dep"] for e in edges] != ["tokio"]:
        failures.append(f"expected exactly the planted tokio edge, got {edges}")
    elif edges[0]["kind"] != "dev" or not edges[0]["target"]:
        failures.append("a target-gated dev-dependency edge lost its kind or target")

    clean_lock = (
        '[[package]]\nname = "serde"\nversion = "1.0.0"\n\n'
        '[[package]]\nname = "tungstenite"\nversion = "0.30.0"\n'
        'dependencies = [\n "bytes",\n "http",\n]\n'
    )
    entries = lock_entries(clean_lock)
    baseline = {"edges": [], "lockfile": {},
                "not_tokio": [{"name": "tungstenite", "versions": ["0.30.0"]}]}
    if check(baseline, [], entries, []):
        failures.append("a clean tree must pass, and did not")

    # 1. Any tokio edge fails, dev/target-gated included.
    if not any("tokio edge" in p for p in check(baseline, edges, entries, [])):
        failures.append("a tokio dev-dependency edge did not fail the gate")
    # 2. tokio in the lock fails, even with no workspace edge.
    dirty = lock_entries(clean_lock + '\n[[package]]\nname = "tokio"\nversion = "1.53.1"\n')
    if not any("tokio package in Cargo.lock" in p for p in check(baseline, [], dirty, [])):
        failures.append("tokio in Cargo.lock did not fail the gate")
    # 3. A not-tokio crate that grows a tokio dependency fails.
    grown = lock_entries(clean_lock.replace(' "http",\n', ' "http",\n "tokio",\n'))
    if not any("reason no longer holds" in p for p in check(baseline, [], grown, [])):
        failures.append("a not_tokio package depending on tokio did not fail the gate")
    # 4. A stale not-tokio note fails.
    stale = dict(baseline, not_tokio=[{"name": "lettre", "versions": ["0.11.23"]}])
    if not any("STALE" in p for p in check(stale, [], entries, [])):
        failures.append("a stale not_tokio note did not fail the gate")
    # 5. A baseline trying to grant an edge fails.
    granted = dict(baseline, edges=[{"crate": "x", "dep": "tokio"}])
    if not any("must stay empty" in p for p in check(granted, [], entries, [])):
        failures.append("a non-empty baseline did not fail the gate")
    # 6. A tokio source line fails; a comment naming tokio does not.
    if not any("tokio in source" in p for p in check(baseline, [], entries, ["x.rs:1: tokio::spawn"])):
        failures.append("a tokio source hit did not fail the gate")
    if not SOURCE_RE.search("#[tokio::test]") or SOURCE_RE.search("let tokio_free = 1;"):
        failures.append("the source pattern misclassifies an attribute or an identifier")
    # 7. A not-tokio version change must be re-checked.
    bumped = lock_entries(clean_lock.replace("0.30.0", "0.31.0"))
    if not any("Re-check the reason" in p for p in check(baseline, [], bumped, [])):
        failures.append("a not_tokio version change did not have to be re-recorded")

    if failures:
        print("tokio_inventory self-test FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print("tokio_inventory self-test: OK (8 planted changes, all caught)")
    return 0


# ---------------------------------------------------------------------------


def load_metadata(root: Path) -> dict:
    out = subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=root,
        text=True,
        encoding="utf-8",
    )
    return json.loads(out)


def render_table(baseline: dict) -> str:
    out = ["| crate | versions | why it is not tokio |", "| --- | --- | --- |"]
    for note in baseline.get("not_tokio", []):
        out.append(
            f"| `{note['name']}` | {', '.join(note.get('versions', []))} | {note.get('why', '?')} |"
        )
    return "\n".join(out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true", help="check the checker")
    parser.add_argument("--list", action="store_true", help="print what was checked")
    parser.add_argument("--table", action="store_true", help="the not-tokio notes as markdown")
    parser.add_argument("--update", action="store_true",
                        help="re-record not_tokio versions (refuses while tokio is present)")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    root = Path(
        subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
    )
    baseline_path = root / BASELINE
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))

    if args.table:
        print(render_table(baseline))
        return 0

    edges = edges_from_metadata(load_metadata(root))
    entries = lock_entries((root / "Cargo.lock").read_text(encoding="utf-8"))
    hits = source_hits(root)

    if args.update:
        if edges or tokio_in_lock(entries) or hits:
            print("refusing --update: tokio is in the tree; the gate below says where.",
                  file=sys.stderr)
        else:
            for note in baseline.get("not_tokio", []):
                note["versions"] = sorted(v for v, _ in entries.get(note["name"], []))
            baseline["edges"] = []
            baseline["lockfile"] = {}
            baseline_path.write_text(json.dumps(baseline, indent=2) + "\n", encoding="utf-8")
            print(f"re-recorded not_tokio versions in {BASELINE}")
            return 0

    if args.list:
        print(f"manifest edges to a tokio crate: {len(edges)}")
        print(f"tokio packages in Cargo.lock:    {len(tokio_in_lock(entries))}")
        print(f"tokio source lines:              {len(hits)}")
        print(f"banned names ({len(TOKIO)}): {', '.join(TOKIO)}")
        print("not tokio (checked against Cargo.lock):")
        for note in baseline.get("not_tokio", []):
            print(f"  {note['name']:<14} {', '.join(sorted(v for v, _ in entries.get(note['name'], [])))}")

    problems = check(baseline, edges, entries, hits)
    if problems:
        print("tokio gate FAILED — tokio was removed from the workspace:", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        return 1
    if not args.list:
        print(
            f"tokio gate: 0 manifest edges, 0 tokio packages in Cargo.lock, 0 source lines; "
            f"{len(baseline.get('not_tokio', []))} not-tokio notes verified."
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
