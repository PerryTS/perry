# JSON Polyglot Benchmark Results

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.5.0 arm64 on perry-macos.
**Date:** 2026-08-08.

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
| rust serde_json (LTO+1cgu) | optimized | 178 | 194 | 4.6 | 178 | 194 | 10 |
| perry (gen-gc + lazy tape) | optimized | 184 | 190 | 1.9 | 183 | 190 | 79 |
| rust serde_json | idiomatic | 188 | 203 | 4.3 | 188 | 203 | 9 |
| bun (default) | idiomatic | 220 | 232 | 3.5 | 219 | 232 | 83 |
| node --max-old=4096 | optimized | 380 | 385 | 4.2 | 373 | 385 | 100 |
| node (default) | idiomatic | 380 | 389 | 3.9 | 373 | 389 | 99 |
| perry (mark-sweep, no lazy) | idiomatic | 1198 | 1206 | 2.6 | 1196 | 1206 | 62 |
| swift -O -wmo (Foundation) | optimized | 3461 | 3487 | 10.0 | 3456 | 3487 | 28 |
| swift -O (Foundation) | idiomatic | 3466 | 3481 | 8.1 | 3452 | 3481 | 28 |

## JSON parse-and-iterate

Per iteration: parse → sum every record's nested.x (touches every element)
→ stringify. The full-tree iteration FORCES Perry's lazy tape to
materialize, so this is the honest comparison for workloads that touch
JSON content. 10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| rust serde_json (LTO+1cgu) | optimized | 179 | 198 | 5.4 | 179 | 198 | 10 |
| rust serde_json | idiomatic | 188 | 203 | 4.3 | 188 | 203 | 9 |
| bun (default) | idiomatic | 222 | 222 | 0.7 | 220 | 222 | 87 |
| node --max-old=4096 | optimized | 384 | 389 | 4.4 | 376 | 389 | 95 |
| node (default) | idiomatic | 385 | 395 | 5.0 | 378 | 395 | 95 |
| perry (mark-sweep, no lazy) | idiomatic | 1247 | 1250 | 1.9 | 1244 | 1250 | 59 |
| perry (gen-gc + lazy tape) | optimized | 2030 | 2048 | 69.2 | 1804 | 2048 | 170 |
| swift -O (Foundation) | idiomatic | 3451 | 3477 | 11.8 | 3441 | 3477 | 28 |
| swift -O -wmo (Foundation) | optimized | 3474 | 3502 | 14.0 | 3450 | 3502 | 28 |
