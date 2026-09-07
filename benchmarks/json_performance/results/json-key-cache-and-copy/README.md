# Wide-key lookup and short scalar copies

The wide-object key-cache change reduces wide parse CPU by **24.04%** versus fused35. Adding bounded short-piece copies reduces small-record stringify by **13.27%** versus wide39, or **11.22%** versus fused35. The combined small-record workload including final collection improves **7.33–8.72%** versus fused35. Neither candidate is promoted over depth22: other parse CPU rows and RSS still regress.

Wide39 source: `c474737bb7c506e079b4939d8fda4579d60b49cf`, branch `codex/json-wide-key-cache-1520`. Copy41 source: `2bb79a1734c0b14db0d26f0fd4206ae5d1ebe92c`, branch `codex/json-short-piece-copy-1520`, based on wide39. Version stays 0.5.1520.

Wide39 uses the existing owning key hash table directly once an object reaches 128 distinct keys, matching the existing duplicate-key indexing threshold. This avoids repeated misses and rotation in the 16-entry linear mirror. The small-key function, owning table, cache storage and root scanner remain unchanged. No hash algorithm or GC policy changes. The existing duplicate-key test covers 127/128/129/130 keys, escaped duplicate names, first-position ordering and last-value semantics.

Copy41 copies short scalar text and unescaped strings with bounded beginning/end loads and stores for lengths through 32 bytes. Payloads above 32 bytes retain the platform bulk copy. No input padding is read or output padding written, and no managed allocation or root boundary changes. Tests cover lengths 0–64, every source/output alignment modulo 16, output sentinels and both sides of protected pages.

[All 38 CPU/RSS rows](launch42/results/recheck/all-38.md), [observed ranges](launch42/results/recheck/summary.json), [retained memory](launch42/results/recheck/memory-summary.json), and [complete lifetimes](launch42/results/lifetimes/summary.json).

| Workload | Fused35, µs | Wide39, µs | Copy41, µs | Copy41 vs Fused35 | Node, µs | Bun, µs |
|---|---:|---:|---:|---:|---:|---:|
| wide_1m parse | 14996.361111 | 11390.638889 | 11397.222222 | -24.000% | 4927.000000 | 4162.527778 |
| small_record stringify | 0.265119 | 0.271377 | 0.235379 | -11.218% | 0.108289 | 0.121179 |
| object_1k stringify | 0.228799 | 0.228403 | 0.222065 | -2.943% | 0.198586 | 0.215600 |
| records_object_1m parse | 2635.549296 | 2646.535211 | 2663.366197 | +1.055% | 2733.563380 | 2146.732394 |

Both new candidates remain at or below both comparison engines in **17/38 CPU rows** and **32/38 peak-RSS rows**. The combined wide parse is still 2.31× Node and 2.74× Bun; small-record stringify is 2.17× Node and 1.94× Bun. These are improvements toward the objective, not completion.

## Regressions kept in the decision

Wide39 versus Fused35: separated slower CPU ranges in empty_object parse (+1.423%), heterogeneous_1m parse (+0.947%), records_array_16k parse (+1.146%), records_array_1m parse (+1.172%), records_array_1m stringify (+0.999%), records_array_8m stringify (+1.157%), records_object_1m stringify (+1.019%), records_object_20m stringify (+0.575%), records_object_8m stringify (+0.906%), small_record stringify (+2.360%), tiny_object stringify (+0.228%).

Copy41 versus Wide39: separated slower CPU ranges in records_array_20m parse (+0.959%), records_object_1m parse (+0.636%), records_object_20m parse (+1.010%), records_object_8m parse (+0.710%), small_record parse (+0.425%), tiny_object parse (+0.372%).

Combined versus Fused35: separated slower CPU ranges in empty_object parse (+2.153%), heterogeneous_1m parse (+1.030%), records_array_16k parse (+0.935%), records_array_1m parse (+0.944%), records_array_20m parse (+1.157%), records_object_1m parse (+1.055%).

The combined build is +2.153% on empty-object parse versus fused35 in this window. Several array/object parse rows are roughly +1%. Wide39 also changes some stringify timings although its stringify source is unchanged; source-level isolation alone does not establish performance isolation. The native empty allocator remains 125 instructions with the same address modulo 64; the inline-object allocator differs between scalar31 and fused35 (741 versus 737 instructions) but is unchanged from fused35 to wide39. These observations do not prove a cause for the regressions.

Wide39 versus Fused35: main peak RSS -48 to +16 KiB.
Retained peak_rss, 40 groups: -16 to +16 KiB; 9 increases.
Retained rss_after, 40 groups: -48 to -16 KiB; 0 increases.

Copy41 versus Wide39: main peak RSS -16 to +80 KiB.
Retained peak_rss, 40 groups: -16 to +112 KiB; 37 increases.
Retained rss_after, 40 groups: -16 to +144 KiB; 30 increases.

Combined versus Fused35: main peak RSS -64 to +64 KiB.
Retained peak_rss, 40 groups: -32 to +112 KiB; 28 increases.
Retained rss_after, 40 groups: -48 to +112 KiB; 8 increases.

Combined versus Depth22: main peak RSS +16 to +816 KiB.
Retained peak_rss, 40 groups: +0 to +128 KiB; 39 increases.
Retained rss_after, 40 groups: -64 to +144 KiB; 20 increases.

No-regression acceptance remains false. In particular, the combined escaped-parse peak is +816 KiB versus depth22. Wide parse still exceeds Bun peak RSS by roughly 80 MiB. This patch reduces work inside JSON; it does not eliminate allocation and later tracing costs.

## Qualification and correctness

The first complete six-engine run, launch40, is preserved as **unqualified**: XProtect reached 7.8% CPU in one of 76 observations, exceeding the unchanged 5% limit. All 228 verification, 1140 timing, 480 retained-memory and 84 lifetime rows completed, but none are pooled into accepted results.
The seven-engine launch42 repeat includes both new candidates and all immediate controls. It completes 266 output checks, 1330 timed trials, 600 retained-memory trials and 105 lifetimes, with 90 clean observations and load 1.636→1.762. No competing workers or XProtect observations above 5% occur. Uploads and hash verification finish before measurement begins.

Wide39 passes 3290 runtime tests; copy41 passes 3292 (four ignored each). Each passes all 40 compiled Node comparisons and all 52 seeded moving-GC checks plus default/off controls. Every seeded case requires actual scheduled collections, copying minors, moved objects and loop polls. The inherited root-array prototype `toJSON` gap is reference-compared and excluded from Node parity. Node-version and root-holder inventories pass. Existing file-size and address-ratchet failures remain byte-for-byte unchanged. Matched shipping runtime/stdlib archives and source/worker hashes are in each arm’s provenance.

## What the profiles establish

The four separate profile43 samples and two separate GC traces are diagnostic only; their CPU/RSS is never pooled with the benchmark. Wide parsing now uses the direct hash helper; the repeated linear-ring comparisons disappear from that hot path. The copy41 wide sample records 1401/2070 main-call samples under the loop GC safepoint (~68%, approximate wall samples), after returning to the generated loop. The two 36-iteration wide traces each have six collections (one full) and **4,212,357 pointer-slot reads**. These are counts, not evidence that the work is avoidable or that GC may be disabled.

The next investigation should protect the new gains while resolving the common-object regressions. Move wide-only decisions out of the existing small-object path where possible, and compare generated code. For the larger gap, investigate allocation/graph ownership and root lifetimes using the fresh traces; small changes to key scanning alone cannot remove the remaining dominant tracing work.

The historical “147-case” inventory is **147 comparisons against 11 older builds over these same 38 workload rows**, not 147 distinct workloads. Its complete comparison requirements and earlier semantic gaps remain open; see historical-147-inventory.json.

Run `python3 benchmarks/json_performance/results/json-key-cache-and-copy/verify.py` to verify sources, profiles, correctness, rejected/qualified windows and summary medians.
