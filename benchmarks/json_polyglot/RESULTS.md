# JSON Polyglot Benchmark Results

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.5.0 arm64 on MacBookPro.
**Date:** 2026-08-03.

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
| rust serde_json (LTO+1cgu) | optimized | 184 | 193 | 2.6 | 184 | 193 | 10 |
| rust serde_json | idiomatic | 191 | 201 | 2.9 | 190 | 201 | 11 |
| perry (gen-gc + lazy tape) | optimized | 213 | 221 | 2.3 | 212 | 221 | 110 |
| bun (default) | idiomatic | 223 | 225 | 1.1 | 221 | 225 | 83 |
| node (default) | idiomatic | 387 | 397 | 4.5 | 382 | 397 | 103 |
| node --max-old=4096 | optimized | 388 | 395 | 3.9 | 384 | 395 | 103 |
| kotlin (kotlinx.serialization) | idiomatic | 455 | 474 | 7.9 | 446 | 474 | 609 |
| kotlin -server -Xmx512m | optimized | 461 | 495 | 14.9 | 439 | 495 | 422 |
| perry (mark-sweep, no lazy) | idiomatic | 608 | 616 | 5.3 | 599 | 616 | 301 |
| go (encoding/json) | idiomatic | 771 | 777 | 2.6 | 768 | 777 | 23 |
| c++ -O3 -flto (nlohmann/json) | optimized | 773 | 777 | 4.1 | 766 | 777 | 25 |
| go -ldflags="-s -w" -trimpath | optimized | 778 | 784 | 3.7 | 770 | 784 | 22 |
| c++ -O2 (nlohmann/json) | idiomatic | 834 | 848 | 4.4 | 831 | 848 | 25 |
| swift -O -wmo (Foundation) | optimized | 3521 | 3617 | 31.8 | 3502 | 3617 | 33 |
| swift -O (Foundation) | idiomatic | 3570 | 3588 | 9.8 | 3557 | 3588 | 32 |

## JSON parse-and-iterate

Per iteration: parse → sum every record's nested.x (touches every element)
→ stringify. The full-tree iteration FORCES Perry's lazy tape to
materialize, so this is the honest comparison for workloads that touch
JSON content. 10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| rust serde_json (LTO+1cgu) | optimized | 181 | 190 | 2.6 | 180 | 190 | 11 |
| rust serde_json | idiomatic | 191 | 198 | 2.2 | 190 | 198 | 11 |
| bun (default) | idiomatic | 225 | 230 | 1.7 | 224 | 230 | 88 |
| node --max-old=4096 | optimized | 389 | 393 | 2.2 | 386 | 393 | 98 |
| node (default) | idiomatic | 390 | 398 | 3.4 | 386 | 398 | 97 |
| kotlin -server -Xmx512m | optimized | 445 | 453 | 8.1 | 425 | 453 | 423 |
| kotlin (kotlinx.serialization) | idiomatic | 473 | 488 | 11.3 | 457 | 488 | 607 |
| perry (mark-sweep, no lazy) | idiomatic | 660 | 669 | 4.8 | 651 | 669 | 301 |
| go -ldflags="-s -w" -trimpath | optimized | 775 | 779 | 2.5 | 769 | 779 | 23 |
| go (encoding/json) | idiomatic | 775 | 782 | 3.8 | 771 | 782 | 23 |
| c++ -O3 -flto (nlohmann/json) | optimized | 788 | 790 | 1.4 | 786 | 790 | 25 |
| c++ -O2 (nlohmann/json) | idiomatic | 864 | 866 | 1.9 | 860 | 866 | 25 |
| perry (gen-gc + lazy tape) | optimized | 2653 | 2920 | 78.4 | 2630 | 2920 | 356 |
| swift -O (Foundation) | idiomatic | 3566 | 3641 | 38.7 | 3518 | 3641 | 33 |
| swift -O -wmo (Foundation) | optimized | 3623 | 3709 | 35.2 | 3593 | 3709 | 34 |
