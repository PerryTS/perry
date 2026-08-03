#!/usr/bin/env python3
"""Fail when a test file exists on disk but no registry lists it — a DARK TEST.

WHY THIS EXISTS
---------------
Some of this repo's suites are driven by an explicit registry — a corpus file, a
TOML manifest, a `mod` declaration — rather than by a glob. A test file added
without its registry line is not a failing test. It is *no test at all*: it
compiles nowhere, runs nowhere, and reports nothing. The PR that added it is
green, the reviewer sees a witness in the diff, and the defect it was written to
catch stays uncovered.

That has now happened four times, all against `test-parity/gc_repsel_corpus.txt`:

  * #7192 and #7216 — two `test_gap_gc_*` stale-root witnesses, each of which
    says in its own header that it is LIVE BY CONSTRUCTION, both dark from merge;
  * #7252 — a third (`test_gap_gc_call_argument_rooting`), caught only once the
    `gc-moving-witnesses` job's own registration assert reached `main`;
  * #7270/#7271 — two more (rest-argument and same-module call-argument
    rooting), caught by the maintainer while merging.

Four occurrences of one mistake is not carelessness, it is a missing gate. Two
partial gates already existed and neither could catch a PR:
`scripts/gc_repsel_matrix.sh` auto-detects unregistered `test_gap_repsel_*` /
`test_gap_specabi_*` files, and `gc-moving-witnesses.yml` adds the
`test_gap_gc_*` prefix — but BOTH live behind a full release build of the
compiler (a 90-minute job), behind a changed-paths relevance filter, and in
workflows that are not in branch protection's required contexts. The check that
mattered could not run on the pull request that needed it.

This script is the cheap half, pulled out to where it can actually block: pure
filesystem and text, no compiler, no Node, ~1 second, run from `lint`, which IS
a required context. It also generalises past that one corpus — the same shape
exists in three other places in the tree (see MECHANISMS).

HOW IT IS BUILT TO BE ABLE TO FAIL (CLAUDE.md, "four ways a gate can be unable
to fail")
------------------------------------------------------------------------------
1. No `continue-on-error`, no `|| true`: the `lint` step is this script's exit
   status.
2. Branch protection needs NO change, because the step lives inside `lint`,
   which is already required. That is deliberate: hazard 2 is the step people
   forget, so this gate is placed where the step does not exist.
3. The `lint` job's concurrency already cancels pull-request runs only.
4. **The subject is asserted live.** Every mechanism declares a floor on its
   candidate set and FAILS if the glob stops matching, so "0 dark files over 0
   candidates" and "0 dark files over 177 candidates" cannot print the same
   verdict. The summary always names the counts: `checked N files against M
   registries`. `--self-test` plants an unregistered file into each mechanism
   and asserts the gate goes red, then removes it and asserts it goes green.

WHY AN EXCLUSION LIST AND NOT A COUNT
-------------------------------------
A numeric threshold cannot tell a new dark file from an old one: fix one, add
one, and the tally is unchanged. Every non-registered candidate is named here
with a reason. A stale exclusion — one that no longer matches a file on disk —
is itself a FAILURE, so an exclusion cannot outlive the file it excuses (the
same rule `scripts/gc_root_dominance_allowlist.json` uses).

Usage:
    python3 scripts/check_test_registration.py             # check the repo
    python3 scripts/check_test_registration.py --self-test # check the checker
    python3 scripts/check_test_registration.py --list      # describe the scope
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Iterable

REPO_ROOT = Path(__file__).resolve().parent.parent


class MissingRegistry(Exception):
    """A mechanism's registry file is absent from the tree it is checking.

    Raised by `Tree.read` instead of letting `FileNotFoundError` propagate, so
    a deleted or renamed registry is a named problem in the report — not a
    traceback that also hides which file was missing.
    """


# ---------------------------------------------------------------------------
# Tree — the repo as the checker sees it.
#
# Every read goes through here so `--self-test` can plant, hide and rewrite
# files WITHOUT touching the working tree. That matters twice over: a checkout
# mutated by its own gate is a bad neighbour to every other CI step, and a
# self-test that runs against a synthetic fixture instead of the real corpus
# proves the fixture is well-formed, not that the gate works. This runs the real
# mechanism definitions over the real registries with one file added.
# ---------------------------------------------------------------------------
class Tree:
    def __init__(
        self,
        root: Path,
        added: Iterable[str] = (),
        removed: Iterable[str] = (),
        overrides: dict[str, str] | None = None,
    ) -> None:
        self.root = root
        self.added = set(added)
        self.removed = set(removed)
        self.overrides = dict(overrides or {})

    def glob(self, pattern: str) -> list[str]:
        hits = {
            p.relative_to(self.root).as_posix()
            for p in self.root.glob(pattern)
            if p.is_file()
        }
        hits |= {a for a in self.added if _fnmatch_path(a, pattern)}
        return sorted(hits - self.removed)

    def exists(self, rel: str) -> bool:
        if rel in self.removed:
            return False
        if rel in self.added or rel in self.overrides:
            return True
        return (self.root / rel).is_file()

    def read(self, rel: str) -> str:
        if rel in self.overrides:
            return self.overrides[rel]
        if rel in self.removed:
            raise MissingRegistry(rel)
        if rel in self.added:
            return ""
        path = self.root / rel
        if not path.is_file():
            raise MissingRegistry(rel)
        return path.read_text(encoding="utf-8", errors="ignore")


def _fnmatch_path(path: str, pattern: str) -> bool:
    """Match a repo-relative path against a `pathlib.glob` pattern.

    `fnmatch` is wrong here: its `*` crosses `/`, so `test-files/*.ts` would
    match `test-files/fixtures/a.ts`. Translate segment by segment instead, with
    `**` as the only separator-crossing token.
    """
    parts = pattern.split("/")
    rx = []
    for part in parts:
        if part == "**":
            rx.append("(?:[^/]+/)*")
            continue
        seg = "".join(
            "[^/]*" if c == "*" else "[^/]" if c == "?" else re.escape(c) for c in part
        )
        rx.append(seg + "/")
    joined = "".join(rx)
    if joined.endswith("/"):
        joined = joined[:-1]
    return re.fullmatch(joined, path) is not None


# ---------------------------------------------------------------------------
# Registry readers.
# ---------------------------------------------------------------------------
def _read_hash_list(tree: Tree, rel: str) -> set[str]:
    """One entry per line, `#` starts a comment (gc_repsel_corpus.txt)."""
    out = set()
    for line in tree.read(rel).splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            out.add(line)
    return out


def _read_toml_paths(tree: Tree, rel: str, key: str) -> set[str]:
    """Every `<key> = "..."` value in a TOML manifest.

    Deliberately a regex and not `tomllib`: the two manifests this reads spell
    their entry paths as plain top-level string keys, and a line-oriented reader
    keeps the failure mode legible (`grep` finds what the checker found). A
    quoted `#` inside the value is not a thing in either file.
    """
    return set(re.findall(r'^\s*%s\s*=\s*"([^"]+)"' % re.escape(key), tree.read(rel), re.M))


_MOD_RE = r"^[^\S\n]*(?:pub(?:\([^)]*\))?[^\S\n]+)?mod[^\S\n]+%s[^\S\n]*[;{]"


def _rust_module_is_declared(tree: Tree, rel: str) -> bool:
    """Is `<dir>/<stem>.rs` named by a `mod` declaration that can reach it?

    Rust resolves `<dir>/<stem>.rs` as child module `<stem>` of the module
    rooted at `<dir>`, whose body is either `<dir>/mod.rs` (2015 layout) or the
    sibling `<dir>.rs` (2018 layout).

    But that module may itself be an INLINE `mod <dir> { … }` block in an
    ancestor file, in which case the declaration lives further up while the file
    still sits under `<dir>/`. `crates/perry/src/commands/compile/resolve/tests/
    declaration_sidecar_tests/compile_package.rs` is exactly that: its
    `mod compile_package;` is inside `mod declaration_sidecar_tests { … }` in
    `resolve/tests.rs`, two levels above. A checker that stopped at the
    immediate parent would report a live test as dark — and a false positive on
    a gate like this one gets the gate disabled, so the walk goes all the way up
    to the crate root. It is still only a couple of file reads per candidate.
    """
    path = Path(rel)
    stem = path.stem
    decl = re.compile(_MOD_RE % re.escape(stem), re.M)
    # crates/<crate>/… — stop at the crate directory.
    parts = path.parts
    floor = 2 if len(parts) > 2 and parts[0] == "crates" else 0

    parent = path.parent
    while len(parent.parts) > floor:
        for cand in ((parent / "mod.rs").as_posix(), parent.with_suffix(".rs").as_posix()):
            if not tree.exists(cand) or cand == rel:
                continue
            text = tree.read(cand)
            if decl.search(text):
                return True
            # `#[path = "…"]` can point at this file under any module name.
            for target in re.findall(r'#\[path\s*=\s*"([^"]+)"\]', text):
                if (Path(cand).parent / target).as_posix() == rel:
                    return True
        parent = parent.parent
    return False


# ---------------------------------------------------------------------------
# Mechanisms — every place in this repo where a test file must be ENUMERATED
# rather than globbed. A glob-driven suite cannot go dark and is not listed
# here; `--list` names those too, so the reader can see what was considered.
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Mechanism:
    id: str
    candidates: tuple[str, ...]
    registry: str
    runner: str
    what: str
    key: Callable[[str], str]
    registered: Callable[[Tree], set[str]]
    # A floor, not a target. Raise it when the corpus grows; NEVER lower it to
    # make a run pass — a collapsed candidate set is the finding, not the
    # obstacle. (Same discipline as MIN_COMPILED in gc_root_dominance_corpus.sh.)
    min_candidates: int
    # candidate key -> why it is legitimately not registered. Justified, dated
    # to its reason, and checked for staleness.
    exclusions: dict[str, str] = field(default_factory=dict)
    # Also flag registry entries whose file is gone. Off for the Rust mechanism,
    # whose "registry" is a distributed set of `mod` declarations rather than a
    # list of paths (an unresolved `mod` is a compile error already).
    check_reverse: bool = True
    # Turn a registry entry into the repo-relative path it claims exists.
    entry_to_path: Callable[[str], str] | None = None


def _stem(rel: str) -> str:
    return Path(rel).stem


MECHANISMS: tuple[Mechanism, ...] = (
    Mechanism(
        id="gc-repsel-corpus",
        what=(
            "GC x representation-selection witnesses. Each file reproduces one "
            "stale-root or representation defect and only bites on a moving arm; "
            "unregistered, it is compiled by nothing and run by nothing."
        ),
        candidates=(
            "test-files/test_gap_gc_*.ts",
            "test-files/test_gap_repsel_*.ts",
            "test-files/test_gap_specabi_*.ts",
        ),
        registry="test-parity/gc_repsel_corpus.txt",
        runner="scripts/gc_repsel_matrix.sh (gc-stress, gc-moving-witnesses)",
        key=_stem,
        registered=lambda t: _read_hash_list(t, "test-parity/gc_repsel_corpus.txt"),
        entry_to_path=lambda e: "test-files/%s.ts" % e,
        min_candidates=45,
    ),
    Mechanism(
        id="feature-matrix-probes",
        what=(
            "TypeScript feature probes. The committed feature matrix is "
            "generated from this manifest, so an unlisted probe is absent from "
            "the matrix as well as from the run."
        ),
        candidates=("test-features/probes/**/*.ts",),
        registry="test-features/feature_matrix.toml",
        runner="scripts/gen_feature_matrix.py (feature-matrix.yml)",
        key=lambda rel: rel[len("test-features/") :],
        registered=lambda t: _read_toml_paths(t, "test-features/feature_matrix.toml", "path"),
        entry_to_path=lambda e: "test-features/%s" % e,
        min_candidates=20,
        exclusions={
            "probes/type_only_imports/model.ts": (
                "helper module, not a probe: imported by probes/type_only_imports/"
                "basic.ts, which IS registered. Listing it would run a file with "
                "no output of its own."
            ),
            "probes/modules/support/type-only-values.ts": (
                "helper module, not a probe: imported by probes/modules/"
                "type-only-imports.ts, which IS registered."
            ),
            "probes/dynamic_import/mod.ts": (
                "helper module, not a probe: it is the TARGET of the "
                "`import(\"./mod.ts\")` under test in probes/dynamic_import/"
                "basic.ts, which IS registered."
            ),
        },
    ),
    Mechanism(
        id="compiler-output-workloads",
        what=(
            "Compiler-output regression fixtures. Each declares IR checks and "
            "runtime budgets in the manifest; a fixture with no manifest entry "
            "has no assertions attached to it at all."
        ),
        candidates=("benchmarks/compiler_output/fixtures/**/*.ts",),
        registry="benchmarks/compiler_output/workloads.toml",
        runner="scripts/compiler_output_regression.py (compiler-output-regression)",
        key=lambda rel: rel,
        registered=lambda t: _read_toml_paths(
            t, "benchmarks/compiler_output/workloads.toml", "source"
        ),
        entry_to_path=lambda e: e,
        min_candidates=18,
        exclusions={
            "benchmarks/compiler_output/fixtures/raw_numeric_layout_smoke.ts": (
                "registered in a DIFFERENT registry, not dark: it is the "
                "`raw_numeric_layouts` workload spec in "
                "scripts/run_memory_stability_tests.sh's "
                "run_target_collector_architecture_gates. It has no IR-check or "
                "budget entry here because it is driven as a target-collector "
                "gate, not as a compiler-output workload."
            ),
            "benchmarks/compiler_output/fixtures/native_memory_fixture_project/"
            "node_modules/@perry-fixtures/native-memory-fixture/index.ts": (
                "vendored package source inside the native_memory fixture "
                "PROJECT, not a workload: it exists to be resolved through "
                "node_modules by the fixture that imports it."
            ),
        },
    ),
    Mechanism(
        id="rust-test-modules",
        what=(
            "Rust test files that cargo does NOT auto-discover. cargo builds "
            "crates/<c>/tests/<suite>.rs on its own, but a file one level deeper "
            "— a suite's module directory, or a #[cfg(test)] submodule under "
            "src/ — compiles only if a `mod` declaration names it. Without one "
            "it is not dead code, it is not code: rustc never parses it, so no "
            "warning fires."
        ),
        candidates=("crates/*/**/tests/**/*.rs",),
        registry="the `mod` declaration in the module's parent (mod.rs or <dir>.rs)",
        runner="cargo test",
        key=lambda rel: rel,
        registered=lambda t: set(),  # unused; see _rust_module_is_declared
        min_candidates=50,
        check_reverse=False,
    ),
)

# Suites that are GLOB-driven and therefore cannot have a dark file. Named so a
# reader can tell "considered and safe" from "not looked at" — an unexplained
# absence from MECHANISMS is exactly the silence this gate exists to remove.
GLOB_DRIVEN = (
    ("run_parity_tests.sh", "find test-files -maxdepth 1 -name '*.ts'; "
     "find test-parity/node-suite -name '*.ts'"),
    ("scripts/gc_root_dominance_corpus.sh", "PATTERNS globs over test-files/; "
     "a pattern that matches nothing is already loud, and MIN_COMPILED floors "
     "the corpus size"),
    ("scripts/gc_root_dominance_dep_corpus.sh", "ONE fixed entry point; the "
     "rest of test-files/gc-dep-corpus/ reaches the compiler only by being "
     "imported from it, so the registry is the import graph. The script "
     "itself asserts that every .ts in that directory produced a module — "
     "which is the check a size floor could NOT be, since ~90 modules of "
     "`zod` swamp any count a missing 40-line source would cross"),
    ("benchmarks/public_baseline.py", "glob list over benchmarks/**"),
    ("cargo test", "crates/<c>/tests/*.rs suite roots are auto-discovered "
     "targets; the deeper files are mechanism `rust-test-modules` above"),
)


# ---------------------------------------------------------------------------
# Checking.
# ---------------------------------------------------------------------------
@dataclass
class Result:
    problems: list[str] = field(default_factory=list)
    n_candidates: int = 0
    n_registered: int = 0
    n_excluded: int = 0


def evaluate(tree: Tree, m: Mechanism, exclusions: dict[str, str] | None = None) -> Result:
    exclusions = m.exclusions if exclusions is None else exclusions
    res = Result()

    paths: list[str] = []
    for pattern in m.candidates:
        paths.extend(tree.glob(pattern))
    paths = sorted(set(paths))
    if m.id == "rust-test-modules":
        # cargo auto-discovers crates/<c>/tests/<suite>.rs; only deeper files
        # need a declaration. `mod.rs` names itself.
        paths = [
            p
            for p in paths
            if not (len(Path(p).parts) == 4 and Path(p).parts[2] == "tests")
            and Path(p).name != "mod.rs"
        ]
    res.n_candidates = len(paths)

    # ★ Liveness. A gate whose subject vanished reports the same "0 problems" as
    # a gate whose subject is clean. Refuse to be that gate.
    if len(paths) < m.min_candidates:
        res.problems.append(
            "%s: candidate set COLLAPSED — %d files match %s but the floor is "
            "%d. Either the tests moved and these globs are stale (in which "
            "case this gate has been checking nothing), or the floor is wrong. "
            "Do not lower the floor to make this pass."
            % (m.id, len(paths), " ".join(m.candidates), m.min_candidates)
        )

    keys = {m.key(p): p for p in paths}

    if m.id == "rust-test-modules":
        undeclared = [
            k for k, p in sorted(keys.items()) if not _rust_module_is_declared(tree, p)
        ]
        registered_keys: set[str] = set(keys) - set(undeclared)
        dark = [k for k in undeclared if k not in exclusions]
    else:
        try:
            registered_keys = m.registered(tree)
        except MissingRegistry as exc:
            res.problems.append(
                "MISSING REGISTRY %s: mechanism %s reads it, but %s does not "
                "exist. Restore the file, or point the mechanism elsewhere."
                % (m.registry, m.id, exc.args[0])
            )
            registered_keys = set()
        dark = sorted(set(keys) - registered_keys - set(exclusions))
    res.n_registered = len(set(keys) & registered_keys)
    res.n_excluded = len(set(keys) & set(exclusions))

    for k in dark:
        res.problems.append(
            "DARK TEST %s\n      exists on disk but is not registered in %s, so "
            "%s never runs it.\n      Register it there, or add it to this "
            "script's `%s` exclusions with a reason."
            % (keys[k], m.registry, m.runner, m.id)
        )

    # A registry entry whose file is gone is the mirror-image rot: the runner
    # either skips it silently or dies on a path that no longer exists. Reuse
    # registered_keys rather than re-reading the registry — a second raw call
    # to m.registered(tree) would also re-raise MissingRegistry above.
    if m.check_reverse and m.entry_to_path is not None:
        for entry in sorted(registered_keys):
            rel = m.entry_to_path(entry)
            if not tree.exists(rel):
                res.problems.append(
                    "ROTTED ENTRY %s lists %r but %s does not exist."
                    % (m.registry, entry, rel)
                )

    # ★ A stale exclusion is a failure, not a leftover. Otherwise an excuse
    # written for one file silently covers whatever takes its name next.
    for k in sorted(exclusions):
        if k not in keys:
            res.problems.append(
                "STALE EXCLUSION %s: %r is excluded in this script but matches "
                "no file on disk. Delete the entry." % (m.id, k)
            )

    return res


def run(tree: Tree, mechanisms: Iterable[Mechanism] = MECHANISMS) -> tuple[list[str], str]:
    problems: list[str] = []
    lines: list[str] = []
    total = 0
    mechanisms = tuple(mechanisms)
    for m in mechanisms:
        res = evaluate(tree, m)
        problems.extend(res.problems)
        total += res.n_candidates
        lines.append(
            "  %-26s %4d candidates  %4d registered  %2d excluded"
            % (m.id, res.n_candidates, res.n_registered, res.n_excluded)
        )
    summary = "checked %d files against %d registries\n%s" % (
        total,
        len(mechanisms),
        "\n".join(lines),
    )
    return problems, summary


# ---------------------------------------------------------------------------
# Self-test. Runs the REAL mechanisms over the REAL tree with a virtual overlay,
# so a pass here is evidence about this repo's registries, not about a fixture.
# ---------------------------------------------------------------------------
_PLANT = {
    "gc-repsel-corpus": "test-files/test_gap_gc_selftest_planted_witness.ts",
    "feature-matrix-probes": "test-features/probes/closures/selftest-planted.ts",
    "compiler-output-workloads": "benchmarks/compiler_output/fixtures/selftest_planted.ts",
    "rust-test-modules": "crates/perry-codegen/tests/native_proof_regressions/selftest_planted.rs",
}


def _self_test(root: Path) -> int:
    failures: list[str] = []
    cases = 0

    def check(name: str, cond: bool, detail: str = "") -> None:
        nonlocal cases
        cases += 1
        if not cond:
            failures.append("%s%s" % (name, (": " + detail) if detail else ""))

    for m in MECHANISMS:
        planted = _PLANT[m.id]
        if (root / planted).exists():
            failures.append(
                "%s: the self-test's plant path %s ALREADY EXISTS in the tree; "
                "pick another so the test cannot pass by accident" % (m.id, planted)
            )
            continue

        # 1. green on the real tree.
        clean = evaluate(Tree(root), m)
        check("%s clean" % m.id, not clean.problems, "; ".join(clean.problems))

        # 2. ★ RED with one unregistered file planted — the whole point.
        red = evaluate(Tree(root, added=[planted]), m)
        check(
            "%s goes red on a planted dark file" % m.id,
            any("DARK TEST %s" % planted in p for p in red.problems),
            "planted %s, got %r" % (planted, red.problems),
        )
        check(
            "%s counts the planted file" % m.id,
            red.n_candidates == clean.n_candidates + 1,
            "%d vs %d" % (red.n_candidates, clean.n_candidates),
        )

        # 2b. excluding the same planted file, with a reason, clears it. Every
        #     mechanism documents this as the second legitimate way out — the
        #     rust-test-modules branch once computed `dark` straight from
        #     `_rust_module_is_declared` and never subtracted `exclusions`, so
        #     an excluded Rust file stayed reported as dark.
        excused = evaluate(
            Tree(root, added=[planted]), m, exclusions={m.key(planted): "self-test"}
        )
        check(
            "%s excluding the planted file clears it" % m.id,
            not any("DARK TEST %s" % planted in p for p in excused.problems),
            repr(excused.problems),
        )

        # 3. green again once it is gone — proves step 2 was the plant and not
        #    some ambient breakage.
        again = evaluate(Tree(root), m)
        check("%s green again" % m.id, not again.problems, "; ".join(again.problems))

        # 4. ★ an empty/shrunken candidate set FAILS. This is the hazard-4 arm:
        #    without it, a stale glob makes every future run vacuously green.
        all_paths: list[str] = []
        for pattern in m.candidates:
            all_paths.extend(Tree(root).glob(pattern))
        empty = evaluate(Tree(root, removed=all_paths), m)
        check(
            "%s fails on a collapsed candidate set" % m.id,
            any("candidate set COLLAPSED" in p for p in empty.problems),
            repr(empty.problems),
        )

        # 5. a stale exclusion fails.
        stale = evaluate(
            Tree(root), m, exclusions={"no/such/file.ts": "deliberately bogus"}
        )
        check(
            "%s fails on a stale exclusion" % m.id,
            any("STALE EXCLUSION" in p for p in stale.problems),
            repr(stale.problems),
        )

        # 6. a registry entry pointing at nothing fails.
        if m.check_reverse:
            body = Tree(root).read(m.registry)
            if m.id == "gc-repsel-corpus":
                body += "\ntest_gap_gc_selftest_entry_with_no_file\n"
            else:
                keyname = "source" if m.id == "compiler-output-workloads" else "path"
                body += '\n%s = "no/such/registered/file.ts"\n' % keyname
            rotted = evaluate(Tree(root, overrides={m.registry: body}), m)
            check(
                "%s fails on a rotted registry entry" % m.id,
                any("ROTTED ENTRY" in p for p in rotted.problems),
                repr(rotted.problems),
            )

        # 6b. a missing registry file (deleted or renamed) FAILS by name,
        #     instead of an unhandled FileNotFoundError crashing the script.
        if m.id != "rust-test-modules":
            missing = evaluate(Tree(root, removed=[m.registry]), m)
            check(
                "%s fails on a missing registry file" % m.id,
                any("MISSING REGISTRY" in p for p in missing.problems),
                repr(missing.problems),
            )

    # 7. ★ FALSE-POSITIVE GUARD. `resolve/tests/declaration_sidecar_tests/
    #    compile_package.rs` IS declared — by `mod compile_package;` inside an
    #    inline `mod declaration_sidecar_tests { … }` block two levels up in
    #    `resolve/tests.rs`. The first draft of this checker only looked at the
    #    immediate parent and condemned it. A gate that cries wolf gets deleted,
    #    so pin the shape.
    inline = (
        "crates/perry/src/commands/compile/resolve/tests/"
        "declaration_sidecar_tests/compile_package.rs"
    )
    if (root / inline).is_file():
        check(
            "a `mod` in an inline block two levels up still counts",
            _rust_module_is_declared(Tree(root), inline),
        )
    else:
        failures.append(
            "the inline-`mod` false-positive guard's subject %s is gone; "
            "re-point it at another live one rather than dropping the case" % inline
        )

    # 8. the path matcher does not let `*` cross a directory separator — the bug
    #    that would silently widen every mechanism's candidate set.
    check(
        "glob `*` does not cross /",
        not _fnmatch_path("test-files/fixtures/a.ts", "test-files/*.ts"),
    )
    check("glob `*` matches in-segment", _fnmatch_path("test-files/a.ts", "test-files/*.ts"))
    check(
        "glob `**` crosses /",
        _fnmatch_path("test-features/probes/x/y.ts", "test-features/probes/**/*.ts"),
    )
    check(
        "glob `**` matches zero segments",
        _fnmatch_path("test-features/probes/y.ts", "test-features/probes/**/*.ts"),
    )

    if failures:
        for f in failures:
            print("SELF-TEST FAIL: %s" % f, file=sys.stderr)
        return 1
    print("check_test_registration self-test: OK (%d cases)" % cases)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test", action="store_true", help="check the checker, then exit")
    ap.add_argument("--list", action="store_true", help="describe the scope and exit")
    args = ap.parse_args()

    if args.self_test:
        return _self_test(REPO_ROOT)

    if args.list:
        print("Registry-driven suites (a file here can go dark):\n")
        for m in MECHANISMS:
            print("  %s" % m.id)
            print("    candidates : %s" % " ".join(m.candidates))
            print("    registry   : %s" % m.registry)
            print("    runner     : %s" % m.runner)
            print("    floor      : %d files" % m.min_candidates)
            print("    %s" % m.what)
            for k, why in sorted(m.exclusions.items()):
                print("    excluded   : %s\n                 %s" % (k, why))
            print()
        print("Glob-driven suites (considered; a file here cannot go dark):\n")
        for name, how in GLOB_DRIVEN:
            print("  %-40s %s" % (name, how))
        print(
            "\nNOT covered: tests/*.sh|py|ts. Those have no registry to diff "
            "against —\n143 of 171 are referenced by nothing in the tree. That "
            "is a separate\narchaeology problem (triage or delete), not an "
            "unregistered-file problem."
        )
        return 0

    problems, summary = run(Tree(REPO_ROOT))
    if problems:
        print("TEST REGISTRATION: a test file exists that nothing runs.\n", file=sys.stderr)
        for p in problems:
            print("  - %s" % p, file=sys.stderr)
        print("\n%s" % summary, file=sys.stderr)
        print(
            "\nA new test file must be registered in its suite's registry or it "
            "will not run.\nSee docs/src/testing/test-registration.md.",
            file=sys.stderr,
        )
        return 1

    print("test registration OK: %s" % summary)
    return 0


if __name__ == "__main__":
    sys.exit(main())
