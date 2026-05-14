# JSON Polyglot Benchmark Results

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.4.0 arm64 on MacBook-Pro-69.
**Date:** 2026-05-14.

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
| c++ -O3 -flto (simdjson) | optimized | 24 | 26 | 0.6 | 24 | 26 | 8 |
| c++ -O2 (simdjson) | idiomatic | 29 | 34 | 1.4 | 29 | 34 | 8 |
| perry (gen-gc + lazy tape) | optimized | 83 | 86 | 1.4 | 81 | 86 | 227 |
| rust serde_json (LTO+1cgu) | optimized | 186 | 190 | 1.4 | 185 | 190 | 11 |
| rust serde_json | idiomatic | 197 | 201 | 1.7 | 195 | 201 | 11 |
| bun (default) | idiomatic | 249 | 252 | 1.3 | 247 | 252 | 81 |
| perry (mark-sweep, no lazy) | idiomatic | 335 | 339 | 1.7 | 333 | 339 | 283 |
| node (default) | idiomatic | 377 | 386 | 4.5 | 370 | 386 | 127 |
| node --max-old=4096 | optimized | 380 | 386 | 4.0 | 373 | 386 | 127 |
| kotlin -server -Xmx512m | optimized | 457 | 470 | 5.3 | 451 | 470 | 424 |
| kotlin (kotlinx.serialization) | idiomatic | 476 | 495 | 8.0 | 467 | 495 | 606 |
| c++ -O3 -flto (nlohmann/json) | optimized | 783 | 785 | 1.8 | 780 | 785 | 25 |
| go -ldflags="-s -w" -trimpath | optimized | 796 | 802 | 3.8 | 788 | 802 | 23 |
| go (encoding/json) | idiomatic | 797 | 829 | 9.9 | 792 | 829 | 23 |
| c++ -O2 (nlohmann/json) | idiomatic | 849 | 851 | 1.1 | 848 | 851 | 25 |
| swift -O -wmo (Foundation) | optimized | 3771 | 3834 | 30.9 | 3698 | 3834 | 34 |
| swift -O (Foundation) | idiomatic | 3783 | 3819 | 18.4 | 3750 | 3819 | 34 |

## JSON parse-and-iterate

Per iteration: parse → sum every record's nested.x (touches every element)
→ stringify. The full-tree iteration FORCES Perry's lazy tape to
materialize, so this is the honest comparison for workloads that touch
JSON content. 10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| c++ -O2 (simdjson) | idiomatic | 24 | 25 | 0.5 | 24 | 25 | 8 |
| c++ -O3 -flto (simdjson) | optimized | 24 | 25 | 0.3 | 24 | 25 | 8 |
| rust serde_json (LTO+1cgu) | optimized | 182 | 184 | 0.9 | 181 | 184 | 11 |
| rust serde_json | idiomatic | 197 | 203 | 1.8 | 196 | 203 | 11 |
| bun (default) | idiomatic | 251 | 254 | 1.2 | 250 | 254 | 86 |
| perry (mark-sweep, no lazy) | idiomatic | 338 | 366 | 8.3 | 336 | 366 | 283 |
| node (default) | idiomatic | 351 | 357 | 2.9 | 346 | 357 | 87 |
| node --max-old=4096 | optimized | 352 | 360 | 5.4 | 343 | 360 | 87 |
| perry (gen-gc + lazy tape) | optimized | 425 | 428 | 2.1 | 421 | 428 | 309 |
| kotlin -server -Xmx512m | optimized | 462 | 527 | 20.4 | 449 | 527 | 424 |
| kotlin (kotlinx.serialization) | idiomatic | 476 | 485 | 3.7 | 473 | 485 | 606 |
| c++ -O3 -flto (nlohmann/json) | optimized | 797 | 828 | 9.2 | 795 | 828 | 25 |
| go -ldflags="-s -w" -trimpath | optimized | 798 | 842 | 13.0 | 794 | 842 | 23 |
| go (encoding/json) | idiomatic | 799 | 805 | 3.1 | 795 | 805 | 23 |
| c++ -O2 (nlohmann/json) | idiomatic | 877 | 882 | 2.6 | 873 | 882 | 25 |
| swift -O (Foundation) | idiomatic | 3742 | 3791 | 18.9 | 3721 | 3791 | 34 |
| swift -O -wmo (Foundation) | optimized | 3758 | 3793 | 23.9 | 3713 | 3793 | 34 |
