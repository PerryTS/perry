#!/usr/bin/env python3
"""Execute resource-bearing async Perry guests below the retained public guard.

Supply a source-matched compiler and WASI archives. The ignored WIT unit test
only produces private fixture files; success requires actual component execution
and positive moving-GC witnesses, not generation or C compilation alone.
"""
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
    for name in ["perry", "runtime", "plain-runtime", "wasi-sdk", "target-dir", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--profile", choices=["dev", "prod"], default="prod")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    (out / "OWNER").write_text("Actual async resource guest qualification; preserve source, logs and artifacts.\n")
    env = {**os.environ, "CARGO_TARGET_DIR": str(args.target_dir.resolve())}
    rows = []

    def run(name, command, cwd=root, environment=env):
        print("START", name, flush=True)
        with (out / (name + ".log")).open("w") as log:
            status = subprocess.run([str(x) for x in command], cwd=cwd, env=environment,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=1800).returncode
        rows.append(dict(phase=name, exitCode=status))
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print("DONE", name, status, flush=True)
        if status:
            raise SystemExit(status)

    run("fixture-build", ["cargo", "-Z", "checksum-freshness", "test", "--locked",
                          "--profile", args.profile, "-p", "perry-wit", "--lib",
                          "--no-run", "--message-format=json"])
    candidates = []
    for line in (out / "fixture-build.log").read_text().splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if item.get("reason") == "compiler-artifact" and item.get("executable"):
            if item["target"]["name"] == "perry_wit":
                candidates.append(Path(item["executable"]))
    assert len(candidates) == 1, "expected the real WIT unit-test executable"
    frozen = out / "binaries"
    frozen.mkdir()
    suffix = ".exe" if os.name == "nt" else ""
    generator = frozen / ("resource_fixture" + suffix)
    shutil.copy2(candidates[0], generator)
    run("generate", [generator, "--exact", "guest_c::asynchronous::tests::async_resource_guest_fixture",
                     "--ignored", "--nocapture", "--test-threads=1"])
    match = re.search(r"PERRY_ASYNC_RESOURCE_FIXTURE=([^\r\n]+)", (out / "generate.log").read_text())
    assert match, "fixture subject did not execute"
    guest = out / "guest"
    shutil.move(match.group(1), guest)
    fixture = root / "test-wasi/components/async-guest-resources"
    for name in ["world.wit", "guest.ts"]:
        assert (guest / name).read_bytes() == (fixture / name).read_bytes(), "stale fixture binary"
    run("host-build", ["cargo", "-Z", "checksum-freshness", "build", "--locked",
                       "--profile", args.profile, "-p", "perry-component-host",
                       "--example", "async_guest_resources"])
    profile_dir = "debug" if args.profile == "dev" else args.profile
    host = frozen / ("async_guest_resources" + suffix)
    shutil.copy2(args.target_dir.resolve() / profile_dir / "examples" / host.name, host)
    library = guest / "libguest.a"
    compile_env = {**env, "WASI_SDK_PATH": str(args.wasi_sdk.resolve()),
                   "PERRY_RUNTIME_DIR": str(args.runtime.resolve().parent),
                   "PERRY_WORKSPACE_ROOT": str(root)}
    run("compile", [args.perry.resolve(), "compile", guest / "guest.ts", "--target", "wasi",
                    "--output-type", "staticlib", "--no-auto-optimize", "--no-cache", "-o", library],
        environment=compile_env)
    generated = guest / "generated"
    marker = "PASS: generated Perry async resources, owned round trips, 8 reverse borrow pairs, list borrows, import loans and local resources"
    components = []
    for kind, archive in [("plain", args.plain_runtime.resolve()), ("instrumented", args.runtime.resolve())]:
        component = guest / (kind + ".component.wasm")
        run(kind + "-link", [args.wasi_sdk.resolve() / "bin" / ("clang" + suffix),
                              "--target=wasm32-wasip2", "-mexec-model=reactor", "-O1", "-g",
                              "-Wall", "-Werror", "-Wno-unused-function", "-I", generated,
                              generated / "plugin.c", generated / "perry_adapter.c",
                              generated / "plugin_component_type.o", "-Wl,--whole-archive", library,
                              "-Wl,--no-whole-archive", archive, "-lsetjmp", "-Wl,--export=cabi_realloc",
                              "-Wl,--stack-first", "-Wl,-z,stack-size=8388608", "-Wl,--fatal-warnings", "-o", component])
        components.append(component)
        run(kind + "-execute", [host, guest / "world.wit", component])
        assert marker in (out / (kind + "-execute.log")).read_text()
        if kind == "instrumented":
            run("moving", [host, guest / "world.wit", component, "--moving"])
            log = (out / "moving.log").read_text()
            assert marker in log
            copied = sum(int(n) for n in re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log))
            assert copied > 0 and "evacuation_ok" in log, "guest moving GC was not exercised"
            (out / "moving.json").write_text(json.dumps(dict(copiedObjects=copied, evacuationVerified=True), indent=2) + "\n")
    paths = [generator, host, args.perry.resolve(), args.runtime.resolve(), args.plain_runtime.resolve(),
             library, *components, guest / "world.wit", guest / "guest.ts", *generated.iterdir()]
    identities = {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths if path.is_file()}
    (out / "artifact-identities.json").write_text(json.dumps(identities, indent=2) + "\n")


if __name__ == "__main__":
    main()
