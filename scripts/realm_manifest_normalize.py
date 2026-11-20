#!/usr/bin/env python3
"""Normalize declaration provenance only; preserve every other realm-v1 fact."""
import argparse
import hashlib
import json
from pathlib import Path


def normalize(manifest):
    if not isinstance(manifest, dict) or manifest.get("version") != 1:
        raise ValueError("expected realm-v1 manifest")
    declarations = manifest.get("declarations")
    if not isinstance(declarations, list):
        raise ValueError("missing declarations")
    result = dict(manifest)
    result["declarations"] = [
        {key: value for key, value in declaration.items() if key not in ("source", "line")}
        for declaration in declarations
    ]
    return json.dumps(result, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--equal-to", type=Path)
    args = parser.parse_args()
    normalized = normalize(json.loads(args.manifest.read_bytes()))
    if args.output:
        args.output.write_bytes(normalized)
    print(hashlib.sha256(normalized).hexdigest(), args.manifest)
    if args.equal_to:
        reference = normalize(json.loads(args.equal_to.read_bytes()))
        print(hashlib.sha256(reference).hexdigest(), args.equal_to)
        if normalized != reference:
            parser.exit(1, "normalized realm manifests differ\n")


if __name__ == "__main__":
    main()
