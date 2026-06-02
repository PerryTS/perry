# Perry vs Rust vs Zig — honest benchmark suite

Three workloads that stress different memory patterns, implemented idiomatically
in Perry, Rust, and Zig. Results include peak RSS, latency percentiles, and
binary size — **including where Perry loses**.

## Workloads

| # | Name | Stresses | Expectation |
|---|------|----------|-------------|
| 1 | JSON pipeline | allocation + GC | Perry loses on RSS, competitive on wall time |
| 2 | HTTP echo+transform | tail latency | GC pauses visible in p99/p999 |
| 3 | Image convolution | tight compute loop | Perry competitive |

## Reproducing

```bash
cd benchmarks/honest_bench
./run.sh                  # build + run everything, write results/results.json
./run.sh --strict-output  # same, but fail non-zero on any output mismatch
python3 scripts/plot.py   # render charts into charts/
python3 scripts/report.py # render REPORT.md from results.json
```

## Layout

```
honest_bench/
├── workloads/
│   ├── 1_json_pipeline/{perry,rust,zig}/
│   ├── 2_http_server/{perry,rust,zig}/
│   └── 3_image_convolution/{perry,rust,zig}/
├── harness/
│   ├── run_bench.sh         # per-binary runner: 5 warmup + 20 measured
│   ├── capture_expected.py  # one-shot Bun reference capture (#441)
│   └── check_output.py      # per-run output match checker (#441)
├── scripts/
│   ├── gen_image.py      # deterministic 4K PPM test fixture
│   ├── gen_json.py       # deterministic 100MB JSON test fixture
│   ├── plot.py           # matplotlib charts -> charts/*.png
│   ├── report.py         # render REPORT.md from results.json
│   └── summary.py        # render results/summary.txt — perf vs correctness (#441)
├── assets/               # generated test fixtures (gitignored)
├── results/
│   ├── results.json      # per-run measurements (committed)
│   ├── expected.json     # Bun reference tokens + output sha256 (committed; #441)
│   └── summary.txt       # latest run's perf vs correctness split (regenerated)
└── run.sh                # top-level driver
```

## Output-correctness gate (#441)

Wall-clock time on a binary producing the wrong output is not a perf win.
The harness compares every measured run's stdout (canonical `key=value`
tokens like `hash=`, `checksum=`, `records_in=`, `dims=`) and any produced
output file (sha256) against a Bun-captured reference cached in
`results/expected.json`.

- **Default mode**: mismatches log + continue. `results/summary.txt` lists
  them under a `CORRECTNESS REGRESSIONS` section, separately from perf rows.
- **`--strict-output`**: any mismatch exits non-zero (CI gate).
- **Refresh the reference** (only when output semantics intentionally change):
  `HONEST_BENCH_REFRESH_EXPECTED=1 ./run.sh` — re-runs Bun once per workload
  and overwrites `expected.json`. Commit the new `expected.json` alongside
  the source change that motivated it.

Volatile lines (timestamps, elapsed-ms readouts) are skipped by name; only
canonical hash/count/checksum/dimension tokens are compared.

## Rules

- Release/optimized builds only (`--release`, `ReleaseFast`, Perry's native path).
- Same algorithm, same data structures, no SIMD intrinsics unless all three have them.
- 5 warmup + 20 measured runs per binary. Median + stddev reported.
- Same machine, same data, same order.
- Test fixtures are deterministic (seeded RNG) so all three languages process
  identical bytes.
- Bun is the truth source for output correctness; perry, rust, zig, node
  are checked against it byte-for-byte (file output) and token-for-token
  (stdout).
