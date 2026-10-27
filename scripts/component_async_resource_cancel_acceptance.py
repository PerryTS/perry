#!/usr/bin/env python3
"""Qualify resource cancellation through independent C/Perry/C components."""
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
    (out / "OWNER").write_text("Actual resource cancellation composition and ownership evidence; preserve.\n")
    env = {**os.environ, "CARGO_TARGET_DIR": str(args.target_dir.resolve())}
    rows = []

    def run(name, command):
        print("START", name, flush=True)
        with (out / (name + ".log")).open("w") as log:
            status = subprocess.run([str(x) for x in command], cwd=root, env=env,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=2400).returncode
        rows.append(dict(phase=name, exitCode=status))
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print("DONE", name, status, flush=True)
        if status:
            raise SystemExit(status)

    common = ["--profile", args.profile]
    for name in ["wasi-sdk", "perry", "runtime", "plain-runtime", "target-dir"]:
        common.extend(["--" + name, getattr(args, name.replace("-", "_")).resolve()])
    guest = out / "resource-guest"
    run("resource-regression", [sys.executable, root / "scripts/component_async_guest_resources_acceptance.py",
                                *common, "--output", guest])
    run("caller-probes", ["cargo", "-Z", "checksum-freshness", "build", "--locked", "--profile", args.profile,
                          "-p", "perry-wit", "--example", "canonical_c", "-p", "perry-component-host",
                          "--example", "async_resource_cancel"])
    suffix = ".exe" if os.name == "nt" else ""
    profile = "debug" if args.profile == "dev" else args.profile
    binaries = args.target_dir.resolve() / profile / "examples"
    frozen = out / "binaries"
    frozen.mkdir()
    for name in ["canonical_c", "async_resource_cancel"]:
        shutil.copy2(binaries / (name + suffix), frozen / (name + suffix))
    generator = frozen / ("canonical_c" + suffix)
    host = frozen / ("async_resource_cancel" + suffix)
    fixture = root / "test-wasi/components/async-resource-cancel"
    peers = []
    for world, source in [("provider", "provider.c"), ("supervisor", "supervisor.c")]:
        folder = out / world
        folder.mkdir()
        for name in ["world.wit", source]:
            shutil.copy2(fixture / name, folder / name)
        generated = folder / "generated"
        run(world + "-generate", [generator, folder / "world.wit", world, generated])
        component = folder / (world + ".component.wasm")
        run(world + "-link", [args.wasi_sdk.resolve() / "bin" / ("clang" + suffix), "--target=wasm32-wasip2",
                              "-mexec-model=reactor", "-O1", "-g", "-Wall", "-Werror", "-I", generated,
                              generated / (world + ".c"), folder / source, generated / (world + "_component_type.o"),
                              "-Wl,--export=cabi_realloc", "-Wl,--fatal-warnings", "-o", component])
        peers.append(component)
    marker = "PASS: resource canonical cancellation, 8 pre-start owners, 8 suspended borrows, 8 ready owned results, 24 live siblings and reusable Store"
    components = []
    for kind in ["plain", "instrumented"]:
        component = out / (kind + ".composed.wasm")
        run(kind + "-compose", [host, "--compose", peers[0], guest / "guest" / (kind + ".component.wasm"), peers[1], component])
        components.append(component)
        run(kind + "-execute", [host, fixture / "world.wit", component])
        assert marker in (out / (kind + "-execute.log")).read_text()
        if kind == "instrumented":
            run("moving", [host, fixture / "world.wit", component, "--moving"])
            log = (out / "moving.log").read_text()
            assert marker in log
            copied = sum(int(n) for n in re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log))
            assert copied > 0 and "evacuation_ok" in log, "resource cancellation moving GC did not execute"
            states = re.search(r"RESOURCE_CANCEL_STATES=([^\r\n]+)", log)
            assert states and sum(json.loads(states.group(1)).values()) == 8
            (out / "moving.json").write_text(json.dumps(dict(copiedObjects=copied, evacuationVerified=True,
                                                             returnedResultRaceStates=json.loads(states.group(1))), indent=2) + "\n")
    paths = [generator, host, *peers, *components, *fixture.iterdir()]
    identities = {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths if path.is_file()}
    (out / "artifact-identities.json").write_text(json.dumps(identities, indent=2) + "\n")


if __name__ == "__main__":
    main()
