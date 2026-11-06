#!/usr/bin/env python3
"""Executable controls for diagnostic claim reports (release compiler/runtime)."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

FIELDS = {"marker", "module", "byte_offset", "key", "verdict", "predicate",
          "proven_receivers", "observed_receiver", "observed_member",
          "observed_callee", "copy", "compile_sha"}


def run(args, **kw):
    return subprocess.run(args, timeout=300, capture_output=True, **kw)


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--node", default="node")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    fixtures = Path(__file__).with_name("verify_facts_s0")
    native = run(["cc", "-shared", "-fPIC", str(fixtures / "invoke.c"), "-o", str(fixtures / "invoke.node")])
    assert native.returncode == 0, native.stderr
    cases = json.loads((fixtures / "cases.json").read_text())
    env = dict(os.environ, PERRY_VERIFY_FACTS="1", PERRY_FACTS_UNITS="0",
               PERRY_NO_CACHE="1", PERRY_ALLOW_PERRY_FEATURES="1")
    results = []
    for name, wrong_claim in cases.items():
        source = fixtures / (name + ".ts")
        oracle = run([args.node, "--experimental-strip-types", str(source)])
        assert oracle.returncode == 0, oracle.stderr
        for claim in ["", wrong_claim]:
            arm = name + ("-false" if claim else "-correct")
            exe = (args.out / arm).resolve()
            e = dict(env, PERRY_FACTS_SABOTAGE=claim)
            compiled = run([str(args.perry), "compile", str(source), "-o", str(exe)], env=e)
            (args.out / (arm + ".compile")).write_bytes(compiled.stdout + compiled.stderr)
            assert compiled.returncode == 0, f"{arm}: compile {compiled.returncode}"
            observed = run([str(exe)], env=e)
            (args.out / (arm + ".stdout")).write_bytes(observed.stdout)
            (args.out / (arm + ".stderr")).write_bytes(observed.stderr)
            if claim:
                assert observed.returncode == 86, f"{arm}: status {observed.returncode}, {observed.stdout!r} {observed.stderr!r}"
                assert b"BODY:" not in observed.stdout, f"{arm}: body began before report"
                lines = observed.stderr.splitlines()
                assert len(lines) == 1, (arm, lines)
                report = json.loads(lines[0])
                assert FIELDS <= report.keys(), (arm, report)
                assert report["marker"] == "PERRY_FALSE_PROOF", report
                assert report["key"] == "<entry>" and report["observed_receiver"] is None, report
                assert report["copy"] and report["compile_sha"], report
            else:
                assert observed.returncode == 0 and observed.stdout == oracle.stdout, (arm, observed)
            results.append({"name": arm, "status": observed.returncode, "passed": True})
            (args.out / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    print(f"{len(results)} executable controls passed")


if __name__ == "__main__":
    main()
