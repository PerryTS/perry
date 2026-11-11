#!/usr/bin/env python3
"""Rebuild the native throw-origin and raw-entry sabotage archive fixtures."""
from pathlib import Path
import os
import subprocess
import tempfile

here = Path(__file__).resolve().parent
llvm = Path(os.environ.get("PERRY_LLVM_BIN", "/usr/lib/llvm-22/bin"))
with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    for tag, target in [
        ("elf-x86_64", "x86_64-unknown-linux-gnu"),
        ("macho-arm64", "arm64-apple-macos11"),
        ("coff-x86_64", "x86_64-pc-windows-msvc"),
    ]:
        for planted in (False, True):
            source = tmp / "throw.c"
            source.write_text((here / "throw-fixture.c").read_text().replace(
                "PLANT", "js_throw(v);" if planted else ""))
            obj = tmp / "runtime.perry_runtime.hash.rcgu.o"
            subprocess.run([str(llvm / "clang"), "-target", target, "-O2",
                            "-fno-inline", "-ffunction-sections", "-c", str(source),
                            "-o", str(obj)], check=True)
            if tag.startswith("coff"):
                data = bytearray(obj.read_bytes())
                data[4:8] = bytes(4)
                obj.write_bytes(data)
            suffix = "-raw" if planted else ""
            archive = here / f"libthrow_{tag}{suffix}.a"
            archive.unlink(missing_ok=True)
            subprocess.run([str(llvm / "llvm-ar"), "rcs", str(archive), str(obj)], check=True)
