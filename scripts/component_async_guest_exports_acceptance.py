#!/usr/bin/env python3
"""Qualify the experimental generated scheduler against a real Perry WASI guest."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["wasi-sdk", "perry", "runtime", "target-dir", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--profile", choices=["dev", "prod"], default="dev")
    parser.add_argument("--imports", action="store_true", help="qualify typed async import callbacks and post-return children")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    fixture_name = "async-guest-imports" if args.imports else "async-guest-exports"
    host_name = "async_guest_imports" if args.imports else "async_guest_exports"
    fixture = root / "test-wasi/components" / fixture_name
    for name in ["world.wit", "guest.ts"]:
        shutil.copyfile(fixture / name, out / name)
    target = args.target_dir.resolve()
    env = {**os.environ, "CARGO_TARGET_DIR": str(target),
           "PERRY_RUNTIME_DIR": str(args.runtime.resolve().parent),
           "WASI_SDK_PATH": str(args.wasi_sdk.resolve()),
           "PERRY_NO_AUTO_OPTIMIZE": "1", "PERRY_NO_CACHE": "1"}
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

    # Qualification snapshots can share a target pool. Invalidate just the
    # probe packages so Cargo cannot reuse a binary from another source path.
    run("invalidate", ["cargo", "clean", "--profile", args.profile,
                       "-p", "perry-wit", "-p", "perry-component-host"])
    run("build", ["cargo", "-Z", "checksum-freshness", "build", "--locked",
                  "--profile", args.profile, "-p", "perry-wit", "--example", "async_guest",
                  "-p", "perry-component-host", "--example", host_name])
    binaries = target / ("debug" if args.profile == "dev" else args.profile) / "examples"
    suffix = ".exe" if os.name == "nt" else ""
    frozen = out / "binaries"
    frozen.mkdir()
    (frozen / "OWNER").write_text("Generated async guest qualification probes; preserve.\n")
    for name in ["async_guest", host_name]:
        shutil.copy2(binaries / (name + suffix), frozen / (name + suffix))
    generator = frozen / ("async_guest" + suffix)
    host = frozen / (host_name + suffix)
    generated = out / "generated"
    run("generate", [generator, out / "world.wit", "plugin", generated])
    library = out / "libguest.a"
    run("compile", [args.perry.resolve(), "compile", out / "guest.ts", "--target", "wasi",
                    "--output-type", "staticlib", "--no-auto-optimize", "--no-cache", "-o", library])
    component = out / "guest.component.wasm"
    run("link", [args.wasi_sdk.resolve() / "bin" / ("clang" + suffix),
                 "--target=wasm32-wasip2", "-mexec-model=reactor", "-O1", "-g",
                 "-Wall", "-Werror", "-Wno-unused-function", "-I", generated,
                 generated / "plugin.c", generated / "perry_adapter.c",
                 generated / "plugin_component_type.o", "-Wl,--whole-archive", library,
                 "-Wl,--no-whole-archive", args.runtime.resolve(), "-lsetjmp",
                 "-Wl,--export=cabi_realloc", "-Wl,--stack-first",
                 "-Wl,-z,stack-size=8388608", "-Wl,--fatal-warnings", "-o", component])
    marker = ("PASS: generated Perry async imports, 64 reverse pairs" if args.imports
              else "PASS: generated Perry async exports, 64 reverse wake pairs")
    for name, flags in [("execute", []), ("moving", ["--moving"])]:
        run(name, [host, out / "world.wit", component, *flags])
        assert marker in (out / (name + ".log")).read_text()
    run("cancel", [host, out / "world.wit", component, "--cancel"])
    cancel_marker = ("PASS: native pending imports cancelled with Store retirement" if args.imports
                     else "PASS: dropping a suspended generated guest call")
    assert cancel_marker in (out / "cancel.log").read_text()
    if args.imports:
        run("failure", [host, out / "world.wit", component, "--failure"])
        assert "PASS: native pending imports cancelled with Store retirement" in (out / "failure.log").read_text()
        run("inline", [host, out / "world.wit", component, "--inline"])
        assert "PASS: ready Rust imports without embedding dispatch" in (out / "inline.log").read_text()
        run("inline-moving", [host, out / "world.wit", component, "--inline-moving"])
        log = (out / "inline-moving.log").read_text()
        assert "PASS: ready Rust imports without embedding dispatch" in log
        copied = sum(int(n) for n in re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log))
        assert copied > 0 and "evacuation_ok" in log, "inline handoff moving GC was not exercised"
        (out / "inline-moving.json").write_text(json.dumps(dict(copiedObjects=copied, evacuationVerified=True), indent=2) + "\n")
    log = (out / "moving.log").read_text()
    copied = sum(int(n) for n in re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log))
    assert copied > 0 and "evacuation_ok" in log, "moving GC was not exercised"
    (out / "moving.json").write_text(json.dumps(dict(copiedObjects=copied, evacuationVerified=True), indent=2) + "\n")
    paths = [args.perry.resolve(), args.runtime.resolve(), host, generator, component,
             out / "world.wit", out / "guest.ts", *generated.iterdir()]
    identities = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    (out / "artifact-identity.json").write_text(json.dumps(identities, indent=2) + "\n")


if __name__ == "__main__":
    main()
