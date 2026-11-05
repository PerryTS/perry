#!/usr/bin/env python3
"""Node-API addons linked statically into a WASI component.

Builds `addon.c` twice with wasi-sdk (as `hello` and `second`, which define the
same global symbols, entry included), stages the project with its two addon
packages, compiles `one.ts` (one addon: a function and a typed array) and
`two.ts` (both addons: isolation) with `perry compile --target wasi`, runs them
under wasmtime and compares stdout with the `.out` files.

    python3 scripts/wasi_addon/run.py <perry> --wasmtime <wasmtime> --output <dir>

Needs WASI_SDK_PATH and the wasm32-wasip2 runtime (PERRY_RUNTIME_DIR).
"""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ADDONS = ("hello", "second")
CASES = ("one", "two")


def build_addons(sdk: Path, project: Path) -> None:
    build = project / "build"
    build.mkdir(parents=True)
    for name in ADDONS:
        obj = build / f"{name}.o"
        subprocess.run([str(sdk / "bin/clang"), "--target=wasm32-wasip2",
                        f"--sysroot={sdk / 'share/wasi-sysroot'}", "-O2", "-c",
                        f'-DADDON_NAME="{name}"', str(HERE / "addon.c"), "-o", str(obj)],
                       check=True)
        subprocess.run([str(sdk / "bin/llvm-ar"), "crs", str(build / f"{name}.a"), str(obj)],
                       check=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("perry", type=Path)
    parser.add_argument("--wasmtime", default="wasmtime")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=600)
    args = parser.parse_args()
    sdk = os.environ.get("WASI_SDK_PATH")
    if not sdk:
        sys.exit("set WASI_SDK_PATH to wasi-sdk")
    project = args.output.resolve() / "project"
    if project.exists():
        shutil.rmtree(project)
    project.mkdir(parents=True)
    for name in ("package.json", *(f"{case}.ts" for case in CASES)):
        shutil.copy(HERE / name, project / name)
    shutil.copytree(HERE / "packages", project / "node_modules")
    build_addons(Path(sdk), project)
    env = dict(os.environ, PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1")
    failures = 0
    for case in CASES:
        wasm = project / f"{case}.wasm"
        compiled = subprocess.run([str(args.perry.resolve()), "compile", f"{case}.ts",
                                   "--target", "wasi", "-o", str(wasm)],
                                  cwd=project, env=env, capture_output=True, text=True,
                                  timeout=args.timeout)
        if compiled.returncode != 0:
            failures += 1
            tail = "\n".join((compiled.stdout + compiled.stderr).strip().splitlines()[-8:])
            print(f"FAIL {case} (compile)\n{tail}")
            continue
        ran = subprocess.run([args.wasmtime, "run", str(wasm)], cwd=project,
                             capture_output=True, text=True, timeout=args.timeout)
        expected = (HERE / f"{case}.out").read_text()
        if ran.returncode != 0 or ran.stdout != expected:
            failures += 1
            print(f"FAIL {case} (run rc={ran.returncode})\nstdout:\n{ran.stdout}"
                  f"stderr:\n{ran.stderr[-2000:]}")
            continue
        print(f"PASS {case}")
    print(f"{len(CASES) - failures}/{len(CASES)} passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
