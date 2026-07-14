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


def select(changed, root: str):
    """-> sorted list of (package, suite) named by the changed paths."""
    selected = set()
    for path in changed:
        m = _TESTS_PATH.match(path.strip())
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
        ]
        for changed, expected in cases:
            got = select(changed, root)
            if got != expected:
                print(f"self-test FAILED for {changed}: {got} != {expected}", file=sys.stderr)
                return 1
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
