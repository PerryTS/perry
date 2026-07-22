#!/usr/bin/env python3
"""Compute which workspace packages a CI `cargo test` run must cover.

Reads the changed file paths (one per line) on stdin and prints the set of
workspace packages to test, one per line, so the per-PR `cargo-test` gate only
exercises crates the diff can actually affect instead of the whole workspace
(~90 min). The release-tag and nightly runs pass `--full` to test everything.

Selection rules:
  * A file under `crates/<dir>/...` selects that crate AND every workspace crate
    that (transitively) depends on it — a reverse-dependency closure, so a change
    to a foundational crate (perry-runtime, perry-hir, …) still fans out to its
    dependents.
  * Infra changes (`.github/`, `scripts/`, `rust-toolchain*`) or any unrecognized
    top-level path force the FULL workspace (conservative — unknown blast radius).
  * Metadata-only changes (`CHANGELOG.md`, `CLAUDE.md`, any `*.md`, `docs/`, the
    root `Cargo.toml`/`Cargo.lock`) select nothing — a version-bump / changelog
    PR runs no tests and is instantly green.
  * `--full` (release tags, nightly cron, workflow_dispatch) selects every
    testable workspace member.

Cross-host UI crates and doc fixtures that don't build in this Linux test scope
are always excluded. The shared list lives in `workspace-architecture.json`;
`perry-runtime` is included when affected and runs single-threaded separately.

Usage:  <changed-files> | python3 scripts/ci_test_scope.py [--full]
"""
import json
import os
import re
import subprocess
import sys

from workspace_architecture import excluded_members, load_policy

# One source of truth shared with Clippy and coverage. The test scope adds the
# documentation fixture because the fixture has no product tests of its own.
EXCLUDED = excluded_members(load_policy(), "linux-test")

INFRA_PREFIXES = (".github/", "scripts/", "rust-toolchain")

# Top-level files with no effect on which crates to test.
IGNORABLE_EXACT = {"Cargo.toml", "Cargo.lock", "CHANGELOG.md", "CLAUDE.md", "LICENSE"}


def _is_ignorable(path: str) -> bool:
    if path in IGNORABLE_EXACT:
        return True
    if path.endswith(".md"):
        return True
    if path.startswith("docs/"):
        return True
    return False


def _load_metadata():
    raw = subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"]
    )
    return json.loads(raw)


def _testable_members(md):
    return {p["name"] for p in md["packages"] if p["name"] not in EXCLUDED}


def _dir_to_pkg(md):
    """Map each `crates/<dir>` directory to its cargo package name."""
    out = {}
    for p in md["packages"]:
        mp = p["manifest_path"]
        if "/crates/" in mp:
            d = mp.split("/crates/", 1)[1].split("/", 1)[0]
            out[d] = p["name"]
    return out


def _runtime_link_augment(seeds):
    """Add `perry` when a runtime-linked crate changes.

    The `perry` compile driver links `libperry_stdlib.a` / `libperry_ffi` and the
    per-package `libperry_ext-*.a` archives into the *compiled output* at runtime,
    not as cargo dependencies — so the cargo dep graph does NOT capture that
    `perry`'s integration tests (which compile + run TS programs against those
    archives) depend on them. `perry-runtime` already reaches `perry` through real
    cargo edges (via perry-ffi); stdlib / ffi / ext crates need this explicit
    edge so a change to them still runs perry's integration suite.
    """
    augmented = set(seeds)
    for s in seeds:
        if s in ("perry-stdlib", "perry-ffi") or s.startswith("perry-ext-"):
            augmented.add("perry")
    return augmented


def _is_fanout_leaf(name):
    """Crates we never fan *into* when a dependency changes.

    `perry-ext-*` and `perry-stdlib` are runtime FFI shims whose UNIT tests are
    self-contained pure-Rust logic — they do not exercise perry-runtime internals,
    so a perry-runtime change need not re-run them per-PR (the nightly full run +
    perry's integration tests cover that interaction). Excluding them from the
    reverse-dep fan-out keeps a foundational change from selecting ~40 crates, and
    avoids perry-runtime feature-unification rebuilds. A direct change *to* one of
    these crates still selects it (it starts as a seed).
    """
    return name == "perry-stdlib" or name.startswith("perry-ext-")


def _reverse_dep_closure(md, seeds):
    """All workspace members that transitively depend on any package in `seeds`.

    Fan-out skips `_is_fanout_leaf` crates (they are not added as dependents and
    are not traversed), so a foundational change does not pull in every FFI shim.
    """
    members = {p["name"] for p in md["packages"]}
    # revdeps[x] = packages that directly depend on x
    revdeps = {}
    for p in md["packages"]:
        for d in p.get("dependencies", []):
            if d["name"] in members:
                revdeps.setdefault(d["name"], set()).add(p["name"])
    affected = set(seeds)
    stack = list(seeds)
    while stack:
        cur = stack.pop()
        for dependent in revdeps.get(cur, ()):
            if dependent in affected or _is_fanout_leaf(dependent):
                continue
            affected.add(dependent)
            stack.append(dependent)
    return affected


def _has_lib_mode() -> int:
    """Exit 0 if any package named on stdin has a `lib` target, else exit 1.

    `cargo test --lib` errors ("no library targets") when *no* selected package
    has a library — e.g. a perry-only diff selects just the bin-only `perry`
    crate. The fast per-PR path uses this to choose `--lib --bins` vs `--bins`.
    """
    names = set(sys.stdin.read().split())
    md = _load_metadata()
    has = any(
        any("lib" in t["kind"] for t in p["targets"])
        for p in md["packages"]
        if p["name"] in names
    )
    return 0 if has else 1


_TEST_ATTR = re.compile(r"#\[\s*(?:tokio::)?test\b")


def _with_tests_mode() -> int:
    """Print the subset of stdin package names whose source contains unit tests.

    The per-PR fast path serializes test-binary links to avoid OOM (each crate's
    test binary statically links the whole runtime); building a test binary for a
    crate with *zero* `#[test]`s is pure wasted link time. Filtering them out
    keeps the wide-fan-out case (a perry-runtime change selects ~50 crates, ~30 of
    which are zero-test FFI shims) under the time budget. Conservative: a crate is
    kept if any `.rs` under its dir has a `#[test]` / `#[tokio::test]` attribute.
    """
    names = set(sys.stdin.read().split())
    md = _load_metadata()
    for p in md["packages"]:
        if p["name"] not in names:
            continue
        # Only scan `src/` — the fast path runs `--lib --bins`, i.e. unit tests
        # compiled into the lib/bin targets. Integration tests under `tests/`
        # are NOT run per-PR, so a crate whose only `#[test]`s live there must
        # not be selected (its lib test binary would have zero tests yet still
        # pay the heavy runtime link).
        crate_dir = os.path.join(os.path.dirname(p["manifest_path"]), "src")
        has_test = False
        for root, _dirs, files in os.walk(crate_dir):
            for fname in files:
                if not fname.endswith(".rs"):
                    continue
                try:
                    with open(os.path.join(root, fname), encoding="utf-8", errors="ignore") as fh:
                        if _TEST_ATTR.search(fh.read()):
                            has_test = True
                            break
                except OSError:
                    pass
            if has_test:
                break
        if has_test:
            print(p["name"])
    return 0


def main() -> int:
    if "--has-lib" in sys.argv:
        return _has_lib_mode()
    if "--with-tests" in sys.argv:
        return _with_tests_mode()

    full = "--full" in sys.argv
    changed = [line.strip() for line in sys.stdin if line.strip()]

    md = _load_metadata()
    testable = _testable_members(md)

    if not full:
        dir_to_pkg = _dir_to_pkg(md)
        seeds = set()
        for f in changed:
            if f.startswith(INFRA_PREFIXES):
                full = True
            elif f.startswith("crates/"):
                d = f.split("/", 2)[1]
                pkg = dir_to_pkg.get(d)
                if pkg is not None:
                    seeds.add(pkg)
                else:
                    # File under crates/ that isn't a known package dir — be safe.
                    full = True
            elif _is_ignorable(f):
                continue
            else:
                # Unrecognized top-level path (build config, etc.) — be safe.
                full = True

    if full:
        selected = testable
    else:
        seeds = _runtime_link_augment(seeds)
        selected = _reverse_dep_closure(md, seeds) & testable

    for name in sorted(selected):
        print(name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
