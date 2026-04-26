# JSON Polyglot Benchmark Results

**Runs per cell:** 2 · **Pinning:** Linux strict (taskset -c 0)
**Hardware:** Linux 6.18.5 x86_64 on vm.
**Date:** 2026-04-26.

Two workloads, each language listed twice (idiomatic / optimized flag profile).
Median wall-clock time is the headline number; p95, σ (population stddev),
min, and max are reported per cell so noise is visible. Lower is better.

## JSON validate-and-roundtrip

Per iteration: parse → stringify → discard. The unmutated parse lets
Perry's lazy tape (v0.5.204+) memcpy the original blob bytes for
stringify, which is why Perry's headline number on this workload is so
low — the lazy path can avoid materializing the parse tree entirely.
10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| rust serde_json (LTO+1cgu) | optimized | 282 | 292 | 10.0 | 272 | 292 | 9 |
| rust serde_json | idiomatic | 298 | 299 | 1.0 | 297 | 299 | 9 |
| bun (default) | idiomatic | 359 | 359 | 0.0 | 359 | 359 | 85 |
| node --max-old=4096 | optimized | 471 | 476 | 4.5 | 467 | 476 | 102 |
| node (default) | idiomatic | 494 | 531 | 36.5 | 458 | 531 | 102 |
| go -ldflags="-s -w" -trimpath | optimized | 997 | 1015 | 17.5 | 980 | 1015 | 16 |
| go (encoding/json) | idiomatic | 1118 | 1121 | 3.0 | 1115 | 1121 | 15 |

## JSON parse-and-iterate

Per iteration: parse → sum every record's nested.x (touches every element)
→ stringify. The full-tree iteration FORCES Perry's lazy tape to
materialize, so this is the honest comparison for workloads that touch
JSON content. 10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| rust serde_json | idiomatic | 271 | 271 | 0.0 | 271 | 271 | 9 |
| rust serde_json (LTO+1cgu) | optimized | 286 | 295 | 8.5 | 278 | 295 | 9 |
| bun (default) | idiomatic | 450 | 453 | 2.5 | 448 | 453 | 95 |
| node (default) | idiomatic | 516 | 551 | 34.5 | 482 | 551 | 103 |
| node --max-old=4096 | optimized | 516 | 555 | 39.0 | 477 | 555 | 103 |
| go (encoding/json) | idiomatic | 1000 | 1009 | 8.5 | 992 | 1009 | 15 |
| go -ldflags="-s -w" -trimpath | optimized | 1075 | 1127 | 51.5 | 1024 | 1127 | 18 |
