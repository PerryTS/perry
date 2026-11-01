#!/usr/bin/env python3
"""Exercise heap roots inside a real Perry P3 command with native GC knobs off."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

MARKER = "PASS P3 guest GC: 24 cyclic class roots, Unicode, arrays, closures, Map/Set, Promise/timer and exception roots\n"
INSTRUMENTS = {
    "PERRY_GC_INSTRUMENTS": "1", "PERRY_GC_FORCE_EVACUATE": "1",
    "PERRY_GC_VERIFY_EVACUATION": "1", "PERRY_GC_DIAG": "1",
    "PERRY_GC_SCHEDULE_SEED": "303", "PERRY_GC_SCHEDULE_RATE": "127",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["perry", "native-runtime-dir", "plain-runtime-dir",
                 "instrumented-runtime-dir", "p3-contract", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    for name in ["gc-host.ts", "gc-guest.ts"]:
        shutil.copy2(root / "test-wasi/components/p3-command" / name, out / name)
    env = dict(os.environ)
    # This profile grants instruments inside the Wasm command only. Reject an
    # ambient native stress configuration so it cannot supply our GC witness.
    ambient = [name for name, value in env.items()
               if name.startswith("PERRY_GC_") and value]
    if ambient:
        raise RuntimeError("native GC controls must be absent: " + ", ".join(ambient))
    env.update(PERRY_RUNTIME_DIR=str(args.native_runtime_dir.resolve()),
               PERRY_NO_AUTO_OPTIMIZE="1", PERRY_CODEGEN_CPU_SLOTS="4")
    rows = []

    def run(name, command, extra=None, expected_status=0):
        result = subprocess.run(list(map(str, command)), cwd=out,
            env=env | (extra or {}), capture_output=True, timeout=600)
        (out / (name + ".stdout")).write_bytes(result.stdout)
        (out / (name + ".stderr")).write_bytes(result.stderr)
        passed = result.returncode == expected_status if expected_status is not None else result.returncode != 0
        rows.append({"phase": name, "exitCode": result.returncode, "passed": passed})
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print(("PASS " if passed else "FAIL ") + name, flush=True)
        if not passed:
            raise SystemExit(1)
        return result

    host = out / ("host.exe" if os.name == "nt" else "host")
    run("compile-native-host", [args.perry.resolve(), "compile", out / "gc-host.ts",
        "--no-cache", "--no-auto-optimize", "-o", host])
    control = run("native-controls-off", [host, "native-control"])
    assert control.stdout.decode() == "PASS native control\n"
    assert "[gc-copy-minor]" not in control.stderr.decode()
    guests = {}
    for kind, directory, extra in [("plain", args.plain_runtime_dir, {}),
                                  ("instrumented", args.instrumented_runtime_dir, INSTRUMENTS)]:
        guest = out / (kind + ".component.wasm")
        run("compile-" + kind, [args.perry.resolve(), "compile", out / "gc-guest.ts",
            "--target", "wasip3", "--no-cache", "--no-auto-optimize", "-o", guest],
            extra | {"PERRY_RUNTIME_DIR": str(directory.resolve())})
        run("contract-" + kind, [args.p3_contract.resolve(), guest])
        guests[kind] = guest
    for kind, mode in [("plain", "off"), ("instrumented", "off"), ("instrumented", "moving")]:
        name = kind + "-" + mode
        result = run(name, [host, guests[kind], mode])
        assert result.stdout.decode().replace("\r\n", "\n") == MARKER, name + " guest output"
        diagnostics = result.stderr.decode()
        copied = sum(map(int, re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", diagnostics)))
        if mode == "moving":
            assert copied > 0 and "evacuation_ok" in diagnostics, "Wasm guest moving-GC witness absent"
        else:
            assert copied == 0 and "[gc-copy-minor]" not in diagnostics, "GC controls leaked into off mode"
        rows[-1]["guestCopiedObjects"] = copied
        rows[-1]["guestEvacuationVerified"] = "evacuation_ok" in diagnostics
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
    refused = run("plain-rejects-instrument-grant", [host, guests["plain"], "moving"], expected_status=None)
    assert "built without the GC instruments" in refused.stderr.decode(), "plain archive failed for an unrelated reason"
    (out / "qualification.json").write_text(json.dumps({
        "phases": rows, "nativeGcControlsOff": True, "wasmGuestMovingGc": True,
        "physicalWasmPageProtectionClaimed": False, "fullWasi030Qualified": False,
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()
