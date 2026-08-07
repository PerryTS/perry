# JSON Polyglot Benchmark Results

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.5.0 arm64 on MacBookPro.
**Date:** 2026-08-07.

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
| rust serde_json (LTO+1cgu) | optimized | 187 | 192 | 2.3 | 183 | 192 | 11 |
| rust serde_json | idiomatic | 199 | 199 | 1.9 | 194 | 199 | 11 |
| perry (gen-gc + lazy tape) | optimized | 201 | 204 | 4.9 | 190 | 204 | 89 |
| bun (default) | idiomatic | 237 | 241 | 3.8 | 227 | 241 | 83 |
| node --max-old=4096 | optimized | 415 | 431 | 7.9 | 402 | 431 | 106 |
| node (default) | idiomatic | 417 | 424 | 5.7 | 406 | 424 | 104 |
| kotlin -server -Xmx512m | optimized | 463 | 473 | 12.8 | 429 | 473 | 424 |
| kotlin (kotlinx.serialization) | idiomatic | 473 | 490 | 12.0 | 444 | 490 | 607 |
| c++ -O3 -flto (nlohmann/json) | optimized | 790 | 795 | 3.4 | 785 | 795 | 25 |
| go (encoding/json) | idiomatic | 804 | 809 | 4.1 | 796 | 809 | 22 |
| go -ldflags="-s -w" -trimpath | optimized | 808 | 814 | 4.1 | 799 | 814 | 22 |
| c++ -O2 (nlohmann/json) | idiomatic | 857 | 870 | 4.5 | 854 | 870 | 25 |
| perry (mark-sweep, no lazy) | idiomatic | 1294 | 1299 | 5.8 | 1277 | 1299 | 59 |
| swift -O -wmo (Foundation) | optimized | 3672 | 3708 | 13.3 | 3657 | 3708 | 34 |
| swift -O (Foundation) | idiomatic | 3711 | 3734 | 10.2 | 3702 | 3734 | 34 |

## JSON parse-and-iterate

Per iteration: parse → sum every record's nested.x (touches every element)
→ stringify. The full-tree iteration FORCES Perry's lazy tape to
materialize, so this is the honest comparison for workloads that touch
JSON content. 10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| rust serde_json (LTO+1cgu) | optimized | 189 | 192 | 2.2 | 184 | 192 | 11 |
| rust serde_json | idiomatic | 199 | 200 | 2.1 | 193 | 200 | 11 |
| bun (default) | idiomatic | 239 | 253 | 6.0 | 231 | 253 | 88 |
| node --max-old=4096 | optimized | 417 | 423 | 5.7 | 404 | 423 | 101 |
| node (default) | idiomatic | 417 | 425 | 10.0 | 398 | 425 | 101 |
| kotlin -server -Xmx512m | optimized | 469 | 487 | 8.3 | 457 | 487 | 425 |
| kotlin (kotlinx.serialization) | idiomatic | 480 | 497 | 12.3 | 453 | 497 | 599 |
| c++ -O3 -flto (nlohmann/json) | optimized | 807 | 813 | 3.1 | 803 | 813 | 25 |
| go (encoding/json) | idiomatic | 807 | 813 | 3.1 | 803 | 813 | 23 |
| go -ldflags="-s -w" -trimpath | optimized | 808 | 817 | 4.1 | 803 | 817 | 24 |
| c++ -O2 (nlohmann/json) | idiomatic | 885 | 889 | 2.3 | 882 | 889 | 25 |
| perry (mark-sweep, no lazy) | idiomatic | 1339 | 1346 | 5.3 | 1328 | 1346 | 59 |
| perry (gen-gc + lazy tape) | optimized | 2327 | 2437 | 164.5 | 1992 | 2437 | 167 |
| swift -O (Foundation) | idiomatic | 3664 | 3698 | 11.8 | 3658 | 3698 | 34 |
| swift -O -wmo (Foundation) | optimized | 3726 | 3761 | 12.2 | 3712 | 3761 | 34 |
