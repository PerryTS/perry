#!/usr/bin/env python3
"""Qualify an installed P3 runtime and public component tools outside the checkout."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

import wasi_release


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["compiler", "wit", "bundle", "p3-contract", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    root = wasi_release.ROOT
    out = args.output.resolve()
    if out.exists() or out.is_relative_to(root):
        parser.error("--output must be a new directory outside the source checkout")
    sdk = Path(os.environ.get("WASI_SDK_PATH", "")).resolve()
    suffix = ".exe" if os.name == "nt" else ""
    clang = sdk / "bin" / ("clang" + suffix)
    if not clang.is_file():
        parser.error("configure WASI_SDK_PATH before qualifying P3 packages")
    out.mkdir(parents=True)
    (out / "OWNER").write_text("Isolated installed P3 runtime and public typed plugin qualification. Preserve.\n")
    installed = out / "install"
    installed.mkdir()
    compiler = installed / ("perry" + suffix)
    wit = installed / ("perry-wit" + suffix)
    shutil.copy2(args.compiler, compiler)
    shutil.copy2(args.wit, wit)
    if os.name == "nt":
        llvm = args.compiler.resolve().parent / "LLVM-C.dll"
        if not llvm.is_file():
            llvm = Path(os.environ.get("LLVM_SYS_221_PREFIX", "")) / "bin/LLVM-C.dll"
        if not llvm.is_file():
            parser.error("the installed Windows tools require their matching LLVM-C.dll")
        shutil.copy2(llvm, installed / llvm.name)
    env = os.environ.copy()
    for name in ["PERRY_RUNTIME_DIR", "PERRY_LIB_DIR", "PERRY_WORKSPACE_ROOT", "CARGO_TARGET_DIR"]:
        env.pop(name, None)
    if any(key.startswith("PERRY_GC_") and value for key, value in env.items()):
        parser.error("installed plain-profile qualification requires native GC controls off")
    env.update(PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1", PERRY_CODEGEN_CPU_SLOTS="4")
    rows = []

    def run(name, argv, failure=None):
        print("START", name, flush=True)
        (out / (name + ".argv.json")).write_text(json.dumps(list(map(str, argv)), indent=2) + "\n")
        result = subprocess.run(list(map(str, argv)), cwd=out, env=env, capture_output=True, timeout=600)
        (out / (name + ".stdout")).write_bytes(result.stdout)
        (out / (name + ".stderr")).write_bytes(result.stderr)
        passed = (result.returncode != 0 and failure in result.stderr) if failure else result.returncode == 0
        rows.append(dict(phase=name, exitCode=result.returncode, passed=passed))
        (out / "phases.json").write_text(json.dumps(rows, indent=2) + "\n")
        if not passed:
            (out / "result.json").write_text(json.dumps(dict(exitCode=result.returncode or 1, failedPhase=name,
                fullWasi030Qualified=False), indent=2) + "\n")
            print(result.stderr.decode(errors="replace")[-5000:], flush=True)
            raise SystemExit(result.returncode or 1)
        print("DONE", name, result.returncode, flush=True)
        return result

    probe = out / "probe.ts"
    probe.write_text('console.log("installed P3 command");\n')
    command = out / "command.component.wasm"
    compile_command = [compiler, "compile", probe, "--target", "wasip3", "--no-auto-optimize", "--no-cache", "-o", command]
    run("missing-p3-runtime", compile_command, b"Could not find libperry_runtime.a")
    version = wasi_release.tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    p3 = installed / "wasm32-wasip3/release"
    wasi_release.install(args.bundle, p3, args.revision, version, "wasm32-wasip3")
    run("installed-command-compile", compile_command)
    run("installed-command-contract", [args.p3_contract.resolve(), command])
    for name in ["runtime", "stdlib"]:
        filename = "perry_" + name + ".lib" if os.name == "nt" else "libperry_" + name + ".a"
        source = args.compiler.resolve().parent / filename
        shutil.copy2(source, installed / filename)
        if name == "runtime":
            shutil.copy2(Path(str(source) + ".gc-effects"), installed / (filename + ".gc-effects"))
    guest = out / "plugin"
    guest.mkdir()
    for name in ["world.wit", "guest.ts", "host.ts"]:
        shutil.copy2(root / "test-wasi/components/public-async" / name, guest / name)
    generated = guest / "generated"
    run("public-generate", [wit, "--guest-c", "--target", "wasip3", guest / "world.wit", generated])
    facade = guest / "facade.component.wasm"
    run("facade-link", [clang, "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g", "-Wall", "-Werror",
        "-I", generated, generated / "perry_producer_facade.c", generated / "perry_producer_forward.c",
        generated / "perry_producer_facade_component_type.o", "-Wl,--export=cabi_realloc", "-Wl,--fatal-warnings", "-o", facade])
    library = guest / "libguest.a"
    run("guest-compile", [compiler, "compile", guest / "guest.ts", "--target", "wasip3", "--output-type", "staticlib",
        "--no-auto-optimize", "--no-cache", "-o", library])
    producer = guest / "producer.component.wasm"
    run("producer-link", [clang, "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g", "-Wall", "-Werror",
        "-Wno-unused-function", "-I", generated, generated / "plugin.c", generated / "perry_adapter.c", generated / "plugin_component_type.o",
        "-Wl,--whole-archive", library, "-Wl,--no-whole-archive", p3 / "libperry_runtime.a", "-lsetjmp",
        "-Wl,--export=cabi_realloc", "-Wl,--stack-first", "-Wl,-z,stack-size=8388608", "-Wl,--fatal-warnings", "-o", producer])
    component = guest / "plugin.component.wasm"
    run("public-compose", [wit, "compose", producer, facade, generated / "perry_producer_facade.json", component])
    run("plugin-contract", [args.p3_contract.resolve(), "--reactor", component, "ping"])
    native = guest / ("host" + suffix)
    compile_host = [compiler, "compile", guest / "host.ts", "--no-auto-optimize", "--no-cache", "-o", native]
    run("missing-native-component-engine", compile_host, b"perry/component requires")
    engine_name = "perry_component_host.lib" if os.name == "nt" else "libperry_component_host.a"
    shutil.copy2(args.compiler.resolve().parent / engine_name, installed / engine_name)
    run("installed-typed-host-compile", compile_host)
    result = run("installed-typed-host-execute", [native, component])
    assert result.stdout.decode().replace("\r\n", "\n") == "PUBLIC ALL-ASYNC P3 PASS\n"
    command_host = out / "command-host.ts"
    command_host.write_text('import { readFileSync } from "node:fs";\n'
        'import { runComponentCommand } from "perry/component";\n'
        'process.exit(runComponentCommand(readFileSync(process.argv[2]), { fuel: 1000000000, memoryBytes: 268435456, '
        'wasi: { inheritStdout: true, inheritStderr: true } }));\n')
    executable = out / ("command-host" + suffix)
    run("installed-command-host-compile", [compiler, "compile", command_host, "--no-auto-optimize", "--no-cache", "-o", executable])
    result = run("installed-command-host-execute", [executable, command])
    assert result.stdout.decode().replace("\r\n", "\n") == "installed P3 command\n"
    paths = [*installed.rglob("*"), native, executable, component, command]
    identities = {str(p.relative_to(out)): dict(bytes=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                  for p in paths if p.is_file()}
    (out / "artifact-identities.json").write_text(json.dumps(identities, indent=2) + "\n")
    (out / "result.json").write_text(json.dumps(dict(exitCode=0, installedPlainP3Qualified=True,
        generatedTypedNativeBindingsExecuted=True, runtimeSource=args.revision,
        repositoryLibraryFallbackRejected=True, fullWasi030Qualified=False), indent=2) + "\n")


if __name__ == "__main__":
    main()
