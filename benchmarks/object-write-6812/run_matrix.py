#!/usr/bin/env python3
"""Run the #6812 object-write matrix with alternating Node/Perry samples."""

from __future__ import annotations

import argparse
import json
import re
import statistics
import subprocess
import sys
from pathlib import Path


CASES = [
    ("Key", "o.x", "key_dot"),
    ("Key", 'o["x"]', "key_literal"),
    ("Key", "stable o[k]", "key_stable_dynamic"),
    ("Key", "alternating dynamic keys", "key_alternating_dynamic"),
    ("RHS", "numeric scalar", "rhs_numeric"),
    ("RHS", "pointer-capable value", "rhs_pointer"),
    ("RHS", "allocating literal", "rhs_allocating"),
    ("RHS", "function call", "rhs_call"),
    ("Receiver shapes", "monomorphic", "shape_monomorphic"),
    ("Receiver shapes", "2-shape", "shape_two"),
    ("Receiver shapes", "4-shape", "shape_four"),
    ("Receiver shapes", "transition before loop", "shape_transition_before_loop"),
    ("Fields/iteration", "1", "fields_one"),
    ("Fields/iteration", "2", "fields_two"),
    ("Fields/iteration", "4", "fields_four"),
    ("Fields/iteration", "8", "fields_eight"),
    ("Loop form", "single counted loop", "loop_single_counted"),
    ("Loop form", "current nested loop", "loop_current_nested"),
    ("Loop form", "stable local bounds", "loop_stable_local_bounds"),
    ("Loop form", "non-zero inner start", "loop_nonzero_start"),
    ("Storage", "inline existing slot", "storage_inline"),
    ("Storage", "wide/overflow object", "storage_overflow"),
    ("Receiver kind", "anonymous object", "receiver_anonymous"),
    ("Receiver kind", "class instance", "receiver_class_instance"),
    ("Receiver kind", "class-id-zero plain object", "receiver_class_id_zero"),
]

RESULT_RE = re.compile(
    r"cell (?P<case>\S+) ms (?P<ms>\d+) writes (?P<writes>\d+) "
    r"sink (?P<sink>-?(?:\d+(?:\.\d+)?|NaN|Infinity))"
)


def run_once(command: list[str]) -> dict[str, int | float | str]:
    completed = subprocess.run(
        command,
        check=True,
        capture_output=True,
        text=True,
    )
    match = RESULT_RE.search(completed.stdout)
    if match is None:
        raise RuntimeError(
            f"missing matrix result in {command!r}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    result: dict[str, int | float | str] = {
        "case": match.group("case"),
        "ms": int(match.group("ms")),
        "writes": int(match.group("writes")),
        "sink": match.group("sink"),
    }
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--node", default="node")
    parser.add_argument("--samples", type=int, default=15)
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument(
        "--cases",
        help="comma-separated matrix case ids (default: all)",
    )
    args = parser.parse_args()

    if args.samples < 1 or args.warmups < 0:
        parser.error("--samples must be positive and --warmups non-negative")
    if not args.perry.is_file():
        parser.error(f"Perry executable does not exist: {args.perry}")

    source = Path(__file__).with_name("matrix.ts").resolve()
    selected = None if args.cases is None else set(args.cases.split(","))
    cases = [case for case in CASES if selected is None or case[2] in selected]
    unknown = set() if selected is None else selected - {case[2] for case in cases}
    if unknown:
        parser.error(f"unknown case ids: {', '.join(sorted(unknown))}")

    report = []
    for dimension, cell, case_id in cases:
        node_command = [
            args.node,
            "--experimental-strip-types",
            str(source),
            case_id,
        ]
        perry_command = [str(args.perry.resolve()), case_id]

        for _ in range(args.warmups):
            node_warm = run_once(node_command)
            perry_warm = run_once(perry_command)
            if (node_warm["writes"], node_warm["sink"]) != (
                perry_warm["writes"],
                perry_warm["sink"],
            ):
                raise RuntimeError(
                    f"warmup output mismatch for {case_id}: "
                    f"Node={node_warm}, Perry={perry_warm}"
                )

        node_samples: list[int] = []
        perry_samples: list[int] = []
        expected_output = None
        for _ in range(args.samples):
            node_result = run_once(node_command)
            perry_result = run_once(perry_command)
            node_output = (node_result["writes"], node_result["sink"])
            perry_output = (perry_result["writes"], perry_result["sink"])
            if node_output != perry_output:
                raise RuntimeError(
                    f"output mismatch for {case_id}: "
                    f"Node={node_result}, Perry={perry_result}"
                )
            if expected_output is not None and expected_output != node_output:
                raise RuntimeError(
                    f"non-deterministic checksum for {case_id}: "
                    f"expected={expected_output}, observed={node_output}"
                )
            expected_output = node_output
            node_samples.append(int(node_result["ms"]))
            perry_samples.append(int(perry_result["ms"]))

        report.append(
            {
                "dimension": dimension,
                "cell": cell,
                "id": case_id,
                "writes": expected_output[0],
                "sink": expected_output[1],
                "node_ms": node_samples,
                "node_median_ms": statistics.median(node_samples),
                "perry_ms": perry_samples,
                "perry_median_ms": statistics.median(perry_samples),
            }
        )
        print(
            f"[{case_id}] Node {statistics.median(node_samples)} ms, "
            f"Perry {statistics.median(perry_samples)} ms",
            file=sys.stderr,
            flush=True,
        )

    print(json.dumps(report, indent=2))
    print()
    print("| Dimension | Cell | Writes | Node median | Perry median |")
    print("|---|---|---:|---:|---:|")
    for row in report:
        print(
            f"| {row['dimension']} | {row['cell']} | {row['writes']} | "
            f"{row['node_median_ms']} ms | {row['perry_median_ms']} ms |"
        )
    print()
    print("<details>")
    print("<summary>Raw alternating samples and checksums</summary>")
    print()
    print("| Cell | Checksum | Node ms | Perry ms |")
    print("|---|---:|---|---|")
    for row in report:
        node_samples = ", ".join(str(sample) for sample in row["node_ms"])
        perry_samples = ", ".join(str(sample) for sample in row["perry_ms"])
        print(
            f"| `{row['id']}` | `{row['sink']}` | "
            f"`[{node_samples}]` | `[{perry_samples}]` |"
        )
    print()
    print("</details>")


if __name__ == "__main__":
    main()
