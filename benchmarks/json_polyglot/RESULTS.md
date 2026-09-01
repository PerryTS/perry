# JSON Polyglot Benchmark Results

**Runs per cell:** 11 · **Pinning:** Linux strict (taskset -c 0)
**Hardware:** Linux 6.17.0-23-generic x86_64 on ideal-mastodon.
**Date:** 2026-09-01.

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
| perry (gen-gc + lazy tape) | optimized | 186 | 188 | 1.4 | 184 | 188 | 119 |
| bun (default) | idiomatic | 189 | 195 | 2.3 | 188 | 195 | 87 |
| rust serde_json (LTO+1cgu) | optimized | 213 | 213 | 0.4 | 212 | 213 | 9 |
| rust serde_json | idiomatic | 227 | 230 | 1.1 | 226 | 230 | 9 |
| node --max-old=4096 | optimized | 379 | 409 | 8.8 | 377 | 409 | 114 |
| node (default) | idiomatic | 407 | 669 | 88.2 | 378 | 669 | 113 |
| perry (mark-sweep, no lazy) | idiomatic | 514 | 936 | 195.6 | 504 | 936 | 79 |

## JSON parse-and-iterate

Per iteration: parse → sum every record's nested.x (touches every element)
→ stringify. The full-tree iteration FORCES Perry's lazy tape to
materialize, so this is the honest comparison for workloads that touch
JSON content. 10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| bun (default) | idiomatic | 198 | 206 | 3.1 | 198 | 206 | 104 |
| rust serde_json (LTO+1cgu) | optimized | 212 | 217 | 1.4 | 212 | 217 | 9 |
| rust serde_json | idiomatic | 227 | 227 | 0.5 | 226 | 227 | 9 |
| node --max-old=4096 | optimized | 380 | 383 | 1.2 | 379 | 383 | 114 |
| node (default) | idiomatic | 382 | 385 | 2.4 | 377 | 385 | 115 |
| perry (mark-sweep, no lazy) | idiomatic | 513 | 521 | 3.1 | 510 | 521 | 79 |
| perry (gen-gc + lazy tape) | optimized | 758 | 1013 | 175.4 | 548 | 1013 | 396 |
