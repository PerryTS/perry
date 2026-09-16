#!/usr/bin/env python3
"""The tokio dependency inventory, as a gate instead of as prose (turnloop P8).

Perry is migrating off tokio onto turnloop (`docs/turnloop/`). Eight lanes have
now landed, and each one ended with a prose list of the paths it did *not*
move. Those lists are the only record of what is left — and by the time P8 read
them they had already gone stale in both directions: paths named as remaining
had been migrated by a later lane, and edges nobody named had appeared. A
migration whose remaining surface is measured by reading eight reports written
at eight different commits cannot be finished, because no one can say when it
is done.

This script is that measurement, re-derived from the tree every time it runs.

WHAT IS GATED (and what is not)
-------------------------------

**Gated, exactly:** the set of *manifest edges* from a workspace crate to a
tokio-family crate, and the set of tokio-family packages in `Cargo.lock`. Both
are exact, machine-derived facts, and both are the thing the migration's goal
is stated in terms of ("tokio no longer appears in Perry's dependency graph,
with the Cargo.lock to prove it").

The comparison is strict equality against `scripts/tokio_inventory.json`, in
BOTH directions, which is deliberate and matches the rule
`scripts/gc_root_dominance_allowlist.json` already follows:

  * a **new** edge fails — tokio cannot creep back in behind a green build;
  * a **stale** entry (in the baseline, gone from the tree) also fails — so a
    lane that removes an edge must delete its own entry, and the file can never
    describe a tree that no longer exists.

**Not gated:** the per-crate count of tokio-shaped *source sites*. It is
recorded and printed because it is the only number that says how much code
sits behind an edge, but a comment mentioning `tokio::spawn` moves it, so
gating it would produce failures that carry no information. Saying so here is
the point: an ungated number in a gate file is a number someone will
eventually trust, and this one must not be.

WHAT AN ENTRY CARRIES
---------------------

Each edge in the JSON carries the four things a reader of
`docs/turnloop/p8-report.md` needs and cannot get from `cargo tree`:

  * `surface`      — what JS reaches this, or "none" if no JS surface does.
  * `reached_when` — the condition under which a program actually takes it.
                     "linked but unreachable" is a different problem from
                     "every worker_threads agent hits it".
  * `blocker`      — what has to exist before it can move.
  * `issue`        — where that is tracked.
  * `plan`         — which group of the costed removal plan in
                     `docs/turnloop/p8-report.md` this edge belongs to. Every
                     edge is in exactly one group, and `--list` prints the
                     per-group totals, so the plan's arithmetic is checked
                     rather than asserted: a plan whose parts do not add up to
                     the whole is a plan that discovers a late item.

`--table` prints those as the markdown inventory table, so the report is
generated from the gate rather than transcribed beside it.

RELATIONSHIP TO `scripts/rust_dependency_inventory.py`
-----------------------------------------------------

That script is a whole-graph census with no baseline and no exit code: it
answers "what does Perry depend on". This one answers "what still depends on
tokio, why, and has that got worse" — one family, with a ratchet. They do not
overlap and neither replaces the other.

USAGE
-----

    python3 scripts/tokio_inventory.py            # --check (the gate)
    python3 scripts/tokio_inventory.py --list     # every edge + source sites
    python3 scripts/tokio_inventory.py --table    # the markdown table
    python3 scripts/tokio_inventory.py --update   # re-record, keeping annotations
    python3 scripts/tokio_inventory.py --self-test
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

# The tokio family: tokio itself, the crates that exist only to run on it, and
# the third-party clients/servers Perry reaches it through. An edge to any of
# these is an edge to tokio — `hyper` without tokio is not a configuration
# Perry has, and `sqlx` is resolved with `runtime-tokio`.
#
# Membership is deliberately by NAME rather than by walking the resolved graph:
# a name list is stable under feature unification, and the whole reason the
# `--workspace` view of `cargo tree -i tokio` overstates the problem is that
# unification pulls `tokio` into crates (perry-runtime through
# `timezone_provider -> combine`) that have no edge of their own.
FAMILY = (
    "tokio",
    "tokio-util",
    "tokio-stream",
    "tokio-rustls",
    "tokio-tungstenite",
    "tungstenite",
    "hyper",
    "hyper-util",
    "hyper-rustls",
    "h2",
    "reqwest",
    "lettre",
    "sqlx",
    "sqlx-core",
    "sqlx-mysql",
    "sqlx-postgres",
    "redis",
    "mongodb",
    "tower",
    "tower-http",
)

# Source shapes that mean "this file runs on tokio". Counted per crate and
# reported, never gated — see the module docstring.
SOURCE_SITE_RE = re.compile(
    r"\b(?:tokio|tokio_rustls|tokio_tungstenite|tokio_util|tokio_stream)::"
    r"|#\[tokio::(?:main|test)\]"
    r"|\bHandle::current\b"
    r"|\bRuntime::new\b"
    r"|\bspawn_blocking\b"
    r"|\bblock_on\b"
    r"|\bnew_current_thread\b"
    r"|\bnew_multi_thread\b"
)

BASELINE = Path("scripts/tokio_inventory.json")

DEFAULT_README = [
    "The turnloop migration's remaining tokio surface (docs/turnloop/p8-report.md).",
    "",
    "`edges` and `lockfile` are GATED by scripts/tokio_inventory.py, strictly and in",
    "both directions: a NEW edge fails, and a STALE entry fails too — so a lane that",
    "removes an edge must delete its own line, and this file can never describe a tree",
    "that is gone. Regenerate with `python3 scripts/tokio_inventory.py --update`, which",
    "preserves every surviving entry's annotations.",
    "",
    "Per edge, the four hand-written fields are the ones `cargo tree` cannot give:",
    "  surface       what JS reaches it, or 'none' if no JS surface does",
    "  reached_when  the condition under which a program actually takes it",
    "  blocker       what has to exist before it can move",
    "  issue         where that is tracked",
    "  plan          its group in the report's costed removal plan; every edge is in",
    "                exactly one group, and `--list` prints the totals",
    "",
    "`source_sites` is NOT gated: a comment naming tokio::spawn moves it, so a failure",
    "there would carry no information. It is recorded because it is the only number",
    "that says how much code sits behind an edge.",
]


# ---------------------------------------------------------------------------
# Fact extraction. Every function here is pure given its input, so --self-test
# can drive them with a synthetic tree and no cargo.
# ---------------------------------------------------------------------------


def edges_from_metadata(metadata: dict) -> list[dict]:
    """Every (workspace crate -> tokio-family crate) manifest edge.

    `cargo metadata --no-deps` reports each member's declared dependencies
    with their kind, optionality and target cfg, which is exactly the edge set
    a `Cargo.lock` entry is derived from — and, unlike `cargo tree`, it is not
    affected by which features happened to unify on the host that ran it. A
    target-gated edge (`perry-ui-gtk4`'s tokio, Linux only) is invisible to
    `cargo tree` on macOS and is reported here.
    """
    edges = []
    for pkg in metadata.get("packages", []):
        for dep in pkg.get("dependencies", []):
            if dep["name"] not in FAMILY:
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
    return sorted(edges, key=edge_key)


def edge_key(edge: dict) -> tuple:
    return (
        edge["crate"],
        edge["dep"],
        edge.get("kind") or "normal",
        bool(edge.get("optional", False)),
        edge.get("target") or "",
    )


def lock_packages(lock_text: str) -> dict[str, list[str]]:
    """Which tokio-family packages `Cargo.lock` holds, and at which versions.

    Parsed lexically rather than with a TOML reader so the gate still runs on a
    lockfile cargo would refuse, which is the state a half-finished dependency
    removal leaves behind.
    """
    found: dict[str, list[str]] = {}
    name = None
    for line in lock_text.splitlines():
        if line.startswith('name = "'):
            name = line[8:].rstrip('"')
        elif line.startswith('version = "') and name is not None:
            if name in FAMILY:
                found.setdefault(name, []).append(line[11:].rstrip('"'))
            name = None
    return {k: sorted(v) for k, v in sorted(found.items())}


def source_sites(root: Path, crates: list[str]) -> dict[str, int]:
    """Lines per crate that name a tokio idiom, comments excluded.

    Informational. Comment lines are dropped because `//! runs on tokio` in a
    module header is not a call site, and P8's own report is full of them.
    """
    counts: dict[str, int] = {}
    for crate in crates:
        src = root / "crates" / crate / "src"
        if not src.is_dir():
            continue
        total = 0
        for path in sorted(src.rglob("*.rs")):
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            for line in text.splitlines():
                stripped = line.lstrip()
                if stripped.startswith("//"):
                    continue
                if SOURCE_SITE_RE.search(line):
                    total += 1
        if total:
            counts[crate] = total
    return dict(sorted(counts.items()))


# ---------------------------------------------------------------------------
# Comparison
# ---------------------------------------------------------------------------


def compare(baseline: dict, edges: list[dict], lock: dict[str, list[str]]) -> list[str]:
    """Problems, one string each. Empty means the gate passes."""
    problems: list[str] = []

    recorded = {edge_key(e): e for e in baseline.get("edges", [])}
    current = {edge_key(e): e for e in edges}

    for key in sorted(current.keys() - recorded.keys()):
        e = current[key]
        problems.append(
            f"NEW tokio edge: {e['crate']} -> {e['dep']} "
            f"(kind={e['kind']}, optional={e['optional']}, target={e['target']}). "
            "Perry is migrating OFF tokio; adding an edge needs an entry in "
            f"{BASELINE} saying which JS surface reaches it and what blocks its removal."
        )

    for key in sorted(recorded.keys() - current.keys()):
        e = recorded[key]
        problems.append(
            f"STALE entry: {e['crate']} -> {e['dep']} is recorded in {BASELINE} "
            "but no longer exists in the tree. Delete the entry in the same commit "
            "that removed the edge — an inventory that describes a tree that is gone "
            "is how the lane reports went stale in the first place."
        )

    recorded_lock = baseline.get("lockfile", {})
    for name in sorted(set(recorded_lock) | set(lock)):
        was = recorded_lock.get(name)
        now = lock.get(name)
        if was == now:
            continue
        if was is None:
            problems.append(
                f"NEW tokio-family package in Cargo.lock: {name} {now}. "
                "Nothing may add one while the migration is open."
            )
        elif now is None:
            problems.append(
                f"GONE from Cargo.lock: {name} (was {was}) — this is the goal, so "
                f"record it: python3 {Path(__file__).name} --update"
            )
        else:
            problems.append(
                f"Cargo.lock version change: {name} {was} -> {now}. Re-record with --update."
            )

    return problems


# ---------------------------------------------------------------------------
# Rendering
# ---------------------------------------------------------------------------


def render_table(baseline: dict) -> str:
    """The inventory, as markdown.

    One block per edge rather than one row: the `blocker` column is a
    paragraph, and a table cell that wide is unreadable in every renderer.
    """
    out: list[str] = []
    crate = None
    for e in sorted(baseline.get("edges", []), key=edge_key):
        if e["crate"] != crate:
            crate = e["crate"]
            out.append("")
            out.append(f"### `{crate}`")
        qual = []
        if e.get("optional"):
            qual.append("optional")
        if (e.get("kind") or "normal") != "normal":
            qual.append(e["kind"])
        if e.get("target"):
            qual.append(e["target"])
        suffix = f" ({', '.join(qual)})" if qual else ""
        out.append("")
        out.append(f"**`{e['dep']}`{suffix}** — {e.get('surface', '?')}")
        out.append("")
        out.append(f"* *reached when:* {e.get('reached_when', '?')}")
        out.append(f"* *blocker:* {e.get('blocker', '?')}")
        out.append(f"* *tracked:* {e.get('issue', '?')}")
    return "\n".join(out).lstrip("\n")


def render_list(baseline: dict, edges: list[dict], lock: dict, sites: dict) -> str:
    out = []
    out.append(f"tokio-family packages in Cargo.lock: {len(lock)}")
    for name, versions in lock.items():
        out.append(f"  {name:<20} {', '.join(versions)}")
    out.append("")
    by_crate: dict[str, list[str]] = {}
    for e in edges:
        by_crate.setdefault(e["crate"], []).append(e["dep"])
    out.append(f"workspace crates with a tokio-family manifest edge: {len(by_crate)}")
    for crate, deps in sorted(by_crate.items()):
        n = sites.get(crate)
        suffix = f"   [{n} source sites]" if n else "   [no source sites — edge only]"
        out.append(f"  {crate:<26} {', '.join(sorted(set(deps)))}{suffix}")
    out.append("")
    orphan = {c: n for c, n in sites.items() if c not in by_crate}
    if orphan:
        out.append(
            "crates with tokio-shaped source but NO manifest edge "
            "(they call the perry-ffi C seam, or the hit is a false positive):"
        )
        for crate, n in sorted(orphan.items()):
            out.append(f"  {crate:<26} {n}")
        out.append("")
    recorded = {edge_key(e): e for e in baseline.get("edges", [])}
    unannotated = [e for e in edges if edge_key(e) in recorded
                   and not recorded[edge_key(e)].get("blocker")]
    if unannotated:
        out.append(f"edges with no recorded blocker: {len(unannotated)}")
    groups: dict[str, int] = {}
    for e in baseline.get("edges", []):
        groups[e.get("plan", "?")] = groups.get(e.get("plan", "?"), 0) + 1
    if groups:
        out.append("")
        out.append(
            "removal-plan groups (docs/turnloop/p8-report.md), "
            f"{sum(groups.values())} edges in {len(groups)} groups:"
        )
        for g, n in sorted(groups.items()):
            out.append(f"  {g:<4} {n}")
    return "\n".join(out)


# ---------------------------------------------------------------------------
# Self-test: the gate must be able to fail
# ---------------------------------------------------------------------------


def self_test() -> int:
    failures: list[str] = []

    metadata = {
        "packages": [
            {
                "name": "crate-a",
                "dependencies": [
                    {"name": "tokio", "kind": None, "optional": False, "target": None},
                    {"name": "serde", "kind": None, "optional": False, "target": None},
                ],
            },
            {
                "name": "crate-b",
                "dependencies": [
                    {"name": "hyper", "kind": None, "optional": True, "target": None},
                    {
                        "name": "tokio",
                        "kind": "dev",
                        "optional": False,
                        "target": 'cfg(target_os = "linux")',
                    },
                ],
            },
            {"name": "crate-c", "dependencies": [{"name": "anyhow", "kind": None}]},
        ]
    }

    edges = edges_from_metadata(metadata)
    if len(edges) != 3:
        failures.append(f"expected 3 family edges from the synthetic metadata, got {len(edges)}")
    if any(e["dep"] in ("serde", "anyhow") for e in edges):
        failures.append("a non-family dependency was reported as a tokio edge")
    if not any(e["target"] == 'cfg(target_os = "linux")' for e in edges):
        failures.append("a target-gated edge was dropped; that is the one cargo tree hides")
    if not any(e["kind"] == "dev" for e in edges):
        failures.append("a dev-dependency edge was dropped")

    lock = lock_packages(
        '[[package]]\nname = "tokio"\nversion = "1.53.1"\n\n'
        '[[package]]\nname = "serde"\nversion = "1.0.0"\n\n'
        '[[package]]\nname = "hyper"\nversion = "1.11.1"\n'
    )
    if lock != {"hyper": ["1.11.1"], "tokio": ["1.53.1"]}:
        failures.append(f"lockfile parse wrong: {lock}")

    baseline = {"edges": edges, "lockfile": lock}
    if compare(baseline, edges, lock):
        failures.append("a baseline that matches the tree must pass, and did not")

    # 1. A new edge must fail.
    grown = edges + [
        {"crate": "crate-c", "dep": "reqwest", "kind": "normal", "optional": False, "target": None}
    ]
    problems = compare(baseline, grown, lock)
    if not any("NEW tokio edge" in p for p in problems):
        failures.append("a NEW tokio edge did not fail the gate")

    # 2. A stale entry must fail — otherwise the file rots exactly the way the
    #    lane reports did.
    shrunk = [e for e in edges if e["dep"] != "hyper"]
    problems = compare(baseline, shrunk, lock)
    if not any("STALE entry" in p for p in problems):
        failures.append("a STALE baseline entry did not fail the gate")

    # 3. A lockfile change in either direction must be recorded.
    problems = compare(baseline, edges, {"tokio": ["1.53.1"]})
    if not any("GONE from Cargo.lock" in p for p in problems):
        failures.append("a package leaving Cargo.lock did not have to be recorded")
    problems = compare(baseline, edges, dict(lock, **{"h2": ["0.4.19"]}))
    if not any("NEW tokio-family package" in p for p in problems):
        failures.append("a package entering Cargo.lock did not fail the gate")
    problems = compare(baseline, edges, dict(lock, tokio=["1.54.0"]))
    if not any("version change" in p for p in problems):
        failures.append("a version change did not have to be recorded")

    # 4. An edge that differs only in optionality is a DIFFERENT edge: flipping
    #    `optional = true` to `false` changes which builds link it.
    flipped = [dict(e, optional=not e["optional"]) for e in edges]
    problems = compare(baseline, flipped, lock)
    if not any("NEW tokio edge" in p for p in problems):
        failures.append("flipping `optional` was not treated as a changed edge")

    if failures:
        print("tokio_inventory self-test FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print("tokio_inventory self-test: OK (7 planted changes, all caught)")
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true", help="check the checker")
    parser.add_argument("--list", action="store_true", help="print the census")
    parser.add_argument("--table", action="store_true", help="print the markdown table")
    parser.add_argument("--update", action="store_true",
                        help="re-record, preserving every surviving entry's annotations")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    root = Path(
        subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
    )
    baseline_path = root / BASELINE
    baseline = json.loads(baseline_path.read_text(encoding="utf-8")) if baseline_path.exists() else {}

    edges = edges_from_metadata(load_metadata(root))
    lock = lock_packages((root / "Cargo.lock").read_text(encoding="utf-8"))
    crates = sorted({p.name for p in (root / "crates").iterdir() if p.is_dir()})
    sites = source_sites(root, crates)

    if args.update:
        recorded = {edge_key(e): e for e in baseline.get("edges", [])}
        merged = []
        for e in edges:
            old = recorded.get(edge_key(e), {})
            merged.append(
                {
                    **e,
                    "surface": old.get("surface", "TODO: what JS reaches this"),
                    "reached_when": old.get("reached_when", "TODO"),
                    "blocker": old.get("blocker", "TODO"),
                    "issue": old.get("issue", "TODO"),
                    "plan": old.get("plan", "TODO"),
                }
            )
        baseline["edges"] = merged
        baseline["lockfile"] = lock
        baseline["source_sites"] = sites
        baseline.pop("_comment", None)
        baseline.setdefault("_README", DEFAULT_README)
        baseline = {
            "_README": baseline["_README"],
            **{k: v for k, v in baseline.items() if k != "_README"},
        }
        baseline_path.write_text(json.dumps(baseline, indent=2) + "\n", encoding="utf-8")
        print(f"recorded {len(merged)} edges and {len(lock)} lockfile packages to {BASELINE}")
        return 0

    if args.table:
        print(render_table(baseline))
        return 0

    if args.list:
        print(render_list(baseline, edges, lock, sites))
        return 0

    problems = compare(baseline, edges, lock)
    if problems:
        print("tokio inventory gate FAILED:", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        print(
            f"\nIf the change is intended, re-record with: "
            f"python3 {BASELINE.parent}/{Path(__file__).name} --update",
            file=sys.stderr,
        )
        return 1
    print(
        f"tokio inventory: {len(edges)} manifest edges across "
        f"{len({e['crate'] for e in edges})} workspace crates, "
        f"{len(lock)} tokio-family packages in Cargo.lock — unchanged."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
