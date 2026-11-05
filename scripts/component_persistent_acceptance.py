#!/usr/bin/env python3
"""Compile a TypeScript host and prove concurrent persistent component calls."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["perry", "runtime-dir", "wit", "wat-encoder", "output"]:
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--moving-gc", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for name in ["host.ts", "world.wit"]:
        shutil.copyfile(root / "test-wasi/components/persistent" / name, out / name)
    heap = (root / "crates/perry-component-host/src/async_values/heap.wat").read_text()
    heap = heap.replace("(module", "(core module $heap", 1)
    fixture = (root / "crates/perry-component-host/src/async_values/task_context.component.wat").read_text()
    fixture = fixture.replace(";; HEAP", heap).replace('"clock"', '"test:driver/clock"')
    (out / "clock.component.wat").write_text(fixture)
    env = os.environ.copy()
    env.update(PERRY_RUNTIME_DIR=str(args.runtime_dir.resolve()),
               PERRY_NO_AUTO_OPTIMIZE="1", PERRY_CODEGEN_CPU_SLOTS="4")
    rows = []

    def run(name, command, extra=None):
        with (out / (name + ".log")).open("w") as log:
            try:
                status = subprocess.run([str(x) for x in command], cwd=out,
                                        env={**env, **(extra or {})}, stdout=log,
                                        stderr=subprocess.STDOUT, timeout=600).returncode
            except (OSError, subprocess.TimeoutExpired) as error:
                log.write(str(error) + "\n")
                status = 124
        rows.append(dict(phase=name, exitCode=status))
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print(f"{'PASS' if status == 0 else 'FAIL'} {name}", flush=True)
        if status:
            raise SystemExit(status)

    run("bindings", [args.wit.resolve(), out / "world.wit", out / "generated"])
    run("component", [args.wat_encoder.resolve(), out / "clock.component.wat", out / "clock.wasm"])
    marker = "PASS persistent components: overlap, reverse completion, context, reuse, GC"
    executables = []
    for moving in ([False, True] if args.moving_gc else [False]):
        name = "moving" if moving else "ordinary"
        executable = out / ("host-" + name + (".exe" if sys.platform == "win32" else ""))
        executables.append(executable)
        instruments = {key: "1" for key in [
            "PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION",
            "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_DIAG", "PERRY_GC_INSTRUMENTS"]} if moving else {}
        run("compile-" + name, [args.perry.resolve(), "compile", out / "host.ts",
                               "--no-auto-optimize", "--no-cache", "-o", executable], instruments)
        run(name, [executable, out / "clock.wasm"], instruments)
        log = (out / (name + ".log")).read_text()
        assert marker in log, "persistent acceptance subject did not finish"
        if moving:
            copied = sum(int(n) for n in re.findall(
                r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log))
            assert copied > 0 and "evacuation_ok" in log, "no verified copying collection ran"
            protected = re.findall(r"\[gc-fromspace-protect\] mode=ProtectPages retired_set=#\d+[^\n]*\bbytes_protected=([1-9]\d*)", log)
            assert protected, "no protected from-space pages were observed"
    identity = {str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in [*executables, out / "clock.wasm", out / "generated/bindings.ts"]}
    (out / "artifact-identity.json").write_text(json.dumps(identity, indent=2) + "\n")


if __name__ == "__main__":
    main()
