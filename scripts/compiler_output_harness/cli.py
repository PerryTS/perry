from __future__ import annotations

import argparse
import sys

from .capture import capture, verify_existing
from .common import DEFAULT_BENCHMARK_RUNS, HarnessError
from .spec import WORKLOADS


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Capture and verify Perry compiler-output evidence for CPU benchmarks."
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    capture_p = sub.add_parser("capture", help="compile, retain artifacts, and verify")
    capture_p.add_argument("--workload", choices=sorted(WORKLOADS), default="image_convolution")
    capture_p.add_argument("--out-dir")
    capture_p.add_argument("--perry")
    capture_p.add_argument("--clang")
    capture_p.add_argument("--target")
    capture_p.add_argument(
        "--clang-arg",
        action="append",
        help=(
            "extra clang argument for analysis-only optimized IR emission; "
            "executed-object assembly gates use Perry's retained object compile plan"
        ),
    )
    capture_p.add_argument("--runs", type=int)
    capture_p.add_argument(
        "--benchmark-mode",
        choices=sorted(DEFAULT_BENCHMARK_RUNS),
        default="smoke",
        help="default run-count profile when --runs is omitted",
    )
    capture_p.add_argument("--compile-timeout", type=int, default=300)
    capture_p.add_argument("--run-timeout", type=int, default=300)
    capture_p.add_argument("--skip-run", action="store_true")
    capture_p.add_argument("--no-gc-trace", action="store_true")
    capture_p.add_argument("--fast-math", action="store_true")
    capture_p.add_argument("--fp-contract", choices=("off", "on", "fast"))
    capture_p.add_argument(
        "--expect-fma",
        choices=("auto", "off", "on"),
        default="auto",
        help="gate FMA instructions in the retained object disassembly",
    )
    capture_p.add_argument("--perf-counters", choices=("auto", "off", "on"), default="auto")
    capture_p.add_argument("--gate", action="store_true")
    capture_p.add_argument("--print-summary", action="store_true")
    capture_p.set_defaults(func=capture)

    verify_p = sub.add_parser("verify", help="verify an existing artifact directory")
    verify_p.add_argument("--workload", choices=sorted(WORKLOADS), default="image_convolution")
    verify_p.add_argument("--artifact-dir", required=True)
    verify_p.add_argument("--target")
    verify_p.add_argument("--clang-arg", action="append")
    verify_p.add_argument("--fp-contract", choices=("off", "on", "fast"))
    verify_p.add_argument("--expect-fma", choices=("auto", "off", "on"), default="auto")
    verify_p.add_argument("--gate", action="store_true")
    verify_p.add_argument("--print-summary", action="store_true")
    verify_p.set_defaults(func=verify_existing)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except HarnessError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2
