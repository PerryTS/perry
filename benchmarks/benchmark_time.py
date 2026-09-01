#!/usr/bin/env python3
"""Extract declared integer-millisecond fields from suite benchmark output.

The suite contains both elapsed times and other numeric metrics.  A numeric
line is not a time merely because it appears first, so every fixture has one
explicit cross-runtime timing contract here.  ``01_startup`` is the sole
external-timing fixture.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence


@dataclass(frozen=True)
class TimingContract:
    """The output labels that have measurement rather than semantic meaning."""

    time_label: str | None
    auxiliary_time_labels: tuple[str, ...] = ()
    metric_labels: tuple[str, ...] = ()

    @property
    def measurement_labels(self) -> frozenset[str]:
        labels = self.auxiliary_time_labels + self.metric_labels
        if self.time_label is not None:
            labels = (self.time_label,) + labels
        return frozenset(labels)


# This is intentionally exhaustive rather than inferred from output position or
# filename.  Some established fixtures use a historical timing label that does
# not match their filename (for example 12_binary_trees -> object_alloc).
TIMING_CONTRACTS: dict[str, TimingContract] = {
    "01_startup": TimingContract(None),
    "02_loop_overhead": TimingContract("loop_overhead"),
    "03_array_write": TimingContract("array_write"),
    "04_array_read": TimingContract("array_read"),
    "05_fibonacci": TimingContract("fibonacci"),
    "06_math_intensive": TimingContract("math_intensive"),
    "07_object_create": TimingContract("object_create"),
    "08_string_concat": TimingContract("string_concat"),
    "09_method_calls": TimingContract("method_calls"),
    "10_nested_loops": TimingContract("nested_loops"),
    "11_prime_sieve": TimingContract("prime_sieve"),
    "12_binary_trees": TimingContract("object_alloc"),
    "13_factorial": TimingContract("accumulate"),
    "14_closure": TimingContract("function_calls"),
    "15_mandelbrot": TimingContract("mandelbrot"),
    "16_matrix_multiply": TimingContract("matrix_multiply"),
    "17_loop_data_dependent": TimingContract("loop_data_dependent"),
    "bench_array_grow": TimingContract("array_grow"),
    "bench_buffer_readwrite": TimingContract("buffer_readwrite"),
    "bench_gc_pressure": TimingContract("gc_pressure"),
    "bench_int_arithmetic": TimingContract("int_arithmetic"),
    "bench_json_readonly": TimingContract("json_readonly"),
    "bench_json_readonly_indexed": TimingContract("json_readonly_indexed"),
    "bench_json_roundtrip": TimingContract("json_roundtrip"),
    "bench_json_typed_roundtrip": TimingContract("json_typed_roundtrip"),
    "bench_numeric_array_downgrade": TimingContract("numeric_array_downgrade"),
    "bench_numeric_array_numeric": TimingContract("numeric_array_numeric"),
    "bench_object_property": TimingContract("object_property"),
    "bench_string_heavy": TimingContract("string_heavy"),
    # Keep ta_untyped_typed_ratio first in the fixture: it is the intentional
    # machine-normalized #5525 Perry-to-Perry regression metric.  Cross-runtime
    # tables compare the declared absolute untyped-access time instead.
    "bench_typed_array_untyped_access": TimingContract(
        "ta_untyped_access",
        auxiliary_time_labels=("ta_typed_access",),
        metric_labels=("ta_untyped_typed_ratio",),
    ),
}


class UnscoreableError(ValueError):
    """Raised when stdout cannot establish an integer elapsed time."""


def benchmark_name(value: str | Path) -> str:
    return Path(value).stem


def timing_contract(value: str | Path) -> TimingContract:
    name = benchmark_name(value)
    try:
        return TIMING_CONTRACTS[name]
    except KeyError as exc:
        raise UnscoreableError(
            f"{name}: no declared cross-runtime millisecond label"
        ) from exc


def extract_time_ms(value: str | Path, output: str) -> int:
    """Return the one declared integer-millisecond value in ``output``."""

    name = benchmark_name(value)
    contract = timing_contract(name)
    label = contract.time_label
    if label is None:
        raise UnscoreableError(
            f"{name}: no internal time-labelled line; measure process startup externally"
        )

    values: list[str] = []
    for raw_line in output.splitlines():
        key, separator, raw_value = raw_line.strip().partition(":")
        if separator and key == label:
            values.append(raw_value.strip())

    if not values:
        raise UnscoreableError(
            f"{name}: missing required time-labelled line {label}:<integer-ms>"
        )
    if len(values) != 1:
        raise UnscoreableError(
            f"{name}: expected one {label}:<integer-ms> line, found {len(values)}"
        )

    raw_value = values[0]
    if not re.fullmatch(r"[0-9]+", raw_value):
        raise UnscoreableError(
            f"{name}: non-integer millisecond value {raw_value!r} for {label}"
        )
    return int(raw_value)


_CONSOLE_LITERAL_RE = re.compile(
    r"console\.log\(\s*(?:\"([^\"]*)\"|'([^']*)')"
)


@dataclass(frozen=True)
class FixtureAudit:
    name: str
    first_output: str
    is_time: bool
    reason: str


def _console_literal_prefixes(source: str) -> list[str]:
    return [
        match.group(1) if match.group(1) is not None else match.group(2)
        for match in _CONSOLE_LITERAL_RE.finditer(source)
    ]


def audit_fixture_first_lines(suite_dir: str | Path) -> list[FixtureAudit]:
    """Statically audit the first console output of every suite fixture."""

    suite_dir = Path(suite_dir)
    fixtures = sorted(suite_dir.glob("*.ts"))
    fixture_names = {path.stem for path in fixtures}
    contract_names = set(TIMING_CONTRACTS)
    if fixture_names != contract_names:
        missing = sorted(fixture_names - contract_names)
        stale = sorted(contract_names - fixture_names)
        raise UnscoreableError(
            f"timing contract coverage mismatch: missing={missing or 'none'}, "
            f"stale={stale or 'none'}"
        )

    audits: list[FixtureAudit] = []
    for fixture in fixtures:
        source = fixture.read_text(encoding="utf-8")
        prefixes = _console_literal_prefixes(source)
        if not prefixes:
            raise UnscoreableError(f"{fixture.stem}: fixture has no static console.log output")

        contract = TIMING_CONTRACTS[fixture.stem]
        emitted_labels = {prefix[:-1] for prefix in prefixes if prefix.endswith(":")}
        declared_labels = contract.measurement_labels
        missing_labels = sorted(declared_labels - emitted_labels)
        if missing_labels:
            raise UnscoreableError(
                f"{fixture.stem}: declared output label(s) not emitted: "
                + ", ".join(missing_labels)
            )

        first = prefixes[0]
        first_label = first[:-1] if first.endswith(":") else None
        display = f"{first}<value>" if first_label is not None else first
        if contract.time_label is not None and first_label == contract.time_label:
            audits.append(FixtureAudit(fixture.stem, display, True, "declared time"))
        elif contract.time_label is None:
            audits.append(
                FixtureAudit(
                    fixture.stem,
                    display,
                    False,
                    "external timing required",
                )
            )
        elif first_label in contract.metric_labels:
            audits.append(
                FixtureAudit(
                    fixture.stem,
                    display,
                    False,
                    f"non-time metric; cross-runtime time is {contract.time_label}",
                )
            )
        else:
            audits.append(
                FixtureAudit(
                    fixture.stem,
                    display,
                    False,
                    f"not the declared time label {contract.time_label}",
                )
            )
    return audits


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    extract = subparsers.add_parser("extract", help="extract a fixture's declared time")
    extract.add_argument("benchmark", help="fixture filename or stem")
    audit = subparsers.add_parser("audit", help="audit suite fixtures' first output lines")
    audit.add_argument(
        "--suite-dir",
        default=str(Path(__file__).resolve().parent / "suite"),
        help="directory containing suite .ts fixtures",
    )
    audit.add_argument("--all", action="store_true", help="show time-first rows too")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _build_parser().parse_args(argv)
    try:
        if args.command == "extract":
            print(extract_time_ms(args.benchmark, sys.stdin.read()))
            return 0

        audits = audit_fixture_first_lines(args.suite_dir)
        shown = audits if args.all else [audit for audit in audits if not audit.is_time]
        non_time_count = len(audits) - sum(audit.is_time for audit in audits)
        print(
            f"Audited {len(audits)} suite fixtures; "
            f"{non_time_count} have a non-time first line:"
        )
        for audit in shown:
            print(f"  {audit.name}: {audit.first_output} ({audit.reason})")
        return 0
    except UnscoreableError as exc:
        print(f"UNSCOREABLE: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
