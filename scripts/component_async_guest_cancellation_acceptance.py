#!/usr/bin/env python3
"""Send real canonical cancellation from a composed C caller to a Perry guest."""
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
    for name in ["wasi-sdk", "perry", "runtime", "target-dir", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--profile", choices=["dev", "prod"], default="dev")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    (out / "OWNER").write_text("Cooperative canonical cancellation acceptance evidence; preserve.\n")
    env = {**os.environ, "CARGO_TARGET_DIR": str(args.target_dir.resolve())}
    rows = []

    def run(name, command):
        with (out / (name + ".log")).open("w") as log:
            status = subprocess.run([str(x) for x in command], cwd=root, env=env,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=2400).returncode
        rows.append(dict(phase=name, exitCode=status))
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print(f"{'PASS' if status == 0 else 'FAIL'} {name}", flush=True)
        if status:
            raise SystemExit(status)

    # Qualify the same new scheduler on its ordinary import paths first.
    guest = out / "guest"
    command = [sys.executable, root / "scripts/component_async_guest_exports_acceptance.py", "--imports", "--profile", args.profile]
    for name in ["wasi-sdk", "perry", "runtime", "target-dir"]:
        command.extend(["--" + name, getattr(args, name.replace("-", "_")).resolve()])
    run("ordinary-import-regression", [*command, "--output", guest])
    run("build-caller-probes", ["cargo", "-Z", "checksum-freshness", "build", "--locked", "--profile", args.profile,
                              "-p", "perry-wit", "--example", "canonical_c", "-p", "perry-component-host", "--example", "async_guest_cancel"])
    binaries = args.target_dir.resolve() / ("debug" if args.profile == "dev" else args.profile) / "examples"
    frozen = out / "binaries"
    frozen.mkdir()
    (frozen / "OWNER").write_text("Frozen cancellation probes; preserve.\n")
    suffix = ".exe" if os.name == "nt" else ""
    for name in ["canonical_c", "async_guest_cancel"]:
        shutil.copy2(binaries / (name + suffix), frozen / (name + suffix))
    generator = frozen / ("canonical_c" + suffix)
    host = frozen / ("async_guest_cancel" + suffix)
    caller = out / "caller"
    caller.mkdir()
    fixture = root / "test-wasi/components/async-guest-cancel"
    for name in ["world.wit", "supervisor.c"]:
        shutil.copyfile(fixture / name, caller / name)
    generated = caller / "generated"
    run("generate-caller", [generator, caller / "world.wit", "supervisor", generated])
    component = caller / "caller.component.wasm"
    run("link-caller", [args.wasi_sdk.resolve() / "bin" / ("clang" + suffix), "--target=wasm32-wasip2",
                        "-mexec-model=reactor", "-O1", "-g", "-Wall", "-Werror", "-I", generated,
                        generated / "supervisor.c", caller / "supervisor.c", generated / "supervisor_component_type.o",
                        "-Wl,--export=cabi_realloc", "-Wl,--fatal-warnings", "-o", component])
    composed = out / "composed.component.wasm"
    run("compose", [host, "--compose", guest / "guest.component.wasm", component, composed])
    marker = "PASS: cooperative canonical cancellation, 8 rounds, 16 native children, live sibling and reusable Store"
    for name, flags in [("execute", []), ("moving", ["--moving"])]:
        run(name, [host, caller / "world.wit", composed, *flags])
        assert marker in (out / (name + ".log")).read_text()
    log = (out / "moving.log").read_text()
    copied = sum(int(n) for n in re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log))
    assert copied > 0 and "evacuation_ok" in log, "cancellation moving GC was not exercised"
    (out / "moving.json").write_text(json.dumps(dict(copiedObjects=copied, evacuationVerified=True), indent=2) + "\n")
    paths = [host, generator, component, composed, caller / "world.wit", caller / "supervisor.c", *generated.iterdir()]
    (out / "artifact-identity.json").write_text(json.dumps({str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}, indent=2) + "\n")


if __name__ == "__main__":
    main()
