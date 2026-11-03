#!/usr/bin/env python3
"""Exercise canonical future/stream readers in a compiled TypeScript host."""
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
    for name in ["host.ts", "world.wit", "stream.wit", "nested.wit", "compound.wit", "paused.wit"]:
        shutil.copyfile(root / "test-wasi/components/readers" / name, out / name)
    fixture = (root / "crates/perry-component-host/src/async_values/producer.component.wat").read_text()
    fixtures = {"future": fixture, "stream": fixture
                .replace("(type $future (future", "(type $future (stream")
                .replace("canon future.", "canon stream.")
                .replace("(func $write (param i32 i32)", "(func $write (param i32 i32 i32)")
                .replace("(call $write (global.get $writer) (i32.const 0))",
                         "(call $write (global.get $writer) (i32.const 0) (i32.const 1))")
                .replace("(i32.ne (local.get 0) (i32.const 5))", "(i32.ne (local.get 0) (i32.const 3))")}
    for kind in ["nested", "compound", "paused"]:
        fixtures[kind] = (root / f"crates/perry-component-host/src/async_values/{kind}-producer.component.wat").read_text()
    for kind, source in fixtures.items():
        (out / f"{kind}.component.wat").write_text(source)
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

    for kind in fixtures:
        world = "world.wit" if kind == "future" else kind + ".wit"
        generated = "generated" if kind == "future" else "generated-" + kind
        run("bindings-" + kind, [args.wit.resolve(), out / world, out / generated])
        run("component-" + kind, [args.wat_encoder.resolve(), out / f"{kind}.component.wat", out / f"{kind}.wasm"])
    marker = "PASS component readers: ownership, temporary await, context, GC cancellation"
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
        run(name, [executable, *[out / f"{kind}.wasm" for kind in fixtures]], instruments)
        log = (out / (name + ".log")).read_text()
        assert marker in log, "reader acceptance subject did not finish"
        if moving:
            copied = sum(int(n) for n in re.findall(
                r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log))
            assert copied > 0 and "evacuation_ok" in log, "no verified copying collection ran"
            protected = re.findall(r"\[gc-fromspace-protect\] mode=ProtectPages retired_set=#\d+[^\n]*\bbytes_protected=([1-9]\d*)", log)
            assert protected, "no protected from-space pages were observed"
    identity = {str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in [*executables, *out.glob("*.wasm"), *out.glob("generated*/bindings.ts")]}
    (out / "artifact-identity.json").write_text(json.dumps(identity, indent=2) + "\n")


if __name__ == "__main__":
    main()
