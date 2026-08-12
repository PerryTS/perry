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
    `crates/perry-codegen/src/`, whose suites are in-process compiles rather
    than `perry compile` subprocesses. As of #7708 it is COMPLETE for that
    crate and checked to stay so: every `crates/perry-codegen/tests/*.rs` must
    be either mapped or listed in `SUITE_EXCLUSIONS` with its failing test and
    an issue, and `--self-test` fails on one that is neither. See its
    docstring.
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

Output is one `<package> <suite> <per-suite timeout seconds>` line per suite.

Usage:  <changed-files> | python3 scripts/ci_e2e_scope.py [--cap N]
        python3 scripts/ci_e2e_scope.py --exclusions
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
# What changed in #7708: this map listed THREE suites. `crates/perry-codegen/
# tests/` holds 24, and 23 of them are the same kind of thing — in-process
# compiles of hand-built HIR with `emit_ir_only: true`. The narrowness was not a
# judgment about the other 20; it was what one commit had time to verify. Nothing
# said so, and nothing could: a suite that is in neither the map nor any rule is
# indistinguishable from a suite nobody needed to map.
#
# So the map is now COMPLETE-BY-CONSTRUCTION for this crate. `_assert_map_covers_
# codegen_suites` (run by `--self-test`, which the `e2e-scoped` job executes
# before it does anything else) requires every `crates/perry-codegen/tests/*.rs`
# on disk to be either mapped here or named in `SUITE_EXCLUSIONS` below. A new
# suite fails CI until someone classifies it. That is the "cannot silently omit
# a suite" property #7708 asked for; the map is still hand-written, but an
# omission is no longer silent.
#
# Measured on arm64 macOS, debug, `--test-threads=1`, one `cargo test -p
# perry-codegen --test <suite>` per row: every one of the 22 mapped suites
# completes in 2.2-10.4 s wall clock, and nearly all of that is cargo's
# freshness check — the test time itself is sub-second (e.g. shadow_slot_hygiene
# 12 tests / 0.10 s, native_proof_buffer_views 36 / 0.2 s). The cost is the
# build, which `e2e-scoped` already pays whenever any perry-codegen suite is in
# scope. They are NOT like the 163 `perry` suites the general refusal protects
# against: no `perry compile` subprocess, no link, no runtime.
#
# Why it exists at all: #7370 changed `crates/perry-codegen/src/` and named no
# suite. `shadow_slot_hygiene` went 0/12 and nothing was red until someone ran
# the nightly tier by hand — the per-PR `cargo-test` gate is `--lib --bins`, so
# it saw none of it.
#
# Every entry is cross-checked against `tests/` on disk (see `select`): an entry
# naming a suite that does not exist FAILS the scope step rather than silently
# dropping out, the same rule `scripts/gc_root_dominance_allowlist.json` uses.
_CODEGEN_SRC = "crates/perry-codegen/src/"

_CODEGEN_SUITES = [
    "app_window_config_options",
    "argless_builtin_extra_args",
    "class_field_store_pointer_test",
    "class_keys_gc_root",
    "constructor_recursion",
    "destructure_call_location",
    "i64_spec_ternary_recursion",
    "loop_safepoint_purity",
    "macos_bundle_chdir_gate",
    "manifest_consistency",
    "native_proof_buffer_views",
    # #7506/#7245: held out until its one failing test was triaged. The
    # composition it guards had drifted from three named callees to three
    # PROPERTIES (the guard-failure edge now reaches `$pshape`, which coerces
    # what it loads, rather than `$generic`), so the test asserted a symbol that
    # no longer had to be there. Re-pointed at the property; 262/262 green, and
    # the suite belongs in the per-PR map rather than in the exclusions.
    "native_proof_regressions",
    "node_test_mock_property_presence",
    "perry_builtin_name_collision",
    "private_guard_declaring_class",
    "scalar_replaced_slot_roots",
    "shadow_slot_hygiene",
    "static_symbol_hygiene",
    "temp_root_operand_temporaries",
    "typed_feedback",
    "typed_shape_declared_at_allocation",
    "typed_shape_descriptor",
    "typed_shape_descriptors",
]

SOURCE_SUITE_MAP = {
    _CODEGEN_SRC: [("perry-codegen", suite) for suite in _CODEGEN_SUITES],
}

# The suites held OUT of the map, one entry per FAILING TEST, because they are
# red on `main` and a gate that is red on arrival is CLAUDE.md hazard 2 with
# extra steps — `e2e-scoped` is not in branch protection, so a permanently-red
# one teaches reviewers to ignore it.
#
# #7708 counted SIX failures across four suites at v0.5.1407. Four have since
# been fixed by unrelated work and nothing recorded it, which is the same
# bookkeeping failure as #797's parity skip-list: an entry that stops being true
# costs nothing to keep. So these entries are SELF-INVALIDATING in both
# directions:
#
#   * the suite must exist on disk (`_assert_exclusions_are_live`), and
#   * the named test must still FAIL. `e2e-scoped` runs exactly these tests and
#     fails the job if one PASSES, with instructions to delete the entry. A fix
#     therefore cannot land while leaving its exclusion behind.
#
# Excluding a TEST rather than a SUITE matters: `native_proof_regressions` is
# 262 tests, and holding all 262 out for one of them is how 261 tests' worth of
# coverage went dark.
SUITE_EXCLUSIONS = [
    (
        "perry-codegen",
        "large_object_barriers",
        "large_local_array_push_inbounds_store_emits_precise_slot_barrier",
        "#7708 — red on main; the other 2 tests in this suite pass.",
    ),
]

# Suites reached through `SOURCE_SUITE_MAP` are exempt from `--cap` and carry a
# tighter per-suite wall-clock bound than a diff-named suite. Both follow from
# the same measured fact: they are in-process and finish in seconds, so neither
# the "a mass rename must not become a multi-hour run" risk the cap exists for
# nor the 25-minute `perry compile` bound applies. Truncating them at the cap
# would reintroduce silent omission one layer up — the mapped set would be
# complete and then quietly cut to 12.
MAPPED_SUITE_TIMEOUT_S = 300
NAMED_SUITE_TIMEOUT_S = 1500


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
    """-> sorted list of (package, suite) named by the changed paths.

    The union of both tiers. `select_split` is the one that keeps them apart;
    this stays for callers (and self-test cases) that only care about the set.
    """
    mapped, named = select_split(changed, root)
    return sorted(set(mapped) | set(named))


def select_split(changed, root: str):
    """-> (mapped, named), each a sorted list of (package, suite).

    `mapped` came from `SOURCE_SUITE_MAP`; `named` was named by the diff. They
    are kept apart because they are charged differently: only `named` is subject
    to `--cap`, and the two carry different per-suite time bounds. A suite in
    both is reported as mapped only, so it is never run twice.
    """
    from_map = set()
    selected = set()
    for path in changed:
        path = path.strip()
        from_map |= _source_map_selection(path, root)
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
    return sorted(from_map), sorted(selected - from_map)


def _assert_map_covers_codegen_suites(root: str) -> None:
    """Every `crates/perry-codegen/tests/*.rs` is mapped or explicitly excluded.

    This is the property #7708 asked for. The map stays hand-written — there is
    still no coverage data — but a suite nobody classified now FAILS instead of
    being indistinguishable from one nobody needed. Adding a suite therefore
    forces a one-line decision: map it, or say in `SUITE_EXCLUSIONS` which test
    is red and under which issue.
    """
    crate_dir = _crate_dir_of(root, "perry-codegen")
    if crate_dir is None:
        return
    on_disk = set(_suites_of(root, crate_dir))
    if not on_disk:
        return
    mapped = {suite for pkg, suite in SOURCE_SUITE_MAP.get(_CODEGEN_SRC, [])}
    excluded = {suite for pkg, suite, _test, _why in SUITE_EXCLUSIONS}
    unclassified = sorted(on_disk - mapped - excluded)
    if unclassified:
        raise SystemExit(
            "ci_e2e_scope: these crates/perry-codegen/tests/*.rs suites are in "
            "neither SOURCE_SUITE_MAP nor SUITE_EXCLUSIONS: "
            + ", ".join(unclassified)
            + ". Add each to the map, or to SUITE_EXCLUSIONS with the failing "
            "test and an issue number. An unclassified suite is invisible to "
            "per-PR CI, which is #7708."
        )


def _assert_exclusions_are_live(root: str) -> None:
    """An exclusion must name a suite that exists, and must not also be mapped.

    The stale half of the bookkeeping. The other half — "the named test must
    still fail" — cannot be answered without running cargo, so `e2e-scoped`
    answers it: it runs exactly these tests and fails if one passes.
    """
    mapped = {(pkg, suite) for pkg, suite in SOURCE_SUITE_MAP.get(_CODEGEN_SRC, [])}
    for pkg, suite, test, why in SUITE_EXCLUSIONS:
        if not test or not why:
            raise SystemExit(
                f"ci_e2e_scope: SUITE_EXCLUSIONS entry {pkg}::{suite} needs both "
                "a test name and a reason (issue number)."
            )
        if (pkg, suite) in mapped:
            raise SystemExit(
                f"ci_e2e_scope: {pkg}::{suite} is both mapped and excluded. "
                "Pick one — a suite cannot be per-PR coverage and a known "
                "failure at the same time."
            )
        crate_dir = _crate_dir_of(root, pkg)
        on_disk = crate_dir is not None and os.path.isfile(
            os.path.join(root, "crates", crate_dir, "tests", suite + ".rs")
        )
        if not on_disk:
            raise SystemExit(
                f"ci_e2e_scope: SUITE_EXCLUSIONS names {pkg}::{suite}, which has "
                f"no crates/*/tests/{suite}.rs. Delete the entry in the same "
                "commit that renames or deletes the suite (#7708)."
            )


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

    # #7708: the coverage rule must actually be able to fail. Sabotage it with a
    # suite on disk that is in neither list and require the raise — a check that
    # has never failed is a check nobody has verified (CLAUDE.md, hazard 4).
    with tempfile.TemporaryDirectory() as root:
        os.makedirs(os.path.join(root, "crates", "perry-codegen", "tests"))
        with open(
            os.path.join(root, "crates", "perry-codegen", "Cargo.toml"),
            "w",
            encoding="utf-8",
        ) as fh:
            fh.write('[package]\nname = "perry-codegen"\n')
        for suite in _CODEGEN_SUITES + [s for _p, s, _t, _w in SUITE_EXCLUSIONS]:
            open(
                os.path.join(root, "crates", "perry-codegen", "tests", suite + ".rs"),
                "w",
            ).close()
        # Classified: passes.
        _assert_map_covers_codegen_suites(root)
        _assert_exclusions_are_live(root)
        # Unclassified: must fail.
        open(
            os.path.join(root, "crates", "perry-codegen", "tests", "brand_new.rs"), "w"
        ).close()
        try:
            _assert_map_covers_codegen_suites(root)
        except SystemExit:
            pass
        else:
            print(
                "self-test FAILED: an unclassified perry-codegen suite must fail "
                "the scope step (#7708)",
                file=sys.stderr,
            )
            return 1

    # The cap applies to diff-named suites only; mapped suites are never cut.
    with tempfile.TemporaryDirectory() as root:
        os.makedirs(os.path.join(root, "crates", "perry-cc", "tests"))
        with open(
            os.path.join(root, "crates", "perry-cc", "Cargo.toml"), "w", encoding="utf-8"
        ) as fh:
            fh.write('[package]\nname = "perry-cc"\n')
        for i in range(20):
            open(
                os.path.join(root, "crates", "perry-cc", "tests", f"s{i:02d}.rs"), "w"
            ).close()
        named = [f"crates/perry-cc/tests/s{i:02d}.rs" for i in range(20)]
        mapped_pairs, named_pairs = select_split(named, root)
        if mapped_pairs or len(named_pairs) != 20:
            print(
                f"self-test FAILED: expected 0 mapped / 20 named, got "
                f"{len(mapped_pairs)}/{len(named_pairs)}",
                file=sys.stderr,
            )
            return 1

    # The real map, against the real repo: every entry must be a suite that
    # exists here and now, not only in the self-test's fixture tree.
    real_root = _repo_root()
    for prefix in SOURCE_SUITE_MAP:
        select([prefix + "probe.rs"], real_root)
    _assert_map_covers_codegen_suites(real_root)
    _assert_exclusions_are_live(real_root)

    print("ci_e2e_scope self-test: ok")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()

    root = _repo_root()

    # `<package> <suite> <failing test>` for every held-out test, so the runner
    # can assert each one still fails and tell the fixer to delete the entry.
    if "--exclusions" in sys.argv:
        for pkg, suite, test, _why in SUITE_EXCLUSIONS:
            print(f"{pkg} {suite} {test}")
        return 0

    cap = DEFAULT_CAP
    if "--cap" in sys.argv:
        cap = int(sys.argv[sys.argv.index("--cap") + 1])

    changed = [line.strip() for line in sys.stdin if line.strip()]
    mapped, named = select_split(changed, root)

    # The cap is a guard against a mass suite rename becoming a multi-hour
    # `perry compile` run. Mapped suites are in-process and finish in seconds,
    # so capping them would only cut coverage the map just made complete.
    if len(named) > cap:
        print(
            f"::notice::{len(named)} integration suites named by this diff exceeds the "
            f"cap of {cap}; running the first {cap} (sorted). The rest are covered by "
            f"the nightly full cargo-test.",
            file=sys.stderr,
        )
        named = named[:cap]

    for pkg, suite in mapped:
        print(f"{pkg} {suite} {MAPPED_SUITE_TIMEOUT_S}")
    for pkg, suite in named:
        print(f"{pkg} {suite} {NAMED_SUITE_TIMEOUT_S}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
