from __future__ import annotations

import json
import os
import subprocess
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
SCHEMA_VERSION = 1

DEFAULT_BENCHMARK_RUNS = {
    "smoke": 1,
    "standard": 5,
    "release": 15,
}

RUNTIME_CALL_PREFIXES = (
    "js_",
    "perry_runtime_",
)

DYNAMIC_PROPERTY_HELPERS = (
    "js_object_get",
    "js_object_set",
    "js_handle_object_get_property",
    "js_native_call_method",
    "js_get_property",
    "js_set_property",
    "js_dynamic",
)

BUFFER_SLOW_PATH_HELPERS = (
    "js_buffer_get",
    "js_buffer_set",
    # Buffer byte indexing currently lowers through the Uint8Array helper
    # surface, so count those helpers for the Buffer-byte material gate too.
    "js_uint8array_get",
    "js_uint8array_set",
)

ARRAY_SLOW_PATH_HELPERS = (
    "js_typed_array_get",
    "js_typed_array_set",
    "js_uint8array_get",
    "js_uint8array_set",
)


class HarnessError(RuntimeError):
    pass


@dataclass
class CommandResult:
    argv: list[str]
    cwd: str
    exit_code: int
    duration_ms: float
    stdout: str
    stderr: str
    stdout_path: str | None = None
    stderr_path: str | None = None

    def to_json(self) -> dict[str, Any]:
        return {
            "argv": self.argv,
            "cwd": self.cwd,
            "exit_code": self.exit_code,
            "duration_ms": self.duration_ms,
            "stdout_path": self.stdout_path,
            "stderr_path": self.stderr_path,
        }


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def relpath(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def run_command(
    argv: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
    timeout: int,
    stdout_path: Path | None = None,
    stderr_path: Path | None = None,
    check: bool = True,
) -> CommandResult:
    start = time.perf_counter()
    proc = subprocess.run(
        argv,
        cwd=str(cwd),
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
    )
    duration_ms = (time.perf_counter() - start) * 1000.0
    if stdout_path is not None:
        write_text(stdout_path, proc.stdout)
    if stderr_path is not None:
        write_text(stderr_path, proc.stderr)
    result = CommandResult(
        argv=argv,
        cwd=str(cwd),
        exit_code=proc.returncode,
        duration_ms=duration_ms,
        stdout=proc.stdout,
        stderr=proc.stderr,
        stdout_path=str(stdout_path) if stdout_path is not None else None,
        stderr_path=str(stderr_path) if stderr_path is not None else None,
    )
    if check and proc.returncode != 0:
        raise HarnessError(
            f"command failed with exit {proc.returncode}: {' '.join(argv)}\n"
            f"stderr:\n{proc.stderr[-4000:]}"
        )
    return result
