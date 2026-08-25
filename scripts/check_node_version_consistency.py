#!/usr/bin/env python3
"""Hold every Node version in the tree to a source a machine can re-derive.

Node is a *correctness input* here, not a toolchain detail (CLAUDE.md,
"TypeScript Parity Status"): when the oracle cannot run a gap test, node exits
non-zero, the harness classifies it `node_fail`, and the test is silently
dropped from the gate instead of going red. CI sat on Node 22 while the suite
grew Node 24/26 features and hid 14 tests that way (#6364).

The defence #6367 chose was a single `.node-version` pin every workflow reads
through `setup-node`'s `node-version-file`. That defence has leaked twice, in
the two shapes this checker owns one rule for each of:

1. **Prose drifting off the file.**  CLAUDE.md's stated oracle version drifted
   from `.node-version` (#7599) and had to be corrected by hand. A restatement
   of a version is now a marker compared against the file it claims to quote,
   so the next drift is a red build rather than a reader's problem.

2. **A new workflow born outside the sweep.**  `npm-launcher.yml` was created
   (#6350) on the same day #6367 converted every *existing* workflow, and kept
   that day's ambient `"22.23.1"` literal for a month — not a decision, an
   omission. Every `node-version:` literal in `.github/workflows/` must now be
   registered here with a reason, so the next one cannot arrive silently.

WHAT THIS DOES NOT CATCH (per CLAUDE.md's gate rules, said plainly)
------------------------------------------------------------------
Whether the pinned version is the *right* one. Nothing here re-runs the gap
suite under two oracles or measures a benchmark; raising a pin stays the
deliberate act CLAUDE.md describes. This checker only guarantees that every
place naming a Node version either derives it from the authoritative file or
carries a written, still-accurate exemption.

It also does not police measurement *outputs*
(`benchmarks/results/public-node-bun-v1.json`,
`benchmarks/honest_bench/results/metadata.json`). Those are already tied to
`benchmarks/public-baseline-config.json` by `public_baseline.validate_public`,
and a second implementation of that policy would be a place for the two to
disagree.

Usage:
    python3 scripts/check_node_version_consistency.py             # check
    python3 scripts/check_node_version_consistency.py --self-test # check me
    python3 scripts/check_node_version_consistency.py --list      # describe
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Callable

REPO = Path(__file__).resolve().parent.parent

# The two independent Node pins this repo maintains, and what each one is for.
# They are equal today and are NOT required to be: CLAUDE.md documents the
# compat-matrix oracle as "independent of the .node-version gap-suite oracle",
# so asserting equality would encode a coupling the project deliberately does
# not have.
GAP_ORACLE = ".node-version"
MATRIX_ORACLE = "external-tools.json"

SEMVER = re.compile(r"^\d+\.\d+\.\d+$")

# ---------------------------------------------------------------------------
# Rule 1 -- restatements of a pin must equal the file they claim to quote.
#
# Each entry is (path, regex with exactly one capture group, source key). The
# regex MUST match at least once: a rewording that loses the marker fails here
# rather than quietly ceasing to be checked, which is the whole point.
# ---------------------------------------------------------------------------
MIRRORS: tuple[tuple[str, str, str], ...] = (
    (
        "CLAUDE.md",
        r"The oracle is Node `(\d+\.\d+\.\d+)`",
        "gap",
    ),
    (
        "CLAUDE.md",
        r"`tools\.node\.version`\s*[—-]\s*currently \*\*(\d+\.\d+\.\d+)\*\*",
        "matrix",
    ),
    (
        "llms.txt",
        r"pinned Node oracle \((\d+\.\d+\.\d+)\)",
        "matrix",
    ),
    (
        "test-parity/node-compat-matrix.baseline.json",
        r'"nodeVersion":\s*"(\d+\.\d+\.\d+)"',
        "matrix",
    ),
)

# Vacuity floors. A scan that silently matched nothing is the failure mode this
# repo has paid for most often (CLAUDE.md, "Four ways a gate can be unable to
# fail", item 4), so both populations have a floor that must itself be able to
# fail -- see self_test().
MIN_MIRRORS = 4
MIN_WORKFLOW_PINS = 20


# ---------------------------------------------------------------------------
# Rule 2/3 -- every literal Node version outside the authoritative files is a
# registered exemption with a reason, an expected value, and a statement about
# whether its major tracks the oracle.
#
# `value` is asserted against the tree. An entry that no longer matches FAILS,
# so a future bump or baseline regeneration must update or delete its own
# exemption instead of leaving a fossil behind (CLAUDE.md's allowlist rule:
# "an entry that matches nothing FAILS, so a fix must delete its entry").
# ---------------------------------------------------------------------------
class Exemption:
    def __init__(
        self,
        path: str,
        value: str,
        major_tracks_oracle: bool,
        reason: str,
        locator: Callable[[str], list[str]] | None = None,
    ) -> None:
        self.path = path
        self.value = value
        self.major_tracks_oracle = major_tracks_oracle
        self.reason = reason
        self.locator = locator


def _workflow_pins(text: str) -> list[str]:
    return [value for _, key, value in _node_version_keys(text) if key == "node-version"]


EXEMPTIONS: tuple[Exemption, ...] = (
    Exemption(
        path=".github/workflows/node-core-subset.yml",
        value="${{ steps.node_core_version.outputs.version }}",
        major_tracks_oracle=False,
        reason=(
            "Runs Node's OWN test corpus, which must be executed by the Node line "
            "it was taken from. Derived from test-compat/node-core/pinned-version.txt "
            "so the coupling is explicit instead of looking like drift (#6367)."
        ),
        locator=_workflow_pins,
    ),
    Exemption(
        path="test-compat/node-core/pinned-version.txt",
        value="v22.x",
        major_tracks_oracle=False,
        reason=(
            "The ref of the vendored node-core test corpus. Bumping it means "
            "re-vendoring that corpus, not chasing the gap-suite oracle."
        ),
    ),
    Exemption(
        path=".github/workflows/release-packages.yml",
        value="26",
        major_tracks_oracle=True,
        reason=(
            "npm *publishing* toolchain (OIDC registry auth), never a test oracle. "
            "Pinned to a major literal rather than node-version-file so a gap-suite "
            "oracle bump cannot move the runtime that publishes releases."
        ),
        locator=_workflow_pins,
    ),
    Exemption(
        path=".github/workflows/release-hono-server.yml",
        value="26",
        major_tracks_oracle=True,
        reason=(
            "npm *publishing* toolchain for @perryts/hono-server, never a test "
            "oracle. Major literal for the same reason as release-packages.yml."
        ),
        locator=_workflow_pins,
    ),
    Exemption(
        path="benchmarks/public-baseline-config.json",
        value="v22.23.1",
        major_tracks_oracle=False,
        reason=(
            "PUBLISHED PERFORMANCE BASELINE -- KNOWN STALE, AND NOT EDITABLE ALONE. "
            "This file is in public_baseline.HARNESS_PATHS, so its bytes feed the "
            "artifact's harness_fingerprint; and validate_public() separately "
            "compares this value against benchmarks/results/public-node-bun-v1.json's "
            "recorded runtime version. Changing it without regenerating turns the "
            "REQUIRED lint job red (measured: ci_public_baseline_check.py exits 2). "
            "The pin and the measurement are therefore atomic by design (#7282/#7958). "
            "To clear this exemption: on the quiet M1 mini (perry@perry-macos.local, "
            "the host recorded in the artifact), set toolchains.node to the oracle and "
            "run ./benchmarks/run_public_baseline.sh -- ~2h, and it enforces "
            "<=25% CPU active for 60 consecutive seconds before each of five "
            "components. Then delete this entry; leaving it stale fails this check."
        ),
        locator=lambda text: [json.loads(text)["toolchains"]["node"]],
    ),
)


# ---------------------------------------------------------------------------
# Scanning
# ---------------------------------------------------------------------------
_NODE_KEY = re.compile(r"^\s*(node-version|node-version-file):\s*(\S.*?)\s*$")


def _node_version_keys(text: str) -> list[tuple[int, str, str]]:
    """Every setup-node version input in a workflow, as (line, key, value).

    Comment lines are skipped so prose like `# Single source of truth:
    .node-version` cannot be read as a pin.
    """
    out: list[tuple[int, str, str]] = []
    for lineno, raw in enumerate(text.splitlines(), 1):
        if raw.lstrip().startswith("#"):
            continue
        m = _NODE_KEY.match(raw)
        if m:
            out.append((lineno, m.group(1), m.group(2).strip().strip('"').strip("'")))
    return out


def _read(path: str) -> str:
    return (REPO / path).read_text(encoding="utf-8")


def _sources() -> dict[str, str]:
    gap = _read(GAP_ORACLE).strip()
    matrix = json.loads(_read(MATRIX_ORACLE))["tools"]["node"]["version"]
    return {"gap": gap, "matrix": matrix}


def check(
    sources: dict[str, str] | None = None,
    mirrors: tuple[tuple[str, str, str], ...] = MIRRORS,
    exemptions: tuple[Exemption, ...] = EXEMPTIONS,
    reader: Callable[[str], str] = _read,
    min_mirrors: int = MIN_MIRRORS,
    min_workflow_pins: int = MIN_WORKFLOW_PINS,
    workflows: list[Path] | None = None,
) -> list[str]:
    failures: list[str] = []
    if sources is None:
        sources = _sources()

    for key, value in sources.items():
        if not SEMVER.match(value):
            failures.append(f"{key} oracle is not a bare X.Y.Z version: {value!r}")

    # Rule 1: restatements.
    mirror_hits = 0
    for path, pattern, source in mirrors:
        found = re.findall(pattern, reader(path))
        if not found:
            failures.append(
                f"{path}: marker {pattern!r} matched nothing -- the prose was "
                f"reworded out from under this check; restore the marker or update it"
            )
            continue
        for value in found:
            mirror_hits += 1
            if value != sources[source]:
                failures.append(
                    f"{path}: states Node {value}, but the {source} pin is "
                    f"{sources[source]}"
                )
    if mirror_hits < min_mirrors:
        failures.append(
            f"only {mirror_hits} version restatements found, floor is {min_mirrors} "
            f"-- the scan is matching less than it used to"
        )

    # Rule 2: every workflow pin is derived or registered.
    registered = {e.path for e in exemptions}
    if workflows is None:
        workflows = sorted((REPO / ".github/workflows").glob("*.yml"))
    workflow_pins = 0
    for wf in workflows:
        rel = wf.relative_to(REPO).as_posix()
        for lineno, key, value in _node_version_keys(wf.read_text(encoding="utf-8")):
            workflow_pins += 1
            if key == "node-version-file":
                if value != GAP_ORACLE:
                    failures.append(
                        f"{rel}:{lineno}: node-version-file is {value!r}, "
                        f"must be {GAP_ORACLE!r}"
                    )
                continue
            if rel not in registered:
                failures.append(
                    f"{rel}:{lineno}: literal node-version {value!r} is not registered. "
                    f"Use `node-version-file: {GAP_ORACLE}`, or add an exemption with a "
                    f"reason to {Path(__file__).name}"
                )
    if workflow_pins < min_workflow_pins:
        failures.append(
            f"only {workflow_pins} setup-node inputs found across workflows, floor is "
            f"{min_workflow_pins} -- the workflow scan is matching less than it used to"
        )

    # Rule 3: every exemption still describes the tree, and declares its major.
    for e in exemptions:
        try:
            text = reader(e.path)
        except (OSError, FileNotFoundError):
            failures.append(f"{e.path}: exemption names a file that does not exist")
            continue
        values = e.locator(text) if e.locator else [text.strip()]
        if e.value not in values:
            failures.append(
                f"{e.path}: exemption expects {e.value!r} but the file has "
                f"{values!r} -- update this exemption or delete it"
            )
            continue
        if e.major_tracks_oracle:
            want = sources["gap"].split(".")[0]
            got = e.value.lstrip("v").split(".")[0]
            if got != want:
                failures.append(
                    f"{e.path}: pinned to Node {e.value}, but it declares "
                    f"major_tracks_oracle and {GAP_ORACLE} is major {want}"
                )
        elif not e.reason.strip():
            failures.append(f"{e.path}: exemption off the oracle major needs a reason")

    return failures


# ---------------------------------------------------------------------------
# Self-test: every rule above must be able to fail.
# ---------------------------------------------------------------------------
def self_test() -> int:
    failures: list[str] = []
    real = _sources()

    def fake_reader(overrides: dict[str, str]) -> Callable[[str], str]:
        return lambda path: overrides.get(path, _read(path))

    # Rule 1a: a drifted restatement is caught.
    drifted = _read("CLAUDE.md").replace(
        f"The oracle is Node `{real['gap']}`", "The oracle is Node `22.23.1`", 1
    )
    if not any(
        "states Node 22.23.1" in f
        for f in check(reader=fake_reader({"CLAUDE.md": drifted}))
    ):
        failures.append("rule 1 does not catch a drifted version restatement")

    # Rule 1b: a marker deleted by rewording is caught, not silently skipped.
    reworded = _read("llms.txt").replace(
        f"pinned Node oracle ({real['matrix']})", "pinned Node oracle", 1
    )
    if not any(
        "matched nothing" in f for f in check(reader=fake_reader({"llms.txt": reworded}))
    ):
        failures.append("rule 1 does not catch a marker removed by rewording")

    # Rule 1c: the mirror vacuity floor must itself be able to fail.
    if not any("floor is" in f for f in check(min_mirrors=99)):
        failures.append("MIN_MIRRORS floor cannot fail")

    # Rule 2a: an unregistered literal pin in a workflow is caught. Written to a
    # temp file inside .github/workflows so the real glob picks it up, then
    # removed -- exercising the discovery path, not just the parser.
    rogue = REPO / ".github/workflows" / "zz__self_test_rogue.yml"
    rogue.write_text(
        "jobs:\n  x:\n    steps:\n      - uses: actions/setup-node@v7\n"
        '        with:\n          node-version: "18"\n',
        encoding="utf-8",
    )
    try:
        if not any("is not registered" in f for f in check()):
            failures.append("rule 2 does not catch an unregistered literal pin")
    finally:
        rogue.unlink()

    # Rule 2b: a node-version-file pointing somewhere else is caught.
    astray = REPO / ".github/workflows" / "zz__self_test_astray.yml"
    astray.write_text(
        "jobs:\n  x:\n    steps:\n      - uses: actions/setup-node@v7\n"
        "        with:\n          node-version-file: .nvmrc\n",
        encoding="utf-8",
    )
    try:
        if not any("must be '.node-version'" in f for f in check()):
            failures.append("rule 2 does not catch a node-version-file pointing elsewhere")
    finally:
        astray.unlink()

    # Rule 2c: a comment mentioning .node-version must not read as a pin.
    if _node_version_keys("      # node-version: 18 (historical)\n"):
        failures.append("workflow scanner reads commented-out pins as real")

    # Rule 2d: the workflow vacuity floor must itself be able to fail.
    if not any("workflow scan is matching less" in f for f in check(min_workflow_pins=9999)):
        failures.append("MIN_WORKFLOW_PINS floor cannot fail")

    # Rule 3a: a stale exemption (tree moved on) is caught.
    stale = Exemption(
        path="test-compat/node-core/pinned-version.txt",
        value="v20.x",
        major_tracks_oracle=False,
        reason="self-test",
    )
    if not any(
        "update this exemption or delete it" in f for f in check(exemptions=(stale,))
    ):
        failures.append("rule 3 does not catch an exemption that stopped matching")

    # Rule 3b: an exemption naming a deleted file is caught.
    ghost = Exemption(
        path="does/not/exist.txt", value="x", major_tracks_oracle=False, reason="s"
    )
    if not any("does not exist" in f for f in check(exemptions=(ghost,))):
        failures.append("rule 3 does not catch an exemption naming a deleted file")

    # Rule 3c: a "tracks the oracle" exemption left behind on an old major fails.
    behind = Exemption(
        path=".github/workflows/release-packages.yml",
        value="26",
        major_tracks_oracle=True,
        reason="self-test",
        locator=_workflow_pins,
    )
    bumped = dict(real, gap="28.0.0")
    if not any(
        "declares major_tracks_oracle" in f
        for f in check(sources=bumped, exemptions=(behind,))
    ):
        failures.append("rule 3 does not catch a tracking exemption stuck on an old major")

    for failure in failures:
        print(f"check_node_version_consistency self-test: {failure}", file=sys.stderr)
    if failures:
        return 1
    print("check_node_version_consistency self-test: OK")
    return 0


def describe() -> int:
    sources = _sources()
    print(f"gap-suite oracle    {GAP_ORACLE}: {sources['gap']}")
    print(f"compat-matrix pin   {MATRIX_ORACLE} tools.node.version: {sources['matrix']}")
    print("\nrestatements checked against a pin:")
    for path, pattern, source in MIRRORS:
        print(f"  {path}  [{source}]  {pattern}")
    print("\nregistered exemptions (each must still match the tree):")
    for e in EXEMPTIONS:
        tracks = "major tracks oracle" if e.major_tracks_oracle else "off-oracle"
        print(f"  {e.path} = {e.value}  ({tracks})")
        print(f"      {e.reason}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.list:
        return describe()
    failures = check()
    for failure in failures:
        print(f"node version consistency: {failure}", file=sys.stderr)
    if failures:
        print(
            f"\n{len(failures)} problem(s). The authoritative pin is {GAP_ORACLE}; "
            f"run --list to see every checked restatement and exemption.",
            file=sys.stderr,
        )
        return 1
    print("node version consistency: OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
