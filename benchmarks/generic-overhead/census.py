#!/usr/bin/env python3
"""Compile the same probes with two Perry builds and count ARM64 instructions.

Counts include cold blocks and exclude callees. They measure static code, not
executed instructions or elapsed time. Each arm keeps its IR and disassembly.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def census(compiler: Path, output: Path, objdump: str, opt: str) -> dict:
    output.mkdir(parents=True, exist_ok=True)
    source = Path(__file__).with_name("probes.ts").resolve()
    env = dict(os.environ, PERRY_NO_AUTO_OPTIMIZE="1", PERRY_LL_OPT_LEVEL=opt)
    with (output / "compile.log").open("w") as log:
        subprocess.run(
            [str(compiler), "compile", str(source), "--no-link", "--no-codegen",
             "--trace", "llvm", "-o", str(output / "probes.o")],
            cwd=output, env=env, stdout=log, stderr=subprocess.STDOUT, check=True,
        )
    asm = subprocess.check_output(
        [objdump, "-dr", "--no-show-raw-insn", str(output / "probes.o")], text=True,
    )
    if "file format mach-o arm64" not in asm and "file format elf64-littleaarch64" not in asm:
        raise RuntimeError("This instruction census expects an ARM64 object")
    (output / "probes.asm").write_text(asm)
    functions = {}
    name = None
    for line in asm.splitlines():
        label = re.match(r"^[0-9a-f]+ <(.+)>:$", line)
        if label:
            symbol = label[1].lstrip("_")
            name = symbol.split("__", 1)[1] if symbol.startswith("perry_fn_probes_ts__") else None
            if name is not None:
                functions[name] = 0
        elif name is not None and re.match(r"^\s+[0-9a-f]+:\s+[a-z][a-z0-9.]*\s", line + " "):
            functions[name] += 1
    if not functions:
        raise RuntimeError("No probe functions found in disassembly")
    return {"compiler": str(compiler), "sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
            "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "opt": opt, "static_instructions": functions}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--objdump", default="llvm-objdump")
    parser.add_argument("--opt", choices=["s", "3"], default="s")
    args = parser.parse_args()
    output = args.out.resolve()
    result = {arm: census(getattr(args, arm).resolve(), output / arm, args.objdump, args.opt)
              for arm in ["before", "after"]}
    report = json.dumps(result, indent=2) + "\n"
    (output / "census.json").write_text(report)
    print(report, end="")


if __name__ == "__main__":
    main()
