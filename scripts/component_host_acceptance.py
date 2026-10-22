#!/usr/bin/env python3
"""Build a Rust component and run it from the matching native Perry toolchain.

Build perry, perry-{runtime,stdlib}-static, perry-component-host, perry-wit,
and the perry-wit componentize example first. Generate runtime GC-effect
sidecars after building the archives. This script builds the small Rust
fixture itself; run it through the build host's normal concurrency gate.
"""
import argparse
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", required=True, type=Path)
    parser.add_argument("--runtime-dir", required=True, type=Path)
    parser.add_argument("--wit", required=True, type=Path)
    parser.add_argument("--componentize", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--moving-gc", action="store_true",
                        help="also require a real moving collection with matching _instr archives")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    fixture = root / "test-wasi/components/transform"
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    work = output / "transform"
    work.mkdir(exist_ok=True)
    for name in ["world.wit", "host.ts"]:
        shutil.copyfile(fixture / name, work / name)
    shutil.copytree(fixture / "rust", work / "rust", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("target"))
    env = os.environ.copy()
    env["PERRY_RUNTIME_DIR"] = str(args.runtime_dir.resolve())
    env["CARGO_TARGET_DIR"] = str(output / "rust-target")
    env["CARGO_INCREMENTAL"] = "0"
    env["CARGO_BUILD_JOBS"] = env.get("CARGO_BUILD_JOBS", "4")
    env["PERRY_CODEGEN_CPU_SLOTS"] = "4"
    # Use the exact supplied archives, not an automatically rebuilt feature set.
    env["PERRY_NO_AUTO_OPTIMIZE"] = "1"
    normal_env = env.copy()
    results = []
    (output / "results.json").write_text("[]\n")

    def run(name, command, expected=None, timeout=600, moving=False):
        start = time.monotonic()
        log = output / (name + ".log")
        try:
            with log.open("w") as stream:
                completed = subprocess.run([str(x) for x in command], cwd=root if command[0] == "cargo" else work,
                    env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=timeout)
            status = completed.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            with log.open("a") as stream:
                stream.write(str(error) + "\n")
            status = 124
        if status == 0 and expected is not None:
            if expected not in log.read_text():
                status = 1
        if status == 0 and moving:
            contents = log.read_text()
            moved = sum(int(value) for value in re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", contents))
            if moved == 0 or "[gc-fromspace-protect]" not in contents:
                with log.open("a") as stream:
                    stream.write("FAIL: moving-GC/quarantine witness was not observed\n")
                status = 1
        results.append({"name": name, "exitCode": status,
                        "seconds": round(time.monotonic() - start, 3), "log": str(log)})
        (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(f"{'PASS' if status == 0 else 'FAIL'} {name}: {log}", flush=True)
        if status:
            raise SystemExit(status if status > 0 else 1)

    run("rust-guest", ["cargo", "build", "--locked", "--release", "--target", "wasm32-unknown-unknown",
        "--manifest-path", work / "rust/Cargo.toml"])
    core = output / "rust-target/wasm32-unknown-unknown/release/perry_component_transform_fixture.wasm"
    component = work / "transform.component.wasm"
    run("componentize", [args.componentize.resolve(), core, component])
    run("bindings", [args.wit.resolve(), work / "world.wit", work / "generated", "plugin"])
    executable = work / ("host.exe" if sys.platform == "win32" else "host")
    run("compile-host", [args.perry.resolve(), "compile", work / "host.ts",
        "--no-auto-optimize", "--no-cache", "-o", executable])
    run("run-host", [executable, component],
        expected="PASS Rust component: 51 calls, typed results, callback GC, disposal", timeout=60)
    if args.moving_gc:
        for knob in ["PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION",
                     "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_DIAG", "PERRY_GC_INSTRUMENTS"]:
            env[knob] = "1"
        instrumented = executable.with_name("host-moving" + executable.suffix)
        run("compile-host-moving", [args.perry.resolve(), "compile", work / "host.ts",
            "--no-auto-optimize", "--no-cache", "-o", instrumented])
        run("run-host-moving", [instrumented, component],
            expected="PASS Rust component: 51 calls, typed results, callback GC, disposal",
            timeout=60, moving=True)
    env = normal_env.copy()
    resources = output / "resources"
    shutil.copytree(fixture.parent / "resources", resources, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("target"))
    run("resources-guest", ["cargo", "build", "--locked", "--release", "--target", "wasm32-unknown-unknown",
        "--manifest-path", resources / "rust/Cargo.toml"])
    core = output / "rust-target/wasm32-unknown-unknown/release/perry_component_resource_fixture.wasm"
    component = resources / "resources.component.wasm"
    run("resources-componentize", [args.componentize.resolve(), core, component])
    run("resources-bindings", [args.wit.resolve(), resources / "world.wit", resources / "generated", "plugin"])
    marker = "PASS Rust resources: ownership, borrow, transfer, exactly-once disposal, destructor traps"
    executable = resources / ("host.exe" if sys.platform == "win32" else "host")
    run("resources-compile", [args.perry.resolve(), "compile", resources / "host.ts",
        "--no-auto-optimize", "--no-cache", "-o", executable])
    run("resources-run", [executable, component], expected=marker, timeout=60)
    if args.moving_gc:
        for knob in ["PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION",
                     "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_DIAG", "PERRY_GC_INSTRUMENTS"]:
            env[knob] = "1"
        instrumented = executable.with_name("host-moving" + executable.suffix)
        run("resources-compile-moving", [args.perry.resolve(), "compile", resources / "host.ts",
            "--no-auto-optimize", "--no-cache", "-o", instrumented])
        run("resources-run-moving", [instrumented, component], expected=marker,
            timeout=60, moving=True)
    env = normal_env.copy()
    network = output / "network"
    shutil.copytree(fixture.parent / "network", network, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("target"))
    run("network-guest", ["cargo", "build", "--locked", "--release", "--target", "wasm32-unknown-unknown",
        "--manifest-path", network / "rust/Cargo.toml"])
    core = output / "rust-target/wasm32-unknown-unknown/release/perry_component_network_fixture.wasm"
    component = network / "network.component.wasm"
    run("network-componentize", [args.componentize.resolve(), core, component])
    run("network-bindings", [args.wit.resolve(), network, network / "generated", "plugin"])
    marker = "PASS component networking: TCP/UDP traffic, exact grants, DNS, isolation"
    for moving in ([False, True] if args.moving_gc else [False]):
        env = normal_env.copy()
        suffix = "-moving" if moving else ""
        if moving:
            for knob in ["PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION",
                         "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_DIAG", "PERRY_GC_INSTRUMENTS"]:
                env[knob] = "1"
        executable = network / ("host" + suffix + (".exe" if sys.platform == "win32" else ""))
        run("network-compile" + suffix, [args.perry.resolve(), "compile", network / "host.ts",
            "--no-auto-optimize", "--no-cache", "-o", executable])
        run("network-run" + suffix, [sys.executable, root / "scripts/component_network_peer.py",
            "--report", output / ("network-peer" + suffix + ".json"), "--timeout", "90", "--",
            executable, component], expected=marker, timeout=100, moving=moving)
    env = normal_env.copy()
    host_resources = output / "host-resources"
    shutil.copytree(fixture.parent / "host-resources", host_resources, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("target"))
    run("host-resources-guest", ["cargo", "build", "--locked", "--release", "--target", "wasm32-unknown-unknown",
        "--manifest-path", host_resources / "rust/Cargo.toml"])
    core = output / "rust-target/wasm32-unknown-unknown/release/perry_component_host_resource_fixture.wasm"
    component = host_resources / "host-resources.component.wasm"
    run("host-resources-componentize", [args.componentize.resolve(), core, component])
    run("host-resources-bindings", [args.wit.resolve(), host_resources / "world.wit", host_resources / "generated", "plugin"])
    marker = "PASS host resources: aliases, ownership, moving payloads, borrows, rollback, cleanup, original exceptions"
    executable = host_resources / ("host.exe" if sys.platform == "win32" else "host")
    run("host-resources-compile", [args.perry.resolve(), "compile", host_resources / "host.ts",
        "--no-auto-optimize", "--no-cache", "-o", executable])
    run("host-resources-run", [executable, component], expected=marker, timeout=60)
    if args.moving_gc:
        for knob in ["PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION",
                     "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_DIAG", "PERRY_GC_INSTRUMENTS"]:
            env[knob] = "1"
        instrumented = executable.with_name("host-moving" + executable.suffix)
        run("host-resources-compile-moving", [args.perry.resolve(), "compile", host_resources / "host.ts",
            "--no-auto-optimize", "--no-cache", "-o", instrumented])
        run("host-resources-run-moving", [instrumented, component], expected=marker,
            timeout=60, moving=True)

    return 0


if __name__ == "__main__":
    sys.exit(main())
