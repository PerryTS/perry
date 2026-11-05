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
# Moving collections with the evacuation verifier, and their log. These are GC
# diagnostics: a program installs them only when they are set while compiling,
# so the same knobs go to the compile and to the run.
GC_KNOBS = {"PERRY_GC_FORCE_EVACUATE": "1", "PERRY_GC_VERIFY_EVACUATION": "1",
            "PERRY_GC_DIAG": "1", "PERRY_GC_LOG": "1"}


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
        (work / "peer.ts").write_text(
            'export function peer() { return { wrt2cSharedKey: 19, peerOnly: "peer" }; }\n')
        source.write_text(
            'import { peer } from "./peer";\n'
            'import { collect } from "perry/gc";\n'
            'const local = { wrt2cSharedKey: 23, localOnly: "local" };\n'
            'const remote = peer();\n'
            'for (let i = 0; i < 4; i++) collect();\n'
            'console.log(local.wrt2cSharedKey + remote.wrt2cSharedKey);\n'
            'console.log(Object.keys(local).join(","), Object.keys(remote).join(","));\n'
            'console.log("image selftest program ran");\n')
        wasm = work / "selftest.wasm"
        (work / "ir").mkdir()
        compile_env = dict(os.environ, PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1",
                           PERRY_RUNTIME_DIR=str(archive_dir), PERRY_SAVE_LL=str(work / "ir"), **GC_KNOBS)
        compiled = subprocess.run([str(perry), "compile", str(source), "--target", perry_target, "-o", str(wasm)],
                                  cwd=work, env=compile_env, capture_output=True, text=True)
        if compiled.returncode or not wasm.is_file():
            sys.exit(f"compile failed ({compiled.returncode}):\n{compiled.stdout}{compiled.stderr}")
        if args.flavour == "p1" and wasi_imports_check.check(wasm):
            sys.exit("the linked wasip1 module imports more than wasi_snapshot_preview1")
        definitions = sum(
            '@perry_atom_77727432635368617265644b6579 = weak_odr' in ir.read_text()
            for ir in (work / "ir").rglob("*.ll"))
        if definitions < 2:
            sys.exit(f"coalescing gate needs two defining LLVM modules, found {definitions}")
        executed = subprocess.run(
            [args.wasmtime, "run", *wasmtime_flags,
             *(arg for knob, value in GC_KNOBS.items() for arg in ("--env", f"{knob}={value}")), str(wasm)],
            cwd=work, capture_output=True, text=True)
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
    # A verified evacuating minor (the `copying_minors` counter is an
    # instrument, absent from this uninstrumented archive) and one full
    # collection per collect().
    verified = len(re.findall(r"^\[gc-verify\] minor=\d+ evacuation_ok ", executed.stderr, re.M))
    if verified == 0 or executed.stderr.count("[gc-log] kind=full") < 4:
        sys.exit("the moving/full-collection subject was not live")
    seeds = re.search(r"wasm static seeds: (\d+) seeds, (\d+) image, (\d+) by-names mints", executed.stderr)
    if not seeds or int(seeds.group(1)) < 2 or seeds.group(1) != seeds.group(2) or int(seeds.group(3)) != 0:
        sys.exit("the linked fixture must install at least two image seeds and mint none by names")
    atom = re.search(r"wrt2cSharedKey atom=(0x[0-9a-f]+) string=(0x[0-9a-f]+) intern=(0x[0-9a-f]+) copies=1", executed.stderr)
    if not atom or atom.group(2) != atom.group(3):
        sys.exit("two-module atom coalescing / intern identity gate did not run")
    if "42\nwrt2cSharedKey,localOnly wrt2cSharedKey,peerOnly\nimage selftest program ran\n" != executed.stdout:
        sys.exit("moving collections changed the fixture's values or keys")
    print(f"wasm image selftest passed: {passed} checks, {section_bytes}-byte perry_strings section; "
          f"{definitions} defining modules, {seeds.group(1)} image seeds, zero by-names mints")
    return 0


if __name__ == "__main__":
    sys.exit(main())
