Latest owner decision and rebased final checks: [final pre-PR verification](statepoints-only-12023-final.md). The performance-blocked assessment below is historical.

# #12023 acceptance checks on main 77071e3d9

Acceptance is still blocked by performance. No production code or rooting semantics changed during this acceptance pass. Measured source: main `77071e3d9f589605ce44511265366e2db9cbb68f` versus retained head `898a8d122c55d4447ec3e63d207f22650a9d3d3f`. Both arms were rebuilt with explicit compiler/runtime/workspace paths. Newer main worker-entry commits arrived after this snapshot; these receipts remain pinned to the explicitly requested base.

## Correctness and regression detection

| Check | Main | Head |
|---|---:|---:|
| Serial runtime unit tests | 5,151 passed, 0 failed, 5 ignored | 5,155 passed, 0 failed, 5 ignored |
| Full serial codegen suite | pass, 45 result summaries | pass, 45 result summaries |
| fmt and eight static GC gates | pass | pass |
| Instrumented GC matrix | 88 fixtures ×4 modes | 88 fixtures ×4 modes |

All 704 GC runs have identical stdout, exit status and verifier results between arms; all requested instrumentation is present. The two existing stream failures match in all four modes: `test_gap_gc_11828_stream_readable_from_pipe.ts` and `test_gap_gc_fetch_streaming_chunks.ts`.

The earlier 5,143/5,147 figures represented passing counts, not four failures. Head adds four passing tests: guarded sort-slice boundaries, native argument-cell rewriting, malformed native ranges failing closed, and native-range walking without materialized slots. Two existing shadow-stack test names were renamed to frame-root names.

Fresh native dominance: 7,745 functions /300 modules, 72,830 safepoints, 54,343 nonempty live bundles, 269,416 relocates, zero unrooted or stale hazards. All 40 planted violations are caught.

Fresh q25/q50/q100/q200 function sizes: 18,122 /36,362 /72,862 /138,767 bytes. The gate passes. Disabling native homes while retaining statepoints produces q200 1,780,525 bytes; the size and growth assertions fail (exit 1). The sabotage worktree is excluded from delivery.

## Locked x86 measurements

Every batch holds `/root/MEASURE.lock`, writes `cc/sp <UTC>` to the holder, runs on CPUs 56–63 with ASLR disabled, and obeys the 30/45-minute limit. Build/test jobs use CPUs 0–55 and eight Cargo jobs. Our own jobs additionally avoid SMT siblings 24–31.

Initial CPU58 measurements were contaminated by foreign compiler threads on reserved CPUs and their siblings. Fifteen identical-executable controls span −14.68% to +9.19% cycles. These runs are retained but are not used as the quiet acceptance comparison.

The final comparison pins both arms to CPU59. A pair is accepted only when CPU27, its SMT sibling, is at most 1% busy during each perf run. Each workload has 15 accepted pairs per THP mode; rejected pairs are preserved. Supported counters are cycles, instructions and L1 data-cache load misses. LLC misses and backend stalls are explicitly unsupported.

| Workload | Cycles Δ, THP off/on | Instructions Δ, THP off/on |
|---|---:|---:|
| commander | −2.35% /−1.99% | −0.287% /−0.291% |
| qs | −0.171% /−0.125% | −0.291% /−0.293% |
| tsc | **+0.773% /+0.623%** | −0.047% /−0.060% |
| Zod | −1.09% /−0.780% | −0.056% /−0.062% |
| hello | −5.70% /−7.70% | −0.023% /−0.025% |

Tsc IPC is 2.5397→2.5160 off and 2.5863→2.5689 on. L1 load misses increase 3.05%/3.70%. Paired mean cycles increase 0.538% off (bootstrap 95% CI −0.010%..1.128%) and 0.809% on (0.139%..1.624%). This is an open acceptance item.

Fifteen additional interleaved tsc profile pairs on the same core confirm a small aggregate cycle increase. The largest upward self-sample movers are runtime field lookup and allocation helpers. `js_object_get_field_ic_front` retains 406 instructions and three stack references; `js_array_length` retains 605 instructions and 17 stack references. Differences in the inspected bodies are addresses, RIP-relative displacements and Rust symbol identifiers. This does not establish a causal layout explanation or exclude caller stack-home traffic. No speculative fix is retained.

Tsc GC diagnostics: three minors and zero full collections on both arms in both THP modes.

## Quiet arm64 comparison and layout experiment

Both SDKs were built sequentially on the build Mac. Each arm's four binaries were preserved and its target deleted before the next arm. All outputs match Node. The mini performs measurement only.

Fifteen counterbalanced pairs per workload, all accepted load samples below 1.81:

| Workload | Median CPU seconds, main→head | Median maximum RSS MiB, main→head |
|---|---:|---:|
| qs | 1.44→1.43 | 23.125→22.750 |
| tsc | 0.74→0.74 | 161.734→152.000 |
| commander | 0.52→0.51 | **25.234→25.484** |
| Zod | 0.04→0.04 | **21.375→21.578** |

Tsc paired mean CPU improves 0.533%. Its diagnostics show three minors and zero fulls.

Commander was relinked from the same objects and libraries with seven header-padding offsets. Text starts shift by 128–8,192 bytes, while each arm retains identical text size. Sixteen binaries pass output/forced-evacuation verification; all five compared hot functions retain their instruction counts, stack references and frame prologues within each arm. Padding is a benchmark experiment, not a production change.

Fifteen quiet extended pairs per layout:

| Padding bytes | Median CPU main→head | CPU Δ |
|---:|---:|---:|
| 0 | 4.89→4.84 | −1.02% |
| 128 | 4.88→4.84 | −0.82% |
| 256 | 4.87→4.86 | −0.21% |
| 512 | 4.88→4.87 | −0.20% |
| 1,024 | 4.86→4.86 | 0.00% |
| 2,048 | 4.84→4.85 | +0.21% |
| 4,096 | 4.84→4.86 | +0.41% |
| 8,192 | 4.84→4.90 | +1.24% |

The default paired CPU CI is entirely negative and the 4/8 KiB CIs entirely positive. This proves commander CPU's delta depends on layout; the earlier +1.22% can be reproduced without changing lowering or frame traffic. `parseOptions` itself shrinks 47,028→39,372 bytes, frame 576→560 bytes, stack-memory instructions 1,603→613.

Commander RSS remains open. Another short-run sweep has positive RSS medians across all eight layouts. A 100-pair default comparison gives CPU 0.52→0.51, RSS medians 25.234→25.500 MiB; the paired mean RSS increase is 86.4 KiB (95% CI 45.92..125.76). The distribution has approximately 256 KiB clusters. Separate THP comparisons give 25,872→26,144 KiB off and 25,872→25,872 KiB on, showing that fifteen-pair medians are unstable. Near-peak snapshots locate varying residency in mimalloc's tag-240 mapping; executable text remains 7,776 KiB and stack 48 KiB. Short-run GC statistics match: 29 minors, zero fulls, live arena 497,352 bytes, capacity 8,388,608 bytes. This is not claimed as an accepted RSS result or a proven retained-heap regression.

Zod's +208 KiB RSS is consistent across the fifteen pairs (paired mean +203.73 KiB). Snapshots show resident executable text 7,392→7,568–7,584 KiB, stack/allocator metadata unchanged. A sequential Zod layout rebuild was started but the required disk guard stopped it at 7.994 GiB free. Completed binaries were archived and owned local copies removed; the stopped target was deleted. Free space subsequently continued below the floor, with no lane build running. Its layout experiment and any consequent fix remain blocked on Mac disk space.

## Delivery and open work

No shadow-frame fallback, GC-gate relaxation, production padding, registry, or name-based exception was introduced. Rooting semantics are unchanged since the previous review.

The remaining items are tsc x86 cycles, commander RSS, and Zod executable-page RSS. A READY verdict is not justified. Resume the Zod experiment when the Mac has enough space above the 8 GiB floor, and resolve tsc with caller profiling / controlled layout or a measured generic lowering improvement.

Raw scripts, binary archives and receipts are preserved under `/root/claude-lanes/sp-work/accept*`. The companion JSON contains measured source identities and tables.
