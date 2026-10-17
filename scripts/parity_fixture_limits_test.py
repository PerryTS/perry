#!/usr/bin/env python3
"""Public CLI probes for fixture containment, not native GC acceptance."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile


HELPER = Path(__file__).with_name("parity_fixture_limits.py")


def self_test():
    actual = subprocess.run(
        [sys.executable, "-B", str(HELPER), "--print-limit",
         str(HELPER.parent.parent / "test-files/test_gap_gc_protected_poll_reclaim.ts")],
        capture_output=True, text=True,
    )
    assert actual.returncode == 0 and actual.stdout == "2048\n", actual
    with tempfile.TemporaryDirectory(prefix="parity-limit-test-") as tmp:
        root = Path(tmp)
        fixture, marker = root / "fixture.ts", root / "executed"
        command = [sys.executable, "-c", f"from pathlib import Path; Path({str(marker)!r}).touch()"]

        def run(*args):
            return subprocess.run([sys.executable, "-B", str(HELPER), *args],
                                  capture_output=True, text=True, timeout=10)

        fixture.write_text("// no resource directive\n")
        result = run(str(fixture), *command)
        assert result.returncode == 0 and marker.exists(), result.stderr
        marker.unlink()
        for value in ("0", "-1", "2048;touch evil", "NaN", "2048\n// parity-rlimit-as-mb: 2048"):
            fixture.write_text("// parity-rlimit-as-mb: " + value + "\n")
            result = run(str(fixture), *command)
            assert result.returncode == 2 and not marker.exists(), result
        fixture.write_text("// parity-rlimit-as-mb: 2048\n")
        assert run("--print-limit", str(fixture)).stdout == "2048\n"
        # Probe the actual OS limit and reject a mapping larger than it,
        # without touching or committing the mapped memory.
        probe = (
            "import json, mmap, resource\n"
            "print(json.dumps(resource.getrlimit(resource.RLIMIT_AS)), flush=True)\n"
            "try: mmap.mmap(-1, 2147483649)\n"
            "except (OSError, OverflowError): pass\n"
            "else: raise SystemExit('address-space ceiling was not enforced')\n"
        )
        result = run(str(fixture), sys.executable, "-c", probe)
        if result.returncode == 125:
            assert "cannot enforce address-space limit" in result.stderr
            result = run(str(fixture), *command)
            assert result.returncode == 125 and not marker.exists(), result
            print("Address-space containment unavailable: command refused before execution")
        else:
            assert result.returncode == 0, result.stderr
            assert all(0 < limit <= 2147483648 for limit in json.loads(result.stdout))
            print("Address-space containment enforced: oversized mapping rejected")
    print("Fixture limits self-test: PASS (CLI, rejection, containment)")


if __name__ == "__main__":
    self_test()
