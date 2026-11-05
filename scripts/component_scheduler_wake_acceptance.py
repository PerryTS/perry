#!/usr/bin/env python3
"""Prove a canonical stream wakeup using maintained C bindings, without Perry JS."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wasi-sdk", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--target-dir", required=True, type=Path)
    parser.add_argument("--profile", default="dev", choices=["dev", "prod"])
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    fixture = root / "test-wasi/components/scheduler-wake"
    for name in ["world.wit", "guest.c"]:
        shutil.copyfile(fixture / name, out / name)
    generated = out / "generated"
    target = args.target_dir.resolve()
    env = {**os.environ, "CARGO_TARGET_DIR": str(target)}
    rows = []

    def run(name, command):
        with (out / (name + ".log")).open("w") as log:
            try:
                status = subprocess.run([str(x) for x in command], cwd=root, env=env,
                                        stdout=log, stderr=subprocess.STDOUT,
                                        timeout=1200).returncode
            except (OSError, subprocess.TimeoutExpired) as error:
                log.write(str(error) + "\n")
                status = 124
        rows.append(dict(phase=name, exitCode=status))
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print(f"{'PASS' if status == 0 else 'FAIL'} {name}", flush=True)
        if status:
            raise SystemExit(status)

    def cargo(*command):
        # Preserve the repository unwind flags; do not replace RUSTFLAGS.
        return ["cargo", "-Z", "checksum-freshness", *command, "--locked",
                "--profile", args.profile]

    run("build", cargo("build", "-p", "perry-wit", "--example", "canonical_c",
                       "-p", "perry-component-host", "--example", "scheduler_wake"))
    binaries = target / ("debug" if args.profile == "dev" else args.profile) / "examples"
    suffix = ".exe" if os.name == "nt" else ""
    run("generate", [binaries / ("canonical_c" + suffix), out / "world.wit",
                     "scheduler-wake", generated])
    sdk = args.wasi_sdk.resolve()
    component = out / "wake.component.wasm"
    run("link", [sdk / "bin" / ("clang" + suffix), "--target=wasm32-wasip2",
                 "-mexec-model=reactor", "-O1", "-g", "-Wall", "-Werror",
                 "-I", generated, generated / "scheduler_wake.c", out / "guest.c",
                 generated / "scheduler_wake_component_type.o",
                 "-Wl,--export=cabi_realloc", "-Wl,--fatal-warnings", "-o", component])
    run("execute", [binaries / ("scheduler_wake" + suffix), out / "world.wit", component])
    marker = "PASS: 128 reverse-order wake pairs, blocked and early wake coalescing, extra-export subtyping, idle after every pair"
    assert marker in (out / "execute.log").read_text(), "wake subject did not finish"
    paths = [component, binaries / ("canonical_c" + suffix),
             binaries / ("scheduler_wake" + suffix), out / "world.wit", out / "guest.c",
             *generated.iterdir()]
    identities = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    (out / "artifact-identity.json").write_text(json.dumps(identities, indent=2) + "\n")


if __name__ == "__main__":
    main()
