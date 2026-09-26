#!/usr/bin/env python3
"""turnloop P0 loop-statistics gate.

Compiles each `test-files/test_turnloop_p0_*.ts` probe with a prebuilt Perry
compiler and archives, checks its stdout against the pinned Node oracle, and
reads the `PERRY_LOOP_STATS=1` exit line to assert that the primary agent
reached its deadlines by waiting, not spinning:

  * turns <= 2 per deadline expiry, zero-event OS waits <= 1 per expiry
    (DESIGN §10 rule 4a);
  * at least one turn and one OS wait where a real wait is due (the subject
    ran — a green run with zero turns would prove nothing);
  * no turn errors.

Usage:
  scripts/turnloop_p0_loop_stats.py [--perry target/perry-dev/perry]
      [--runtime-dir DIR]
"""

import argparse
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# probe -> (deadline expiries, minimum turns that must have happened).
# deadline_05 and submillisecond_remainder may find their timer already due at
# the first park (Perry fires a sub-millisecond delay at once; scheduler delay
# on a loaded host can eat a 0.4 ms remainder), so they assert no-spin only.
# The exact sub-millisecond wait is proven deterministically by the Rust test
# `event_pump::agent_loop::tests::sub_and_whole_millisecond_deadlines_wait_without_spinning`.
PROBES = {
    "deadline_05": (1, 0),
    "deadline_2": (1, 1),
    "deadline_10": (1, 1),
    "submillisecond_remainder": (1, 0),
    "interval": (3, 1),
    "promise_churn": (1, 1),
    "idle": (1, 1),
}

UNPARKED = "[perry-loop] driver=turnloop parked=0"
STATS = re.compile(
    r"\[perry-loop\] driver=turnloop turns=(\d+) os_waits=(\d+) "
    r"zero_event_waits=(\d+) turn_errors=(\d+)"
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--perry", default=str(ROOT / "target/perry-dev/perry"))
    parser.add_argument("--runtime-dir")
    args = parser.parse_args()

    perry = Path(args.perry).resolve()
    runtime_dir = Path(args.runtime_dir).resolve() if args.runtime_dir else perry.parent
    for archive in ("libperry_runtime.a", "libperry_stdlib.a"):
        if not (runtime_dir / archive).is_file():
            print(f"missing {runtime_dir / archive}: build the -static wrappers first", file=sys.stderr)
            return 2
    node_version = subprocess.check_output(["node", "--version"], text=True).strip()
    pinned = "v" + (ROOT / ".node-version").read_text().strip()
    if node_version != pinned:
        print(f"node {node_version} is not the pinned oracle {pinned}", file=sys.stderr)
        return 2

    env = dict(os.environ, PERRY_RUNTIME_DIR=str(runtime_dir), PERRY_NO_AUTO_OPTIMIZE="1")
    env.pop("PERRY_LOOP_STATS", None)
    failures = []
    with tempfile.TemporaryDirectory(prefix="perry-turnloop-p0-") as out:
        for probe, (expiries, min_turns) in PROBES.items():
            source = ROOT / f"test-files/test_turnloop_p0_{probe}.ts"
            binary = Path(out) / probe
            compiled = subprocess.run(
                [str(perry), str(source), "--no-cache", "-o", str(binary)],
                env=env, capture_output=True, text=True, timeout=600,
            )
            if compiled.returncode != 0:
                failures.append(f"{probe}: compile failed\n{compiled.stdout}{compiled.stderr}")
                continue
            oracle = subprocess.run(
                ["node", "--experimental-strip-types", str(source)],
                capture_output=True, text=True, timeout=60,
            ).stdout
            started = time.monotonic()
            run = subprocess.run(
                [str(binary)], env=dict(env, PERRY_LOOP_STATS="1"),
                capture_output=True, text=True, timeout=60,
            )
            wall_ms = (time.monotonic() - started) * 1000.0
            problems = []
            if run.returncode != 0:
                problems.append(f"exit {run.returncode}")
            if run.stdout != oracle:
                problems.append(f"stdout {run.stdout!r} != node {oracle!r}")
            found = STATS.findall(run.stderr)
            if not found and run.stderr.count(UNPARKED) == 1:
                found = [("0", "0", "0", "0")]
            if len(found) != 1:
                problems.append(f"expected one turnloop stats line, stderr={run.stderr!r}")
                line = f"{probe}: no stats"
            else:
                turns, os_waits, zero, errors = map(int, found[0])
                line = (f"{probe}: turns={turns} os_waits={os_waits} zero_event_waits={zero} "
                        f"turn_errors={errors} wall_ms={wall_ms:.1f}")
                if turns > 2 * expiries:
                    problems.append(f"{turns} turns for {expiries} expiries (spin)")
                if zero > expiries:
                    problems.append(f"{zero} zero-event waits for {expiries} expiries")
                if turns < min_turns or os_waits < min_turns:
                    problems.append("the precise park never waited (subject did not run)")
                if errors:
                    problems.append("unexpected turn errors")
            print(("PASS " if not problems else "FAIL ") + line, flush=True)
            if problems:
                failures.append(f"{probe}: " + "; ".join(problems))
    for failure in failures:
        print(failure, file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
