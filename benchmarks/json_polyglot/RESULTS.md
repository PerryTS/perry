# JSON Polyglot Benchmark Results

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.5.0 arm64 on MacBookPro.
**Date:** 2026-08-06.

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
| rust serde_json (LTO+1cgu) | optimized | 180 | 189 | 2.5 | 180 | 189 | 11 |
| rust serde_json | idiomatic | 190 | 199 | 2.5 | 190 | 199 | 10 |
| perry (gen-gc + lazy tape) | optimized | 194 | 197 | 1.5 | 192 | 197 | 87 |
| bun (default) | idiomatic | 221 | 235 | 5.1 | 219 | 235 | 84 |
| node (default) | idiomatic | 384 | 390 | 4.0 | 377 | 390 | 102 |
| node --max-old=4096 | optimized | 387 | 392 | 3.3 | 382 | 392 | 103 |
| kotlin -server -Xmx512m | optimized | 436 | 444 | 4.5 | 431 | 444 | 422 |
| kotlin (kotlinx.serialization) | idiomatic | 455 | 460 | 5.3 | 441 | 460 | 608 |
| c++ -O3 -flto (nlohmann/json) | optimized | 760 | 784 | 8.0 | 758 | 784 | 25 |
| go (encoding/json) | idiomatic | 764 | 776 | 4.4 | 758 | 776 | 23 |
| go -ldflags="-s -w" -trimpath | optimized | 765 | 768 | 2.9 | 758 | 768 | 23 |
| c++ -O2 (nlohmann/json) | idiomatic | 824 | 832 | 2.5 | 822 | 832 | 25 |
| perry (mark-sweep, no lazy) | idiomatic | 1287 | 1298 | 3.6 | 1285 | 1298 | 61 |
| swift -O -wmo (Foundation) | optimized | 3473 | 3484 | 9.1 | 3456 | 3484 | 31 |
| swift -O (Foundation) | idiomatic | 3528 | 3544 | 8.1 | 3519 | 3544 | 32 |

## JSON parse-and-iterate

Per iteration: parse → sum every record's nested.x (touches every element)
→ stringify. The full-tree iteration FORCES Perry's lazy tape to
materialize, so this is the honest comparison for workloads that touch
JSON content. 10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| rust serde_json (LTO+1cgu) | optimized | 184 | 192 | 2.2 | 184 | 192 | 11 |
| rust serde_json | idiomatic | 190 | 197 | 2.1 | 189 | 197 | 10 |
| bun (default) | idiomatic | 223 | 235 | 3.6 | 222 | 235 | 91 |
| node (default) | idiomatic | 384 | 395 | 4.7 | 378 | 395 | 96 |
| node --max-old=4096 | optimized | 397 | 399 | 4.7 | 383 | 399 | 98 |
| kotlin -server -Xmx512m | optimized | 437 | 446 | 4.1 | 434 | 446 | 423 |
| kotlin (kotlinx.serialization) | idiomatic | 451 | 466 | 5.5 | 446 | 466 | 607 |
| go -ldflags="-s -w" -trimpath | optimized | 767 | 919 | 43.9 | 762 | 919 | 23 |
| go (encoding/json) | idiomatic | 768 | 864 | 28.5 | 761 | 864 | 23 |
| c++ -O3 -flto (nlohmann/json) | optimized | 771 | 773 | 1.5 | 769 | 773 | 25 |
| c++ -O2 (nlohmann/json) | idiomatic | 849 | 851 | 1.5 | 846 | 851 | 25 |
| perry (mark-sweep, no lazy) | idiomatic | 1340 | 1346 | 2.6 | 1338 | 1346 | 58 |
| perry (gen-gc + lazy tape) | optimized | 2981 | 3391 | 169.1 | 2867 | 3391 | 218 |
| swift -O (Foundation) | idiomatic | 3481 | 3500 | 11.5 | 3464 | 3500 | 32 |
| swift -O -wmo (Foundation) | optimized | 3537 | 3555 | 11.0 | 3517 | 3555 | 30 |
