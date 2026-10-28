#!/usr/bin/env python3
"""Qualify nested resource custody through real C/Perry/C components."""
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
    for name in ["wasi-sdk", "perry", "runtime", "plain-runtime", "target-dir", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--profile", choices=["dev", "prod"], default="prod")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    (out / "OWNER").write_text("Compound resource canonical cancellation and custody evidence; preserve.\n")
    env = {**os.environ, "CARGO_TARGET_DIR": str(args.target_dir.resolve())}
    rows = []

    def run(name, command, environment=env):
        print("START", name, flush=True)
        with (out / (name + ".log")).open("w") as log:
            status = subprocess.run([str(x) for x in command], cwd=root, env=environment,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=2400).returncode
        rows.append(dict(phase=name, exitCode=status))
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print("DONE", name, status, flush=True)
        if status:
            raise SystemExit(status)

    common = ["--profile", args.profile]
    for name in ["wasi-sdk", "perry", "runtime", "plain-runtime", "target-dir"]:
        common.extend(["--" + name, getattr(args, name.replace("-", "_")).resolve()])
    scalar = out / "scalar"
    # This builds and freezes the same host and fixture producer used below,
    # while verifying that the shared harness still covers its original profile.
    run("scalar-regression", [sys.executable, root / "scripts/component_async_resource_cancel_acceptance.py",
                               *common, "--output", scalar])
    generator = scalar / "resource-guest/binaries/resource_fixture"
    suffix = ".exe" if os.name == "nt" else ""
    host = scalar / "binaries" / ("async_resource_cancel" + suffix)
    canonical = scalar / "binaries" / ("canonical_c" + suffix)
    run("generate", [generator.with_suffix(suffix) if suffix else generator,
                     "--exact", "guest_c::asynchronous::tests::async_compound_resource_guest_fixture",
                     "--ignored", "--nocapture", "--test-threads=1"])
    match = re.search(r"PERRY_ASYNC_RESOURCE_FIXTURE=([^\r\n]+)", (out / "generate.log").read_text())
    assert match, "compound fixture subject did not execute"
    guest = out / "guest"
    shutil.move(match.group(1), guest)
    fixture = root / "test-wasi/components/async-compound-resources"
    for name in ["world.wit", "guest.ts"]:
        assert (guest / name).read_bytes() == (fixture / name).read_bytes(), "stale compound fixture"
    compile_env = {**env, "WASI_SDK_PATH": str(args.wasi_sdk.resolve()),
                   "PERRY_RUNTIME_DIR": str(args.runtime.resolve().parent), "PERRY_WORKSPACE_ROOT": str(root)}
    library = guest / "libguest.a"
    run("compile", [args.perry.resolve(), "compile", guest / "guest.ts", "--target", "wasi",
                    "--output-type", "staticlib", "--no-auto-optimize", "--no-cache", "-o", library], compile_env)
    peers = []
    for world in ["provider", "supervisor"]:
        folder = out / world
        folder.mkdir()
        for name in ["world.wit", world + ".c"]:
            shutil.copy2(fixture / name, folder / name)
        generated = folder / "generated"
        run(world + "-generate", [canonical, folder / "world.wit", world, generated])
        component = folder / (world + ".component.wasm")
        run(world + "-link", [args.wasi_sdk.resolve() / "bin" / ("clang" + suffix), "--target=wasm32-wasip2",
                              "-mexec-model=reactor", "-O1", "-g", "-Wall", "-Werror", "-I", generated,
                              generated / (world + ".c"), folder / (world + ".c"), generated / (world + "_component_type.o"),
                              "-Wl,--export=cabi_realloc", "-Wl,--fatal-warnings", "-o", component])
        peers.append(component)
    generated = guest / "generated"
    components = []
    marker = "PASS: compound resource canonical cancellation, records, lists, options, tuples, variants, owned errors, 24 duplicate-owner rejections, 188 exactly-once drops and 32 live siblings"
    for kind, archive in [("plain", args.plain_runtime.resolve()), ("instrumented", args.runtime.resolve())]:
        component = guest / (kind + ".component.wasm")
        run(kind + "-link", [args.wasi_sdk.resolve() / "bin" / ("clang" + suffix), "--target=wasm32-wasip2",
                              "-mexec-model=reactor", "-O1", "-g", "-Wall", "-Werror", "-Wno-unused-function", "-I", generated,
                              generated / "plugin.c", generated / "perry_adapter.c", generated / "plugin_component_type.o",
                              "-Wl,--whole-archive", library, "-Wl,--no-whole-archive", archive, "-lsetjmp",
                              "-Wl,--export=cabi_realloc", "-Wl,--stack-first", "-Wl,-z,stack-size=8388608",
                              "-Wl,--fatal-warnings", "-o", component])
        composed = out / (kind + ".composed.wasm")
        run(kind + "-compose", [host, "--compose-compound", peers[0], component, peers[1], composed])
        components.extend([component, composed])
        run(kind + "-execute", [host, fixture / "world.wit", composed])
        assert marker in (out / (kind + "-execute.log")).read_text()
        if kind == "instrumented":
            run("moving", [host, fixture / "world.wit", composed, "--moving"])
            log = (out / "moving.log").read_text()
            assert marker in log
            copied = sum(int(n) for n in re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log))
            assert copied > 0 and "evacuation_ok" in log, "compound moving GC did not execute"
            states = re.search(r"RESOURCE_UNCLAIMED_CANCEL_STATES=([^\r\n]+)", log)
            assert states and json.loads(states.group(1)) == {"4": 8}, "unclaimed compound cancellation did not execute"
            (out / "moving.json").write_text(json.dumps(dict(copiedObjects=copied, evacuationVerified=True,
                                                             unclaimedResultCancelStates=json.loads(states.group(1))), indent=2) + "\n")
    paths = [generator.with_suffix(suffix) if suffix else generator, host, canonical, args.perry.resolve(),
             args.runtime.resolve(), args.plain_runtime.resolve(), library, *peers, *components,
             *fixture.iterdir(), *generated.iterdir()]
    (out / "artifact-identities.json").write_text(json.dumps({str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                                                             for p in paths if p.is_file()}, indent=2) + "\n")


if __name__ == "__main__":
    main()
