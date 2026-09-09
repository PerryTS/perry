Longer interleaved candidate/reference checks; Node supplies the separate output oracle.
The quiet gate passed. Raw samples are in summary.json and timing.jsonl.

| Fixture / operation | Calls | Repeats | 0.5.1527 CPU µs | Merge CPU µs | Ratio | 0.5.1527 peak MiB | Merge peak MiB |
|---|---:|---:|---:|---:|---:|---:|---:|
| long_string_1m / stringify | 32768 | 9 | 32.695801 | 32.120117 | 1.0179 | 54.281 | 54.422 |
| records_array_16k / roundtrip | 6185 | 25 | 25.693614 | 25.937753 | 0.9906 | 68.656 | 68.797 |
| records_array_16k / roundtrip | 20000 | 9 | 24.851750 | 25.073950 | 0.9911 | 68.688 | 68.781 |

The earlier roundtrip peak increase did not reproduce in these longer checks.
ASCII stringify CPU samples overlap broadly (candidate 29.14–35.08 µs, reference 30.55–35.13 µs); the 1.8% higher median alone does not establish a regression. Complete-matrix acceptance remains separate.
