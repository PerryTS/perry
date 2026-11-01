#!/usr/bin/env python3
"""Execute actual P3 resource-bearing I/O and named-interface sync factories."""
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
    (out / "OWNER").write_text("Actual P3 resource-bearing endpoint evidence; preserve.\n")
    for key, value in os.environ.items():
        if key.startswith("PERRY_GC_") and value:
            raise SystemExit(f"Native host GC controls must be off: {key}")
    env = {**os.environ, "CARGO_TARGET_DIR": str(args.target_dir.resolve()),
           "PERRY_WORKSPACE_ROOT": str(root), "WASI_SDK_PATH": str(args.wasi_sdk.resolve()),
           "PERRY_RUNTIME_DIR": str(args.native_runtime_dir.resolve())}
    rows = []

    def run(name, command, expected=0, extra_env=None):
        with (out / (name + ".log")).open("w") as log:
            status = subprocess.run([str(x) for x in command], cwd=root, env=env | (extra_env or {}),
                                    stdout=log, stderr=subprocess.STDOUT, timeout=1800).returncode
        passed = status == expected
        rows.append(dict(phase=name, exitCode=status, passed=passed))
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print(f"{'PASS' if passed else 'FAIL'} {name}", flush=True)
        if not passed:
            raise SystemExit(status or 1)

    run("build-probes", ["cargo", "-Z", "checksum-freshness", "build", "--locked",
                         "--profile", "perry-dev", "-p", "perry-wit", "--example", "async_guest",
                         "--example", "actor_compose",
                         "-p", "perry-component-host", "--example", "guest_endpoint_resources"])
    binaries = out / "binaries"
    binaries.mkdir()
    suffix = ".exe" if os.name == "nt" else ""
    for name in ["async_guest", "actor_compose", "guest_endpoint_resources"]:
        shutil.copy2(args.target_dir.resolve() / "perry-dev/examples" / (name + suffix), binaries / (name + suffix))
    guest = out / "guest"
    guest.mkdir()
    fixture = root / "test-wasi/components/guest-endpoint-resources"
    for name in ["world.wit", "guest.ts"]:
        shutil.copy2(fixture / name, guest / name)
    generated = guest / "generated"
    run("public-guard", [args.wit.resolve(), "--guest-c", guest / "world.wit", out / "public"], expected=1)
    assert "future/stream guest values" in (out / "public-guard.log").read_text()
    assert not (out / "public").exists(), "guard must reject before publishing files"
    run("generate", [binaries / ("async_guest" + suffix), guest / "world.wit", "plugin", generated])
    facade = guest / "facade.component.wasm"
    run("facade-link", [args.wasi_sdk.resolve() / "bin" / ("clang" + suffix),
                       "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g",
                       "-Wall", "-Werror", "-I", generated,
                       generated / "perry_producer_facade.c", generated / "perry_producer_forward.c",
                       generated / "perry_producer_facade_component_type.o",
                       "-Wl,--export=cabi_realloc", "-Wl,--fatal-warnings", "-o", facade])
    libraries = []
    components = [facade]
    marker = "PASS: actual Perry P3 resource-bearing future/stream reads and writes, named-interface synchronous factories, partial owner suffixes, cancellation retry, close discard, 33 exactly-once drops and Store reuse"

    def compose(name, producer, public):
        run(name, [binaries / ("actor_compose" + suffix), producer, facade,
                   generated / "perry_producer_facade.json", public])

    def link(directory, archive, component):
        return [args.wasi_sdk.resolve() / "bin" / ("clang" + suffix),
                "--target=wasm32-wasip3", "-mexec-model=reactor", "-O1", "-g",
                "-Wall", "-Werror", "-Wno-unused-function", "-I", directory,
                directory / "plugin.c", directory / "perry_adapter.c",
                directory / "plugin_component_type.o", "-Wl,--whole-archive", library,
                "-Wl,--no-whole-archive", archive, "-lsetjmp", "-Wl,--export=cabi_realloc",
                "-Wl,--stack-first", "-Wl,-z,stack-size=8388608", "-Wl,--fatal-warnings", "-o", component]

    for kind, archive in [("plain", args.plain_runtime.resolve()), ("instrumented", args.runtime.resolve())]:
        library = guest / ("libguest-" + kind + ".a")
        libraries.append(library)
        # WASI programs select runtime installers while compiling. Merely
        # linking an instrumented archive leaves diagnostic hooks uninstalled.
        diagnostics = {"PERRY_GC_DIAG": "1", "PERRY_GC_VERIFY_EVACUATION": "1"} if kind == "instrumented" else {}
        run(kind + "-compile", [args.perry.resolve(), "compile", guest / "guest.ts", "--target", "wasip3",
                                "--output-type", "staticlib", "--no-auto-optimize", "--no-cache", "-o", library],
            extra_env=diagnostics)
        component = guest / (kind + ".component.wasm")
        producer = guest / (kind + ".producer.component.wasm")
        run(kind + "-link", link(generated, archive, producer))
        compose(kind + "-compose", producer, component)
        components.extend([producer, component])
        interface = "perry:guest-endpoint-resources/api@1.0.0"
        if kind == "plain":
            run("reject-missing-interface", [args.p3_contract.resolve(), "--reactor", component,
                                              "--instance-export=perry:missing/api@1.0.0"], expected=1)
            diagnostic = (out / "reject-missing-interface.log").read_text()
            diagnostic = json.loads(diagnostic.rsplit("Error: ", 1)[1])
            assert "missing_instances=[\"perry:missing/api@1.0.0\"]" in diagnostic
        run(kind + "-contract", [args.p3_contract.resolve(), "--reactor", component,
                                 "--instance-export=" + interface,
                                 "--instance-export=perry:guest-endpoint-resources/mirror@1.0.0"])
        imports = [line for line in (out / (kind + "-contract.log")).read_text().splitlines()
                   if line.startswith("import ")]
        assert len(imports) == 18 and all(line.endswith("@0.3.0") for line in imports)
        run(kind + "-execute", [binaries / ("guest_endpoint_resources" + suffix), guest / "world.wit", component])
        assert marker in (out / (kind + "-execute.log")).read_text()
        if kind == "plain":
            run("plain-rejects-instrument-grant",
                [binaries / ("guest_endpoint_resources" + suffix), guest / "world.wit", component, "--moving"],
                expected=1)
            assert "built without the GC instruments" in (out / "plain-rejects-instrument-grant.log").read_text()
        if kind == "instrumented":
            run("moving", [binaries / ("guest_endpoint_resources" + suffix), guest / "world.wit", component, "--moving"])
            log = (out / "moving.log").read_text()
            copied = sum(map(int, re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", log)))
            assert marker in log and copied > 0 and "evacuation_ok" in log
            (out / "moving.json").write_text(json.dumps(dict(guestCopiedObjects=copied, guestEvacuationVerified=True), indent=2) + "\n")
    paths = [args.perry.resolve(), args.wit.resolve(), args.plain_runtime.resolve(), args.runtime.resolve(),
             *libraries, *components, *binaries.iterdir(), *generated.iterdir()]
    identities = {str(p): dict(bytes=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                  for p in paths if p.is_file()}
    (out / "artifact-identities.json").write_text(json.dumps(identities, indent=2) + "\n")


if __name__ == "__main__":
    main()
