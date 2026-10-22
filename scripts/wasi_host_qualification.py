#!/usr/bin/env python3
"""Qualify native P1 hosting and typed P2 hosting with matching local artifacts."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
EXIT_CASE = "lifecycle/return-on-exit-false.ts"
MOVING_CASE = "semantics/context-lifetime.ts"
MOVING_KNOBS = (
    "PERRY_GC_INSTRUMENTS", "PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION",
    "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_DIAG",
)
P2_PHASES = {
    "rust-guest", "componentize", "bindings", "compile-host", "run-host",
    "compile-host-moving", "run-host-moving",
    *(f"{prefix}-{phase}" for prefix in ("resources", "host-resources", "network")
      for phase in ("guest", "componentize", "bindings", "compile", "run", "compile-moving", "run-moving")),
}
COMMAND_PHASES = {
    "rust-command",
    *(f"{prefix}-{phase}" for prefix in ("command", "command-moving")
      for phase in ("compile", "run", "library-compile", "library-run")),
}


def complete_phases(rows: list[dict], expected: set[str]) -> bool:
    return (len(rows) == len(expected) and {row["name"] for row in rows} == expected
            and all(row["exitCode"] == 0 for row in rows))


def discover(root: Path) -> list[Path]:
    return sorted(path for path in root.rglob("*.ts")
                  if "fixtures" not in path.relative_to(root).parts
                  and not path.name.endswith(".d.ts"))


def verdict(expected: int, node_exit: int, node_stdout: bytes,
            compile_exit: int, perry_exit: int | None, perry_stdout: bytes) -> list[str]:
    errors = []
    if node_exit != expected:
        errors.append(f"Node exited {node_exit}, expected {expected}")
    if compile_exit != 0:
        errors.append(f"Perry compilation exited {compile_exit}")
    if perry_exit != expected:
        errors.append(f"Perry exited {perry_exit}, expected {expected}")
    # Only line endings and trailing newlines vary; error text is not scrubbed.
    normalize = lambda value: value.replace(b"\r\n", b"\n").rstrip(b"\n")
    if normalize(node_stdout) != normalize(perry_stdout):
        errors.append("stdout differs from the pinned Node oracle")
    return errors


def moving_witness(stderr: bytes) -> dict:
    text = stderr.decode("utf-8", errors="replace")
    copied = sum(map(int, re.findall(r"^\[gc-copy-minor\] ran[^\n]*\bcopied_objects=(\d+)", text, re.M)))
    checks = len(re.findall(r"^\[gc-verify\].*evacuation_ok", text, re.M))
    quarantine = bool(re.search(r"^\[gc-fromspace-protect\].*retired_set=", text, re.M))
    return {"copiedObjects": copied, "evacuationChecks": checks,
            "quarantineObserved": quarantine,
            "passed": copied > 0 and checks > 0 and quarantine}


def run(command: list[str], log: Path, env: dict, timeout: int) -> tuple[int, bytes, bytes]:
    try:
        result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, timeout=timeout)
        status, stdout, stderr = result.returncode, result.stdout, result.stderr
    except (OSError, subprocess.TimeoutExpired) as error:
        status, stdout, stderr = 124, b"", str(error).encode("utf-8")
    log.with_suffix(".stdout.log").write_bytes(stdout)
    log.with_suffix(".stderr.log").write_bytes(stderr)
    return status, stdout, stderr


def sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def unit_results(text: str) -> dict[str, int]:
    text = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", text)
    result = {}
    for segment in re.split(r"\bRunning unittests", text)[1:]:
        header = segment.splitlines()[0]
        for package in ("perry_component_host", "perry_wasm_host"):
            if package + "-" not in header:
                continue
            matches = re.findall(r"test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;", segment)
            if package in result or len(matches) != 1 or int(matches[0]) == 0:
                raise ValueError(f"Missing complete passing test result for {package}")
            result[package] = int(matches[0])
    if set(result) != {"perry_component_host", "perry_wasm_host"}:
        raise ValueError("Both native host crate test suites must execute")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("perry", "runtime-dir", "wit", "componentize", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--node", default=os.environ.get("NODE_BIN", "node"))
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    subprocess.run(["git", "diff", "--quiet", "HEAD", "--"], cwd=ROOT, check=True)
    unit_log = output / "native-host-units.log"
    units = unit_results(unit_log.read_text(encoding="utf-8"))
    pinned = (ROOT / ".node-version").read_text(encoding="utf-8").strip().removeprefix("v")
    actual = subprocess.check_output([args.node, "--version"], text=True).strip().removeprefix("v")
    if actual != pinned:
        raise SystemExit(f"Node {actual} does not match pinned oracle {pinned}")
    runtime = args.runtime_dir.resolve()
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(runtime), PERRY_NO_AUTO_OPTIMIZE="1",
               PERRY_CODEGEN_CPU_SLOTS="4")
    for knob in MOVING_KNOBS:
        env.pop(knob, None)
    fixture = ROOT / "test-parity/node-suite/wasi"
    cases = discover(fixture)
    names = {path.relative_to(fixture).as_posix() for path in cases}
    if len(cases) < 56 or not {EXIT_CASE, MOVING_CASE}.issubset(names):
        raise SystemExit("The complete P1 suite and exit/collection fixtures are required")
    results = []
    rows_path = output / "p1-results.json"
    for moving, sources in [(False, cases), (True, [fixture / MOVING_CASE])]:
        case_env = env | ({knob: "1" for knob in MOVING_KNOBS} if moving else {})
        for source in sources:
            name = source.relative_to(fixture).as_posix()
            directory = output / ("p1-moving" if moving else "p1") / name.removesuffix(".ts")
            directory.mkdir(parents=True, exist_ok=True)
            executable = directory / ("case.exe" if sys.platform == "win32" else "case")
            expected = 7 if name == EXIT_CASE else 0
            node_exit, node_out, _ = run([args.node, "--expose-gc", str(source)], directory / "node", env, 60)
            compile_exit, _, _ = run([str(args.perry.resolve()), "compile", str(source),
                                     "--enable-wasm-runtime", "--no-cache", "-o", str(executable)],
                                    directory / "compile", case_env, 180)
            perry_exit, perry_out, perry_err = (None, b"", b"")
            if compile_exit == 0:
                perry_exit, perry_out, perry_err = run([str(executable)], directory / "perry", case_env, 60)
            errors = verdict(expected, node_exit, node_out, compile_exit, perry_exit, perry_out)
            witness = moving_witness(perry_err) if moving else None
            if witness is not None and not witness["passed"]:
                errors.append("moving collection, evacuation verification or quarantine witness missing")
            results.append({"name": name, "moving": moving, "expectedExit": expected,
                            "nodeExit": node_exit, "compileExit": compile_exit,
                            "perryExit": perry_exit, "errors": errors, "movingGc": witness,
                            "executableSha256": sha(executable) if compile_exit == 0 else None})
            rows_path.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
            # Retain logs and identity; do not accumulate 57 large native binaries.
            if executable.is_file():
                executable.unlink()
            print(f"{'FAIL' if errors else 'PASS'} P1 {'moving ' if moving else ''}{name}", flush=True)
    # Run P2 even when P1 reports failures so each platform yields both verdicts.
    with (output / "p2-driver.log").open("wb") as log:
        command = [sys.executable, str(ROOT / "scripts/component_host_acceptance.py"),
                   "--perry", str(args.perry.resolve()), "--runtime-dir", str(runtime),
                   "--wit", str(args.wit.resolve()), "--componentize", str(args.componentize.resolve()),
                   "--output", str(output / "p2"), "--moving-gc"]
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=3600)
    p2 = json.loads((output / "p2/results.json").read_text(encoding="utf-8"))
    p2_passed = result.returncode == 0 and complete_phases(p2, P2_PHASES)
    p2_witnesses = {}
    for phase in ("run-host-moving", "resources-run-moving", "network-run-moving", "host-resources-run-moving"):
        log = output / "p2" / (phase + ".log")
        p2_witnesses[phase] = moving_witness(log.read_bytes() if log.is_file() else b"")
    p2_passed = p2_passed and all(w["passed"] for w in p2_witnesses.values())
    with (output / "command-driver.log").open("wb") as log:
        command = [sys.executable, str(ROOT / "scripts/component_command_acceptance.py"),
                   "--perry", str(args.perry.resolve()), "--runtime-dir", str(runtime),
                   "--output", str(output / "command"), "--moving-gc",
                   "--library-component", str(output / "p2/transform/transform.component.wasm"),
                   "--library-bindings", str(output / "p2/transform/generated")]
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=3600)
    commands = json.loads((output / "command/results.json").read_text(encoding="utf-8"))
    command_witnesses = {}
    for phase in ("command-moving-run", "command-moving-library-run"):
        log = output / "command" / (phase + ".log")
        command_witnesses[phase] = moving_witness(log.read_bytes() if log.is_file() else b"")
    commands_passed = (result.returncode == 0 and complete_phases(commands, COMMAND_PHASES)
                       and all(w["passed"] for w in command_witnesses.values()))
    artifacts = [args.perry.resolve(), args.wit.resolve(), args.componentize.resolve()]
    for stem in ("perry_runtime", "perry_runtime_instr", "perry_stdlib", "perry_stdlib_instr",
                 "perry_wasm_host", "perry_component_host"):
        archive = runtime / (stem + ".lib" if sys.platform == "win32" else "lib" + stem + ".a")
        if not archive.is_file():
            raise SystemExit(f"Missing matching archive: {archive}")
        artifacts.append(archive)
    passed = all(not row["errors"] for row in results) and p2_passed and commands_passed
    report = {"sourceRevision": revision, "host": platform.platform(), "machine": platform.machine(),
              "nodeVersion": actual, "p1Cases": len(results), "p1Passed": sum(not r["errors"] for r in results),
              "p2Phases": len(p2), "p2Passed": p2_passed, "allPassed": passed,
              "commandPhases": len(commands), "commandsPassed": commands_passed,
              "commandMovingGc": command_witnesses,
              "hostUnitTests": units, "unitLogSha256": sha(unit_log),
              "p2MovingGc": p2_witnesses,
              "artifactSha256": {path.name: sha(path) for path in artifacts}}
    (output / "qualification.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
