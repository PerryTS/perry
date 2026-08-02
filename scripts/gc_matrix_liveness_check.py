#!/usr/bin/env python3
"""Assert every GC-matrix arm actually exercised the collector path it claims.

WHY THIS EXISTS (#7255)
-----------------------
`scripts/gc_repsel_matrix.sh` classifies a cell as UNVER when the output matched
the oracle but the arm was measurably inert. UNVER is deliberately "not green".
It is also, deliberately, **not red** — the matrix's exit status counts only
FAIL. So an arm that stops biting corpus-wide produces a table of yellow cells
and exit 0, and nothing in CI says a word.

That is not hypothetical. #7024 measured the `default` arm at copy-minor 0/22,
fixed it to 12/22, and the script's header recorded the fix in its strongest
formatting. #7161 then flipped `PERRY_GC_MOVING_LOOP_POLLS` default-OFF as a
stopgap for #7154, which took the same arm back to **0/49** — and the header
still read `12/22`, because it was a hand-maintained number. Four of the six
PR-gating arms were inert for weeks. Two open crash reports (#6982, #7018) could
not be judged in either direction, because both crash *inside* a copying minor
and the arms that were supposed to run one ran none.

This is CLAUDE.md's fourth way a gate can be unable to fail, in its purest form:
the gate ran, and its subject never did. The rule it mechanises is CLAUDE.md's
own — *"a gate must assert its subject was live"*.

WHAT IT ASSERTS
---------------
For every arm in a matrix JSON report whose `requires` is not `none`:

  * **inert + unregistered → FAIL.** The arm satisfied its own liveness
    requirement on ZERO cells and nothing in the tree says that is expected.
  * **live + registered → FAIL.** `test-parity/gc_matrix_inert_arms.txt` lists
    an arm as known-inert, and it just bit. The blocker named in the entry has
    lifted; delete the entry (and, usually, promote the arm back into
    `PR_ARMS`). A registry that can only be added to rots exactly the way the
    header did.

The registry is the *only* way to be inert without failing, every entry must
cite an issue number, and each entry's own text says what must become true for
it to be deleted. There is no hand-maintained count anywhere: the numbers are
derived from the run and printed on every invocation.

USAGE
-----
    python3 scripts/gc_matrix_liveness_check.py <report.json>
    python3 scripts/gc_matrix_liveness_check.py --check-registry   # from `lint`
    python3 scripts/gc_matrix_liveness_check.py --self-test        # from `lint`
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
REGISTRY = REPO_ROOT / "test-parity" / "gc_matrix_inert_arms.txt"
MATRIX = REPO_ROOT / "scripts" / "gc_repsel_matrix.sh"

# requires= value -> (human name, cell counter key(s) summed to decide "it bit")
REQUIREMENTS = {
    "scavenge": ("copying young-gen minor", ("scavenged",)),
    "move": ("any relocation", ("evacuated", "scavenged")),
    "collect": ("any GC cycle", ("cycles",)),
    "none": ("nothing (explicit control)", ()),
}


class Violation(Exception):
    pass


def parse_registry(text: str) -> dict:
    """`arm | #issue | reason` per line; `#` in column 1 starts a comment."""
    entries = {}
    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = [p.strip() for p in line.split("|")]
        if len(parts) != 3:
            raise Violation(
                "%s:%d: expected `arm | #issue | reason`, got %r" % (REGISTRY.name, lineno, raw)
            )
        arm, issue, reason = parts
        if arm in entries:
            raise Violation("%s:%d: duplicate entry for arm %r" % (REGISTRY.name, lineno, arm))
        if not re.fullmatch(r"#\d+", issue):
            raise Violation(
                "%s:%d: the second field must be the blocking issue as `#NNNN`, got %r. "
                "An entry with no issue is an arm nobody is going to turn back on."
                % (REGISTRY.name, lineno, issue)
            )
        if not reason:
            raise Violation("%s:%d: empty reason" % (REGISTRY.name, lineno))
        entries[arm] = (issue, reason)
    return entries


def matrix_arms(text: str) -> dict:
    """{arm id: requires} as declared in `scripts/gc_repsel_matrix.sh`'s ARMS array."""
    body = re.search(r"^ARMS=\((.*?)^\)$", text, re.S | re.M)
    if not body:
        raise Violation("could not find the ARMS=( ... ) array in %s" % MATRIX.name)
    arms = {}
    for line in re.finditer(r'^"([^"]*)"$', body.group(1), re.M):
        fields = line.group(1).split("|")
        if len(fields) < 4:
            raise Violation("malformed arm record: %r" % line.group(1)[:60])
        arms[fields[0]] = fields[3]
    if not arms:
        raise Violation("the ARMS=( ... ) array in %s parsed to nothing" % MATRIX.name)
    return arms


def matrix_pr_arms(text: str) -> list:
    """The `PR_ARMS=` subset — what `gc-stress` actually runs on a pull request."""
    match = re.search(r'^PR_ARMS="([^"]*)"', text, re.M)
    if not match:
        raise Violation("could not find PR_ARMS= in %s" % MATRIX.name)
    return [a for a in match.group(1).split(",") if a]


def arm_liveness(cells: list, arm_id: str, requires: str) -> tuple:
    """(live_cell_count, total_cell_count, summed counter) for one arm."""
    keys = REQUIREMENTS[requires][1]
    live = total = summed = 0
    for cell in cells:
        if cell.get("arm") != arm_id:
            continue
        total += 1
        value = sum(int(cell.get(k, 0) or 0) for k in keys)
        summed += value
        if value > 0:
            live += 1
    return live, total, summed


def check_report(report: dict, registry: dict, out=sys.stdout) -> list:
    """Returns a list of violation strings; prints the derived liveness table."""
    arms = report.get("arms") or []
    cells = report.get("cells") or []
    if not arms:
        return ["the report declares no arms — the matrix never ran an arm"]

    violations = []
    out.write("liveness gate (#7255): every arm must have exercised what it claims\n")
    for arm in arms:
        arm_id, requires = arm["id"], arm["requires"]
        if requires not in REQUIREMENTS:
            violations.append(
                "arm %s declares requires=%s, which is not one of %s"
                % (arm_id, requires, "/".join(sorted(REQUIREMENTS)))
            )
            continue
        live, total, summed = arm_liveness(cells, arm_id, requires)
        registered = registry.get(arm_id)
        if requires == "none":
            verdict = "n/a"
        elif live == 0 and registered is None:
            verdict = "INERT"
            violations.append(
                "arm %s declares requires=%s (%s) and satisfied it on 0/%d cells. "
                "The arm is inert: every green cell under it is green for the wrong "
                "reason. Either fix the arm, re-declare its requires=, or register it "
                "in %s with the issue that blocks it."
                % (arm_id, requires, REQUIREMENTS[requires][0], total, REGISTRY.name)
            )
        elif live == 0:
            verdict = "inert (known %s)" % registered[0]
        elif registered is not None:
            verdict = "STALE-REGISTRY"
            violations.append(
                "arm %s is listed as known-inert in %s (%s) but satisfied requires=%s "
                "on %d/%d cells. The blocker has lifted — delete the entry, and put the "
                "arm back where it belongs (usually PR_ARMS)."
                % (arm_id, REGISTRY.name, registered[0], requires, live, total)
            )
        else:
            verdict = "live"
        out.write(
            "  %-24s requires=%-9s live %2d/%-3d  counter=%-12d %s\n"
            % (arm_id, requires, live, total, summed, verdict)
        )

    exercised = {a["id"] for a in arms}
    for arm_id, (issue, _) in sorted(registry.items()):
        if arm_id not in exercised:
            out.write("  %-24s (registered inert %s; not exercised by this run)\n" % (arm_id, issue))
    return violations


def check_registry(out=sys.stdout) -> list:
    """Static checks that need no run: names resolve, and the gate is wired in."""
    violations = []
    try:
        registry = parse_registry(REGISTRY.read_text()) if REGISTRY.exists() else {}
    except Violation as exc:
        return [str(exc)]

    matrix_text = MATRIX.read_text()
    try:
        known = matrix_arms(matrix_text)
        pr_arms = matrix_pr_arms(matrix_text)
    except Violation as exc:
        return [str(exc)]

    for arm_id, (issue, _) in sorted(registry.items()):
        if arm_id not in known:
            violations.append(
                "%s lists arm %r (%s), which no longer exists in %s. A registry entry "
                "for a deleted arm hides nothing and outlives its reason."
                % (REGISTRY.name, arm_id, issue, MATRIX.name)
            )

    for arm_id in pr_arms:
        if arm_id not in known:
            violations.append("PR_ARMS names %r, which is not in the ARMS table" % arm_id)

    # THE STRUCTURAL HALF OF #7255, and the reason this check is worth having in
    # `lint` at all: the run-time gate can be satisfied by registering every
    # inert arm, which would leave a PR subset that cannot relocate anything and
    # still exits 0. So the subset must retain at least one arm that claims to
    # relocate AND is not on the known-inert list. This is the property the
    # matrix header used to assert in prose ("THIS SUBSET CAN NOW REPRODUCE THE
    # RELOCATING-MINOR DEFECT CLASS") with a number that went stale.
    relocating = [
        arm
        for arm in pr_arms
        if known.get(arm) in ("scavenge", "move") and arm not in registry
    ]
    if not relocating:
        violations.append(
            "PR_ARMS (%s) contains no arm that both claims to relocate "
            "(requires=scavenge/move) and is absent from %s. On a pull request the "
            "matrix would then be unable to observe a raw reference held across a "
            "relocating collection at all — the #6993 class (#6951, #6972, #6982, "
            "#6991, #6992), which is the hole #7255 was filed about. Registering "
            "arms as known-inert is not a way to buy silence: put a live relocating "
            "arm back in the subset." % (",".join(pr_arms), REGISTRY.name)
        )

    out.write(
        "registry: %d known-inert arm(s), %d arms declared in %s\n"
        % (len(registry), len(known), MATRIX.name)
    )
    out.write(
        "PR subset: %s\n  relocating and not known-inert: %s\n"
        % (",".join(pr_arms), ",".join(relocating) or "NONE")
    )
    for arm_id, (issue, reason) in sorted(registry.items()):
        out.write("  %-24s %-8s %s\n" % (arm_id, issue, reason[:100]))
    return violations


def _report(arms, cells):
    return {"arms": arms, "cells": cells}


def _cell(arm, **kw):
    base = {"test": kw.pop("test", "t"), "arm": arm, "result": kw.pop("result", "PASS")}
    base.update({"cycles": 0, "evacuated": 0, "scavenged": 0})
    base.update(kw)
    return base


def self_test() -> int:
    import io

    failures = []

    def expect(name, violations, want):
        got = len(violations)
        if (got > 0) != want:
            failures.append("%s: wanted %s, got %r" % (name, "a violation" if want else "clean", violations))

    sink = io.StringIO()

    # An arm that never scavenged and is not registered is the #7255 defect.
    expect(
        "inert scavenge arm is rejected",
        check_report(
            _report([{"id": "default", "requires": "scavenge"}], [_cell("default", cycles=3)]),
            {},
            sink,
        ),
        True,
    )
    # ...and is accepted once registered, because the blocker is named.
    expect(
        "registered inert arm is accepted",
        check_report(
            _report([{"id": "default", "requires": "scavenge"}], [_cell("default", cycles=3)]),
            {"default": ("#7161", "stopgap")},
            sink,
        ),
        False,
    )
    # A registry entry that has outlived its cause must be loud, not silent.
    expect(
        "stale registry entry is rejected",
        check_report(
            _report([{"id": "default", "requires": "scavenge"}], [_cell("default", scavenged=17)]),
            {"default": ("#7161", "stopgap")},
            sink,
        ),
        True,
    )
    expect(
        "live arm passes",
        check_report(
            _report([{"id": "evac", "requires": "scavenge"}], [_cell("evac", scavenged=17)]),
            {},
            sink,
        ),
        False,
    )
    # #7025: `move` may be satisfied by the C4b mark-sweep evacuation alone, but
    # `scavenge` may NOT — that distinction is the whole point of two counters.
    expect(
        "move is satisfied by evacuated-only",
        check_report(
            _report([{"id": "m", "requires": "move"}], [_cell("m", evacuated=9)]),
            {},
            sink,
        ),
        False,
    )
    expect(
        "scavenge is NOT satisfied by evacuated-only",
        check_report(
            _report([{"id": "s", "requires": "scavenge"}], [_cell("s", evacuated=9)]),
            {},
            sink,
        ),
        True,
    )
    expect(
        "requires=none is never a violation",
        check_report(
            _report([{"id": "ctl", "requires": "none"}], [_cell("ctl")]),
            {},
            sink,
        ),
        False,
    )
    expect(
        "collect arm needs a cycle",
        check_report(
            _report([{"id": "c", "requires": "collect"}], [_cell("c")]),
            {},
            sink,
        ),
        True,
    )
    expect(
        "an unknown requires= value is rejected",
        check_report(
            _report([{"id": "x", "requires": "sometimes"}], [_cell("x")]),
            {},
            sink,
        ),
        True,
    )
    expect(
        "an empty report is rejected",
        check_report(_report([], []), {}, sink),
        True,
    )
    # A liveness verdict must be per-arm, not pooled: one live arm must not
    # vouch for an inert one (the shape #7025 describes for counters).
    expect(
        "one live arm does not vouch for an inert one",
        check_report(
            _report(
                [{"id": "live", "requires": "scavenge"}, {"id": "dead", "requires": "scavenge"}],
                [_cell("live", scavenged=5), _cell("dead", cycles=5)],
            ),
            {},
            sink,
        ),
        True,
    )

    for text, want_ok in [
        ("arm | #123 | reason", True),
        ("arm | 123 | reason", False),      # issue number must be an issue number
        ("arm | #123", False),              # three fields
        ("arm | #123 | a\narm | #124 | b", False),  # duplicate
        ("arm | #123 |", False),            # empty reason
        ("# just a comment\n\n", True),
    ]:
        try:
            parse_registry(text)
            ok = True
        except Violation:
            ok = False
        if ok != want_ok:
            failures.append("registry parse of %r: wanted ok=%s" % (text, want_ok))

    # The parsers must survive the real file, or the registry check is vacuous —
    # a regex that silently matches nothing is its own way for a gate to pass.
    try:
        text = MATRIX.read_text()
        arms = matrix_arms(text)
        pr_arms = matrix_pr_arms(text)
        if len(arms) < 5 or arms.get("default") != "scavenge":
            failures.append("matrix_arms parsed %r from the real matrix script" % (arms,))
        if "default" not in pr_arms:
            failures.append("matrix_pr_arms parsed %r from the real matrix script" % (pr_arms,))
        if set(pr_arms) - set(arms):
            failures.append("PR_ARMS names arms that are not in ARMS: %r" % (set(pr_arms) - set(arms),))
    except Violation as exc:
        failures.append("parsing the real matrix script failed: %s" % exc)

    for failure in failures:
        print("SELF-TEST FAIL: %s" % failure, file=sys.stderr)
    print("self-test: %d checks, %d failures" % (12 + 6 + 3, len(failures)))
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("report", nargs="?", help="matrix JSON written by --json")
    parser.add_argument("--check-registry", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--report-only",
        action="store_true",
        help="print the verdict but always exit 0 (local exploration; never in CI)",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    if args.check_registry:
        violations = check_registry()
    elif args.report:
        try:
            registry = parse_registry(REGISTRY.read_text()) if REGISTRY.exists() else {}
        except Violation as exc:
            print("ERROR: %s" % exc, file=sys.stderr)
            return 1
        with open(args.report) as handle:
            report = json.load(handle)
        violations = check_report(report, registry)
    else:
        parser.error("pass a report path, --check-registry, or --self-test")

    for violation in violations:
        print("LIVENESS GATE: %s" % violation, file=sys.stderr)
    if violations and not args.report_only:
        print(
            "\n%d arm(s) did not exercise the collector path they claim. See #7255: an "
            "arm that never bit makes every green cell under it green for the wrong "
            "reason." % len(violations),
            file=sys.stderr,
        )
        return 1
    if violations:
        print("(--report-only: %d violation(s) not enforced)" % len(violations), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
