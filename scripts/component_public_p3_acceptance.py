#!/usr/bin/env python3
"""Exercise public P3 generation/composition, resource futures and typed native loading."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["perry", "wit", "native-runtime-dir", "plain-runtime", "runtime",
                 "wasi-sdk", "target-dir", "p3-contract", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--probe-dir", type=Path, help="explicit prebuilt independent resource host")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    (out / "OWNER").write_text("Public P3 generation/composition and typed loading evidence. Preserve.\n")
    for key, value in os.environ.items():
        if key.startswith("PERRY_GC_") and value:
            raise SystemExit(f"Native host GC controls must be off: {key}")
    env = os.environ | {"PERRY_WORKSPACE_ROOT": str(root), "PERRY_RUNTIME_DIR": str(args.native_runtime_dir.resolve()),
                       "WASI_SDK_PATH": str(args.wasi_sdk.resolve()), "PERRY_CODEGEN_CPU_SLOTS": "4"}
    rows = []

    def run(name, argv, expected=0):
        print("START", name, flush=True)
        with (out / (name + ".stdout")).open("w") as stdout, (out / (name + ".stderr")).open("w") as stderr:
            code = subprocess.run(list(map(str, argv)), cwd=out, env=env,
                                  stdout=stdout, stderr=stderr, timeout=1800).returncode
        rows.append(dict(phase=name, exitCode=code, passed=code == expected))
        (out / "phases.json").write_text(json.dumps(rows, indent=2) + "\n")
        print("DONE", name, code, flush=True)
        if code != expected:
            (out / "result.json").write_text(json.dumps(dict(exitCode=code or 1, failedPhase=name,
                fullWasi030Qualified=False), indent=2) + "\n")
            print((out / (name + ".stderr")).read_text()[-5000:], flush=True)
            raise SystemExit(code or 1)

    common = []
    for name in ["perry", "wit", "native-runtime-dir", "plain-runtime", "runtime", "wasi-sdk", "target-dir", "p3-contract"]:
        common += ["--" + name, getattr(args, name.replace("-", "_")).resolve()]
    probe = ["--probe-dir", args.probe_dir.resolve()] if args.probe_dir else []
    run("public-resource-futures", [sys.executable, root / "scripts/component_endpoint_labels_acceptance.py",
        *common, *probe, "--public-p3", "--output", out / "labels"])
    for label, payload in [("future-borrow", "future<borrow<item>>"), ("stream-borrow", "stream<borrow<item>>"),
                           ("nested-borrow", "future<list<borrow<item>>>")]:
        wit = out / (label + ".wit")
        wit.write_text("package perry:invalid-parameters; interface api { resource item; type pending = " + payload +
                       "; consume: func(value: pending); } world plugin { export api; }\n")
        output = out / (label + "-generated")
        run("reject-" + label, [args.wit.resolve(), "--guest-c", "--target", "wasip3", wit, output], 1)
        assert "async endpoint payloads cannot contain borrow<T>" in (out / ("reject-" + label + ".stderr")).read_text()
        assert not output.exists()
    guest = out / "async-only"
    guest.mkdir()
    for name in ["world.wit", "guest.ts", "host.ts"]:
        shutil.copy2(root / "test-wasi/components/public-async" / name, guest / name)
    generated = guest / "generated"
    run("async-only-generate", [args.wit.resolve(), "--guest-c", "--target", "wasip3", guest / "world.wit", generated])
    suffix = ".exe" if os.name == "nt" else ""
    clang = args.wasi_sdk.resolve() / "bin" / ("clang" + suffix)
    facade = guest / "facade.component.wasm"
    run("async-only-facade-link", [clang, "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g", "-Wall", "-Werror",
        "-I", generated, generated / "perry_producer_facade.c", generated / "perry_producer_forward.c",
        generated / "perry_producer_facade_component_type.o", "-Wl,--export=cabi_realloc", "-Wl,--fatal-warnings", "-o", facade])
    library = guest / "libguest.a"
    run("async-only-compile", [args.perry.resolve(), "compile", guest / "guest.ts", "--target", "wasip3",
        "--output-type", "staticlib", "--no-auto-optimize", "--no-cache", "-o", library])
    producer = guest / "producer.component.wasm"
    run("async-only-producer-link", [clang, "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g", "-Wall", "-Werror",
        "-Wno-unused-function", "-I", generated, generated / "plugin.c", generated / "perry_adapter.c", generated / "plugin_component_type.o",
        "-Wl,--whole-archive", library, "-Wl,--no-whole-archive", args.plain_runtime.resolve(), "-lsetjmp",
        "-Wl,--export=cabi_realloc", "-Wl,--stack-first", "-Wl,-z,stack-size=8388608", "-Wl,--fatal-warnings", "-o", producer])
    component = guest / "public.component.wasm"
    run("async-only-compose", [args.wit.resolve(), "compose", producer, facade, generated / "perry_producer_facade.json", component])
    run("async-only-contract", [args.p3_contract.resolve(), "--reactor", component, "ping"])
    native = guest / ("host" + suffix)
    run("async-only-host-compile", [args.perry.resolve(), "compile", guest / "host.ts", "--no-auto-optimize", "--no-cache", "-o", native])
    run("async-only-host-execute", [native, component])
    assert (out / "async-only-host-execute.stdout").read_text() == "PUBLIC ALL-ASYNC P3 PASS\n"
    paths = [args.perry.resolve(), args.wit.resolve(), args.plain_runtime.resolve(), args.runtime.resolve(),
             facade, library, producer, component, native, *generated.iterdir()]
    identities = {str(path): dict(bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest())
                  for path in paths if path.is_file()}
    (out / "artifact-identities.json").write_text(json.dumps(identities, indent=2) + "\n")
    (out / "result.json").write_text(json.dumps(dict(exitCode=0, publicP3PipelineQualified=True,
        generatedTypedNativeBindingsExecuted=True, invalidBorrowedEndpointsRejectedBeforeOutput=True,
        fullWasi030Qualified=False), indent=2) + "\n")


if __name__ == "__main__":
    main()
