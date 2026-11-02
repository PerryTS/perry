#!/usr/bin/env python3
"""Execute generated P3 future payloads with independent labels of one resource interface."""
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
    (out / "OWNER").write_text("Actual repeated-label P3 resource futures. Preserve.\n")
    for key, value in os.environ.items():
        if key.startswith("PERRY_GC_") and value:
            raise SystemExit(f"Native host GC controls must be off: {key}")
    env = os.environ | {"CARGO_TARGET_DIR": str(args.target_dir.resolve()),
                       "PERRY_WORKSPACE_ROOT": str(root),
                       "WASI_SDK_PATH": str(args.wasi_sdk.resolve()),
                       "PERRY_RUNTIME_DIR": str(args.native_runtime_dir.resolve())}
    rows = []

    def run(name, command, expected=0, extra=None):
        print("START", name, flush=True)
        with (out / (name + ".log")).open("w") as log:
            status = subprocess.run(list(map(str, command)), cwd=root, env=env | (extra or {}),
                                    stdout=log, stderr=subprocess.STDOUT, timeout=1800).returncode
        rows.append(dict(phase=name, exitCode=status, passed=status == expected))
        (out / "phases.json").write_text(json.dumps(rows, indent=2) + "\n")
        print("DONE", name, status, flush=True)
        if status != expected:
            (out / "result.json").write_text(json.dumps(dict(exitCode=status or 1,
                failedPhase=name, fullWasi030Qualified=False), indent=2) + "\n")
            print((out / (name + ".log")).read_text()[-5000:], flush=True)
            raise SystemExit(status or 1)

    run("build-probes", ["cargo", "-Z", "checksum-freshness", "build", "--locked",
        "--profile", "perry-dev", "-p", "perry-wit", "--example", "async_guest",
        "--example", "actor_compose", "-p", "perry-component-host", "--example", "guest_endpoint_labels"])
    binaries = out / "binaries"
    binaries.mkdir()
    suffix = ".exe" if os.name == "nt" else ""
    tools = {}
    for name in ["async_guest", "actor_compose", "guest_endpoint_labels"]:
        tools[name] = binaries / (name + suffix)
        shutil.copy2(args.target_dir.resolve() / "perry-dev/examples" / (name + suffix), tools[name])
    guest = out / "guest"
    guest.mkdir()
    fixture = root / "test-wasi/components/guest-endpoint-labels"
    for name in ["world.wit", "guest.ts"]:
        shutil.copy2(fixture / name, guest / name)
    generated = guest / "generated"
    run("public-guard", [args.wit.resolve(), "--guest-c", guest / "world.wit", out / "public"], expected=1)
    assert "future/stream guest values" in (out / "public-guard.log").read_text()
    assert not (out / "public").exists()
    run("generate", [tools["async_guest"], guest / "world.wit", "plugin", generated])
    contract_line = next(line for line in (generated / "guest.ts").read_text().splitlines()
                         if line.startswith("const contract = "))
    manifest = json.loads(json.loads(contract_line.removeprefix("const contract = ").removesuffix(";")))
    foreign = "perry:guest-endpoint-labels/foreign@1.0.0"
    labels = [foreign, "left", "right"]
    keys = {item["name"]: item["resources"]["item"] for item in manifest["imports"]}
    assert set(keys) == set(labels) and len(set(keys.values())) == 3
    (out / "nominal-interfaces.json").write_text(json.dumps(keys, indent=2) + "\n")
    clang = args.wasi_sdk.resolve() / "bin" / ("clang" + suffix)
    facade = guest / "facade.component.wasm"
    run("facade-link", [clang, "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g",
        "-Wall", "-Werror", "-I", generated, generated / "perry_producer_facade.c",
        generated / "perry_producer_forward.c", generated / "perry_producer_facade_component_type.o",
        "-Wl,--export=cabi_realloc", "-Wl,--fatal-warnings", "-o", facade])
    components = [facade]
    libraries = []
    api = "perry:guest-endpoint-labels/api@1.0.0"
    marker = "PASS: actual repeated-label C/Perry/C resource futures, 32 wrong-owner rejections, 12 exactly-once drops and Store reuse"
    for kind, archive in [("plain", args.plain_runtime.resolve()), ("instrumented", args.runtime.resolve())]:
        library = guest / ("libguest-" + kind + ".a")
        libraries.append(library)
        controls = {"PERRY_GC_DIAG": "1", "PERRY_GC_VERIFY_EVACUATION": "1"} if kind == "instrumented" else {}
        run(kind + "-compile", [args.perry.resolve(), "compile", guest / "guest.ts", "--target", "wasip3",
            "--output-type", "staticlib", "--no-auto-optimize", "--no-cache", "-o", library], extra=controls)
        producer = guest / (kind + ".producer.component.wasm")
        component = guest / (kind + ".component.wasm")
        run(kind + "-link", [clang, "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g",
            "-Wall", "-Werror", "-Wno-unused-function", "-I", generated,
            generated / "plugin.c", generated / "perry_adapter.c", generated / "plugin_component_type.o",
            "-Wl,--whole-archive", library, "-Wl,--no-whole-archive", archive, "-lsetjmp",
            "-Wl,--export=cabi_realloc", "-Wl,--stack-first", "-Wl,-z,stack-size=8388608",
            "-Wl,--fatal-warnings", "-o", producer])
        run(kind + "-compose", [tools["actor_compose"], producer, facade,
            generated / "perry_producer_facade.json", component])
        components.extend([producer, component])
        declaration = ["--interface-import=" + label for label in labels]
        if kind == "plain":
            run("reject-undeclared-left", [args.p3_contract.resolve(), "--reactor", component,
                "--instance-export=" + api, *[d for d in declaration if d != "--interface-import=left"]], expected=1)
            assert "non-P3 external import: left" in (out / "reject-undeclared-left.log").read_text()
        run(kind + "-contract", [args.p3_contract.resolve(), "--reactor", component,
            "--instance-export=" + api, *declaration])
        report = (out / (kind + "-contract.log")).read_text()
        for label in labels:
            assert "declared application interface " + label in report
        system = [line for line in report.splitlines() if line.startswith("import ")]
        assert system and all(line.endswith("@0.3.0") for line in system)
        run(kind + "-execute", [tools["guest_endpoint_labels"], guest / "world.wit", component])
        assert marker in (out / (kind + "-execute.log")).read_text()
        if kind == "instrumented":
            run("moving", [tools["guest_endpoint_labels"], guest / "world.wit", component, "--moving"])
            log = (out / "moving.log").read_text()
            copied = sum(map(int, re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log)))
            assert marker in log and copied > 0 and "evacuation_ok" in log
            (out / "moving.json").write_text(json.dumps(dict(guestCopiedObjects=copied,
                guestEvacuationVerified=True), indent=2) + "\n")
    paths = [args.perry.resolve(), args.wit.resolve(), args.plain_runtime.resolve(), args.runtime.resolve(),
             *libraries, *components, *tools.values(), *generated.iterdir(), guest / "world.wit", guest / "guest.ts"]
    identities = {str(path): dict(bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest())
                  for path in paths if path.is_file()}
    (out / "artifact-identities.json").write_text(json.dumps(identities, indent=2) + "\n")
    (out / "result.json").write_text(json.dumps(dict(exitCode=0, fullWasi030Qualified=False,
        repeatedLabelInteropQualified=True, wrongOwnerRejectionsPerRun=32,
        exactlyOnceDropsPerRun=12, source=subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()), indent=2) + "\n")


if __name__ == "__main__":
    main()
