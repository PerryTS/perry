#!/usr/bin/env python3
"""Generate the TypeScript feature compatibility matrix (#801).

The matrix is a small, committed baseline: every probe runs once under Node's
TypeScript stripper and once through a Perry-compiled native binary. Current
gaps are recorded in the markdown output instead of failing the generator; CI
uses `--check` to fail only when the committed matrix drifts from the probes.
"""

from __future__ import annotations

import argparse
import difflib
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CONFIG = REPO_ROOT / "test-features" / "feature_matrix.toml"
DEFAULT_MARKDOWN = REPO_ROOT / "test-features" / "feature_matrix.md"
DEFAULT_PERRY = REPO_ROOT / "target" / "release" / "perry"

NOISE = re.compile(
    r"^\(node:\d+\) (ExperimentalWarning|Warning|\[DEP\d+\]|\[MODULE_TYPELESS)"
    r"|^\(Use `node --trace"
)


@dataclass(frozen=True)
class Probe:
    category: str
    name: str
    path: Path
    description: str


@dataclass(frozen=True)
class Result:
    probe: Probe
    status: str
    node_exit: int | None
    perry_exit: int | None
    detail: str
    output: str


def normalize(text: str) -> str:
    lines: list[str] = []
    for raw in text.replace("\r\n", "\n").split("\n"):
        line = raw.rstrip()
        if NOISE.search(line):
            continue
        lines.append(line)
    while lines and lines[-1] == "":
        lines.pop()
    return "\n".join(lines)


def first_line(text: str) -> str:
    for line in text.splitlines():
        stripped = line.strip()
        if stripped:
            return stripped
    return "(no output)"


def shell_words(raw: object) -> list[str]:
    if raw is None:
        return []
    if isinstance(raw, list) and all(isinstance(item, str) for item in raw):
        return raw
    raise ValueError("command argument lists must be string arrays")


def read_config(path: Path) -> tuple[list[str], list[Probe]]:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    settings = data.get("settings", {})
    if not isinstance(settings, dict):
        raise ValueError("[settings] must be a TOML table")
    node_args = shell_words(settings.get("node_args"))

    raw_probes = data.get("probe", [])
    if not isinstance(raw_probes, list):
        raise ValueError("[[probe]] entries are required")

    probes: list[Probe] = []
    seen: set[tuple[str, str]] = set()
    for item in raw_probes:
        if not isinstance(item, dict):
            raise ValueError("each [[probe]] entry must be a table")
        try:
            category = item["category"]
            name = item["name"]
            rel_path = item["path"]
        except KeyError as exc:
            raise ValueError(f"probe is missing required field {exc.args[0]!r}") from exc
        if not all(isinstance(value, str) for value in (category, name, rel_path)):
            raise ValueError("probe category, name, and path must be strings")
        key = (category, name)
        if key in seen:
            raise ValueError(f"duplicate probe {category}/{name}")
        seen.add(key)
        description = item.get("description", "")
        if not isinstance(description, str):
            raise ValueError("probe description must be a string")
        probe_path = (path.parent / rel_path).resolve()
        if not probe_path.exists():
            raise FileNotFoundError(probe_path)
        probes.append(Probe(category, name, probe_path, description))

    return node_args, probes


def run(cmd: list[str], *, cwd: Path, env: dict[str, str], timeout: int) -> tuple[int, str]:
    try:
        proc = subprocess.run(
            cmd,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
        )
        return proc.returncode, proc.stdout.decode("utf-8", errors="replace")
    except subprocess.TimeoutExpired as exc:
        out = exc.stdout.decode("utf-8", errors="replace") if exc.stdout else ""
        return 124, out
    except FileNotFoundError as exc:
        return 127, str(exc)


def compile_and_run_perry(
    perry_bin: Path,
    probe: Probe,
    tmpdir: Path,
    timeout: int,
    base_env: dict[str, str],
) -> tuple[str, int | None, str]:
    binary = tmpdir / f"{probe.category}-{probe.name}"
    env = dict(base_env)
    env.setdefault("PERRY_ALLOW_UNIMPLEMENTED", "1")
    env.setdefault("PERRY_NO_AUTO_OPTIMIZE", "1")
    compile_exit, compile_out = run(
        [str(perry_bin), str(probe.path), "-o", str(binary)],
        cwd=REPO_ROOT,
        env=env,
        timeout=timeout,
    )
    if compile_exit != 0:
        return "COMPILE-FAIL", compile_exit, normalize(compile_out)

    run_exit, run_out = run([str(binary)], cwd=REPO_ROOT, env=env, timeout=timeout)
    if run_exit != 0:
        return "RUNTIME-FAIL", run_exit, normalize(run_out)
    return "PASS", run_exit, normalize(run_out)


def run_probe(
    probe: Probe,
    *,
    node_cmd: str,
    node_args: list[str],
    perry_bin: Path,
    tmpdir: Path,
    timeout: int,
    base_env: dict[str, str],
) -> Result:
    node_exit, node_out_raw = run(
        [node_cmd, *node_args, str(probe.path)],
        cwd=REPO_ROOT,
        env=base_env,
        timeout=timeout,
    )
    node_out = normalize(node_out_raw)
    if node_exit != 0:
        return Result(
            probe=probe,
            status="NODE-FAIL",
            node_exit=node_exit,
            perry_exit=None,
            detail=f"Node exit {node_exit}: {first_line(node_out)}",
            output=node_out,
        )

    perry_status, perry_exit, perry_out = compile_and_run_perry(
        perry_bin, probe, tmpdir, timeout, base_env
    )
    if perry_status != "PASS":
        return Result(
            probe=probe,
            status=perry_status,
            node_exit=node_exit,
            perry_exit=perry_exit,
            detail=f"Perry exit {perry_exit}: {first_line(perry_out)}",
            output=perry_out,
        )

    if perry_out != node_out:
        detail = "stdout differs"
        if first_line(node_out) != first_line(perry_out):
            detail = f"Node `{first_line(node_out)}` vs Perry `{first_line(perry_out)}`"
        return Result(
            probe=probe,
            status="DIFF",
            node_exit=node_exit,
            perry_exit=perry_exit,
            detail=detail,
            output=perry_out,
        )

    return Result(
        probe=probe,
        status="PASS",
        node_exit=node_exit,
        perry_exit=perry_exit,
        detail=first_line(node_out),
        output=node_out,
    )


def md_escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace("|", "\\|").replace("\n", "<br>")


def render_markdown(results: list[Result], config: Path) -> str:
    total = len(results)
    passing = sum(1 for result in results if result.status == "PASS")
    categories = sorted({result.probe.category for result in results})

    lines = [
        "# TypeScript Feature Matrix",
        "",
        "Generated by `scripts/gen_feature_matrix.py` from "
        f"`{config.relative_to(REPO_ROOT)}`.",
        "",
        "This is a compatibility radar, not a feature gate. A non-PASS row "
        "means the current Perry output differs from the Node oracle for that "
        "probe; CI fails only when this committed matrix is stale.",
        "",
        f"Summary: {passing}/{total} probes pass across {len(categories)} categories.",
        "",
        "| category | probe | status | detail |",
        "| --- | --- | --- | --- |",
    ]
    for result in sorted(results, key=lambda item: (item.probe.category, item.probe.name)):
        lines.append(
            "| "
            + " | ".join(
                [
                    md_escape(result.probe.category),
                    md_escape(result.probe.name),
                    result.status,
                    md_escape(result.detail),
                ]
            )
            + " |"
        )

    lines.extend(["", "## Categories", ""])
    lines.append("| category | pass | total |")
    lines.append("| --- | ---: | ---: |")
    for category in categories:
        subset = [result for result in results if result.probe.category == category]
        category_pass = sum(1 for result in subset if result.status == "PASS")
        lines.append(f"| {md_escape(category)} | {category_pass} | {len(subset)} |")

    lines.append("")
    return "\n".join(lines)


def write_report(results: list[Result], path: Path, config: Path) -> None:
    by_status: dict[str, int] = {}
    for result in results:
        by_status[result.status] = by_status.get(result.status, 0) + 1
    payload = {
        "config": str(config.relative_to(REPO_ROOT)),
        "summary": {
            "probes": len(results),
            "passing": by_status.get("PASS", 0),
            "by_status": by_status,
        },
        "probes": [
            {
                "category": result.probe.category,
                "name": result.probe.name,
                "path": str(result.probe.path.relative_to(REPO_ROOT)),
                "description": result.probe.description,
                "status": result.status,
                "node_exit": result.node_exit,
                "perry_exit": result.perry_exit,
                "detail": result.detail,
            }
            for result in sorted(results, key=lambda item: (item.probe.category, item.probe.name))
        ],
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def check_or_write(
    markdown: str,
    output: Path,
    *,
    check: bool,
    generated_output: Path | None,
) -> int:
    if generated_output is not None:
        generated_output.parent.mkdir(parents=True, exist_ok=True)
        generated_output.write_text(markdown, encoding="utf-8")

    if not check:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(markdown, encoding="utf-8")
        return 0

    existing = output.read_text(encoding="utf-8") if output.exists() else ""
    if existing == markdown:
        print(f"{output.relative_to(REPO_ROOT)} is up to date")
        return 0

    diff = difflib.unified_diff(
        existing.splitlines(keepends=True),
        markdown.splitlines(keepends=True),
        fromfile=f"{output.relative_to(REPO_ROOT)} (committed)",
        tofile=f"{output.relative_to(REPO_ROOT)} (generated)",
    )
    sys.stderr.writelines(diff)
    return 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--output", type=Path, default=DEFAULT_MARKDOWN)
    parser.add_argument("--generated-output", type=Path, default=None)
    parser.add_argument("--report", type=Path, default=None)
    parser.add_argument("--perry-bin", type=Path, default=DEFAULT_PERRY)
    parser.add_argument("--node-cmd", "--node-bin", dest="node_cmd", default="node")
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)

    config = args.config.resolve()
    output = args.output.resolve()
    perry_bin = args.perry_bin.resolve()
    if not perry_bin.exists():
        print(f"error: Perry binary not found: {perry_bin}", file=sys.stderr)
        return 2

    node_args, probes = read_config(config)
    env = os.environ.copy()
    env.update({
        "FORCE_COLOR": "0",
        "NO_COLOR": "1",
        "NODE_DISABLE_COLORS": "1",
    })

    with tempfile.TemporaryDirectory(prefix="perry-feature-matrix-") as raw_tmp:
        tmpdir = Path(raw_tmp)
        results = [
            run_probe(
                probe,
                node_cmd=args.node_cmd,
                node_args=node_args,
                perry_bin=perry_bin,
                tmpdir=tmpdir,
                timeout=args.timeout,
                base_env=env,
            )
            for probe in probes
        ]

    markdown = render_markdown(results, config)
    if args.report is not None:
        write_report(results, args.report.resolve(), config)
    return check_or_write(
        markdown,
        output,
        check=args.check,
        generated_output=args.generated_output.resolve() if args.generated_output else None,
    )


if __name__ == "__main__":
    raise SystemExit(main())
