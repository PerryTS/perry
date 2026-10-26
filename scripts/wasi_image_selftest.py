#!/usr/bin/env python3
"""Run the runtime's string-image classifier checks as a linked wasm program.

perry-runtime's own test target does not build for wasm32, and the checks that
matter here depend on the linked layout: the section bounds the linker
synthesizes for `perry_strings`, the data segment below `__data_end`, the
stack, and the heap. So the checks are compiled into a purpose-built runtime
archive (feature `wasm-image-selftest`), run at program start from
`js_gc_init`, and executed here under wasmtime from a program that Perry
compiles and links the same way as any other `--target wasi` program.

`--flavour p1` runs the same checks in a `--target wasip1` core module and
checks that the linked module imports only `wasi_snapshot_preview1`.

Needs the same environment as wasi_build_runtime.py (wasi-sdk configured).
"""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import wasi_imports_check  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
# flavour -> (rust target triple of the archive, perry --target, extra wasmtime flags)
FLAVOURS = {
    "p2": ("wasm32-wasip2", "wasi", []),
    "p1": ("wasm32-wasip1", "wasip1", ["-W", "exceptions=y"]),
}
MINIMUM_CHECKS = 4


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("perry", type=Path, help="perry compiler built with --features target-wasi")
    parser.add_argument("--wasmtime", default=os.environ.get("WASMTIME", "wasmtime"))
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target" / "wasi-image-selftest",
                        help="separate cargo target dir so the selftest archive never replaces the shipped one")
    parser.add_argument("--flavour", choices=sorted(FLAVOURS), default="p2",
                        help="p2: component (--target wasi); p1: core module (--target wasip1)")
    args = parser.parse_args()
    triple, perry_target, wasmtime_flags = FLAVOURS[args.flavour]
    perry = args.perry.resolve()

    env = dict(os.environ, CARGO_TARGET_DIR=str(args.target_dir.resolve()))
    built = subprocess.run([sys.executable, str(ROOT / "scripts" / "wasi_build_runtime.py"), triple, "--image-selftest"],
                           cwd=ROOT, env=env, capture_output=True, text=True)
    sys.stdout.write(built.stdout)
    sys.stderr.write(built.stderr)
    if built.returncode:
        return built.returncode
    archive = re.search(r"^WASI archive: (.+)$", built.stdout, re.M)
    if not archive:
        sys.exit("wasi_build_runtime.py reported no archive")
    archive_dir = Path(archive.group(1)).parent

    with tempfile.TemporaryDirectory(prefix="perry-image-selftest-") as temporary:
        work = Path(temporary)
        source = work / "selftest.ts"
        source.write_text('console.log("image selftest program ran");\n')
        wasm = work / "selftest.wasm"
        compile_env = dict(os.environ, PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1",
                           PERRY_RUNTIME_DIR=str(archive_dir))
        compiled = subprocess.run([str(perry), "compile", str(source), "--target", perry_target, "-o", str(wasm)],
                                  cwd=work, env=compile_env, capture_output=True, text=True)
        if compiled.returncode or not wasm.is_file():
            sys.exit(f"compile failed ({compiled.returncode}):\n{compiled.stdout}{compiled.stderr}")
        if args.flavour == "p1" and wasi_imports_check.check(wasm, {wasi_imports_check.PREVIEW1}):
            sys.exit("the linked wasip1 module imports more than wasi_snapshot_preview1")
        executed = subprocess.run([args.wasmtime, "run", *wasmtime_flags, str(wasm)], cwd=work, capture_output=True, text=True)
    sys.stdout.write(executed.stdout)
    sys.stderr.write(executed.stderr)
    if executed.returncode:
        sys.exit(f"selftest program exited {executed.returncode}")
    summary = re.search(r"(\d+) checks passed, perry_strings (0x[0-9a-f]+)\.\.(0x[0-9a-f]+) \((\d+) bytes\)",
                        executed.stderr)
    if not summary:
        sys.exit("the selftest did not run: the archive was not built with the selftest feature")
    passed, section_bytes = int(summary.group(1)), int(summary.group(4))
    if passed < MINIMUM_CHECKS or section_bytes == 0:
        sys.exit(f"selftest too weak: {passed} checks, {section_bytes}-byte perry_strings section")
    print(f"wasm image selftest passed: {passed} checks, {section_bytes}-byte perry_strings section")
    return 0


if __name__ == "__main__":
    sys.exit(main())
