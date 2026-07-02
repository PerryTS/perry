from __future__ import annotations

from pathlib import Path
from typing import Any

from .common import HarnessError, REPO_ROOT

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - only Python < 3.11.
    tomllib = None  # type: ignore[assignment]


DEFAULT_SPEC_PATH = REPO_ROOT / "benchmarks/compiler_output/workloads.toml"


def load_workload_spec(path: Path | str = DEFAULT_SPEC_PATH) -> dict[str, Any]:
    if tomllib is None:
        raise HarnessError("Python 3.11+ is required for stdlib TOML parsing")
    spec_path = Path(path)
    if not spec_path.is_absolute():
        spec_path = REPO_ROOT / spec_path
    if not spec_path.exists():
        raise HarnessError(f"workload spec not found: {spec_path}")
    data = tomllib.loads(spec_path.read_text(encoding="utf-8"))
    validate_workload_spec(data)
    return data


def validate_workload_spec(data: dict[str, Any]) -> None:
    if int(data.get("schema_version", 0) or 0) != 1:
        raise HarnessError("workload spec schema_version must be 1")
    workloads = data.get("workloads")
    if not isinstance(workloads, dict) or not workloads:
        raise HarnessError("workload spec must define [workloads.<name>] entries")
    for name, workload in workloads.items():
        if not isinstance(workload, dict):
            raise HarnessError(f"workload {name!r} must be a table")
        for field in ("source", "kind", "vectorization", "runtime_budgets"):
            if field not in workload:
                raise HarnessError(f"workload {name!r} missing required field {field!r}")
        vector = workload["vectorization"]
        if not isinstance(vector, dict):
            raise HarnessError(f"workload {name!r} vectorization must be a table")
        for field in ("min_vectorized_loops", "allowed_missed_reason_kinds"):
            if field not in vector:
                raise HarnessError(
                    f"workload {name!r} vectorization missing required field {field!r}"
                )
        if not isinstance(vector.get("allowed_missed_reason_kinds"), list):
            raise HarnessError(
                f"workload {name!r} vectorization.allowed_missed_reason_kinds must be a list"
            )
        if not isinstance(workload.get("runtime_budgets"), dict):
            raise HarnessError(f"workload {name!r} runtime_budgets must be a table")
        stdout_checks = workload.get("stdout_checks", [])
        if not isinstance(stdout_checks, list):
            raise HarnessError(f"workload {name!r} stdout_checks must be a list")
        for check in stdout_checks:
            if not isinstance(check, dict) or not check.get("name"):
                raise HarnessError(f"workload {name!r} stdout_checks need names")
            if any(key in check for key in ("contains", "contains_all", "contains_any")):
                raise HarnessError(
                    f"workload {name!r} stdout check {check['name']!r} must not use "
                    "substring matching"
                )
            if "equals" not in check and "line_equals" not in check:
                raise HarnessError(
                    f"workload {name!r} stdout check {check['name']!r} must use "
                    "equals or line_equals"
                )
        native_rep_checks = workload.get("native_rep_checks")
        if native_rep_checks is not None:
            if not isinstance(native_rep_checks, dict):
                raise HarnessError(f"workload {name!r} native_rep_checks must be a table")
            if not isinstance(native_rep_checks.get("require_records", []), list):
                raise HarnessError(
                    f"workload {name!r} native_rep_checks.require_records must be a list"
                )
            if not isinstance(
                native_rep_checks.get("allow_materialization_reasons", []), list
            ):
                raise HarnessError(
                    f"workload {name!r} native_rep_checks.allow_materialization_reasons must be a list"
                )
            if not isinstance(native_rep_checks.get("materialization_regions", []), list):
                raise HarnessError(
                    f"workload {name!r} native_rep_checks.materialization_regions must be a list"
                )
            for required in native_rep_checks.get("require_records", []) or []:
                if not isinstance(required, dict) or not required.get("name"):
                    raise HarnessError(
                        f"workload {name!r} native_rep_checks require_records need names"
                    )
        for region in workload.get("named_regions", []) or []:
            if not region.get("name"):
                raise HarnessError(f"workload {name!r} has a named region without name")
            selectors = region.get("selectors", [])
            if region.get("required") and not selectors:
                raise HarnessError(
                    f"workload {name!r} named region {region.get('name')!r} is required "
                    "but has no selectors"
                )


SPEC = load_workload_spec()
WORKLOADS: dict[str, dict[str, Any]] = SPEC["workloads"]
