#!/usr/bin/env python3
"""Compare literal Error static reads against Node across early realm bootstraps."""

import argparse
import os
from pathlib import Path
import subprocess
import sys

CONSTRUCTORS = (
    "Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError",
    "EvalError", "URIError", "AggregateError", "SuppressedError",
)
STATICS = ("isError", "captureStackTrace", "prepareStackTrace", "stackTraceLimit")
GLOBALS = (
    "TextEncoderStream", "TextDecoderStream", "CompressionStream", "DecompressionStream",
    "Headers", "Request", "Response", "FormData", "Blob", "File",
    "ReadableStream", "WritableStream", "TransformStream",
)


def matrix_source(trigger):
    # Literal property reads are essential: computed keys canonicalize the
    # spelling before the probe and would conceal duplicate atom identities.
    references = [f'globalThis["{ctor}"]["{key}"]'
                  for ctor in CONSTRUCTORS for key in STATICS]
    anchor = "0" if trigger == "none" else f'globalThis["{trigger}"]["prototype"]["constructor"]'
    references.insert(len(STATICS), anchor)
    lines = ["const values:any[]=[" + ",".join(references) + "];"]
    for ci, ctor in enumerate(CONSTRUCTORS):
        for pi, key in enumerate(STATICS):
            index = ci * len(STATICS) + pi + (ci > 0)
            lines.append(f'console.log("{ctor}.{key}",typeof values[{index}],'
                         f'values[{index}]===values[{pi}]);')
    for key in ("parseFloat", "parseInt"):
        lines.append(f'console.log("Number.{key}",typeof Number.{key},Number.{key}==={key});')
    return "\n".join(lines) + "\n"


def cases():
    yield "tiny", ('const values:any[]=[globalThis["Error"]["isError"],'
                   'globalThis["TextEncoderStream"]["prototype"]["constructor"],'
                   'globalThis["TypeError"]["isError"]];\n'
                   'console.log(typeof values[0],typeof values[2],values[0]===values[2]);\n')
    yield "compact", ('import * as events from "node:events";\n'
                      'import * as stream from "node:stream";\n'
                      'const values:any[]=[globalThis["Error"]["isError"],'
                      'globalThis["Error"]["prepareStackTrace"],'
                      'globalThis["TextEncoderStream"]["prototype"]["constructor"],'
                      'globalThis["TypeError"]["isError"],'
                      'globalThis["TypeError"]["prepareStackTrace"]];\n'
                      'console.log("isError",typeof values[0],typeof values[3],values[0]===values[3]);\n'
                      'console.log("prepareStackTrace",typeof values[1],typeof values[4],values[1]===values[4]);\n')
    for trigger in ("none", *GLOBALS):
        yield trigger, matrix_source(trigger)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--runtime-dir", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--node", default="node")
    parser.add_argument("--case", action="append", dest="selected")
    args = parser.parse_args()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(args.runtime_dir.resolve()),
               PERRY_ALLOW_PERRY_FEATURES="1", NODE_NO_WARNINGS="1")
    env.pop("PERRY_FACTS_EXPLAIN", None)
    failures = 0
    rows = ["case\tcompile_rc\tperry_rc\tnode_rc\tmatch"]
    selected = set(args.selected or ())
    available = dict(cases())
    unknown = selected - available.keys()
    if unknown:
        parser.error("unknown cases: " + ", ".join(sorted(unknown)))
    for name, source in available.items():
        if selected and name not in selected:
            continue
        path = work / f"{name}.ts"
        path.write_text(source)
        binary = work / f"{name}.bin"
        with (work / f"{name}.compile.log").open("wb") as log:
            compiled = subprocess.run(
                [str(args.perry.resolve()), "compile", str(path), "--cache-dir",
                 str(work / "cache" / name), "-o", str(binary)],
                env=env, stdout=log, stderr=subprocess.STDOUT, timeout=300, check=False)
        if compiled.returncode:
            failures += 1
            rows.append(f"{name}\t{compiled.returncode}\t-\t-\tFAIL")
            continue
        actual = subprocess.run([str(binary)], env=env, capture_output=True, timeout=30, check=False)
        oracle = subprocess.run([args.node, str(path)], env=env, capture_output=True, timeout=30, check=False)
        actual_output = actual.stdout + actual.stderr
        oracle_output = oracle.stdout + oracle.stderr
        (work / f"{name}.perry.log").write_bytes(actual_output)
        (work / f"{name}.node.log").write_bytes(oracle_output)
        matched = actual.returncode == oracle.returncode == 0 and actual_output == oracle_output
        failures += not matched
        rows.append(f"{name}\t0\t{actual.returncode}\t{oracle.returncode}\t{'PASS' if matched else 'FAIL'}")
        print(rows[-1], flush=True)
    (work / "results.tsv").write_text("\n".join(rows) + "\n")
    return int(failures != 0)


if __name__ == "__main__":
    sys.exit(main())
