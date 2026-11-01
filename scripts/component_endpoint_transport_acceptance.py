#!/usr/bin/env python3
"""Execute actual P3 guest endpoint custody; guest read/write APIs stay pending."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["perry", "wit", "native-runtime-dir", "plain-runtime", "runtime",
                 "wasi-sdk", "target-dir", "p3-contract", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    (out / "OWNER").write_text("Actual P3 guest endpoint transport evidence; preserve.\n")
    for key, value in os.environ.items():
        if key.startswith("PERRY_GC_") and value:
            raise SystemExit(f"Native host GC controls must be off: {key}")
    env = {**os.environ, "CARGO_TARGET_DIR": str(args.target_dir.resolve()),
           "PERRY_WORKSPACE_ROOT": str(root), "WASI_SDK_PATH": str(args.wasi_sdk.resolve()),
           "PERRY_RUNTIME_DIR": str(args.native_runtime_dir.resolve())}
    rows = []

    def run(name, command, expected=0):
        with (out / (name + ".log")).open("w") as log:
            status = subprocess.run([str(x) for x in command], cwd=root, env=env,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=1800).returncode
        passed = status == expected
        rows.append(dict(phase=name, exitCode=status, passed=passed))
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print(f"{'PASS' if passed else 'FAIL'} {name}", flush=True)
        if not passed:
            raise SystemExit(status or 1)

    run("build-probes", ["cargo", "-Z", "checksum-freshness", "build", "--locked",
                         "--profile", "perry-dev", "-p", "perry-wit", "--example", "async_guest",
                         "-p", "perry-component-host", "--example", "guest_endpoint_transport"])
    binaries = out / "binaries"
    binaries.mkdir()
    suffix = ".exe" if os.name == "nt" else ""
    for name in ["async_guest", "guest_endpoint_transport"]:
        shutil.copy2(args.target_dir.resolve() / "perry-dev/examples" / (name + suffix), binaries / (name + suffix))
    guest = out / "guest"
    guest.mkdir()
    fixture = root / "test-wasi/components/guest-endpoint-transport"
    for name in ["world.wit", "guest.ts"]:
        shutil.copy2(fixture / name, guest / name)
    generated = guest / "generated"
    run("public-guard", [args.wit.resolve(), "--guest-c", guest / "world.wit", out / "public"], expected=1)
    assert "future/stream guest values" in (out / "public-guard.log").read_text()
    assert not (out / "public").exists(), "guard must reject before publishing files"
    run("generate", [binaries / ("async_guest" + suffix), guest / "world.wit", "plugin", generated])
    library = guest / "libguest.a"
    run("compile", [args.perry.resolve(), "compile", guest / "guest.ts", "--target", "wasip3",
                    "--output-type", "staticlib", "--no-auto-optimize", "--no-cache", "-o", library])
    components = []
    marker = "PASS: actual Perry P3 endpoint transport, compound/char/void/unit/byte/nested values, partial chunks, explicit/GC cleanup and Store reuse"
    for kind, archive in [("plain", args.plain_runtime.resolve()), ("instrumented", args.runtime.resolve())]:
        component = guest / (kind + ".component.wasm")
        run(kind + "-link", [args.wasi_sdk.resolve() / "bin" / ("clang" + suffix),
                             "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g",
                             "-Wall", "-Werror", "-Wno-unused-function", "-I", generated,
                             generated / "plugin.c", generated / "perry_adapter.c",
                             generated / "plugin_component_type.o", "-Wl,--whole-archive", library,
                             "-Wl,--no-whole-archive", archive, "-lsetjmp", "-Wl,--export=cabi_realloc",
                             "-Wl,--stack-first", "-Wl,-z,stack-size=8388608", "-Wl,--fatal-warnings", "-o", component])
        components.append(component)
        if kind == "plain":
            run("reject-undeclared-types", [args.p3_contract.resolve(), "--reactor", component,
                                            "pass-future"], expected=1)
            assert "non-P3 external import: payload" in (out / "reject-undeclared-types.log").read_text()
        run(kind + "-contract", [args.p3_contract.resolve(), "--reactor", component,
                                 "pass-future", "pass-stream", "pass-pair", "character", "empty",
                                 "units", "bytes", "nested-value", "dispose", "forget", "checkpoint",
                                 "--type-import=payload", "--type-import=pending",
                                 "--type-import=values", "--type-import=pair"])
        run(kind + "-execute", [binaries / ("guest_endpoint_transport" + suffix), guest / "world.wit", component])
        assert marker in (out / (kind + "-execute.log")).read_text()
        if kind == "instrumented":
            run("moving", [binaries / ("guest_endpoint_transport" + suffix), guest / "world.wit", component, "--moving"])
            log = (out / "moving.log").read_text()
            copied = sum(map(int, re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log)))
            assert marker in log and copied > 0 and "evacuation_ok" in log
            (out / "moving.json").write_text(json.dumps(dict(guestCopiedObjects=copied, guestEvacuationVerified=True), indent=2) + "\n")
    paths = [args.perry.resolve(), args.wit.resolve(), args.plain_runtime.resolve(), args.runtime.resolve(),
             library, *components, *binaries.iterdir(), *generated.iterdir()]
    identities = {str(p): dict(bytes=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                  for p in paths if p.is_file()}
    (out / "artifact-identities.json").write_text(json.dumps(identities, indent=2) + "\n")


if __name__ == "__main__":
    main()
