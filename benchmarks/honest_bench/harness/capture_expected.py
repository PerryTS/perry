#!/usr/bin/env python3
"""Capture reference output for one workload by running it under Bun.

Bun is the truth source for honest_bench's output-correctness gate (it runs on
every workload via the existing harness). This helper runs the workload once
under Bun, extracts canonical `key=value` tokens from stdout (hash, checksum,
counts, dimensions — anything that isn't a wall-clock timestamp), optionally
sha256s a produced output file, and emits a JSON object describing the
expected state. The driver (run.sh) writes the union into results/expected.json
which is committed and only updated when output semantics intentionally change.

Usage:
    capture_expected.py <workload> [--output-file=PATH] -- <command...>

Example:
    capture_expected.py image_convolution -- bun run image_conv.ts
    capture_expected.py json_pipeline_small --output-file=/tmp/out_bun.json -- \\
        bun run json_pipeline.ts /tmp/in.json /tmp/out_bun.json
"""
import argparse
import hashlib
import json
import re
import subprocess
import sys

# Match `name=value` pairs where value is a non-whitespace alphanumeric / hex
# / dotted / signed / colon-bearing token. Excludes the `=` so it can't gobble
# `Date.now()`-style assignments — the workloads we run only emit
# canonical-form tokens (hash=…, records_in=…, dims=NxM, etc).
TOKEN_RE = re.compile(r"\b([a-zA-Z_][a-zA-Z0-9_]*)=([0-9a-zA-Z_.\-:x+]+)")


def extract_tokens(text: str) -> dict:
    out: dict[str, str] = {}
    for m in TOKEN_RE.finditer(text):
        out[m.group(1)] = m.group(2)
    return out


def sha256_of(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    # Split on `--` manually so options like `--output-file=…` aren't
    # swallowed by argparse.REMAINDER.
    argv = sys.argv[1:]
    if "--" in argv:
        idx = argv.index("--")
        parser_args, cmd = argv[:idx], argv[idx + 1:]
    else:
        parser_args, cmd = argv, []

    p = argparse.ArgumentParser()
    p.add_argument("workload")
    p.add_argument("--output-file", default=None,
                   help="path to a file the workload writes (will be sha256'd)")
    a = p.parse_args(parser_args)

    if not cmd:
        print("error: missing command after --", file=sys.stderr)
        return 2

    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.stderr.write(f"reference command failed (exit {r.returncode}):\n")
        sys.stderr.write(r.stderr[:400])
        return 2

    tokens = extract_tokens(r.stdout)
    out = {
        "workload": a.workload,
        "tokens": tokens,
        "stdout_len": len(r.stdout),
    }
    if a.output_file:
        try:
            out["output_file_sha256"] = sha256_of(a.output_file)
            out["output_file_path"] = a.output_file
        except FileNotFoundError:
            sys.stderr.write(f"warning: --output-file {a.output_file} not produced\n")

    print(json.dumps(out, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
