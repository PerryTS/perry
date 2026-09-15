#!/usr/bin/env python3
"""Compile fresh P0 subjects, compare Node output, and assert real OS waits."""
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
COMPILER = Path(os.environ.get("PERRY_BIN", TARGET / "perry-dev/perry"))
RUNTIME = Path(os.environ.get("PERRY_RUNTIME_DIR", COMPILER.parent)).resolve()
assert subprocess.check_output(["node", "--version"], text=True).strip() == "v26.5.1"
for archive in ["libperry_runtime.a", "libperry_stdlib.a"]:
    assert (RUNTIME / archive).is_file(), f"build static wrappers first: {archive}"
env = dict(os.environ, PERRY_RUNTIME_DIR=str(RUNTIME), PERRY_NO_AUTO_OPTIMIZE="1")
subjects = {
    "deadline_05": 1, "deadline_2": 1, "deadline_10": 1,
    "idle": 1, "promise_churn": 1, "interval": 3,
}
with tempfile.TemporaryDirectory(prefix="perry-p0-") as out:
    for subject, expiries in subjects.items():
        source = ROOT / f"test-files/test_turnloop_p0_{subject}.ts"
        binary = Path(out) / subject
        subprocess.run([str(COMPILER), str(source), "--no-cache", "-o", str(binary)],
                       env=env, check=True, capture_output=True, text=True, timeout=180)
        oracle = subprocess.check_output(["node", "--experimental-strip-types", str(source)],
                                         text=True, timeout=15)
        result = subprocess.run([str(binary)], env=dict(env, PERRY_LOOP_STATS="1"),
                                capture_output=True, text=True, check=True, timeout=15)
        assert result.stdout == oracle, (subject, result.stdout, oracle)
        stats = re.findall(
            r"\[perry-loop\] driver=turnloop turns=(\d+) os_waits=(\d+) "
            r"zero_event_waits=(\d+) native_ticks=(\d+)", result.stderr)
        assert len(stats) == 1, (subject, "driver subject did not run", result.stderr)
        turns, waits, zeros, native = map(int, stats[0])
        assert 0 < turns <= 2 * expiries, (subject, stats)
        assert 0 < waits <= 2 * expiries, (subject, stats)
        assert zeros <= expiries, (subject, stats)
        assert native == 0, (subject, "quiet path entered Tokio", stats)
        print(f"PASS {subject}: turns={turns} os_waits={waits} "
              f"zero_event_waits={zeros} native_ticks={native}", flush=True)
