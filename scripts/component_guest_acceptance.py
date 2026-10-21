#!/usr/bin/env python3
"""Build a Perry library component and execute it from a typed Rust host.

Supply a matching target-wasi compiler, perry-wit, wasm32-wasip2 runtime
archive, and wasi-sdk. Run through the build host's concurrency gate: this
harness also builds the Wasmtime embedding fixture. Each phase records its
actual exit status and a separate log; compilation alone is not acceptance.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", required=True, type=Path)
    parser.add_argument("--wit", required=True, type=Path)
    parser.add_argument("--componentize", required=True, type=Path)
    parser.add_argument("--runtime-dir", required=True, type=Path)
    parser.add_argument("--wasi-sdk", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    fixture = root / "test-wasi/components/transform"
    output = args.output.resolve()
    work = output / "transform"
    work.mkdir(parents=True, exist_ok=True)
    for name in ["world.wit", "guest.ts"]:
        shutil.copyfile(fixture / name, work / name)
    shutil.copytree(fixture / "rust-host", work / "rust-host", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("target"))
    shutil.copytree(fixture / "rust-logger", work / "rust-logger", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("target"))
    sdk = args.wasi_sdk.resolve()
    suffix = ".exe" if sys.platform == "win32" else ""
    runtime = args.runtime_dir.resolve() / "libperry_runtime.a"
    if not runtime.is_file():
        parser.error(f"missing wasm32-wasip2 runtime: {runtime}")
    env = os.environ.copy()
    env.update(PERRY_RUNTIME_DIR=str(runtime.parent), WASI_SDK_PATH=str(sdk),
               CARGO_TARGET_DIR=str(output / "rust-host-target"),
               CARGO_BUILD_JOBS="4", CARGO_INCREMENTAL="0",
               PERRY_CODEGEN_CPU_SLOTS="4", PERRY_NO_AUTO_OPTIMIZE="1")
    results = []
    # Clear a previous verdict before any build can fail.
    (output / "results.json").write_text("[]\n")

    def run(name, command, expected=None, timeout=600):
        log = output / (name + ".log")
        start = time.monotonic()
        try:
            with log.open("w") as stream:
                completed = subprocess.run([str(x) for x in command], cwd=work,
                    env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=timeout)
            status = completed.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            with log.open("a") as stream:
                stream.write(str(error) + "\n")
            status = 124
        if status == 0 and expected and expected not in log.read_text():
            status = 1
        results.append({"name": name, "exitCode": status,
                        "seconds": round(time.monotonic() - start, 3), "log": str(log)})
        (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(f"{'PASS' if status == 0 else 'FAIL'} {name}: {log}", flush=True)
        if status:
            raise SystemExit(status if status > 0 else 1)

    def build_guest(directory, prefix=""):
        generated = directory / "generated"
        run(prefix + "guest-bindings", [args.wit.resolve(), "--guest-c", directory / "world.wit",
                               generated, "plugin"])
        library = directory / "libtransform.a"
        run(prefix + "compile-guest", [args.perry.resolve(), "compile", directory / "guest.ts",
                              "--target", "wasi", "--output-type", "staticlib",
                              "--no-auto-optimize", "--no-cache", "-o", library])
        component = directory / "transform.component.wasm"
        # wasi-sdk's P2 driver componentizes the linked reactor, combining the
        # application WIT metadata with its libc/WASI imports. Include all TS
        # modules and the feature-installer constructor from the static archive.
        run(prefix + "link-guest-component", [sdk / "bin" / ("clang" + suffix),
            "--target=wasm32-wasip2", f"--sysroot={sdk / 'share/wasi-sysroot'}",
            "-mexec-model=reactor", "-O1", "-g", "-I", generated,
            generated / "plugin.c", generated / "perry_adapter.c",
            generated / "plugin_component_type.o", "-Wl,--whole-archive", library,
            "-Wl,--no-whole-archive", runtime, "-lsetjmp",
            "-Wl,--export=cabi_realloc", "-Wl,--stack-first",
            "-Wl,-z,stack-size=8388608", "-Wl,--fatal-warnings", "-o", component])
        return component

    component = build_guest(work)
    run("build-rust-host", ["cargo", "build", "--release", "--manifest-path",
                            work / "rust-host/Cargo.toml"], timeout=1800)
    host = output / "rust-host-target/release" / ("perry-component-rust-host" + suffix)
    run("run-rust-host", [host, component], timeout=120,
        expected="PASS Perry component: Rust host, 51 typed calls, Unicode, bytes, guest GC, imports")
    run("build-rust-logger", ["cargo", "build", "--release", "--target",
        "wasm32-unknown-unknown", "--manifest-path", work / "rust-logger/Cargo.toml"])
    core = output / "rust-host-target/wasm32-unknown-unknown/release/perry_component_rust_logger.wasm"
    logger = work / "logger.component.wasm"
    run("componentize-logger", [args.componentize.resolve(), core, logger])
    composed = work / "composed.component.wasm"
    composer = output / "rust-host-target/release" / ("compose" + suffix)
    run("compose", [composer, component, logger, composed])
    run("run-composed", [host, composed, "--composed"], timeout=120,
        expected="PASS direct composition: Perry guest imports Rust logger, 51 audited calls")
    values = output / "values"
    values.mkdir(exist_ok=True)
    for name in ["world.wit", "guest.ts"]:
        shutil.copyfile(fixture.parent / "values" / name, values / name)
    values_component = build_guest(values, "values-")
    run("run-values", [host, values_component, "--values"], timeout=120,
        expected="PASS canonical values: 14 import/export calls")
    return 0


if __name__ == "__main__":
    sys.exit(main())
