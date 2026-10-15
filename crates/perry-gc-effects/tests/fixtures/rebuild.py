#!/usr/bin/env python3
"""Reproduce the checked-in header-free LLVM 22 fixtures (not used by tests)."""
from pathlib import Path
import os
import subprocess
import tempfile

here = Path(__file__).resolve().parent
root = here.parents[3]
llvm = Path(os.environ.get("PERRY_LLVM_BIN", "/usr/lib/llvm-22/bin"))
targets = {
    "elf-x86_64": "x86_64-unknown-linux-gnu",
    "macho-arm64": "arm64-apple-macos11",
    "coff-x86_64": "x86_64-pc-windows-msvc",
}
with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    for tag, triple in targets.items():
        for plant in (False, True):
            suffix = "-planted" if plant else ""
            src = tmp / f"fixture_{tag}{suffix}.c"
            src.write_text((here / "fixture.c").read_text().replace(
                "PLANT", "x = js_proxy_get(x);" if plant else ""))
            obj = tmp / "8a1b2c3d-hot_text.o"
            out = here / ("libfixture_" + tag + ("-planted" if plant else "") + ".a")
            subprocess.run([str(llvm / "clang"), "-target", triple, "-O2",
                            "-ffunction-sections", "-fno-inline", "-fno-builtin",
                            "-c", str(src), "-o", str(obj)], check=True)
            if tag.startswith("coff"):
                data = bytearray(obj.read_bytes())
                assert data[:2] == b"\x64\x86"
                data[4:8] = bytes(4)  # COFF TimeDateStamp, no executable meaning
                obj.write_bytes(data)
            out.unlink(missing_ok=True)
            subprocess.run([str(llvm / "llvm-ar"), "rcs", str(out), str(obj)], check=True)
        obj = tmp / "9abcdef0-hot_text.o"
        out = here / ("libhot_text_" + tag + ".a")
        subprocess.run([str(llvm / "clang"), "-target", triple, "-ffreestanding",
                        "-O3", "-ffunction-sections", "-c",
                        str(root / "crates/perry-runtime/src/hot_text.c"),
                        "-o", str(obj)], check=True)
        if tag.startswith("coff"):
            data = bytearray(obj.read_bytes())
            assert data[:2] == b"\x64\x86"
            data[4:8] = bytes(4)
            obj.write_bytes(data)
        out.unlink(missing_ok=True)
        subprocess.run([str(llvm / "llvm-ar"), "rcs", str(out), str(obj)], check=True)
