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
import re
from pathlib import Path
import shutil
import subprocess
import sys
import time


def prepare_rust_host_manifest(path, root):
    """Retain the standalone fixture's source selection after copying it."""
    manifest = path.read_text()
    for relative in ["crates/perry-wasi", "third_party/turnloop/turnloop",
                     "third_party/turnloop/turnloop-http"]:
        original = json.dumps("../../../../" + relative)
        assert manifest.count(original) == 1, f"Rust host path changed: {relative}"
        manifest = manifest.replace(original, json.dumps(str(root / relative)))
    path.write_text(manifest)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", required=True, type=Path)
    parser.add_argument("--wit", required=True, type=Path)
    parser.add_argument("--componentize", required=True, type=Path)
    parser.add_argument("--runtime-dir", required=True, type=Path)
    parser.add_argument("--wasi-sdk", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--cargo-config", type=Path,
                        help="additional Cargo configuration for provider dependency qualification")
    parser.add_argument("--native-runtime-dir", type=Path,
                        help="also qualify native Perry hosting with WASI providers")
    parser.add_argument("--moving-gc", action="store_true")
    parser.add_argument("--guest-moving-gc", action="store_true",
                        help="qualify guest evacuation; requires a WASI runtime built with --gc-instruments")
    args = parser.parse_args()
    if args.moving_gc and not args.native_runtime_dir:
        parser.error("--moving-gc requires --native-runtime-dir")
    root = Path(__file__).resolve().parent.parent
    fixture = root / "test-wasi/components/transform"
    output = args.output.resolve()
    work = output / "transform"
    work.mkdir(parents=True, exist_ok=True)
    for name in ["world.wit", "guest.ts"]:
        shutil.copyfile(fixture / name, work / name)
    shutil.copytree(fixture / "rust-host", work / "rust-host", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("target"))
    # Keep the copied fixture executable from an arbitrary output directory.
    # Its in-tree manifest uses the same Turnloop provider as the native host.
    prepare_rust_host_manifest(work / "rust-host/Cargo.toml", root)

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
               CARGO_BUILD_JOBS=env.get("CARGO_BUILD_JOBS", "4"), CARGO_INCREMENTAL="0",
               PERRY_CODEGEN_CPU_SLOTS="4", PERRY_NO_AUTO_OPTIMIZE="1")
    results = []
    # Clear a previous verdict before any build can fail.
    (output / "results.json").write_text("[]\n")

    def run(name, command, expected=None, timeout=600, environment=None, moving=False, guest_moving=False, provider=False):
        if provider and args.cargo_config:
            command = ["cargo", "--config", args.cargo_config.resolve(), *command[1:]]
        log = output / (name + ".log")
        start = time.monotonic()
        try:
            with log.open("w") as stream:
                completed = subprocess.run([str(x) for x in command], cwd=root if command[0] == "cargo" else work,
                    env=environment if environment is not None else env, stdout=stream, stderr=subprocess.STDOUT, timeout=timeout)
            status = completed.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            with log.open("a") as stream:
                stream.write(str(error) + "\n")
            status = 124
        if status == 0 and expected and expected not in log.read_text():
            status = 1
        if status == 0 and (moving or guest_moving):
            contents = log.read_text()
            moved = sum(int(n) for n in re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", contents))
            if moved == 0 or (moving and "[gc-fromspace-protect]" not in contents):
                with log.open("a") as stream:
                    stream.write("FAIL: copying-GC witness (and native quarantine when applicable) was not observed\n")
                status = 1
        results.append({"name": name, "exitCode": status,
                        "seconds": round(time.monotonic() - start, 3), "log": str(log)})
        (output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(f"{'PASS' if status == 0 else 'FAIL'} {name}: {log}", flush=True)
        if status:
            raise SystemExit(status if status > 0 else 1)

    def build_guest(directory, prefix="", environment=None):
        generated = directory / "generated"
        run(prefix + "guest-bindings", [args.wit.resolve(), "--guest-c", directory / "world.wit",
                               generated, "plugin"])
        library = directory / "libtransform.a"
        run(prefix + "compile-guest", [args.perry.resolve(), "compile", directory / "guest.ts",
                              "--target", "wasi", "--output-type", "staticlib",
                              "--no-auto-optimize", "--no-cache", "-o", library], environment=environment)
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
    if args.cargo_config:
        run("rust-host-provider-lock", ["cargo", "update", "--manifest-path",
            work / "rust-host/Cargo.toml", "-p", "turnloop"], provider=True)
    run("build-rust-host", ["cargo", "build", "--locked", "--release", "--manifest-path",
                            work / "rust-host/Cargo.toml"], timeout=1800, provider=True)
    host = output / "rust-host-target/release" / ("perry-component-rust-host" + suffix)
    run("run-rust-host", [host, component], timeout=120,
        expected="PASS Perry component: Rust host, 51 typed calls, Unicode, bytes, guest GC, imports")
    run("build-rust-logger", ["cargo", "build", "--locked", "--release", "--target",
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
    resources = output / "resources"
    shutil.copytree(fixture.parent / "guest-resources", resources, dirs_exist_ok=True)
    # The moving run's guest knobs (perry-component-host `--resources-moving`)
    # include PERRY_GC_DIAG, whose `[gc-copy-minor]` witness the GC diagnostics
    # print; a program installs them only when the knob is set while compiling.
    guest_env = env.copy()
    if args.guest_moving_gc:
        guest_env.update(PERRY_GC_FORCE_EVACUATE="1", PERRY_GC_VERIFY_EVACUATION="1", PERRY_GC_DIAG="1")
    resource_component = build_guest(resources, "resources-", guest_env)
    run("run-resources", [host, resource_component, "--resources"], timeout=120,
        expected="PASS Perry guest resources: imported ownership, rollback, nested transfers, exported payload GC, expired borrows, 51 explicit destructions, deferred cleanup, trap")
    if args.guest_moving_gc:
        run("run-resources-moving", [host, resource_component, "--resources-moving"], timeout=120,
            expected="PASS Perry guest resources:", guest_moving=True)
    dual_resources = output / "dual-resources"
    shutil.copytree(fixture.parent / "guest-resources-dual", dual_resources, dirs_exist_ok=True)
    dual_component = build_guest(dual_resources, "dual-resources-")
    run("run-dual-resources", [host, dual_component, "--resources-dual"], timeout=120,
        expected="PASS dual resource interface: distinct import/export identities, cross-type rejection, nested transfer and destruction")
    if args.native_runtime_dir:
        native_env = env.copy()
        native_env["PERRY_RUNTIME_DIR"] = str(args.native_runtime_dir.resolve())
        native_env["PERRY_COMPONENT_AMBIENT"] = "must-not-leak"

        def native_host(directory, guest, source, marker, extra=(), moving=False):
            run(source + "-bindings", [args.wit.resolve(), directory / "world.wit",
                directory / "generated", "plugin"])
            executable = directory / (source + suffix)
            host_env = native_env.copy()
            if moving:
                for knob in ["PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION",
                             "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_DIAG", "PERRY_GC_INSTRUMENTS"]:
                    host_env[knob] = "1"
            run(source + "-compile", [args.perry.resolve(), "compile", directory / (source + ".ts"),
                "--no-auto-optimize", "--no-cache", "-o", executable], environment=host_env)
            run(source + "-run", [executable, guest, *extra], expected=marker,
                timeout=120, environment=host_env, moving=moving)

        shutil.copyfile(fixture / "perry-host.ts", work / "perry-host.ts")
        marker = "PASS Perry to Perry: 51 calls, typed results, callback GC, disposal"
        native_host(work, component, "perry-host", marker)
        if args.moving_gc:
            shutil.copyfile(fixture / "perry-host.ts", work / "perry-host-moving.ts")
            native_host(work, component, "perry-host-moving", marker, moving=True)
        capabilities = output / "capabilities"
        shutil.copytree(fixture.parent / "capabilities", capabilities, dirs_exist_ok=True)
        capability_component = build_guest(capabilities, "capabilities-")
        data = capabilities / "data"
        for name in ["one", "two"]:
            directory = data / name
            directory.mkdir(parents=True, exist_ok=True)
            (directory / "input.txt").write_text(name)
            (directory / "output.txt").unlink(missing_ok=True)
        (data / "outside.txt").write_text("must-not-leak")
        escape = data / "one/escape.txt"
        escape.unlink(missing_ok=True)
        escape.symlink_to("../outside.txt")
        native_host(capabilities, capability_component, "host",
            "PASS WASI providers: opt-in, instance isolation, args, env, read-only, write grants, escape denial",
            extra=[data])
    return 0


if __name__ == "__main__":
    sys.exit(main())
