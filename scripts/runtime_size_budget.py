#!/usr/bin/env python3
"""Linked-size budget for small programs (the runtime every binary carries).

Why this exists: between the July and September size campaigns the runtime a
small program links regrew from ~4.7 MB to ~7.1 MB, and nothing noticed. The
only size signals were archive (`.a`) sizes and a report-only job, and an
archive can grow by megabytes without a linked program growing at all — or
the reverse. What users ship is the dead-stripped, linked program, so that is
what this gates.

For each fixture in benchmarks/runtime_size_budget/*.ts it compiles with the
given perry (the normal pipeline: auto-optimize and all), RUNS the binary and
requires the output to match `<name>.expected` — a gate must prove its
subject was live, and a broken build can easily be small — then compares the
stripped size with benchmarks/runtime_size_budget/budget.json for this
platform.

  --check            fail when a fixture exceeds its budget by more than the
                     tolerance (default mode)
  --update           rewrite this platform's budgets from the measured sizes
                     (review the diff: a budget going UP is the thing to justify)
  --perry PATH       compiler to use (default: $PERRY_BIN, target/release/perry)

A shrink beyond the tolerance is reported but does not fail; tighten the
budget with --update so the win is locked in.
"""
import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "benchmarks" / "runtime_size_budget"
BUDGET = FIXTURES / "budget.json"


def platform_key() -> str:
    machine = platform.machine().lower()
    machine = {"x86_64": "x64", "amd64": "x64", "aarch64": "arm64"}.get(machine, machine)
    return f"{sys.platform}-{machine}"


def measure(perry: str, fixture: Path, work: Path) -> int:
    out = work / fixture.stem
    env = dict(os.environ)
    env.setdefault("PERRY_WORKSPACE_ROOT", str(ROOT))
    compile_cmd = [perry, "compile", str(fixture), "-o", str(out)]
    result = subprocess.run(compile_cmd, cwd=work, env=env, capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit(
            f"FAIL {fixture.name}: compile failed (exit {result.returncode})\n"
            f"{result.stdout[-3000:]}{result.stderr[-3000:]}"
        )
    # Interleaved exactly like the `node … 2>&1` that produced `.expected`.
    run = subprocess.run(
        [str(out)], cwd=work, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=60
    )
    expected = (fixture.with_suffix(".expected")).read_text()
    actual = run.stdout
    if run.returncode != 0 or actual != expected:
        raise SystemExit(
            f"FAIL {fixture.name}: the binary did not reproduce {fixture.stem}.expected "
            f"(exit {run.returncode}); a size measured on a broken build means nothing.\n"
            f"--- expected\n{expected}--- actual\n{actual}"
        )
    stripped = work / f"{fixture.stem}.stripped"
    shutil.copyfile(out, stripped)
    subprocess.run(["strip", str(stripped)], check=True, capture_output=True)
    return stripped.stat().st_size


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--update", action="store_true")
    parser.add_argument("--perry", default=os.environ.get("PERRY_BIN", str(ROOT / "target/release/perry")))
    args = parser.parse_args()
    args.perry = str(Path(args.perry).resolve())

    budget = json.loads(BUDGET.read_text()) if BUDGET.exists() else {"tolerance_pct": 2.0, "platforms": {}}
    tolerance = float(budget.get("tolerance_pct", 2.0))
    key = platform_key()
    fixtures = sorted(FIXTURES.glob("*.ts"))
    if not fixtures:
        print(f"FAIL: no fixtures under {FIXTURES}")
        return 1

    sizes = {}
    with tempfile.TemporaryDirectory(prefix="perry-size-budget-") as tmp:
        for fixture in fixtures:
            sizes[fixture.stem] = measure(args.perry, fixture, Path(tmp))

    if args.update:
        budget.setdefault("platforms", {})[key] = {name: {"max_bytes": size} for name, size in sizes.items()}
        BUDGET.write_text(json.dumps(budget, indent=2, sort_keys=True) + "\n")
        for name, size in sizes.items():
            print(f"{name:10} {size:>12,}")
        print(f"updated {BUDGET.relative_to(ROOT)} [{key}]")
        return 0

    limits = budget.get("platforms", {}).get(key)
    if limits is None:
        print(f"no budget for platform {key}; measured (not gated):")
        for name, size in sizes.items():
            print(f"  {name:10} {size:>12,}")
        return 0
    failed = False
    print(f"runtime size budget [{key}], tolerance {tolerance}%")
    for name, size in sizes.items():
        limit = limits.get(name, {}).get("max_bytes")
        if limit is None:
            print(f"  {name:10} {size:>12,}  (no budget: add it with --update)")
            failed = True
            continue
        delta = (size - limit) / limit * 100.0
        verdict = "ok"
        if delta > tolerance:
            verdict = "OVER BUDGET"
            failed = True
        elif delta < -tolerance:
            verdict = "under budget: tighten with --update"
        print(f"  {name:10} {size:>12,}  budget {limit:>12,}  {delta:+6.2f}%  {verdict}")
    if failed:
        print(
            "\nA small program's linked size grew. Find the new retainer before raising the\n"
            "budget: link with PERRY_LINK_MAP=<file> and PERRY_EXTRA_LINK_ARGS='-Wl,-why_live,<sym>'."
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
