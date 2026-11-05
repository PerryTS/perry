#!/usr/bin/env python3
"""Select a complete target/ext set; recovery accepts only public npm bytes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile


def select(root, catalog, version, verify_npm):
    triples = json.loads(catalog.read_text())
    expected = {
        f"@perryts/perry-{kind}-{triple}": (kind, triple)
        for kind, key in (("target", "targets"), ("ext", "ext"))
        for triple in triples[key]
    }
    candidates = {name: [] for name in expected}
    for path in sorted(root.rglob("*.tgz")):
        # npm-pack-check's root also contains the wrapper and host packages.
        if not path.name.startswith(("perryts-perry-target-", "perryts-perry-ext-")):
            continue
        with tarfile.open(path, "r:gz") as archive:
            package = json.load(archive.extractfile("package/package.json"))
            target = json.load(archive.extractfile("package/target.json"))
        name = package["name"]
        if name not in expected or target["target_triple"] != expected[name][1]:
            raise ValueError(f"{path}: unexpected target/ext package {name}")
        packed_version = package["version"]
        if target["perry_version"] != packed_version or (version and packed_version != version):
            raise ValueError(f"{path}: package/target version does not match {version or packed_version}")
        candidates[name].append((path, packed_version))

    selected = []
    for name, choices in candidates.items():
        if not choices:
            raise ValueError(f"missing packed package: {name}")
        if verify_npm:
            public_sha = json.loads(subprocess.check_output([
                "npm", "view", f"{name}@{version}", "dist.shasum", "--json",
                "--registry=https://registry.npmjs.org",
            ], text=True))
            if not isinstance(public_sha, str) or len(public_sha) != 40:
                raise ValueError(f"{name}@{version}: no public npm SHA-1")
            matching = []
            for path, packed_version in choices:
                with path.open("rb") as stream:
                    sha = hashlib.sha1()
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                        sha.update(chunk)
                if sha.hexdigest() == public_sha:
                    matching.append((path, packed_version))
            if not matching:
                raise ValueError(f"{name}@{version}: no artifact matches public npm SHA-1 {public_sha}")
            choices = matching
            print(f"OK {name}@{version} {public_sha}", file=sys.stderr)
        elif len(choices) != 1:
            raise ValueError(f"{name}: multiple tarballs require npm verification")
        path, packed_version = choices[0]
        kind, triple = expected[name]
        selected.append((str(path.resolve()), kind, triple, packed_version, name.split("/")[1]))
    return selected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--catalog", type=Path, default=Path(os.environ.get("PACKAGED_TRIPLES", "npm/packaged-triples.json")))
    parser.add_argument("--version", default="")
    parser.add_argument("--verify-npm", action="store_true")
    args = parser.parse_args()
    if args.verify_npm and not args.version:
        parser.error("--verify-npm requires --version")
    for row in select(args.root, args.catalog, args.version, args.verify_npm):
        print("\t".join(row))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"::error::{error}")
