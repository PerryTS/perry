#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import os
import platform
import re
import signal
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_PACKAGES_FILE = Path(__file__).with_name("packages.json")
DEFAULT_OUT_DIR = REPO_ROOT / ".npm-sweep-results"
CSV_FIELDS = [
    "timestamp_utc",
    "git_sha",
    "package",
    "requested",
    "resolved_version",
    "status",
    "first_failure_line",
    "install_ms",
    "compile_ms",
    "run_ms",
    "total_ms",
    "perry_version",
    "node_version",
    "npm_version",
]
SUCCESS_STATUSES = {"pass", "compile-pass", "planned"}
PRIORITY_FAILURE_MARKERS = (
    "undefined symbol",
    "undefined reference",
    "refusing to link",
)
FAILURE_MARKERS = (
    *PRIORITY_FAILURE_MARKERS,
    "module not found",
    "cannot find",
    "not found",
    "typeerror",
    "referenceerror",
    "syntaxerror",
    "rangeerror",
    "error:",
    "failed",
    "panic",
    "exception",
)


@dataclass
class PackageTarget:
    name: str
    version: str = "latest"
    import_spec: str | None = None
    compile_packages: list[str] = field(default_factory=list)
    tier: str = ""
    reason: str = ""

    @property
    def requested(self) -> str:
        return f"{self.name}@{self.version}" if self.version else self.name

    def normalized(self) -> "PackageTarget":
        if not self.import_spec:
            self.import_spec = self.name
        if not self.compile_packages:
            self.compile_packages = [self.name]
        return self


@dataclass
class CommandResult:
    argv: list[str]
    exit_code: int
    stdout: str
    stderr: str
    duration_ms: int
    timed_out: bool = False


def parse_package_spec(spec: str) -> PackageTarget:
    value = spec.strip()
    if not value:
        raise ValueError("empty package spec")
    if value.startswith("@"):
        slash = value.find("/")
        if slash == -1:
            raise ValueError(f"scoped package spec is missing a name: {spec}")
        at = value.rfind("@")
        if at > slash:
            return PackageTarget(name=value[:at], version=value[at + 1 :] or "latest").normalized()
        return PackageTarget(name=value, version="latest").normalized()
    if "@" in value:
        name, version = value.rsplit("@", 1)
        if name and version:
            return PackageTarget(name=name, version=version).normalized()
    return PackageTarget(name=value, version="latest").normalized()


def safe_name(name: str) -> str:
    trimmed = name.strip().lstrip("@")
    safe = re.sub(r"[^A-Za-z0-9._-]+", "-", trimmed).strip("-")
    return safe or "package"


def load_manifest(path: Path) -> tuple[list[PackageTarget], int]:
    raw = json.loads(path.read_text(encoding="utf-8"))
    default_limit = int(raw.get("default_limit", 0))
    targets = []
    for entry in raw.get("packages", []):
        target = PackageTarget(
            name=entry["name"],
            version=entry.get("version", "latest"),
            import_spec=entry.get("import_spec") or entry.get("import"),
            compile_packages=entry.get("compile_packages")
            or entry.get("compilePackages")
            or [],
            tier=entry.get("tier", ""),
            reason=entry.get("reason", ""),
        ).normalized()
        targets.append(target)
    return targets, default_limit


def select_targets(args: argparse.Namespace) -> list[PackageTarget]:
    explicit_specs: list[str] = []
    for group in args.packages or []:
        explicit_specs.extend(part.strip() for part in group.split(",") if part.strip())
    explicit_specs.extend(args.package or [])
    if explicit_specs:
        targets = [parse_package_spec(spec) for spec in explicit_specs]
    else:
        targets, manifest_limit = load_manifest(Path(args.packages_file))
        if args.limit is None and manifest_limit > 0:
            args.limit = manifest_limit
    if args.limit is not None and args.limit > 0:
        targets = targets[: args.limit]
    return targets


def decode_timeout_output(value: str | bytes | None) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return value


def run_command(argv: list[str], cwd: Path, timeout_secs: int, env: dict[str, str] | None = None) -> CommandResult:
    start = time.monotonic()
    try:
        proc = subprocess.Popen(
            argv,
            cwd=cwd,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        try:
            stdout, stderr = proc.communicate(timeout=timeout_secs)
            return CommandResult(
                argv=argv,
                exit_code=proc.returncode,
                stdout=stdout,
                stderr=stderr,
                duration_ms=int((time.monotonic() - start) * 1000),
            )
        except subprocess.TimeoutExpired:
            signal_process_tree(proc, signal.SIGTERM)
            try:
                stdout, stderr = proc.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                signal_process_tree(proc, signal.SIGKILL)
                stdout, stderr = proc.communicate()
            return CommandResult(
                argv=argv,
                exit_code=124,
                stdout=decode_timeout_output(stdout),
                stderr=decode_timeout_output(stderr),
                duration_ms=int((time.monotonic() - start) * 1000),
                timed_out=True,
            )
    except FileNotFoundError as exc:
        return CommandResult(
            argv=argv,
            exit_code=127,
            stdout="",
            stderr=str(exc),
            duration_ms=int((time.monotonic() - start) * 1000),
        )


def signal_process_tree(proc: subprocess.Popen[str], sig: signal.Signals) -> None:
    if proc.poll() is not None:
        return
    try:
        os.killpg(proc.pid, sig)
    except Exception:
        if sig == signal.SIGTERM:
            proc.terminate()
        else:
            proc.kill()


def one_line(text: str, limit: int = 300) -> str:
    compact = re.sub(r"\s+", " ", text.strip())
    if len(compact) <= limit:
        return compact
    return compact[: limit - 1] + "..."


def first_failure_line(*streams: str) -> str:
    lines = []
    for stream in streams:
        lines.extend(raw.strip() for raw in stream.splitlines() if raw.strip())

    for line in lines:
        lowered = line.lower()
        if any(marker in lowered for marker in PRIORITY_FAILURE_MARKERS):
            return one_line(line)

    fallback = ""
    for line in lines:
        if not fallback and not line.startswith("npm notice"):
            fallback = line
        lowered = line.lower()
        if any(marker in lowered for marker in FAILURE_MARKERS) or line.startswith(("Error", "FAIL")):
            return one_line(line)
    return one_line(fallback)


def command_log(result: CommandResult) -> str:
    parts = [
        "$ " + " ".join(result.argv),
        f"exit_code={result.exit_code} timed_out={str(result.timed_out).lower()} duration_ms={result.duration_ms}",
        "",
        "--- stdout ---",
        result.stdout.rstrip(),
        "",
        "--- stderr ---",
        result.stderr.rstrip(),
        "",
    ]
    return "\n".join(parts)


def generated_package_json(target: PackageTarget) -> dict[str, Any]:
    return {
        "name": f"perry-npm-sweep-{safe_name(target.name)}",
        "private": True,
        "type": "module",
        "dependencies": {
            target.name: target.version or "latest",
        },
        "perry": {
            "compilePackages": target.compile_packages,
            "allow": {
                "compilePackages": target.compile_packages,
            },
            "experiments": {
                "treeShake": True,
            },
            "defines": {
                "process.env.DEV": "false",
                "process.env.NODE_ENV": "production",
            },
        },
    }


def generated_entry(target: PackageTarget) -> str:
    package_name = json.dumps(target.name)
    import_spec = json.dumps(target.import_spec or target.name)
    return "\n".join(
        [
            f"import * as packageValue from {import_spec};",
            "",
            "const keys = Object.keys(packageValue).sort();",
            "console.log(JSON.stringify({",
            f"  package: {package_name},",
            "  namespaceType: typeof packageValue,",
            "  keyCount: keys.length,",
            "  sampleKeys: keys.slice(0, 8),",
            "}));",
            "",
        ]
    )


def write_fixture(target: PackageTarget, work_dir: Path) -> None:
    work_dir.mkdir(parents=True, exist_ok=True)
    (work_dir / "package.json").write_text(
        json.dumps(generated_package_json(target), indent=2) + "\n",
        encoding="utf-8",
    )
    (work_dir / "entry.ts").write_text(generated_entry(target), encoding="utf-8")


def read_resolved_version(work_dir: Path, package_name: str) -> str:
    package_json = work_dir / "node_modules" / package_name / "package.json"
    if not package_json.exists():
        return ""
    try:
        raw = json.loads(package_json.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return ""
    return str(raw.get("version", ""))


def copy_fixture_files(work_dir: Path, log_dir: Path) -> None:
    for name in ("package.json", "entry.ts"):
        source = work_dir / name
        if source.exists():
            shutil.copyfile(source, log_dir / name)


def empty_step() -> dict[str, Any]:
    return {"exit_code": None, "duration_ms": 0, "timed_out": False, "log": ""}


def run_target(target: PackageTarget, args: argparse.Namespace, out_dir: Path, work_parent: Path) -> dict[str, Any]:
    started = time.monotonic()
    safe = safe_name(target.name)
    log_dir = out_dir / "logs" / safe
    log_dir.mkdir(parents=True, exist_ok=True)
    result: dict[str, Any] = {
        "package": target.name,
        "requested": target.requested,
        "resolved_version": "",
        "import_spec": target.import_spec or target.name,
        "compile_packages": target.compile_packages,
        "tier": target.tier,
        "reason": target.reason,
        "status": "planned" if args.dry_run else "pending",
        "first_failure_line": "",
        "install": empty_step(),
        "compile": empty_step(),
        "run": empty_step(),
        "total_ms": 0,
        "logs_dir": str(log_dir.relative_to(out_dir)),
    }
    if args.dry_run:
        result["total_ms"] = int((time.monotonic() - started) * 1000)
        return result

    work_dir = work_parent / safe
    if work_dir.exists():
        shutil.rmtree(work_dir)
    write_fixture(target, work_dir)
    copy_fixture_files(work_dir, log_dir)

    npm_argv = [args.npm_bin, "install", "--silent", "--no-audit", "--no-fund"]
    if args.ignore_scripts:
        npm_argv.append("--ignore-scripts")
    install = run_command(npm_argv, cwd=work_dir, timeout_secs=args.install_timeout)
    (log_dir / "install.log").write_text(command_log(install), encoding="utf-8")
    result["install"] = {
        "exit_code": install.exit_code,
        "duration_ms": install.duration_ms,
        "timed_out": install.timed_out,
        "log": str((log_dir / "install.log").relative_to(out_dir)),
    }
    if install.exit_code != 0:
        result["status"] = "install-timeout" if install.timed_out else "install-fail"
        result["first_failure_line"] = first_failure_line(install.stderr, install.stdout)
        result["total_ms"] = int((time.monotonic() - started) * 1000)
        return result

    result["resolved_version"] = read_resolved_version(work_dir, target.name)

    compile_result = run_command(
        [args.perry_bin, "entry.ts", "-o", "out"],
        cwd=work_dir,
        timeout_secs=args.compile_timeout,
    )
    (log_dir / "compile.log").write_text(command_log(compile_result), encoding="utf-8")
    result["compile"] = {
        "exit_code": compile_result.exit_code,
        "duration_ms": compile_result.duration_ms,
        "timed_out": compile_result.timed_out,
        "log": str((log_dir / "compile.log").relative_to(out_dir)),
    }
    if compile_result.exit_code != 0:
        result["status"] = "compile-timeout" if compile_result.timed_out else "compile-fail"
        result["first_failure_line"] = first_failure_line(compile_result.stderr, compile_result.stdout)
        result["total_ms"] = int((time.monotonic() - started) * 1000)
        return result

    if args.skip_run:
        result["status"] = "compile-pass"
        result["total_ms"] = int((time.monotonic() - started) * 1000)
        return result

    run_result = run_command(["./out"], cwd=work_dir, timeout_secs=args.run_timeout)
    (log_dir / "run.log").write_text(command_log(run_result), encoding="utf-8")
    result["run"] = {
        "exit_code": run_result.exit_code,
        "duration_ms": run_result.duration_ms,
        "timed_out": run_result.timed_out,
        "log": str((log_dir / "run.log").relative_to(out_dir)),
    }
    if run_result.exit_code != 0:
        result["status"] = "run-timeout" if run_result.timed_out else "run-fail"
        result["first_failure_line"] = first_failure_line(run_result.stderr, run_result.stdout)
    else:
        result["status"] = "pass"
    result["total_ms"] = int((time.monotonic() - started) * 1000)
    return result


def tool_version(argv: list[str], cwd: Path) -> str:
    result = run_command(argv, cwd=cwd, timeout_secs=10)
    text = first_failure_line(result.stdout, result.stderr)
    return text if result.exit_code == 0 else ""


def git_sha() -> str:
    result = run_command(["git", "rev-parse", "HEAD"], cwd=REPO_ROOT, timeout_secs=10)
    if result.exit_code == 0:
        return result.stdout.strip()
    return ""


def build_metadata(args: argparse.Namespace, targets: list[PackageTarget]) -> dict[str, Any]:
    now = datetime.now(timezone.utc).replace(microsecond=0)
    return {
        "schema_version": 1,
        "timestamp_utc": now.isoformat().replace("+00:00", "Z"),
        "git_sha": git_sha(),
        "platform": platform.platform(),
        "python_version": platform.python_version(),
        "perry_bin": args.perry_bin,
        "perry_version": "" if args.dry_run else tool_version([args.perry_bin, "--version"], REPO_ROOT),
        "node_version": tool_version([args.node_bin, "--version"], REPO_ROOT),
        "npm_version": tool_version([args.npm_bin, "--version"], REPO_ROOT),
        "mode": "dry-run" if args.dry_run else ("compile-only" if args.skip_run else "compile-run"),
        "package_count": len(targets),
        "strict": bool(args.strict),
    }


def csv_row(metadata: dict[str, Any], result: dict[str, Any]) -> dict[str, Any]:
    return {
        "timestamp_utc": metadata["timestamp_utc"],
        "git_sha": metadata["git_sha"],
        "package": result["package"],
        "requested": result["requested"],
        "resolved_version": result["resolved_version"],
        "status": result["status"],
        "first_failure_line": result["first_failure_line"],
        "install_ms": result["install"]["duration_ms"],
        "compile_ms": result["compile"]["duration_ms"],
        "run_ms": result["run"]["duration_ms"],
        "total_ms": result["total_ms"],
        "perry_version": metadata["perry_version"],
        "node_version": metadata["node_version"],
        "npm_version": metadata["npm_version"],
    }


def write_csv(path: Path, metadata: dict[str, Any], results: list[dict[str, Any]], append: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    needs_header = not append or not path.exists() or path.stat().st_size == 0
    mode = "a" if append else "w"
    with path.open(mode, encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=CSV_FIELDS)
        if needs_header:
            writer.writeheader()
        for result in results:
            writer.writerow(csv_row(metadata, result))


def write_summary(path: Path, metadata: dict[str, Any], results: list[dict[str, Any]]) -> None:
    pass_count = sum(1 for row in results if row["status"] in SUCCESS_STATUSES)
    fail_count = len(results) - pass_count
    lines = [
        "## npm compilePackages sweep",
        "",
        f"- Timestamp: `{metadata['timestamp_utc']}`",
        f"- Commit: `{metadata['git_sha'][:12]}`",
        f"- Mode: `{metadata['mode']}`",
        f"- Packages: `{len(results)}` total, `{pass_count}` passing/planned, `{fail_count}` failing",
        "",
        "| Package | Version | Status | First failure line |",
        "|---------|---------|--------|--------------------|",
    ]
    for row in results:
        failure = row["first_failure_line"].replace("|", "\\|") if row["first_failure_line"] else ""
        version = row["resolved_version"] or row["requested"]
        lines.append(f"| `{row['package']}` | `{version}` | `{row['status']}` | {failure} |")
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def write_results(out_dir: Path, metadata: dict[str, Any], results: list[dict[str, Any]], history: Path | None) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "results.json").write_text(
        json.dumps({"metadata": metadata, "results": results}, indent=2) + "\n",
        encoding="utf-8",
    )
    write_csv(out_dir / "results.csv", metadata, results, append=False)
    write_summary(out_dir / "summary.md", metadata, results)
    if history is not None:
        write_csv(history, metadata, results, append=True)


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the advisory npm compilePackages sweep.")
    parser.add_argument("--packages-file", default=str(DEFAULT_PACKAGES_FILE))
    parser.add_argument("--packages", action="append", help="Comma-separated package specs, for example express,zod@latest.")
    parser.add_argument("--package", action="append", help="Single package spec. May be repeated.")
    parser.add_argument("--limit", type=int, help="Limit package count. Use 0 for all selected packages.")
    parser.add_argument("--out-dir", default=str(DEFAULT_OUT_DIR))
    parser.add_argument("--history", help="Append trend rows to this CSV file.")
    parser.add_argument("--perry-bin", default=os.environ.get("PERRY_BIN", str(REPO_ROOT / "target/release/perry")))
    parser.add_argument("--node-bin", default=os.environ.get("NODE_BIN", "node"))
    parser.add_argument("--npm-bin", default=os.environ.get("NPM_BIN", "npm"))
    parser.add_argument("--work-dir", help="Temporary fixture parent. Defaults to a disposable temp directory.")
    parser.add_argument("--keep-workdirs", action="store_true", help="Keep generated fixture directories after the run.")
    parser.add_argument("--dry-run", action="store_true", help="Write planned rows without invoking npm or Perry.")
    parser.add_argument("--skip-run", action="store_true", help="Stop after successful Perry compile/link.")
    parser.add_argument("--strict", action="store_true", help="Exit non-zero if any package fails.")
    parser.add_argument("--ignore-scripts", action="store_true", help="Pass --ignore-scripts to npm install.")
    parser.add_argument("--install-timeout", type=int, default=180)
    parser.add_argument("--compile-timeout", type=int, default=300)
    parser.add_argument("--run-timeout", type=int, default=60)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    targets = select_targets(args)
    out_dir = Path(args.out_dir)
    metadata = build_metadata(args, targets)
    history = Path(args.history) if args.history else None
    if not targets:
        write_results(out_dir, metadata, [], history)
        print(f"npm sweep: no packages selected; wrote {out_dir}")
        return 0

    temp_context: tempfile.TemporaryDirectory[str] | None = None
    if args.work_dir:
        work_parent = Path(args.work_dir)
        work_parent.mkdir(parents=True, exist_ok=True)
    else:
        temp_context = tempfile.TemporaryDirectory(prefix="perry-npm-sweep-")
        work_parent = Path(temp_context.name)

    try:
        results = []
        for index, target in enumerate(targets, start=1):
            print(f"[{index}/{len(targets)}] {target.requested}", flush=True)
            results.append(run_target(target, args, out_dir, work_parent))
        write_results(out_dir, metadata, results, history)
    finally:
        if temp_context is not None and not args.keep_workdirs:
            temp_context.cleanup()

    failures = [row for row in results if row["status"] not in SUCCESS_STATUSES]
    print(f"npm sweep: wrote {out_dir}")
    print(f"npm sweep: {len(results) - len(failures)} passing/planned, {len(failures)} failing")
    return 1 if args.strict and failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
