#!/usr/bin/env python3
"""Compute which integration-test suites a CI run must execute for a diff.

The per-PR `cargo-test` gate runs `--lib --bins` only (see
`scripts/ci_test_scope.py` and the `cargo-test` job in `.github/workflows/
test.yml`): the `crates/<pkg>/tests/*.rs` integration suites are NEVER executed
on a pull request. They each shell out to `perry compile` (~1-6 min apiece,
163 of them in `crates/perry` alone), so running them all per-PR is off the
table — but the consequence was that **a PR's own new acceptance suite could
not fail its own CI** (#5960; #5938 landed with `capture_rereg_renamed_class.rs`
red through green required checks).

This script closes that hole by scoping the e2e tier to the diff: it reads the
changed file paths (one per line) on stdin and prints the suites the diff names,
one `<package> <suite>` pair per line, for `cargo test -p <package> --test
<suite>`.

Selection rules (a suite is a `tests/*.rs` target of a workspace crate):
  * `crates/<dir>/tests/<suite>.rs` — the direct case: an added or modified
    acceptance suite runs. Deleted files are skipped (the target is gone).
  * `crates/<dir>/tests/<suite>/<file>.rs` — a *module directory* of a suite
    (e.g. `perry-codegen/tests/native_proof_regressions/invalidation.rs`, which
    `native_proof_regressions.rs` declares with `mod`): selects `<suite>`.
  * `crates/<dir>/tests/<shared>/...` where no `<shared>.rs` suite exists (e.g.
    a `common/` helper module or a `fixtures/` data dir): every suite in that
    crate can be affected, so all of them are selected.
  * `SOURCE_SUITE_MAP` — one hand-maintained exception to the rule below, for
    the in-process root-lowering suites. See its docstring.
  * Everything else selects nothing. In particular a plain `src/` change does
    NOT map to suites: there is no coverage data to map it with, and a
    crate-level map (`perry-codegen` -> all 163 `perry` suites) is exactly the
    full run this scoping exists to avoid. The nightly full `cargo test` stays
    the backstop for regressions in suites the diff does not name.

Cross-host UI crates that don't build on the Linux CI image are excluded
(shared with `ci_test_scope.EXCLUDED`).

The result is capped (`--cap`, default 12) so a mass rename of suites can't turn
one PR into a multi-hour integration run; the overflow is reported and left to
the nightly.

Usage:  <changed-files> | python3 scripts/ci_e2e_scope.py [--cap N]
        python3 scripts/ci_e2e_scope.py --self-test
"""
import os
import re
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ci_test_scope import EXCLUDED  # noqa: E402  (same-dir helper)

DEFAULT_CAP = 12

# The ONE source -> suite mapping (#7507). Hand-maintained and narrow by design.
#
# The general refusal above stands: there is no coverage data, and a crate-level
# map is the full run this scoping exists to avoid. This is a named exception
# with a stated cost.
#
# Why these suites are not like the 163 `perry` ones the rule protects against:
# they are in-process compiles of hand-built HIR with `emit_ir_only: true` — no
# `perry compile` subprocess, no link, no runtime. Measured on arm64 macOS,
# debug, `--test-threads=1`: shadow_slot_hygiene 12 tests / 0.10 s,
# scalar_replaced_slot_roots 11 / 0.12 s, temp_root_operand_temporaries 19 /
# 0.16 s. Under half a second of test time; the cost is the build, which
# `e2e-scoped` already pays whenever any perry-codegen suite is in scope.
#
# Why it exists at all: #7370 changed `crates/perry-codegen/src/` and named no
# suite. `shadow_slot_hygiene` went 0/12 and nothing was red until someone ran
# the nightly tier by hand — the per-PR `cargo-test` gate is `--lib --bins`, so
# it saw none of it.
#
# NOT YET LISTED, deliberately: `native_proof_regressions` and
# `native_proof_buffer_views`. Both carry pre-existing failures that are not
# this map's to fix (2 each: typed-array artifact records and integer-modulo /
# typed-f64 clone lowering). A new gate has never been green, and wiring in a
# suite that is red on arrival makes `e2e-scoped` red on most perry-codegen PRs
# — CLAUDE.md hazard 2 with extra steps, since that job is not in branch
# protection and so cannot block anything, and reviewers learn to ignore it.
# Add them here in the same commit that turns them green.
#
# Every entry is cross-checked against `tests/` on disk (see `select`): an entry
# naming a suite that does not exist FAILS the scope step rather than silently
# dropping out, the same rule `scripts/gc_root_dominance_allowlist.json` uses.
SOURCE_SUITE_MAP = {
    "crates/perry-codegen/src/": [
        ("perry-codegen", "shadow_slot_hygiene"),
        ("perry-codegen", "scalar_replaced_slot_roots"),
        ("perry-codegen", "temp_root_operand_temporaries"),
    ],
}

_TESTS_PATH = re.compile(r"^crates/([^/]+)/tests/(.+)$")
_PKG_NAME = re.compile(r'^\s*name\s*=\s*"([^"]+)"', re.M)


def _repo_root() -> str:
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _package_name(root: str, crate_dir: str):
    """Package name from `crates/<crate_dir>/Cargo.toml`, or None.

    Parsed directly instead of via `cargo metadata` so the CI scope step can run
    before any Rust toolchain is installed — PRs that name no suite must not pay
    for a toolchain at all.
    """
    manifest = os.path.join(root, "crates", crate_dir, "Cargo.toml")
    try:
        with open(manifest, encoding="utf-8") as fh:
            text = fh.read()
    except OSError:
        return None
    m = _PKG_NAME.search(text)
    return m.group(1) if m else None


def _suites_of(root: str, crate_dir: str):
    """Every `tests/*.rs` integration target of a crate, by suite name."""
    tests_dir = os.path.join(root, "crates", crate_dir, "tests")
    try:
        entries = os.listdir(tests_dir)
    except OSError:
        return []
    return sorted(
        e[:-3]
        for e in entries
        if e.endswith(".rs") and os.path.isfile(os.path.join(tests_dir, e))
    )


def _crate_dir_of(root: str, pkg: str):
    """The `crates/<dir>` whose Cargo.toml declares `pkg`, or None."""
    try:
        entries = os.listdir(os.path.join(root, "crates"))
    except OSError:
        return None
    for entry in sorted(entries):
        if _package_name(root, entry) == pkg:
            return entry
    return None


def _source_map_selection(path: str, root: str):
    """Suites `SOURCE_SUITE_MAP` names for one changed source path.

    Raises `SystemExit` when an entry names a suite that is not on disk: an
    entry that matches nothing must FAIL, not quietly select less. A suite
    renamed out from under this map is exactly how the map would rot back into
    the gap it was added to close.
    """
    hits = set()
    for prefix, pairs in SOURCE_SUITE_MAP.items():
        if not path.startswith(prefix):
            continue
        for pkg, suite in pairs:
            crate_dir = _crate_dir_of(root, pkg)
            on_disk = crate_dir is not None and os.path.isfile(
                os.path.join(root, "crates", crate_dir, "tests", suite + ".rs")
            )
            if not on_disk:
                raise SystemExit(
                    f"ci_e2e_scope: SOURCE_SUITE_MAP names {pkg}::{suite}, which "
                    f"has no crates/*/tests/{suite}.rs. Update the map in the "
                    f"same commit that renames or deletes the suite (#7507)."
                )
            if pkg not in EXCLUDED:
                hits.add((pkg, suite))
    return hits


def select(changed, root: str):
    """-> sorted list of (package, suite) named by the changed paths."""
    selected = set()
    for path in changed:
        path = path.strip()
        selected |= _source_map_selection(path, root)
        m = _TESTS_PATH.match(path)
        if not m:
            continue
        crate_dir, rest = m.group(1), m.group(2)
        pkg = _package_name(root, crate_dir)
        if pkg is None or pkg in EXCLUDED:
            continue

        if "/" not in rest:
            if not rest.endswith(".rs"):
                continue
            suite = rest[:-3]
            # Skip deletions: the target no longer exists.
            if os.path.isfile(os.path.join(root, "crates", crate_dir, "tests", rest)):
                selected.add((pkg, suite))
            continue

        head = rest.split("/", 1)[0]
        sibling = os.path.join(root, "crates", crate_dir, "tests", head + ".rs")
        if os.path.isfile(sibling):
            # Module directory of `<head>.rs`.
            selected.add((pkg, head))
        else:
            # Shared helper / fixture dir (`common/`, `fixtures/`): any suite in
            # the crate can depend on it.
            for suite in _suites_of(root, crate_dir):
                selected.add((pkg, suite))
    return sorted(selected)


def _self_test() -> int:
    with tempfile.TemporaryDirectory() as root:
        def touch(rel, body=""):
            full = os.path.join(root, rel)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "w", encoding="utf-8") as fh:
                fh.write(body)

        touch("crates/perry/Cargo.toml", '[package]\nname = "perry"\n')
        touch("crates/perry/tests/issue_5024_proto.rs")
        touch("crates/perry/tests/other_suite.rs")
        touch("crates/perry-codegen/Cargo.toml", '[package]\nname = "perry-codegen"\n')
        touch("crates/perry-codegen/tests/native_proof_regressions.rs")
        touch("crates/perry-codegen/tests/native_proof_regressions/invalidation.rs")
        for suite in {s for _, s in SOURCE_SUITE_MAP["crates/perry-codegen/src/"]}:
            touch(f"crates/perry-codegen/tests/{suite}.rs")
        touch("crates/perry-cc/Cargo.toml", '[package]\nname = "perry-cc"\n')
        touch("crates/perry-cc/tests/alpha.rs")
        touch("crates/perry-cc/tests/beta.rs")
        touch("crates/perry-cc/tests/common/mod.rs")
        touch("crates/perry-ui-ios/Cargo.toml", '[package]\nname = "perry-ui-ios"\n')
        touch("crates/perry-ui-ios/tests/ui.rs")

        cases = [
            # direct suite change
            (["crates/perry/tests/issue_5024_proto.rs"], [("perry", "issue_5024_proto")]),
            # source-only change selects nothing
            (["crates/perry/src/main.rs", "CHANGELOG.md"], []),
            # deleted suite is skipped (no file on disk)
            (["crates/perry/tests/deleted_suite.rs"], []),
            # module dir of a suite maps to the suite
            (
                ["crates/perry-codegen/tests/native_proof_regressions/invalidation.rs"],
                [("perry-codegen", "native_proof_regressions")],
            ),
            # shared helper dir selects every suite in that crate
            (
                ["crates/perry-cc/tests/common/mod.rs"],
                [("perry-cc", "alpha"), ("perry-cc", "beta")],
            ),
            # cross-host UI crates are excluded
            (["crates/perry-ui-ios/tests/ui.rs"], []),
            # dedup across several paths of the same suite
            (
                [
                    "crates/perry/tests/other_suite.rs",
                    "crates/perry/tests/other_suite.rs",
                    "crates/perry/tests/issue_5024_proto.rs",
                ],
                [("perry", "issue_5024_proto"), ("perry", "other_suite")],
            ),
            # unknown crate dir
            (["crates/nope/tests/x.rs"], []),
            # #7507: a perry-codegen SOURCE change selects exactly the mapped
            # root-lowering suites — the case that made #7370 land silently.
            (
                ["crates/perry-codegen/src/codegen/helpers.rs"],
                sorted(SOURCE_SUITE_MAP["crates/perry-codegen/src/"]),
            ),
            # …and no other crate's `src/` maps to anything. The general
            # refusal is still the rule; this is one named exception.
            (["crates/perry-hir/src/lower.rs", "crates/perry/src/main.rs"], []),
            # a perry-codegen source change plus one of its own suites is the
            # union, deduplicated.
            (
                [
                    "crates/perry-codegen/src/gc_map.rs",
                    "crates/perry-codegen/tests/native_proof_regressions.rs",
                ],
                sorted(
                    set(SOURCE_SUITE_MAP["crates/perry-codegen/src/"])
                    | {("perry-codegen", "native_proof_regressions")}
                ),
            ),
        ]
        for changed, expected in cases:
            got = select(changed, root)
            if got != expected:
                print(f"self-test FAILED for {changed}: {got} != {expected}", file=sys.stderr)
                return 1

        # An entry that matches nothing must FAIL, not select less. Sabotage the
        # map with a suite that is not on disk and require the raise.
        saved = dict(SOURCE_SUITE_MAP)
        SOURCE_SUITE_MAP["crates/perry-codegen/src/"] = [
            ("perry-codegen", "renamed_away_suite")
        ]
        try:
            select(["crates/perry-codegen/src/anything.rs"], root)
        except SystemExit:
            pass
        else:
            print(
                "self-test FAILED: SOURCE_SUITE_MAP naming a nonexistent suite "
                "must fail the scope step (#7507)",
                file=sys.stderr,
            )
            return 1
        finally:
            SOURCE_SUITE_MAP.clear()
            SOURCE_SUITE_MAP.update(saved)

    # The real map, against the real repo: every entry must be a suite that
    # exists here and now, not only in the self-test's fixture tree.
    real_root = _repo_root()
    for prefix in SOURCE_SUITE_MAP:
        select([prefix + "probe.rs"], real_root)

    print("ci_e2e_scope self-test: ok")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()

    cap = DEFAULT_CAP
    if "--cap" in sys.argv:
        cap = int(sys.argv[sys.argv.index("--cap") + 1])

    changed = [line.strip() for line in sys.stdin if line.strip()]
    pairs = select(changed, _repo_root())

    if len(pairs) > cap:
        print(
            f"::notice::{len(pairs)} integration suites named by this diff exceeds the "
            f"cap of {cap}; running the first {cap} (sorted). The rest are covered by "
            f"the nightly full cargo-test.",
            file=sys.stderr,
        )
        pairs = pairs[:cap]

    for pkg, suite in pairs:
        print(f"{pkg} {suite}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
