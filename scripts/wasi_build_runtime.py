#!/usr/bin/env python3
"""Build the WASI runtime archive on any host with wasi-sdk configured."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def runtime_features() -> list[str]:
    """The perry-runtime default features that build for wasm32."""
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"], cwd=ROOT))
    runtime = next(p for p in metadata["packages"] if p["name"] == "perry-runtime")
    defaults = runtime["features"]["default"]
    # Same documented exclusion as wasi_check.sh: the 64-bit-only allocator.
    # All other defaults, including new ones, build. (turnloop UDP, `node:dgram`,
    # is not compiled for wasm32 at all; its install is a no-op there.)
    excluded = {"alloc-mimalloc"}
    if excluded - set(defaults):
        sys.exit("Stale WASI feature exclusions; update wasi_check.sh and this script")
    return [f for f in defaults if f not in excluded]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gc-instruments", action="store_true", help="include GC instruments for execution qualification")
    parser.add_argument("--image-selftest", action="store_true",
                        help="build the archive that runs the string-image classifier checks at program start "
                             "(use a separate CARGO_TARGET_DIR; see wasi_image_selftest.py)")
    args = parser.parse_args()
    if not os.environ.get("CC_wasm32_wasip2"):
        sys.exit("Configure wasi-sdk first (scripts/wasi_toolchain.ps1 or wasi_toolchain.sh)")
    features = ",".join(f"perry-runtime/{f}" for f in runtime_features())
    if args.gc_instruments:
        features += ",perry-runtime/gc-instruments"
    if args.image_selftest:
        features += ",perry-runtime/wasm-image-selftest"
    command = ["cargo", "build", "--locked", "--release", "-p", "perry-runtime-static",
               "--target", "wasm32-wasip2", "--no-default-features", "--features", features,
               "--message-format=json-render-diagnostics"]
    # Cargo may legitimately reuse a fresh archive. Observe the target
    # artifact message instead of requiring an mtime change on every build.
    required = {"perry_runtime"}
    live = set()
    with subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, text=True) as process:
        for line in process.stdout:
            try:
                message = json.loads(line)
            except ValueError:
                continue
            if (message.get("reason") == "compiler-artifact"
                    and message["target"]["name"] in required):
                for filename in message.get("filenames", []):
                    archive = Path(filename)
                    if ("wasm32-wasip2" in archive.parts and archive.suffix == ".a"
                            and archive.is_file()):
                        print(f"WASI archive: {archive}", flush=True)
                        live.add(message["target"]["name"])
        status = process.wait()
    if status:
        return status
    if live != required:
        sys.exit(f"Cargo reported no wasm32-wasip2 archives for {sorted(required - live)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
