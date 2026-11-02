#!/usr/bin/env python3
"""Verify terminal actor cancellation and native-child retirement with a real C caller."""
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
    (out / "OWNER").write_text("Returned private actor canonical cancellation evidence. Preserve.\n")
    for key, value in os.environ.items():
        if key.startswith("PERRY_GC_") and value:
            raise SystemExit(f"Native host GC controls must be off: {key}")
    env = os.environ | {"CARGO_TARGET_DIR": str(args.target_dir.resolve()),
                       "PERRY_WORKSPACE_ROOT": str(root),
                       "PERRY_RUNTIME_DIR": str(args.native_runtime_dir.resolve()),
                       "WASI_SDK_PATH": str(args.wasi_sdk.resolve())}
    rows = []

    def run(name, command, expected=0, extra=None):
        print("START", name, flush=True)
        with (out / (name + ".log")).open("w") as log:
            code = subprocess.run(list(map(str, command)), cwd=root, env=env | (extra or {}),
                                  stdout=log, stderr=subprocess.STDOUT, timeout=1800).returncode
        rows.append(dict(phase=name, exitCode=code, passed=code == expected))
        (out / "phases.json").write_text(json.dumps(rows, indent=2) + "\n")
        print("DONE", name, code, flush=True)
        if code != expected:
            (out / "result.json").write_text(json.dumps(dict(exitCode=code or 1,
                failedPhase=name, fullWasi030Qualified=False), indent=2) + "\n")
            print((out / (name + ".log")).read_text()[-5000:], flush=True)
            raise SystemExit(code or 1)

    run("build-probes", ["cargo", "-Z", "checksum-freshness", "build", "--locked", "--profile", "perry-dev",
        "-p", "perry-wit", "--example", "async_guest", "--example", "canonical_c",
        "-p", "perry-component-host", "--example", "guest_returned_actor"])
    binaries = out / "binaries"
    binaries.mkdir()
    suffix = ".exe" if os.name == "nt" else ""
    tools = {}
    for name in ["async_guest", "canonical_c", "guest_returned_actor"]:
        tools[name] = binaries / (name + suffix)
        shutil.copy2(args.target_dir.resolve() / "perry-dev/examples" / (name + suffix), tools[name])
    guest = out / "guest"
    guest.mkdir()
    fixture = root / "test-wasi/components/guest-returned-actor"
    for name in ["world.wit", "guest.ts", "supervisor.c"]:
        shutil.copy2(fixture / name, guest / name)
    generated = guest / "generated"
    caller = guest / "caller"
    run("public-guard", [args.wit.resolve(), "--guest-c", guest / "world.wit", out / "public", "plugin"], expected=1)
    assert "async guest functions require" in (out / "public-guard.log").read_text()
    assert not (out / "public").exists()
    run("generate-producer", [tools["async_guest"], guest / "world.wit", "plugin", generated])
    run("generate-caller", [tools["canonical_c"], guest / "world.wit", "supervisor", caller])
    clang = args.wasi_sdk.resolve() / "bin" / ("clang" + suffix)
    c_component = guest / "caller.component.wasm"
    run("caller-link", [clang, "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g",
        "-Wall", "-Werror", "-I", caller, caller / "supervisor.c", guest / "supervisor.c",
        caller / "supervisor_component_type.o", "-Wl,--export=cabi_realloc", "-Wl,--fatal-warnings", "-o", c_component])
    marker = "PASS: returned private actor, four positive handle-drop controls, four SubtaskCancelAfterTerminal traps and four exactly-once native child retirements"
    artifacts = [c_component]
    for kind, archive in [("plain", args.plain_runtime.resolve()), ("instrumented", args.runtime.resolve())]:
        library = guest / ("libguest-" + kind + ".a")
        producer = guest / (kind + ".producer.component.wasm")
        component = guest / (kind + ".component.wasm")
        controls = {"PERRY_GC_DIAG": "1", "PERRY_GC_VERIFY_EVACUATION": "1"} if kind == "instrumented" else {}
        run(kind + "-compile", [args.perry.resolve(), "compile", guest / "guest.ts", "--target", "wasip3",
            "--output-type", "staticlib", "--no-auto-optimize", "--no-cache", "-o", library], extra=controls)
        run(kind + "-link", [clang, "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g",
            "-Wall", "-Werror", "-Wno-unused-function", "-I", generated, generated / "plugin.c",
            generated / "perry_adapter.c", generated / "plugin_component_type.o", "-Wl,--whole-archive",
            library, "-Wl,--no-whole-archive", archive, "-lsetjmp", "-Wl,--export=cabi_realloc",
            "-Wl,--stack-first", "-Wl,-z,stack-size=8388608", "-Wl,--fatal-warnings", "-o", producer])
        run(kind + "-compose", [tools["guest_returned_actor"], "--compose", producer, c_component,
            generated / "perry_producer_facade.json", component])
        run(kind + "-contract", [args.p3_contract.resolve(), "--reactor", component, "exercise",
            "--function-import=empty", "--function-import=proceed", "--function-import=mark", "--function-import=cancel-mode"])
        run(kind + "-execute", [tools["guest_returned_actor"], guest / "world.wit", component])
        assert marker in (out / (kind + "-execute.log")).read_text()
        if kind == "instrumented":
            run("moving", [tools["guest_returned_actor"], guest / "world.wit", component, "--moving"])
            log = (out / "moving.log").read_text()
            copied = sum(map(int, re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log)))
            assert marker in log and copied > 0 and "evacuation_ok" in log
            (out / "moving.json").write_text(json.dumps(dict(copiedObjects=copied,
                evacuationVerified=True), indent=2) + "\n")
        artifacts.extend([library, producer, component])
    paths = [*artifacts, *tools.values(), *generated.iterdir(), *caller.iterdir(),
             *(guest / name for name in ["world.wit", "guest.ts", "supervisor.c"]),
             args.perry.resolve(), args.wit.resolve(), args.plain_runtime.resolve(), args.runtime.resolve()]
    identities = {str(path): dict(bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest())
                  for path in paths if path.is_file()}
    (out / "artifact-identities.json").write_text(json.dumps(identities, indent=2) + "\n")
    (out / "result.json").write_text(json.dumps(dict(exitCode=0, returnedActorCancellationQualified=True,
        terminalTrap="SubtaskCancelAfterTerminal", positiveControlsPerRun=4,
        exactlyOnceNativeRetirementsPerRun=4, fullWasi030Qualified=False), indent=2) + "\n")


if __name__ == "__main__":
    main()
