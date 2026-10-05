#!/usr/bin/env python3
"""Read the loop_polls fixture's GC parity-env without interpreting shell code.

The parity harness uses whitespace-separated KEY=VALUE assignments. Restrict
the matrix extension to GC witness settings: metadata cannot change an OFF
arm, compiler selection, library providers, pressure, or the shipped control.
"""

from __future__ import annotations

import argparse
from decimal import Decimal
from pathlib import Path
import re
import sys


DIRECTIVE = re.compile(r"^\s*//\s*parity-env:\s*(.*)$", re.MULTILINE)
ON_KEYS = {
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_FORCE_EVACUATE",
    "PERRY_GC_VERIFY_EVACUATION",
    "PERRY_GC_BUDGETED_OLD_RECLAIM",
}
UINT_KEYS = {
    "PERRY_GC_SCHEDULE_SEED",
    "PERRY_GC_SCHEDULE_ALLOC_KB",
    "PERRY_GC_PROTECT_FROMSPACE_DEPTH",
    "PERRY_GC_MAJOR_PACING_FLOOR_MB",
    "PERRY_GC_MAJOR_PACING_GROWTH",
}


def parse(text: str) -> str:
    lines = DIRECTIVE.findall(text)
    if not lines:
        return ""
    if len(lines) != 1 or not lines[0].strip():
        raise ValueError("require exactly one nonempty parity-env directive")
    values: dict[str, str] = {}
    for assignment in lines[0].split():
        key, sep, value = assignment.partition("=")
        if not sep or key in values:
            raise ValueError(f"invalid or duplicate assignment {assignment!r}")
        if key in ON_KEYS:
            valid = value == "1"
        elif key in UINT_KEYS:
            valid = bool(re.fullmatch(r"[0-9]{1,20}", value)) and int(value) <= 2**64 - 1
            if key in ("PERRY_GC_PROTECT_FROMSPACE_DEPTH", "PERRY_GC_MAJOR_PACING_GROWTH"):
                valid = valid and int(value) > 0
        elif key == "PERRY_GC_SCHEDULE_RATE":
            valid = bool(re.fullmatch(r"(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)", value))
            valid = valid and Decimal(0) <= Decimal(value) <= Decimal(1)
        elif key == "PERRY_GC_PROTECT_FROMSPACE":
            valid = value in ("1", "poison")
        else:
            raise ValueError(f"unsupported GC witness setting {key!r}")
        if not valid:
            raise ValueError(f"invalid GC witness setting {assignment!r}")
        values[key] = value
    if any(key in values for key in ("PERRY_GC_SCHEDULE_RATE", "PERRY_GC_SCHEDULE_ALLOC_KB")):
        if "PERRY_GC_SCHEDULE_SEED" not in values:
            raise ValueError("schedule rate/allocation gating requires a seed")
    if "PERRY_GC_PROTECT_FROMSPACE_DEPTH" in values and "PERRY_GC_PROTECT_FROMSPACE" not in values:
        raise ValueError("from-space depth requires protection")
    return " ".join(f"{key}={value}" for key, value in values.items())


def fixture_env(arm: str, text: str) -> str:
    # This early return is deliberate: even malformed metadata must not alter
    # compilation or execution of a shipped/default/OFF control.
    return parse(text) if arm == "loop_polls" else ""


def self_test() -> None:
    witness = "// parity-env: PERRY_GC_SCHEDULE_SEED=10061 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1\n"
    assert fixture_env("loop_polls", witness) == witness.split(": ", 1)[1].strip()
    for arm in ("shipped_default", "default", "safepoint_minor", "gen_gc_off", "wb_off", "rep_ptr_shape_off"):
        assert fixture_env(arm, witness) == ""
        assert fixture_env(arm, "// parity-env: PATH=/tmp/evil") == ""
    budgeted = "PERRY_GC_BUDGETED_OLD_RECLAIM=1 PERRY_GC_MAJOR_PACING_FLOOR_MB=1 PERRY_GC_MAJOR_PACING_GROWTH=1"
    assert parse("// parity-env: " + budgeted + "\n") == budgeted
    assert parse("// no metadata\n") == ""
    assert parse("  // parity-env: PERRY_GC_MOVING_LOOP_POLLS=1\n") == "PERRY_GC_MOVING_LOOP_POLLS=1"
    invalid = (
        "PATH=/tmp/evil", "PERRY_NO_AUTO_OPTIMIZE=1", "PERRY_RUNTIME_DIR=/tmp/foreign",
        "PERRY_GC_MOVING_LOOP_POLLS=0", "PERRY_GC_SCHEDULE_SEED=$(touch /tmp/evil)",
        "PERRY_GC_SCHEDULE_SEED=1;echo", "PERRY_GC_SCHEDULE_SEED=`id`",
        "PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_SEED=2",
        "PERRY_GC_SCHEDULE_SEED=18446744073709551616", "PERRY_GC_SCHEDULE_SEED=-1",
        "PERRY_GC_SCHEDULE_RATE=1", "PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=NaN",
        "PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1.01", "PERRY_GC_PROTECT_FROMSPACE_DEPTH=0",
        "PERRY_GC_PROTECT_FROMSPACE_DEPTH=4", "PERRY_GC_BUDGETED_OLD_RECLAIM=0",
        "PERRY_GC_MAJOR_PACING_GROWTH=0", "PERRY_GC_MAJOR_PACING_FLOOR_MB=1.5",
        "", "PERRY_GC_FORCE_EVACUATE=1\n// parity-env: PERRY_GC_VERIFY_EVACUATION=1",
    )
    for settings in invalid:
        try:
            parse("// parity-env: " + settings)
        except ValueError:
            pass
        else:
            raise AssertionError(f"accepted unsafe/malformed metadata: {settings!r}")
    print("GC matrix fixture env self-test: PASS (syntax, injection rejection, OFF/control isolation)")
    from gc_matrix_fixture_env_test import self_test as routing_self_test

    routing_self_test()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture", type=Path, nargs="?")
    parser.add_argument("--arm", default="loop_polls")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.fixture is None:
        parser.error("fixture is required")
    try:
        print(fixture_env(args.arm, args.fixture.read_text(encoding="utf-8")))
    except (OSError, ValueError) as exc:
        print(f"{args.fixture}: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
