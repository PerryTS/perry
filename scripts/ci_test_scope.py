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

Cross-host UI crates and doc fixtures that don't build on the Linux CI image are
always excluded (mirrors the historical exclude list in test.yml). `perry-runtime`
is included when affected; the workflow runs it single-threaded separately.

Usage:  <changed-files> | python3 scripts/ci_test_scope.py [--full]
"""
import json
import subprocess
import sys

# Excluded from the Linux cargo-test gate (see test.yml): cross-host UI backends
# (objc2 / win32 / NDK / gtk) and the doc fixture crate.
EXCLUDED = {
    "perry-ui-macos",
    "perry-ui-ios",
    "perry-ui-visionos",
    "perry-ui-tvos",
    "perry-ui-watchos",
    "perry-ui-gtk4",
    "perry-ui-android",
    "perry-ui-windows",
    "perry-ui-windows-winui",
    "perry-doc-fixture-my-bindings",
}

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


def _reverse_dep_closure(md, seeds):
    """All workspace members that transitively depend on any package in `seeds`."""
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
            if dependent not in affected:
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


def main() -> int:
    if "--has-lib" in sys.argv:
        return _has_lib_mode()

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
