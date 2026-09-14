#!/usr/bin/env python3
"""Temporarily omit each registered production restore; require its real-throw test to fail."""
import argparse
import json
import os
from pathlib import Path
import re
import resource
import subprocess


def build(root, log):
    command = ["cargo", "test", "-p", "perry-runtime", "--lib", "--no-run", "--message-format=json"]
    result = subprocess.run(command, cwd=root, text=True, capture_output=True)
    log.with_suffix(".stderr").write_text(result.stderr)
    log.write_text(result.stdout)
    result.check_returncode()
    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")]
    return next(a["executable"] for a in artifacts if a.get("reason") == "compiler-artifact" and a.get("executable") and a["target"]["name"] == "perry_runtime")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    root = Path(__file__).resolve().parents[2]
    path = root / "crates/perry-runtime/src/exception/savepoints.rs"
    original = path.read_text()
    needle = "$($(#[$attr])* $restore(self.$name);)*"
    assert original.count(needle) == 1
    names = re.findall(r"^    (\w+): [^\n]+,\n    capture: ", original, flags=re.MULTILINE)
    captures = re.findall(r"^    capture: ", original, flags=re.MULTILINE)
    assert names and len(names) == len(set(names)) == len(captures), names
    mutant = original.replace(needle, '''$($(#[$attr])* {
                    if std::env::var("PERRY_TEST_SKIP_CATCH_RESTORE").as_deref() != Ok(stringify!($name)) {
                        $restore(self.$name);
                    }
                })*''')
    results = []
    try:
        path.write_text(mutant)
        executable = build(root, args.output / "mutant-build.jsonl")
        for name in names:
            test = f"exception::savepoints::restore_tests::{name}"
            result = subprocess.run([executable, "--exact", test, "--test-threads=1", "--nocapture"], env=dict(os.environ, PERRY_TEST_SKIP_CATCH_RESTORE=name), text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (args.output / (name + ".log")).write_text(result.stdout)
            # A filter matching zero tests, or an unrelated startup failure,
            # must not count as a detected missing restore.
            detected = result.returncode != 0 and "inner catch must preserve its enclosing scope" in result.stdout
            results.append({"restore": name, "test": test, "exit_code": result.returncode, "detected": detected})
            print(name, result.returncode, detected, flush=True)
        (args.output / "fault-results.json").write_text(json.dumps(results, indent=2) + "\n")
        assert all(r["detected"] for r in results), results
    finally:
        path.write_text(original)
        # The source AND executable must be restored; a later green run must
        # not accidentally exercise an environment-controlled mutant.
        executable = build(root, args.output / "restored-build.jsonl")
        with (args.output / "restored-tests.log").open("w") as stream:
            subprocess.run([executable, "exception::", "--test-threads=1"], stdout=stream, stderr=subprocess.STDOUT, check=True)


if __name__ == "__main__":
    main()
