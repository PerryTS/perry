#!/usr/bin/env python3
"""Run a real Rust WASI command inside matching native Perry archives.

Use the normal build-host concurrency gate. component_check.sh supplies the
compiler, component-host archive, runtime/stdlib archives and GC sidecars.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", required=True, type=Path)
    parser.add_argument("--runtime-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--moving-gc", action="store_true")
    parser.add_argument("--library-component", type=Path)
    parser.add_argument("--library-bindings", type=Path)
    args = parser.parse_args()
    if bool(args.library_component) != bool(args.library_bindings):
        parser.error("--library-component and --library-bindings must be supplied together")
    root = Path(__file__).resolve().parent.parent
    fixture = root / "test-wasi/components/command"
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    work = output / "fixture"
    shutil.copytree(fixture, work, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("target"))
    env = os.environ.copy()
    env.update(PERRY_RUNTIME_DIR=str(args.runtime_dir.resolve()),
               CARGO_TARGET_DIR=str(output / "rust-target"), CARGO_INCREMENTAL="0",
               CARGO_BUILD_JOBS=env.get("CARGO_BUILD_JOBS", "4"),
               PERRY_CODEGEN_CPU_SLOTS="4", PERRY_NO_AUTO_OPTIMIZE="1")
    # A host environment entry must not leak into the default command grant.
    env["COMMAND_AMBIENT_SECRET"] = "must-not-reach-guest"
    results = []
    (output / "results.json").write_text("[]\n")

    def run(name, command, *, execute=False, moving=False, integration=False):
        start = time.monotonic()
        log = output / (name + ".log")
        try:
            with log.open("w") as stream:
                process = subprocess.run([str(arg) for arg in command],
                    cwd=root if command[0] == "cargo" else work,
                    env=env, stdout=stream, stderr=subprocess.STDOUT,
                    timeout=600 if not execute else 180)
            status = process.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            with log.open("a") as stream:
                stream.write(str(error) + "\n")
            status = 124
        if status == 0 and execute:
            contents = log.read_text()
            required = [
                "PASS WASI commands: returns, exit statuses, traps, isolated capabilities, fuel, host survival",
                "GUEST stdout alpha", "GUEST stderr alpha",
            ]
            if integration:
                required = ["PASS command/library integration: nested command exits, traps, callback GC, library survival"]
            failures = [f"expected exactly one {text!r}" for text in required
                        if contents.count(text) != 1]
            if "UNEXPECTED ambient" in contents or "GUEST stdout beta" in contents or "GUEST stderr beta" in contents:
                failures.append("ungranted command stdio reached the host")
            if moving:
                copied = sum(int(n) for n in re.findall(
                    r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", contents))
                if copied == 0 or "[gc-fromspace-protect]" not in contents:
                    failures.append("moving GC/quarantine witness missing")
            if failures:
                status = 1
                with log.open("a") as stream:
                    stream.write("\nFAIL: " + "; ".join(failures) + "\n")
        results.append({"name": name, "exitCode": status,
                        "seconds": round(time.monotonic() - start, 3), "log": str(log)})
        (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(f"{'PASS' if status == 0 else 'FAIL'} {name}: {log}", flush=True)
        if status:
            raise SystemExit(status if status > 0 else 1)

    run("rust-command", ["cargo", "build", "--locked", "--release", "--target", "wasm32-wasip2",
                         "--manifest-path", work / "rust/Cargo.toml"])
    component = output / "rust-target/wasm32-wasip2/release/perry-component-command-fixture.wasm"
    if args.library_bindings:
        shutil.copytree(args.library_bindings.resolve(), work / "generated", dirs_exist_ok=True)
    for moving in ([False, True] if args.moving_gc else [False]):
        name = "command-moving" if moving else "command"
        grants = output / (name + "-grants")
        if grants.exists():
            shutil.rmtree(grants)
        for token in ["alpha", "beta"]:
            directory = grants / token
            directory.mkdir(parents=True)
            (directory / "input.txt").write_text(token)
        (grants / "outside.txt").write_text("must-not-escape-preopen")
        if moving:
            for knob in ["PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION",
                         "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_DIAG", "PERRY_GC_INSTRUMENTS"]:
                env[knob] = "1"
        executable = output / (name + (".exe" if sys.platform == "win32" else ""))
        run(name + "-compile", [args.perry.resolve(), "compile", work / "host.ts",
                                "--no-auto-optimize", "--no-cache", "-o", executable])
        run(name + "-run", [executable, component, grants], execute=True, moving=moving)
        if args.library_component:
            executable = output / (name + "-library" + (".exe" if sys.platform == "win32" else ""))
            run(name + "-library-compile", [args.perry.resolve(), "compile", work / "library-host.ts",
                "--no-auto-optimize", "--no-cache", "-o", executable])
            run(name + "-library-run", [executable, component, args.library_component.resolve()],
                execute=True, moving=moving, integration=True)


if __name__ == "__main__":
    main()
