#!/usr/bin/env python3
"""Fail the build when two reserved class ids collide.

WHY THIS IS A SCRIPT AND NOT A UNIT TEST
----------------------------------------
#7576 shipped `iterator_class_ids_are_pairwise_distinct`, a Rust test that
hand-enumerates seven iterator class ids and asserts they differ. That test is
good and stays. It also could not have caught #7587, which was a collision
between `JSX_NODE_CLASS_ID` and `RAW_JSON_CLASS_ID` — a different family, so
not in the list.

That is the whole problem with an enumerated list: it only covers the constants
somebody remembered to add, and the failure mode being guarded against is
*forgetting that a constant exists*. A gate whose coverage depends on the same
attention the bug depends on is not a gate. So this scans the source instead —
every new constant is covered the moment it is written, with no list to update.

WHAT THE BUG LOOKS LIKE
-----------------------
Both known collisions were introduced the same way: an author copied a
neighbouring constant, bumped the low byte, and wrote a comment describing
where it sits relative to its neighbour — without checking whether the slot was
already taken. Both comments were accurate about the neighbour and wrong about
the slot.

Neither was inert:

* #7576 — `ITERATOR_HELPER_CLASS_ID` and `STRING_ITERATOR_CLASS_ID` were both
  `0xFFFF_0009`. Every dispatch tower matches the String arm first, so EVERY
  helper object dispatched as a String iterator and the entire TC39
  iterator-helpers surface returned `undefined`. Silently for `Iterator.from`.

* #7587 — `JSX_NODE_CLASS_ID` and `RAW_JSON_CLASS_ID` were both `0xFFFF_00A0`.
  `value/to_string.rs` discriminates on `class_id` alone and returns field 0,
  and both types keep their payload in field 0, so `String(JSON.rawJSON(x))`
  took the JSX path and *looked correct by coincidence of layout*.

The second is the more instructive one: a collision can produce right-looking
output for as long as the two types happen to agree structurally, and break the
day either layout changes.

USAGE
-----
    python3 scripts/class_id_collisions.py           # check, exit 1 on collision
    python3 scripts/class_id_collisions.py --list    # print the id map

Exit status is non-zero on a collision, or if the scan finds implausibly few
constants — a scan that silently matches nothing reports "no collisions" and
means nothing. That floor is the same discipline `gc_root_dominance_corpus.sh`
applies with MIN_COMPILED.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
from collections import defaultdict

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Scan every workspace crate, not just perry-runtime: a class id minted in
# perry-stdlib collides with a perry-runtime one just as thoroughly, and the
# whole point of this file is not to depend on someone remembering the scope.
CRATES = ROOT / "crates"

# `pub const FOO_CLASS_ID: u32 = 0xFFFF_0009;` and the pub(crate)/plain forms.
# The value may carry `_` separators and any case.
DECL = re.compile(
    r"^\s*(?:pub\s*(?:\(\s*[^)]*\s*\)\s*)?)?const\s+"
    r"(?P<name>[A-Z0-9_]*CLASS_ID)\s*:\s*u32\s*=\s*"
    r"(?P<value>0[xX][0-9A-Fa-f_]+)\s*;",
    re.MULTILINE,
)

# A scan that matches nothing is indistinguishable from a clean scan. This is
# the floor, not a target: raise it when the set genuinely grows, never lower
# it to make a run pass.
MIN_CONSTANTS = 12


def collect() -> dict[int, list[tuple[str, str]]]:
    """value -> [(constant name, repo-relative file:line), ...]"""
    found: dict[int, list[tuple[str, str]]] = defaultdict(list)
    for path in sorted(CRATES.rglob("*.rs")):
        if "/target/" in str(path):
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if "CLASS_ID" not in text:
            continue
        for m in DECL.finditer(text):
            value = int(m.group("value").replace("_", ""), 16)
            line = text.count("\n", 0, m.start()) + 1
            rel = path.relative_to(ROOT)
            found[value].append((m.group("name"), f"{rel}:{line}"))
    return found


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true", help="print the id map and exit")
    args = ap.parse_args()

    found = collect()
    total = sum(len(v) for v in found.values())

    if args.list:
        for value in sorted(found):
            for name, where in found[value]:
                print(f"  {value:#010x}  {name:<44} {where}")
        print(f"\n{total} reserved class ids across {len(found)} distinct values")
        return 0

    if total < MIN_CONSTANTS:
        print(
            f"::error::found only {total} class-id constants, expected at least "
            f"{MIN_CONSTANTS}. The scan pattern has probably gone stale — a scan "
            f"that matches nothing reports 'no collisions' and means nothing.",
            file=sys.stderr,
        )
        return 2

    # Two DIFFERENT names on one value is the bug. The SAME name on one value
    # is a deliberate cross-crate mirror — `perry-ext-events` restates the
    # runtime's `ABORT_SIGNAL_CLASS_ID` so it can recognise runtime AbortSignal
    # objects, which is correct and must not be reported. Distinguishing them
    # by name is what makes this gate usable rather than permanently red.
    collisions = {
        v: entries
        for v, entries in found.items()
        if len({name for name, _ in entries}) > 1
    }

    # The mirror has its own failure mode, in the opposite direction: one name
    # carrying DIFFERENT values in different crates. That is a mirror that has
    # drifted, and it is worse than a collision because each crate is
    # self-consistent — the runtime stamps one id and the ext crate tests for
    # another, so the type simply stops being recognised across the boundary
    # with nothing anywhere looking wrong.
    by_name: dict[str, set[int]] = defaultdict(set)
    where_by_name: dict[str, list[str]] = defaultdict(list)
    for value, entries in found.items():
        for name, where in entries:
            by_name[name].add(value)
            where_by_name[name].append(f"{value:#010x}  {where}")
    drifted = {n: v for n, v in by_name.items() if len(v) > 1}

    if not collisions and not drifted:
        mirrors = sum(1 for e in found.values() if len(e) > 1)
        note = f", {mirrors} intentional cross-crate mirror(s)" if mirrors else ""
        print(f"Class-id audit passed ({total} reserved ids{note}).")
        return 0

    if collisions:
        print("Class-id COLLISION — two DIFFERENT ids share a value:\n", file=sys.stderr)
        for value, entries in sorted(collisions.items()):
            print(f"  {value:#010x}", file=sys.stderr)
            for name, where in entries:
                print(f"      {name:<44} {where}", file=sys.stderr)
            print(file=sys.stderr)
        print(
            "Every dispatch tower matches these in a FIXED ORDER, so the later arm\n"
            "becomes unreachable and its whole method surface dies silently (#7576),\n"
            "and any site that discriminates on class_id alone will cross-match —\n"
            "which can look CORRECT for as long as the two types agree on field\n"
            "layout, and break the day either changes (#7587).\n\n"
            "Give one of them an unused value. `--list` prints the map.\n",
            file=sys.stderr,
        )

    if drifted:
        print("Class-id MIRROR DRIFT — one name, different values:\n", file=sys.stderr)
        for name in sorted(drifted):
            print(f"  {name}", file=sys.stderr)
            for line in sorted(where_by_name[name]):
                print(f"      {line}", file=sys.stderr)
            print(file=sys.stderr)
        print(
            "A cross-crate mirror that has drifted is worse than a collision:\n"
            "each crate is internally consistent, so nothing looks wrong, but the\n"
            "type stops being recognised across the boundary. Make them equal.",
            file=sys.stderr,
        )

    return 1


if __name__ == "__main__":
    raise SystemExit(main())
