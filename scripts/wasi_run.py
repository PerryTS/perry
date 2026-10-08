#!/usr/bin/env python3
"""Portable WASI acceptance runner (Windows, Linux and macOS).

Run existing smoke fixtures or the additional tracking-issue probes. The
probes are deliberately separate until their implementations work; failures
are reported and exit nonzero, never silently treated as expected passes.
"""

from __future__ import annotations

import argparse
import difflib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
GAP_SAMPLES = [
    "test_gap_array_methods", "test_gap_string_methods", "test_gap_closures",
    "test_gap_class_advanced", "test_gap_map_set_extended", "test_gap_json_advanced",
    "test_gap_regexp_advanced", "test_gap_async_advanced", "test_gap_error_extensions",
    "test_gap_object_methods",
]
CORE_ACCEPTANCE = {"bytes", "date", "filesystem", "gc", "tagged_values", "timer"}


def normalized(data: bytes) -> str:
    return data.decode("utf-8", errors="replace").replace("\r\n", "\n")


def run_case(perry: Path, wasmtime: str, source: Path, output: Path,
             timeout: int, expected_output: str | None = None) -> dict:
    name = source.stem
    wasm = output / f"{name}.wasm"
    wasm.unlink(missing_ok=True)
    config_path = source.with_suffix(".json")
    config = json.loads(config_path.read_text()) if config_path.exists() else {}
    result = {"name": name, "stage": "compile", "passed": False}
    env = dict(os.environ, PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1")
    try:
        compiled = subprocess.run(
            [str(perry), "compile", str(source), "--target", "wasi", "-o", str(wasm)],
            cwd=output, env=env, capture_output=True, timeout=timeout)
        (output / f"{name}.compile.log").write_bytes(compiled.stdout + compiled.stderr)
        result["compile_status"] = compiled.returncode
        if compiled.returncode or not wasm.is_file():
            result["detail"] = normalized(compiled.stderr)[-4000:]
            return result
        result["stage"] = "run"
        executed = subprocess.run(
            [wasmtime, "run", *config.get("wasmtime_args", []), str(wasm),
             *config.get("program_args", [])],
            cwd=output, capture_output=True, timeout=timeout)
        (output / f"{name}.stderr").write_bytes(executed.stderr)
        actual = normalized(executed.stdout) + f"exit {executed.returncode}\n"
        (output / f"{name}.stdout").write_text(actual, encoding="utf-8")
        expected = (expected_output if expected_output is not None
                    else normalized(source.with_suffix(".out").read_bytes()))
        result.update(run_status=executed.returncode, passed=actual == expected)
        if not result["passed"]:
            result["detail"] = "".join(difflib.unified_diff(
                expected.splitlines(True), actual.splitlines(True),
                fromfile="expected", tofile="actual")) + normalized(executed.stderr)[-4000:]
    except subprocess.TimeoutExpired:
        result["detail"] = f"{result['stage']} timed out after {timeout}s"
    except OSError as exc:
        result["detail"] = str(exc)
    return result


def main() -> int:
    # Compiler diagnostics may contain Unicode even in a legacy Windows
    # console. A diagnostic must not crash the runner before its JSON report.
    sys.stdout.reconfigure(errors="backslashreplace")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("perry", type=Path)
    parser.add_argument("--wasmtime", default=os.environ.get("WASMTIME", "wasmtime"))
    parser.add_argument("--suite", choices=["smoke", "core", "acceptance", "gap"], default="smoke")
    parser.add_argument("--node", default="node", help="pinned Node oracle for --suite gap")
    parser.add_argument("--filter", action="append", default=[], help="name substring; repeat to select a union")
    parser.add_argument("--timeout", type=int, default=120, help="seconds per compile/run")
    parser.add_argument("--output", type=Path, help="retain logs and JSON report here")
    args = parser.parse_args()
    fixtures = ROOT / "scripts" / ("wasi_smoke" if args.suite == "smoke" else "wasi_acceptance")
    sources = ([ROOT / "test-files" / f"{name}.ts" for name in GAP_SAMPLES]
               if args.suite == "gap" else sorted(fixtures.glob("*.ts")))
    if args.suite == "core":
        missing = CORE_ACCEPTANCE - {p.stem for p in sources}
        if missing:
            parser.error(f"missing core fixtures: {', '.join(sorted(missing))}")
        sources = [p for p in sources if p.stem in CORE_ACCEPTANCE]
    sources = [p for p in sources if not args.filter or any(f in p.stem for f in args.filter)]
    if not sources:
        parser.error("no fixtures selected")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    perry = args.perry.resolve()
    if not perry.is_file():
        parser.error(f"compiler not found: {perry}")
    if args.suite == "gap":
        required = "v" + (ROOT / ".node-version").read_text().strip().removeprefix("v")
        version = subprocess.check_output([args.node, "--version"], text=True).strip()
        if version != required:
            parser.error(f"Node oracle is {version}; expected {required} from .node-version")
    with tempfile.TemporaryDirectory(prefix="perry-wasi-") as temporary:
        output = args.output.resolve() if args.output else Path(temporary)
        output.mkdir(parents=True, exist_ok=True)
        # Each test gets its own cwd so preopen fixtures and emitted sidecars
        # cannot affect later tests. Paths containing spaces stay single argv.
        results = []
        for source in sources:
            case_output = output / source.stem
            case_output.mkdir(exist_ok=True)
            expected = None
            if args.suite == "gap":
                oracle = subprocess.run(
                    [args.node, "--experimental-strip-types", str(source)],
                    capture_output=True, timeout=args.timeout, cwd=case_output)
                if oracle.returncode:
                    # A failing oracle must not silently remove a test from
                    # coverage (CLAUDE.md's node_fail warning).
                    print(f"FAIL {source.stem}: Node oracle exited {oracle.returncode}")
                    results.append({"name": source.stem, "stage": "oracle", "passed": False})
                    continue
                expected = normalized(oracle.stdout) + "exit 0\n"
                (case_output / "oracle.stdout").write_text(expected, encoding="utf-8")
            result = run_case(perry, args.wasmtime, source, case_output, args.timeout, expected)
            results.append(result)
            print(f"{'ok' if result['passed'] else 'FAIL'} {result['name']} ({result['stage']})", flush=True)
            if not result["passed"]:
                print(result.get("detail", ""), flush=True)
        (output / "report.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
        passed = sum(r["passed"] for r in results)
        print(f"{passed}/{len(results)} passed")
        return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
