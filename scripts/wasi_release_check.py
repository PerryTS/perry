#!/usr/bin/env python3
"""Execute the packaged WASI runtime with the exact release compiler outside its checkout."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

import wasi_release
import wasi_run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", required=True, type=Path)
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--target", choices=sorted(wasi_release.TARGETS), default=wasi_release.TARGET,
                        help="Rust target triple of the bundle (default: %(default)s)")
    parser.add_argument("--revision", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    root = wasi_release.ROOT
    flavour = wasi_release.TARGETS[args.target]
    # The `perry compile --target` name of this bundle's flavour: wasi or wasip1.
    perry_target = wasi_run.TARGETS[flavour]
    output = args.output.resolve()
    if output.is_relative_to(root) or output.exists():
        parser.error("--output must be a new directory outside the source checkout")
    output.mkdir(parents=True)
    installed = output / "install"
    installed.mkdir()
    compiler = installed / args.compiler.name
    shutil.copy2(args.compiler, compiler)
    # Windows release compilers link the packaged LLVM C API dynamically.
    llvm = args.compiler.parent / "LLVM-C.dll"
    if not llvm.exists() and os.environ.get("LLVM_SYS_221_PREFIX"):
        llvm = Path(os.environ["LLVM_SYS_221_PREFIX"]) / "bin/LLVM-C.dll"
    if sys.platform == "win32" and not llvm.is_file():
        raise SystemExit("The Windows release compiler requires its matching LLVM-C.dll")
    if llvm.exists():
        shutil.copy2(llvm, installed / llvm.name)
    env = os.environ.copy()
    for name in ["PERRY_RUNTIME_DIR", "PERRY_LIB_DIR"]:
        env.pop(name, None)
    env.update(PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1")
    source = output / "probe.ts"
    source.write_text('console.log("release runtime probe");\n')
    result = subprocess.run([str(compiler), "compile", str(source), "--target", perry_target,
        "--no-auto-optimize", "--no-cache", "-o", str(output / "probe.wasm")],
        cwd=output, env=env, capture_output=True, timeout=120)
    (output / "missing-runtime.log").write_bytes(result.stdout + result.stderr)
    if result.returncode == 0 or b"Could not find libperry_runtime.a" not in result.stderr:
        raise SystemExit("The isolated compiler did not reject its missing WASI runtime; inspect missing-runtime.log")
    version = wasi_release.tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    wasi_release.install(args.bundle, installed / args.target / "release", args.revision, version,
                         args.target)
    # `--flavour p1` also checks that every module built from the installed
    # runtime imports nothing but `wasi_snapshot_preview1`.
    for suite in ["smoke", "acceptance", "gap"]:
        subprocess.run([sys.executable, str(root / "scripts/wasi_run.py"), str(compiler),
            "--flavour", flavour, "--suite", suite, "--output", str(output / suite),
            "--keep-intermediates"], cwd=output, env=env, check=True)


if __name__ == "__main__":
    main()
