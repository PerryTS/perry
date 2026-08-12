#!/usr/bin/env python3
"""Decide whether the per-PR `ext-link` gate must run, and over which packages.

#7656: a `perry-runtime` change broke the LINK of five `perry-ext-*` crates and
no per-PR gate could have caught it (#7650, fixed in #7655). It would have
surfaced at the next tag, days later.

## Why the existing scope cannot see this

`ci_test_scope.py` selects a reverse-dependency closure, and `_is_fanout_leaf`
deliberately keeps `perry-ext-*` / `perry-stdlib` OUT of the fan-out. That
exclusion is correct on its own terms — those crates' unit tests are
self-contained pure-Rust logic and re-running ~40 of them on every foundational
change is exactly the cost the scoping exists to avoid.

What it misses is that for these crates the coupling is not the test, it is the
**link**. They pull in a feature-stripped runtime through `perry-ffi`'s
`runtime-link`, built with `-Wl,-dead_strip`, so any new reference edge inside
`perry-runtime` can keep a chain alive the stripper had been removing. In #7650
the edge was a single added call (`pin_object` -> `arena::classify_heap_space`)
in code that had previously done a raw flag write. The symptom is
`Undefined symbols for architecture arm64`, not a failing assertion — so
BUILDING the ext crates is the entire check, and running their tests adds
nothing.

Hence a separate, link-only arm rather than widening the closure: `cargo test
--no-run` builds (and therefore links) the test binaries and stops.

## Selection

The gate runs when the diff touches a crate whose object code ends up inside
those archives — `perry-runtime`, `perry-stdlib`, `perry-ffi` — or any
`perry-ext-*` crate directly.

The package list is DERIVED from the workspace, never hand-listed: a new
`crates/perry-ext-<x>/` is covered the day it lands. A hand-maintained list is
the failure mode #7748 had to repair in `ci_e2e_scope.py`, where the map named
3 of 24 suites and nothing could say so.

Usage:  <changed-files> | python3 scripts/ci_ext_link_scope.py
        python3 scripts/ci_ext_link_scope.py --count-linked <cargo-json>
        python3 scripts/ci_ext_link_scope.py --self-test
"""
import os
import sys

# A change to any of these can alter what the ext archives' dead-strip pass
# keeps, so they arm the gate. `perry-ffi` is here because it is the crate that
# actually declares `runtime-link`.
LINK_SOURCE_PREFIXES = (
    "crates/perry-runtime/",
    "crates/perry-stdlib/",
    "crates/perry-ffi/",
)

EXT_PREFIX = "crates/perry-ext-"


def _repo_root() -> str:
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def ext_packages(root: str):
    """Every `perry-ext-*` workspace member, from the directory listing.

    Derived rather than listed so a new ext crate is covered on the day it
    lands. Only directories carrying a `Cargo.toml` count, so a stray path
    cannot inject a bogus `-p` argument.
    """
    crates = os.path.join(root, "crates")
    try:
        entries = os.listdir(crates)
    except OSError:
        return []
    return sorted(
        e
        for e in entries
        if e.startswith("perry-ext-")
        and os.path.isfile(os.path.join(crates, e, "Cargo.toml"))
    )


def arms(changed) -> bool:
    """Does this diff require the ext link check?"""
    for path in changed:
        path = path.strip()
        if not path:
            continue
        if path.startswith(LINK_SOURCE_PREFIXES) or path.startswith(EXT_PREFIX):
            return True
    return False


def _self_test() -> int:
    root = _repo_root()

    cases = [
        (["crates/perry-runtime/src/gc/mod.rs"], True),
        (["crates/perry-stdlib/src/lib.rs"], True),
        (["crates/perry-ffi/src/async_runtime.rs"], True),
        (["crates/perry-ext-http/src/lib.rs"], True),
        # Nothing that ends up in the archives.
        (["crates/perry-hir/src/lower.rs"], False),
        (["crates/perry-codegen/src/expr/mod.rs"], False),
        (["docs/src/index.md", "CLAUDE.md"], False),
        ([], False),
        # One armed path among many is enough.
        (["README.md", "crates/perry-runtime/src/value.rs"], True),
    ]
    for changed, expected in cases:
        got = arms(changed)
        if got != expected:
            print(f"self-test FAILED: {changed} -> {got}, want {expected}", file=sys.stderr)
            return 1

    # The package list must be non-empty and must actually name ext crates. A
    # scope rule that selected zero packages would make the gate green forever
    # — the failure mode this repo has shipped four times (#6942/#6946, #7024,
    # #7025) and the first thing #7656 asks to get right.
    pkgs = ext_packages(root)
    if len(pkgs) < 5:
        print(
            f"self-test FAILED: expected the workspace to have several "
            f"perry-ext-* crates, found {len(pkgs)}: {pkgs}",
            file=sys.stderr,
        )
        return 1
    if any(not p.startswith("perry-ext-") for p in pkgs):
        print(f"self-test FAILED: non-ext package in list: {pkgs}", file=sys.stderr)
        return 1
    # The five that actually failed in #7650 must be covered by the derived
    # list — if a rename drops one, this says so instead of quietly shrinking.
    for name in (
        "perry-ext-pdf",
        "perry-ext-lru-cache",
        "perry-ext-node-forge",
        "perry-ext-mongodb",
        "perry-ext-http",
    ):
        if name not in pkgs:
            print(
                f"self-test FAILED: {name} (one of #7650's five) is not in the "
                f"derived package list",
                file=sys.stderr,
            )
            return 1

    print(f"ci_ext_link_scope self-test: ok ({len(pkgs)} ext crates)")
    return 0


def count_linked(path: str) -> int:
    """Test binaries cargo reports as linked, from `--message-format=json`.

    THE COUNT IS THE POINT. A scope rule that silently selected zero packages,
    or a cargo invocation that built nothing, leaves the gate green forever —
    the failure mode this repo has shipped four times (#6942/#6946, #7024,
    #7025), and the first thing #7656 asks to get right. The job asserts this is
    non-zero, so "nothing threw" is never mistaken for "the subject was live".
    """
    import json

    linked = 0
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            try:
                msg = json.loads(line)
            except Exception:
                continue
            if msg.get("reason") == "compiler-artifact" and msg.get("executable"):
                linked += 1
                print(f"linked {msg['target']['name']}", file=sys.stderr)
    return linked


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()

    if "--count-linked" in sys.argv:
        path = sys.argv[sys.argv.index("--count-linked") + 1]
        print(count_linked(path))
        return 0

    changed = [line for line in sys.stdin]
    if not arms(changed):
        return 0
    for pkg in ext_packages(_repo_root()):
        print(pkg)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
