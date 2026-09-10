# Main full-scan diagnosis (R17)

**The lazy route pays for tape construction and later reparsing on full scans.** On unchanged main, forcing direct parsing reduces measured CPU by 55.1% for the 16 KiB scan, 28.8% for 1 MiB and 30.0% for 8 MiB. Direct parsing beats both Node and Bun on those measured scan rows. It also makes parse-only and sparse access 47–126% slower, with much higher RSS for the 8 MiB parse/sparse rows. Globally disabling lazy parsing would trade away existing wins.

This branch contains diagnostic evidence only. Source is main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531); no production runtime, collector policy, threshold or routing change was made. These results describe that pinned main, not a later release. Earlier accepted JSON work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).

## Matched CPU and RSS

Same M1/8 GiB host; Node 26.5.1 and Bun 1.3.14. Ten predeclared cases, seven interleaved fresh-process repetitions per case/engine, twice the original full-matrix iteration counts and unchanged warmup. CPU is median user+system microseconds per operation; RSS is median process peak RSS in MiB, including startup. All 280 timed checksums and 50 complete-output verifications pass. The two Perry modes execute **the exact same worker binary**; `main_direct` sets `PERRY_JSON_TAPE=0`, while `main_auto` unsets it. Node/Bun use their ordinary modes.

The 20 MiB scan is a same-route control: it exceeds the existing automatic lazy admission bound. Route selection changes construction and lifetime together; the difference is not an isolated parser-phase timer or a production speedup.

| Array / operation | Iterations | Auto CPU µs/op | Direct CPU µs/op | Node CPU µs/op | Bun CPU µs/op | Direct vs auto |
|---|---:|---:|---:|---:|---:|---:|
| 16k parse | 22568 | 13.507 | 29.588 | 40.367 | 33.787 | +119.06% |
| 16k sparse | 15736 | 20.175 | 29.656 | 40.674 | 33.830 | +47.00% |
| 16k scan | 7522 | 68.741 | 30.843 | 39.157 | 34.813 | -55.13% |
| 1m parse | 340 | 911.176 | 1868.962 | 2706.556 | 2143.018 | +105.12% |
| 1m sparse | 322 | 966.003 | 1869.301 | 2741.904 | 2174.137 | +93.51% |
| 1m scan | 138 | 2932.094 | 2088.159 | 2748.007 | 2233.478 | -28.78% |
| 8m parse | 32 | 8294.781 | 18745.812 | 31940.812 | 21069.875 | +126.00% |
| 8m sparse | 30 | 8815.667 | 18987.933 | 32034.100 | 21231.733 | +115.39% |
| 8m scan | 16 | 23703.188 | 16594.875 | 31251.000 | 21550.500 | -29.99% |
| 20m scan | 8 | 41573.375 | 41544.875 | 85140.500 | 54149.625 | -0.07% |

| Array / operation | Auto peak MiB | Direct peak MiB | Node peak MiB | Bun peak MiB |
|---|---:|---:|---:|---:|
| 16k parse | 63.438 | 32.359 | 65.906 | 73.406 |
| 16k sparse | 73.719 | 32.359 | 66.047 | 71.109 |
| 16k scan | 505.000 | 31.922 | 66.031 | 77.547 |
| 1m parse | 67.094 | 71.141 | 125.672 | 111.031 |
| 1m sparse | 67.797 | 71.172 | 125.656 | 126.781 |
| 1m scan | 300.625 | 70.984 | 130.391 | 93.219 |
| 8m parse | 108.812 | 495.547 | 379.422 | 141.359 |
| 8m sparse | 109.172 | 470.844 | 365.312 | 186.594 |
| 8m scan | 309.406 | 262.750 | 248.578 | 174.859 |
| 20m scan | 393.172 | 393.172 | 428.969 | 237.062 |

[All samples and comparisons](results/scan-profile-r17-validation/modes-analysis.json), [raw timings](results/quiet-scan-profile-r17-modes/timing.jsonl), [output hashes](results/quiet-scan-profile-r17-modes/verify.jsonl), [predeclared cases](results/scan-profile-r17-validation/mode-cases.json).

## Actual sampled stacks

Four instrumented 1 MiB profiles use `/usr/bin/sample` against the unchanged main worker. Each has over 750 main-thread samples, successful worker/sampler exits and complete output matching Node. Counts target about 1.4 seconds of CPU from the mode medians: parse auto/direct 1,537/750; scan auto/direct 478/671. These are short sampled inclusive stack shares, not exact phase timings or confidence intervals.

| Profile | Main-thread samples | Tape build | Full lazy materialization | Direct array parser | Collection |
|---|---:|---:|---:|---:|---:|
| parse main_auto | 759 | 82.48% | 0.00% | 0.00% | 8.17% |
| parse main_direct | 758 | 0.00% | 0.00% | 80.87% | 4.62% |
| scan main_auto | 753 | 25.90% | 51.39% | 51.26% | 11.82% |
| scan main_direct | 755 | 0.00% | 0.00% | 76.95% | 6.89% |

**The materialization and direct-parser columns overlap and must not be added.** In the auto scan, the direct parser is nested inside full materialization. Tape building plus materialization account for about 77% of the samples, consistent with the source path that validates a tape and then reparses the blob. The matched timings establish the route tradeoff; the samples locate the work.

Profile RSS is instrumented and counts differ between modes, so it is not used as a matched memory comparison. The separate matched 1 MiB scan above is 300.625 MiB auto versus 70.984 MiB direct.

[Profile analysis](results/scan-profile-r17-validation/profiles-analysis.json), [profile records](results/quiet-scan-profile-r17-profiles/profiles.json), [predeclared profile counts](results/scan-profile-r17-validation/profile-cases.json).

## Object lifetime and resident memory

Nine further probes retain the final output, explicitly collect, verify the live output again, then clear `last`, `input` and `retained` and collect again. Both explicit collections occur **after** measured work. Every output matches Node before and after the live collection; all 18 manual full collections ran and reclaimed bytes. The original harness's result RSS is before output verification; `MEMORY_LIVE` is after it, so the latter may include output-string allocation. The independent RSS monitor is approximate and is not `ru_maxrss`. These instrumented probes are not CPU speedup measurements.

All memory columns below are MiB. “First full freed” is collector-reported reclaimed bytes, not an RSS decrease. “Final arena live” is managed arena accounting, not total process memory.

| Probe | Iterations | RSS after work | RSS after verification | RSS after live GC | RSS after dropped GC | First full freed | Final arena live |
|---|---:|---:|---:|---:|---:|---:|---:|
| 16k scan main_auto | 7522 | 504.72 | 504.98 | 570.66 | 570.70 | 174.95 | 0.42 |
| 16k scan main_auto | 15044 | 991.34 | 991.59 | 1116.73 | 1117.50 | 349.81 | 0.42 |
| 16k scan main_direct | 7522 | 31.78 | 32.00 | 33.77 | 33.80 | 9.93 | 0.28 |
| 1m scan main_auto | 138 | 299.69 | 304.84 | 377.47 | 377.47 | 222.39 | 1.26 |
| 1m scan main_auto | 478 | 953.50 | 956.06 | 1200.36 | 1200.36 | 764.16 | 1.26 |
| 1m scan main_direct | 138 | 70.30 | 74.25 | 77.84 | 80.02 | 18.99 | 1.11 |
| 1m scan main_direct | 478 | 70.33 | 74.28 | 76.19 | 77.97 | 6.89 | 1.11 |
| 1m parse main_auto | 340 | 66.83 | 68.62 | 68.73 | 68.78 | 2.34 | 1.08 |
| 1m parse main_direct | 340 | 71.61 | 76.31 | 77.48 | 79.59 | 0.84 | 1.11 |

The native tape is already released deterministically by `install_materialized` → `release_tape_after_materialize` → `json_tape_store::release`; its allocation drops and external-byte accounting decreases. RSS growth alone therefore does not establish a tape leak.

There is evidence of excess survival: the 1 MiB/138 auto-scan's first copying minor attributes about 12.1 MiB of promoted strings, objects and arrays to `remembered_set/lazy_array`. Lazy headers stay old and immovable, and their materialized/cache edges can keep young graphs alive until full collection. Later cycles include untraced in-place promotion; the scan has 65 copying-minor reports and no full collection before the two manual ones. The first manual full reclaims 222.39 MiB. At exit managed arena live bytes are 1.26 MiB, yet the post-drop RSS snapshot is 377.47 MiB. Reclamation happened; resident-memory retention remains a separate issue. This does not identify exactly which native allocator or runtime capacity holds every remaining page.

The longer auto probes grow substantially, while the matched direct scans remain near 32 MiB (16 KiB input) and 70 MiB (1 MiB input) before verification. The parse-only auto control instead runs 16 automatic `OldGenBytes` full collections plus the two manual ones, and zero copying minors. The analyzer preserves every cycle and selects the two Manual events explicitly. Its initial mistaken assumption that every case had only two full summaries, and the correction, are recorded; no collection or failure is hidden.

[Verified lifetime results](results/scan-profile-r17-validation/memory-analysis.json), [probe source](results/scan-profile-r17-validation/memory-worker.ts), [all probe records](results/quiet-scan-profile-r17-retry1-memory/memory-diagnostics.json), [analysis development notes](results/scan-profile-r17-validation/analysis-development-notes.json).

## Windows, provenance and validation limits

All times are UTC on 2026-09-10. Each window had the exclusive benchmark lock and was archived as the first remote operation after termination.

| Window | Start–finish | Load before → after | Verdict |
|---|---|---|---|
| Modes | 21:22:37–21:25:19 | 1.218 → 2.115 | Pass |
| Samples | 21:34:40–21:34:46 | 2.346 → 2.446 | Pass |
| Initial memory | 21:47:50–21:48:02 | 2.407 → 2.504 | **Excluded**, over the unchanged 2.5 limit |
| Identical memory retry | 21:51:00–21:51:11 | 1.738 → 2.060 | Pass |

The initial memory outputs were archived and excluded before examining RSS to select the retry. Counts, input hashes, worker and runtime were unchanged. [Excluded window](results/quiet-scan-profile-r17-memory/INVALID_WINDOW.md). Raw logs and outputs are losslessly compressed with per-window hash manifests; failed-window evidence remains available.

The compiler and both static archives come from the original fresh main build using `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`. It took 338.89 seconds, with all three mtimes after build start. R17 copied the frozen artifacts by verified hashes and recompiled its workers. Its cross-directory object-byte equality check initially failed before remote staging: generated objects embed different absolute source paths. Source hashes and all 114 defined worker symbols agree; cross-path object-byte equality is not claimed. Both timed Perry modes still share one identical R17 executable.

All 16 mode inputs, 18 profile inputs and 21 memory inputs were verified against staged hashes. The diagnostic worker's local before/after-GC smoke agrees with Node but is not performance evidence. [Fresh main build](results/scan-profile-r17-validation/main-build-provenance.json), [reused artifacts](results/scan-profile-r17-validation/reference-main.json), [worker provenance](results/scan-profile-r17-validation/main-workers-provenance.json), [memory-worker provenance](results/scan-profile-r17-validation/memory-worker-provenance.json), [path difference](results/scan-profile-r17-validation/cross-path-object-review.json).

R17 changes no runtime source and does not rerun the full behavioral qualification. The same frozen main's prior Node fixtures, stringify options and actual moving/protected-GC checks are linked in the [validation reference](results/scan-profile-r17-validation/validation-reference.json). Prior native static findings remain unsuppressed; this diagnosis is not a new all-clean static or GC-safety verdict. Existing lazy stringify crashes/noncanonical output and fractional-spacing differences remain baseline gaps, not conformance passes. Script lint passes 73/74 checks with the existing public-benchmark freshness failure; the file cap passes. Results are preserved in [lint provenance](results/scan-profile-r17-validation/script-lint-source.json) and [lint output](results/scan-profile-r17-validation/script-lint.log.gz). Compile and two CI-only gates were explicitly skipped, so full CI is not claimed.

## Next implementation

Prototype full-array construction from the validated tape, using the existing collection-suppressed construction batch and ordinary final objects. Preserve cached element identity and mutations, duplicate-key behavior, escaping/numbers and active incremental/full-GC fallbacks. The first experiment should change only the existing full-materialization producer; leave lazy admission, sparse access and GC scheduling intact. Verify that it removes duplicate parsing in actual profiles, then measure both parse and stringify, consumption, rotating inputs, retained results, short calls and options against the same main before considering a PR.

This CPU experiment cannot by itself promise lower lifetime-related RSS. The old-header retention and post-reclamation resident memory need separate evidence and a bounded fix, without hiding collection cost outside the reported workload. No runtime PR or release bump is created for this diagnostic branch, and the original no-regression/all-rows objective remains open.

[Diagnostic archive manifest](results/scan-profile-r17-validation/manifest.json).
