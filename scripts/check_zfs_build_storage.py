#!/usr/bin/env python3
"""Validate pre-provisioned Linux agent storage; never create or alter a dataset."""
import os
from pathlib import Path
import subprocess
import sys


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def validate(cache, targets, command=run):
    datasets = []
    for label, path in (("MBX_CACHE_DIR", cache), ("MBX_TARGET_ROOT", targets)):
        if not path or not Path(path).is_dir():
            raise ValueError(f"{label} must name an existing build-storage directory")
        resolved = str(Path(path).resolve())
        if command("findmnt", "-n", "-o", "FSTYPE", "-T", resolved) != "zfs":
            raise ValueError(f"{label} must be on ZFS: {resolved}")
        dataset = command("findmnt", "-n", "-o", "SOURCE", "-T", resolved)
        compression = command("zfs", "get", "-H", "-o", "value", "compression", dataset)
        if compression != "lz4" and compression != "zstd" and not compression.startswith("zstd-"):
            raise ValueError(f"{dataset}: enable lz4 or zstd compression before building")
        if command("zfs", "get", "-H", "-o", "value", "dedup", dataset) != "off":
            raise ValueError(f"{dataset}: use compression with dedup=off for build storage")
        quota = command("zfs", "get", "-H", "-o", "value", "quota", dataset)
        if quota in ("none", "0", "-"):
            raise ValueError(f"{dataset}: set a finite quota for the agent build dataset")
        datasets.append(dataset)
    if datasets[0] != datasets[1]:
        raise ValueError("MBX store and managed targets must be on the same ZFS dataset")
    return datasets[0]


def main():
    if sys.platform != "linux":
        sys.exit("ZFS agent builds require a Linux host; use build-dev on this platform")
    if os.environ.get("CARGO_TARGET_DIR") or os.environ.get("CARGO_BUILD_BUILD_DIR"):
        sys.exit("Unset Cargo target/build directory overrides so MBX owns ZFS placement")
    try:
        dataset = validate(os.environ.get("MBX_CACHE_DIR"), os.environ.get("MBX_TARGET_ROOT"))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(f"Build storage check failed: {error}. See docs/src/contributing/build-storage.md")
    print(f"PASS: compressed, quota-bounded ZFS build storage ({dataset})")


if __name__ == "__main__":
    main()
