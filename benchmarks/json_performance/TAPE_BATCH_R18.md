# Batched tape record construction (R18)

**Rejected: the longer control recheck confirms a regression.** Full-scan CPU improves 4.36% at 1 MiB and 6.00% at 8 MiB, but an unrelated 1 MiB object parse slows 0.98%, with separated samples and all 11 paired repetitions slower. The 16 KiB scan is essentially flat, and RSS changes little. This does not meet the no-regression requirement; the prototype is preserved on `codex/json-tape-batch-r18`, without a runtime PR or release bump.

Measured source `0c20dd61f03d2024354edd50eb8ca1bbf67fa582`, directly based on main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). [R17's diagnosis](https://github.com/PerryTS/perry/blob/fb87e2b4ff7d6a052d8a549f7c0595c00cd70941/benchmarks/json_performance/SCAN_PROFILE_R17.md) motivates this experiment. Earlier accepted JSON work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).

## What changed

Only the existing full lazy-materialization producer changes. A validated tape supplies record and field boundaries for records with at most eight field pairs, containing scalars or flat scalar arrays. The producer reuses the direct parser's string/number decoders, shape cache, ordinary object constructors and collection-suppressed construction batch. Unsupported later subtrees use the direct parser at their source offsets; an unsupported first record declines before allocation. The completed array uses its known length. Existing publication patches cached values back over the new slots, preserving their identity and mutations.

There is no new cache, leaf-end metadata, GC policy, threshold, lazy-admission or sparse-access change. GC files, parse entry dispatch, string constructors and stringify implementation retain main's source. The change therefore still pays to reconstruct cached subtrees before the existing patch overwrites them, and still decodes strings and numbers from their source bytes.

## CPU screen: all 18 cases

Same M1/8 GiB host; Node 26.5.1, Bun 1.3.14. Eighteen cases were declared before timing. Large-case work is twice the original matrix counts; tiny/small counts and all warmups stay unchanged. Seven interleaved fresh-process repetitions per engine/case give 504 timed trials and 90 full-output verifications. All checksums, complete-output hashes, declared iteration/warmup counts and CPU/RSS sample vectors pass verification. Both arms use identical generated worker objects linked to their respective frozen runtimes.

CPU is median user+system microseconds per operation. “Slower pairs” compares candidate to main; “separated” means the two seven-sample ranges do not overlap, not a statistical confidence interval.

| Fixture / operation | Main µs/op | R18 µs/op | Node µs/op | Bun µs/op | R18 vs main | Slower pairs | Sample ranges |
|---|---:|---:|---:|---:|---:|---:|---|
| tiny_object / parse | 0.037 | 0.037 | 0.082 | 0.045 | -0.26% | 0/7 | Overlap |
| tiny_object / stringify | 0.034 | 0.034 | 0.037 | 0.043 | +0.15% | 6/7 | Overlap |
| small_record / parse | 0.099 | 0.099 | 0.338 | 0.244 | +0.00% | 4/7 | Overlap |
| small_record / stringify | 0.046 | 0.046 | 0.108 | 0.120 | -0.57% | 2/7 | Overlap |
| object_1k / parse | 0.082 | 0.082 | 0.533 | 0.229 | +0.03% | 3/7 | Overlap |
| records_array_1m / scan | 2930.674 | 2802.870 | 2886.493 | 2233.036 | -4.36% | 0/7 | Faster, separated |
| records_object_1m / parse | 1916.364 | 1935.586 | 2768.173 | 2141.531 | +1.00% | 7/7 | Slower, separated |
| records_object_8m / parse | 15883.400 | 15968.950 | 31887.100 | 21212.350 | +0.54% | 7/7 | Slower, separated |
| records_array_20m / parse | 40249.000 | 40431.875 | 87820.750 | 53745.375 | +0.45% | 7/7 | Slower, separated |
| records_array_20m / sparse | 40312.500 | 40444.250 | 89574.500 | 53115.750 | +0.33% | 5/7 | Overlap |
| records_array_20m / scan | 41264.375 | 41417.375 | 83864.000 | 54168.000 | +0.37% | 7/7 | Slower, separated |
| records_object_20m / parse | 40278.250 | 40468.875 | 89233.875 | 52948.625 | +0.47% | 7/7 | Slower, separated |
| records_array_16k / parse | 13.521 | 13.501 | 39.286 | 33.777 | -0.15% | 1/7 | Overlap |
| records_array_16k / sparse | 20.317 | 20.177 | 39.744 | 33.813 | -0.69% | 0/7 | Faster, separated |
| records_array_16k / scan | 68.580 | 68.465 | 40.388 | 34.768 | -0.17% | 1/7 | Overlap |
| records_array_1m / parse | 913.282 | 910.356 | 2657.753 | 2141.788 | -0.32% | 2/7 | Overlap |
| records_array_1m / sparse | 966.037 | 966.332 | 2793.379 | 2175.466 | +0.03% | 2/7 | Overlap |
| records_array_8m / scan | 23696.000 | 22274.250 | 30834.062 | 21791.812 | -6.00% | 0/7 | Faster, separated |

[Predeclared counts](results/tape-batch-r18-validation/screen-cases.json), [all samples and comparisons](results/tape-batch-r18-validation/screen-analysis.json), [raw timings](results/quiet-tape-batch-r18-screen-focus/timing.jsonl), [full-output hashes](results/quiet-tape-batch-r18-screen-focus/verify.jsonl).

## Peak resident memory

Median process peak RSS, in MiB, including startup. This is separate from the instrumented sampling run below.

| Fixture / operation | Main MiB | R18 MiB | Node MiB | Bun MiB | R18 − main MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / parse | 32.266 | 32.344 | 59.562 | 69.094 | +0.078 |
| tiny_object / stringify | 32.312 | 32.359 | 59.672 | 129.828 | +0.047 |
| small_record / parse | 79.531 | 79.625 | 59.641 | 79.906 | +0.094 |
| small_record / stringify | 33.328 | 33.359 | 59.734 | 378.703 | +0.031 |
| object_1k / parse | 64.938 | 65.000 | 61.859 | 71.312 | +0.062 |
| records_array_1m / scan | 300.625 | 300.578 | 130.391 | 93.203 | -0.047 |
| records_object_1m / parse | 71.141 | 71.203 | 125.594 | 88.734 | +0.062 |
| records_object_8m / parse | 312.156 | 312.234 | 266.719 | 141.438 | +0.078 |
| records_array_20m / parse | 393.156 | 393.234 | 402.500 | 232.094 | +0.078 |
| records_array_20m / sparse | 393.156 | 393.234 | 402.469 | 255.203 | +0.078 |
| records_array_20m / scan | 393.172 | 393.250 | 429.109 | 237.000 | +0.078 |
| records_object_20m / parse | 393.156 | 393.234 | 402.438 | 254.219 | +0.078 |
| records_array_16k / parse | 63.438 | 63.516 | 65.875 | 73.391 | +0.078 |
| records_array_16k / sparse | 74.781 | 73.766 | 66.047 | 71.125 | -1.016 |
| records_array_16k / scan | 505.000 | 505.094 | 66.078 | 77.609 | +0.094 |
| records_array_1m / parse | 67.094 | 67.188 | 125.672 | 110.953 | +0.094 |
| records_array_1m / sparse | 67.797 | 67.844 | 125.609 | 126.984 | +0.047 |
| records_array_8m / scan | 309.406 | 306.938 | 248.594 | 174.609 | -2.469 |

The 1 MiB scan is nearly unchanged in RSS; the 8 MiB scan saves about 2.47 MiB. This experiment does not resolve the excess survival or post-reclamation resident-memory retention documented in R17.

## Longer control recheck

The largest separated control slowdown, `records_object_1m / parse`, was rechecked with 324 iterations, two warmups and 11 repetitions per engine. That is four times the original matrix work. The two frozen workers are unchanged. All 44 timed checksums and five complete-output verifications pass.

| Main µs/op | R18 µs/op | Node µs/op | Bun µs/op | R18 vs main | Slower pairs | RSS delta |
|---:|---:|---:|---:|---:|---:|---:|
| 1868.299383 | 1886.546296 | 2722.697531 | 2138.345679 | +0.9767% | 11/11, separated | +0.09375 MiB |

This confirms rejection without spending a full qualification run. It does not establish why unchanged source paths slowed. No attribution to a filename, compiler layout, allocator, GC policy or rebuild identity is proven here.

[Recheck declaration](results/tape-batch-r18-validation/recheck-cases.json), [all samples](results/tape-batch-r18-validation/recheck-analysis.json), [raw timings](results/quiet-tape-batch-r18-recheck-focus/timing.jsonl).

## Actual sampled mechanism

Two predeclared, instrumented 1 MiB scans run 600 iterations each, with two warmups. Both outputs match Node, worker and sampler exits are zero, and the 2 GiB/20-second watchdogs are not reached. All 105 staged input hashes match. The profiles establish actual candidate producer entry, beyond the separate linkage witness.

These are inclusive short sampled stack shares, not exact phase timers. Full materialization contains the tape producer or direct array parser, so those columns overlap and **must not be added**.

| Arm | Main-thread samples | Tape build | Full materialization | New tape producer | Direct array parser | Collection |
|---|---:|---:|---:|---:|---:|---:|
| main | 752 | 26.20% | 51.20% | 0.00% | 50.93% | 12.10% |
| candidate | 756 | 25.13% | 50.40% | 49.74% | 0.00% | 12.43% |

The new producer replaces the full-array parser in the sampled candidate, yet materialization still accounts for about half the sampled work. Removing structural reparsing this way yields only the modest uninstrumented gains above; it does not remove object/string allocation or scalar decoding. Sampling RSS and elapsed time are diagnostic and are not used as speedup measurements.

[Profile analysis](results/tape-batch-r18-validation/profiles-analysis.json), [profile records](results/quiet-tape-batch-r18-profiles/profiles.json), [profile declaration](results/tape-batch-r18-validation/profile-cases.json), [linkage witness](results/tape-batch-r18-validation/producer-symbols.json).

The 16 KiB fixture has 120 records. The existing sequential-scan rule requires a streak of at least `max(64, length / 64)` and fewer than half the records cached. At a streak of 64, the second condition is already false; sequential traversal also does not reach the alternative cumulative-walk threshold. Thus this row never enters the changed producer. The 1 MiB and 8 MiB arrays have 7,600 and 59,000 records and can enter it. This is a source-derived admission analysis, not an invented phase attribution for the small row. [Guard and fixture counts](results/tape-batch-r18-validation/scan-admission-analysis.json).

## Correctness, GC and artifact provenance

- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: **293 pass on the committed source**. Two focused preliminary tests also pass. The new Rust coverage checks tape-produced scalars/records against the direct decoder and verifies declines preserve parser position. The new TypeScript fixture mixes duplicate/escaped keys, Unicode, numeric edges, shallow and fallback records, retained outputs, cached aliases/mutations, and scan/stringify-triggered materialization.
- Both arms pass **37 Node behavioral runs** across automatic/tape/direct parsing and normal/scheduled/full GC, plus **14 stringify-option checks**. Scheduled runs assert actual movement and protected retired sets. The new fixture has 620 protected sets and 90,315 moved objects in auto/tape modes, and 620/88,873 in direct mode, on both arms.
- All sixteen emitted IR files match main after removing only the first native ModuleID path comment; shadow IR uses no normalization. Native analysis retains **eight unsuppressed findings identical to main**: five unrooted globals, two string-handle warnings and one stale allocation value. The new fixture adds one of those string-handle warnings to this expanded baseline corpus. Coverage is 2,469 safepoints, 1,819 live bundles and 8,998 relocates. Both shadow checks and the ordinary-worker/callback native subsets pass. This is not an all-clean native static result.
- All 24 lazy probes preserve main's exit codes and full stdout: six 180-record zero/true-spacing cases still SIGSEGV, two plain whitespace/duplicate-key cases still return noncanonical JSON, and the other sixteen match Node. The main/Bun versus Node fractional-spacing difference also remains. These are preserved baseline gaps, not conformance passes.
- Script lint passes **73/74**, with the existing public-benchmark freshness failure. The file cap passes. The linted staged patch is byte-identical to the final committed source, verified separately. Compile tier and two CI-only checks were skipped; full CI is not claimed.

The exact production command was `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`. From the clean committed source it took 332.49 seconds. The compiler and both archives have mtimes after build start and were frozen with verified hashes. Mtime-only invalidation of the two static-wrapper entrypoints and compiler entrypoint is recorded; source bytes and build flags are unchanged. Main reuses the original fresh main build by verified hashes, with workers, fixtures and IR recompiled at the common R18 paths.

The first options-validation setup attempt failed before executing Perry because the JS oracle worker had not been copied. That failure is preserved; after copying the unchanged JS and building main workers, all fourteen checks passed. No failed subject run is discarded.

[Units](results/tape-batch-r18-validation/unit-source.json), [build](results/tape-batch-r18-validation/build-provenance.json), [source and freshness](results/tape-batch-r18-validation/source.json), [main reference](results/tape-batch-r18-validation/reference-main.json), [candidate behavior](results/tape-batch-r18-validation/candidate-fixture-validation.json), [options](results/tape-batch-r18-validation/candidate-options-validation.json), [static comparison](results/tape-batch-r18-validation/root-comparison.json), [lint equivalence](results/tape-batch-r18-validation/lint-source-equivalence.json), [setup failure](results/tape-batch-r18-validation/main-options-setup-failure.log.gz).

## Preserved windows and next investigation

All times are UTC on 2026-09-10. Every terminal window was archived as the first subsequent remote operation.

| Window | Start–finish | Load before → after | Verdict |
|---|---|---|---|
| 18-case screen | 22:32:12–22:36:10 | 1.618 → 2.118 | Quiet pass |
| Two profiles | 22:38:22–22:38:26 | 1.790 → 1.727 | Quiet pass |
| Longer control | 22:41:35–22:42:07 | 1.214 → 1.835 | Quiet pass |

Total: **548 uninstrumented timed trials, 95 full-output verification trials and two instrumented profiles**. No full-50, access, rotating, retained-output, short-call or options performance qualification ran. Prepared drivers are not execution evidence. Parsing and stringify remain in the unchanged full objective.

Before another code variant, an independently rebuilt main should be compared to the existing frozen main reference on the recurring controls. Prior identical-binary A/A checks were flat; they do not test rebuilding. This is a diagnostic next step, not evidence that the existing baseline is wrong. The 120-record path also needs a producer that can efficiently combine already-cached records with the uncached remainder, with a cost-based admission review and identity/mutation/GC checks. Merely changing the threshold to make one row win is not a validated solution.

[Validation archive manifest](results/tape-batch-r18-validation/manifest.json).
