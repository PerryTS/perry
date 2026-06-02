#!/usr/bin/env python3
"""Check one measured run's stdout (and optional output file) against the
cached Bun reference in results/expected.json.

Emits a single-line JSON object on stdout:
    {"output_match": true|false|null, "output_match_reason": "..."}

`null` means: no expected entry exists for the workload (treated as "not
checked" by the driver — used during the bootstrap phase before
expected.json is populated).
"""
import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

TOKEN_RE = re.compile(r"\b([a-zA-Z_][a-zA-Z0-9_]*)=([0-9a-zA-Z_.\-:x+]+)")

# Tokens that are inherently runtime-variable. We never compare these even if
# the reference happens to record them — protects against future workload
# additions that print elapsed-ms / Date.now()-style values inline.
VOLATILE_TOKENS = frozenset({
    "elapsed_ms", "elapsed_us", "elapsed_ns",
    "wall_ms", "duration_ms",
    "timestamp", "now", "started_at", "finished_at",
})


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
    p = argparse.ArgumentParser()
    p.add_argument("--expected-json", required=True)
    p.add_argument("--workload", required=True)
    p.add_argument("--stdout-file", required=True)
    p.add_argument("--output-file", default=None,
                   help="path the run produced (compared by sha256)")
    a = p.parse_args()

    expected_path = Path(a.expected_json)
    if not expected_path.exists():
        print(json.dumps({"output_match": None,
                          "output_match_reason": "no expected.json"}))
        return 0
    expected_all = json.loads(expected_path.read_text())
    expected = expected_all.get(a.workload)
    if not expected:
        print(json.dumps({"output_match": None,
                          "output_match_reason": f"no entry for {a.workload}"}))
        return 0

    actual_text = Path(a.stdout_file).read_text(errors="replace")
    actual_tokens = extract_tokens(actual_text)
    expected_tokens = expected.get("tokens", {})

    mismatches: list[str] = []
    for k, v in expected_tokens.items():
        if k in VOLATILE_TOKENS:
            continue
        av = actual_tokens.get(k)
        if av is None:
            mismatches.append(f"missing {k}=")
        elif av != v:
            mismatches.append(f"{k}={av} (expected {v})")

    if a.output_file and "output_file_sha256" in expected:
        try:
            actual_sha = sha256_of(a.output_file)
        except FileNotFoundError:
            mismatches.append(f"output file not produced: {a.output_file}")
        else:
            if actual_sha != expected["output_file_sha256"]:
                exp_short = expected["output_file_sha256"][:12]
                act_short = actual_sha[:12]
                mismatches.append(
                    f"output file sha mismatch (expected {exp_short}…, got {act_short}…)"
                )

    if mismatches:
        print(json.dumps({"output_match": False,
                          "output_match_reason": "; ".join(mismatches)}))
    else:
        print(json.dumps({"output_match": True, "output_match_reason": ""}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
