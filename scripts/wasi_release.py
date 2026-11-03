#!/usr/bin/env python3
"""Package and verify the standalone WASI runtime from a pinned release checkout.

Each bundle contains only the runtime of one WASI target, wasm32-wasip2 (the
default) or wasm32-wasip1. Native stdlib/UI archives are not part of these
targets. The release's host compiler must enable perry/target-wasi.
"""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
# Each bundle target and the `wasi_run.py --flavour` that qualifies it.
TARGETS = {"wasm32-wasip2": "p2", "wasm32-wasip1": "p1"}
TARGET = "wasm32-wasip2"
LIBRARY = "libperry_runtime.a"


def bundle_name(target=TARGET):
    return f"perry-cross-{target}.tar.gz"


def digest(stream):
    result = hashlib.sha256()
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        result.update(chunk)
    return result.hexdigest()


def verify(bundle, revision, version, target=TARGET):
    """Validate the complete bundle before publishing or extracting any file."""
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("Expected a full source revision")
    with tarfile.open(bundle, "r:gz") as archive:
        entries = archive.getmembers()
        if (len(entries) != 2 or {item.name for item in entries} != {LIBRARY, "manifest.json"}
                or any(not item.isfile() for item in entries)):
            raise ValueError("WASI bundle must contain exactly the runtime and its manifest")
        if archive.getmember("manifest.json").size > 65536:
            raise ValueError("Oversized WASI manifest")
        manifest = json.load(archive.extractfile("manifest.json"))
        if (manifest.get("schema_version") != 1 or manifest.get("target_triple") != target
                or manifest.get("source_revision") != revision
                or manifest.get("perry_version") != version):
            raise ValueError("WASI bundle does not match the requested source, version, or target")
        files = manifest.get("files")
        if not isinstance(files, list) or len(files) != 1 or files[0].get("path") != LIBRARY:
            raise ValueError("Invalid WASI runtime file manifest")
        entry = archive.getmember(LIBRARY)
        if entry.size != files[0].get("size") or not 8 < entry.size <= 512 * 1024 * 1024:
            raise ValueError("WASI runtime size mismatch")
        stream = archive.extractfile(LIBRARY)
        if stream.read(8) != b"!<arch>\n":
            raise ValueError("WASI runtime is not a static archive")
        stream.seek(0)
        if digest(stream) != files[0].get("sha256"):
            raise ValueError("WASI runtime checksum mismatch")
        return manifest


def package(runtime, output, revision, version, target=TARGET):
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("Expected a full source revision")
    with runtime.open("rb") as stream:
        checksum = digest(stream)
    manifest = {
        "schema_version": 1, "perry_version": version, "target_triple": target,
        "source_revision": revision,
        "files": [{"path": LIBRARY, "sha256": checksum, "size": runtime.stat().st_size}],
    }
    encoded = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    output.mkdir(parents=True, exist_ok=True)
    bundle = bundle_name(target)
    destination = output / bundle
    with tempfile.TemporaryDirectory(dir=output) as temporary:
        pending = Path(temporary) / bundle
        with pending.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for name, size, stream in [
                    ("manifest.json", len(encoded), io.BytesIO(encoded)),
                    (LIBRARY, runtime.stat().st_size, runtime.open("rb")),
                ]:
                    with stream:
                        info = tarfile.TarInfo(name)
                        info.size, info.mode, info.mtime = size, 0o644, 0
                        archive.addfile(info, stream)
        verify(pending, revision, version, target)
        os.replace(pending, destination)
    with destination.open("rb") as stream:
        checksum = digest(stream)
    (output / (bundle + ".sha256")).write_text(f"{checksum}  {bundle}\n", encoding="ascii")
    return destination


def install(bundle, destination, revision, version, target=TARGET):
    # No extractall: names and regular-file types must have passed verification.
    verify(bundle, revision, version, target)
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination) as temporary:
        with tarfile.open(bundle, "r:gz") as archive:
            for name in [LIBRARY, "manifest.json"]:
                with archive.extractfile(name) as source, (Path(temporary) / name).open("wb") as target:
                    shutil.copyfileobj(source, target)
        for name in [LIBRARY, "manifest.json"]:
            os.replace(Path(temporary) / name, destination / name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["package", "verify", "install"])
    parser.add_argument("--target", choices=sorted(TARGETS), default=TARGET,
                        help="Rust target triple of the runtime (default: %(default)s)")
    parser.add_argument("--revision", required=True, help="expected full release source SHA")
    parser.add_argument("--version", help="expected Perry version; defaults to this checkout")
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    checkout_version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    version = args.version or checkout_version
    if args.action == "package":
        if not args.runtime or not args.output:
            parser.error("package requires --runtime and --output")
        if version != checkout_version:
            parser.error("package version must match the release checkout")
        actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        if actual != args.revision:
            parser.error("checkout does not match the expected release source")
        subprocess.run(["git", "diff", "--exit-code", "HEAD", "--"], cwd=ROOT, check=True,
                       stdout=subprocess.DEVNULL)
        print(package(args.runtime, args.output, actual, version, args.target))
    else:
        if not args.bundle or (args.action == "install" and not args.output):
            parser.error("verify/install requires --bundle; install also requires --output")
        if args.action == "install":
            install(args.bundle, args.output, args.revision, version, args.target)
        else:
            verify(args.bundle, args.revision, version, args.target)
        print(f"Verified {args.target} runtime for {version} ({args.revision})")


if __name__ == "__main__":
    main()
