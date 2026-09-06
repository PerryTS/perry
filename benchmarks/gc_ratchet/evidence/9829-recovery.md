# GC ratchet recovery: issue #9829

The shared-CI gate is restored by repairing probe 10, accepting 46 specific
deterministic counter deltas, and excluding one demonstrated noisy capacity cell.
Every other distribution and every tolerance band is retained. The original
macOS RSS/timing pin is preserved; the resized workload needs a separate timing
capture on its original quiet host before using the pinned_host profile.

This is the internal Perry-vs-Perry GC artifact. The public Node/Bun performance
baseline and package versions are unchanged.

## Provenance and acceptance boundary

The pre-fix subject is main d36a1af0c205ebdc7cf7f75b351ea34b5bc0fc0b and
[scheduled run 33989581881](https://github.com/PerryTS/perry/actions/runs/33989581881).
A fresh Linux x86-64 compiler and both static archive wrappers reproduce all
126 deterministic **medians** from macOS CI, including all 43 failing rows.
125 complete distributions match; CI's large-Eden capacity has one +2 MiB sample.
The same variation appears locally, so it was investigated rather than discarded.

The gate failed unwatched from 2026-08-18 to 2026-09-06. Its failing cell count
moved 43 -> 48 -> 43, with intervening harness failures. This pin is one sample
of a moving set, not accumulated independent regressions.

[9829-recovery.json](9829-recovery.json) contains original deterministic samples,
Node correctness results, source and binary hashes, eight historical builds
(four adjacent-commit A/Bs), the protected-probe sabotage control, the retention
classification, and 21 executions of the capacity probe. All builds use the same
package set: perry, perry-runtime-static, perry-stdlib-static, in release with
codegen-units=16 and LTO disabled. Measurement pins the runtime archive directory
and disables automatic workload-specific archive rebuilding.

The receipt distinguishes **measured** changes from **bracketed** windows.
The latter are the accepted window-level explanations from the
[issue's completed classification](https://github.com/PerryTS/perry/issues/9829#issuecomment-5555805084),
not claims that an individual candidate was bisected. Repeating that distinction
in the machine-readable causes prevents a future reader treating every candidate
as established fact.

## What the measurements establish

- **#8313, smaller objects:** the first 19 crossings were already bisected
  [byte-exact in the issue](https://github.com/PerryTS/perry/issues/9829#issuecomment-5555657705).
  Smaller allocations lower byte traffic and change which cohort reaches a minor.
  No post-release heap_used increase on probe 12 is accepted here: it returned
  inside its original band.
- **#8657, array growth generation:** our c2da03439 -> 06e1ab349 build pair
  reproduces the large 04/05 copy/promotion jumps. Probe 04 reaches exactly today's
  149400 copied bytes and 682 promoted objects. The safety fix keeps old array
  growth targets out of the nursery and avoids collection while growing a young
  array. Probe 07 capacity rises by 2 MiB and probe 14 copied objects move 340 -> 330.
- **#8806, transition cache:** our 066f9d595 -> b3e88f1fb pair reproduces all four
  August 26 crossing values exactly: 01 copied=6684; 06 copied=11714;
  09 promoted=6560; 11 copied=6562.
- **#8900, weak targets:** our d02892491 -> e18b24c42 pair confirms part of the
  later drift: probe 10 copied objects fall 8487 -> 7457 and copied bytes fall
  529144 -> 327328. The transition target becomes a rewrite-only edge, with dead
  cache entries pruned. It does **not** explain every cell in that window.
- **#9373, short-concat memo:** our 3fc54c441 -> d923b8dcf pair moves probe 13
  from no copies to 356717 copies / 20594360 copied bytes, with promotions falling
  531314 -> 365297. Reused row tags change nursery occupancy and collection
  cohorts under the existing policy. The earlier #9359 candidate was incorrect:
  it adds checks behind PERRY_GC_VERIFY_MARK. Later values have further offsets.
- **#9514 control:** compiling current main with PERRY_CONCAT_SITE_CACHE=0 changes
  no deterministic median in these probes. It is not used as an explanation.
- **Retention classification:** current main with the repaired probe has zero
  conservative-stack excess on all 14 probes. Probe 07's accepted live-byte tail
  is real (+87616 bytes), classified as the allocation/packing tradeoff in the
  September 4 window; it is not described as a memory improvement or an exact
  single-commit attribution. Its live bytes remain gated after this acceptance.

## Probe 10 must collect

The included #9833 repair raises the workload from 200000 to 2400000 iterations.
Seven-repeat measurements give nine minors, 18660 copied objects, 4722 promoted
objects, and 88858544 freed bytes. Live bytes fall 464072 -> 244648, already
inside the original 220384 + 65536 band, so that cell is **not** re-pinned.

The fixed probe passes with PERRY_GC_PROTECT_FROMSPACE=1 and depth=64.
Removing only its allocating RHS yields zero minor cycles, copies, promotions,
and freed bytes. This independently demonstrates the allocating store window;
stdout alone would not establish evacuation coverage.

## Probe 13 capacity is not deterministic

One unchanged binary, 21 runs under its declared 64 MiB nursery:

| metric | observed |
|---|---:|
| heap_total_bytes | 61865984 (18 runs), 63963136 (3 runs) |
| heap_used_bytes | 227048 (21/21) |
| minor_cycles / step_cycles | 3 / 3 (21/21) |
| copied_objects / copied_bytes | 377569 / 21429712 (21/21) |
| promoted_objects / promoted_bytes | 386271 / 21777792 (21/21) |
| freed_bytes | 108243696 (21/21) |
| Node-equivalent stdout | 21/21 |

arena/stats.rs defines heapTotal as reserved block capacity and heapUsed as the
live-object census. Capacity changes with identical live bytes and collector
work in the same binary; the exact allocation/block-order source of the spare
2 MiB is not isolated. CI independently exhibits the same two capacities.
Choosing seven runs without the outlier would conceal the premise failure.
Only this probe's heap_total_bytes becomes informational through the existing
evidence-validated probe_overrides mechanism. It is still measured and printed;
its live bytes, correctness, collection liveness, and all GC counters remain
gated. No band is widened and the old capacity distribution is preserved.

## Accepted cells

Only the cells below change. A row containing a bracketed cause retains that
evidence grade even when another part of its history was measured exactly.
Cause descriptions and full commit hashes are in accepted_deterministic_deltas;
that receipt has no role in granting tolerance allowances.

| Probe | Metric | Previous | Accepted | Evidence / PRs |
|---|---|---:|---:|---|
| 01_nursery_churn | copied_objects | 6,354 | 4,977 | bracketed + measured: #8313, #8806, #8887 |
| 01_nursery_churn | copied_bytes | 430,008 | 234,536 | bracketed + measured: #8313, #8806, #8887 |
| 01_nursery_churn | freed_bytes | 13,201,144 | 11,299,648 | bracketed + measured: #8313, #8806, #8887 |
| 02_survivor_promotion | copied_bytes | 1,780,080 | 1,450,992 | measured: #8313 |
| 03_cross_gen_writes | freed_bytes | 11,304,368 | 9,240,008 | measured: #8313 |
| 04_dead_after_deep_stack | copied_objects | 450 | 3,622 | measured: #8313, #8657 |
| 04_dead_after_deep_stack | copied_bytes | 25,832 | 149,400 | measured: #8313, #8657 |
| 04_dead_after_deep_stack | promoted_objects | 10 | 682 | measured: #8313, #8657 |
| 04_dead_after_deep_stack | freed_bytes | 83,880,432 | 73,346,416 | measured: #8313, #8657 |
| 05_closure_capture | copied_objects | 5,272 | 20,079 | bracketed + measured: #8313, #8657, #9628 |
| 05_closure_capture | copied_bytes | 233,384 | 743,384 | bracketed + measured: #8313, #8657, #9628 |
| 05_closure_capture | promoted_objects | 0 | 3,390 | bracketed + measured: #8313, #8657, #9628 |
| 05_closure_capture | promoted_bytes | 0 | 122,120 | bracketed + measured: #8313, #8657, #9628 |
| 05_closure_capture | freed_bytes | 32,422,176 | 26,862,320 | bracketed + measured: #8313, #8657, #9628 |
| 06_string_retention | copied_objects | 11,138 | 298 | bracketed + measured: #8806, #8887 |
| 06_string_retention | copied_bytes | 682,240 | 72,288 | bracketed + measured: #8806, #8887 |
| 06_string_retention | promoted_objects | 4,960 | 14 | bracketed + measured: #8806, #8887 |
| 06_string_retention | promoted_bytes | 248,512 | 1,072 | bracketed + measured: #8806, #8887 |
| 06_string_retention | freed_bytes | 75,244,808 | 68,136,816 | bracketed + measured: #8806, #8887 |
| 07_array_grow_evacuate | heap_used_bytes | 984,840 | 1,072,456 | bracketed + measured: #8657, #8887, #9706 |
| 07_array_grow_evacuate | heap_total_bytes | 23,068,672 | 25,165,824 | bracketed + measured: #8657, #8887, #9706 |
| 07_array_grow_evacuate | copied_objects | 6,387 | 4,991 | bracketed + measured: #8657, #8887, #9706 |
| 08_map_set_sidetables | copied_objects | 3,017 | 2,489 | measured: #8313 |
| 09_try_catch_roots | copied_objects | 0 | 4,962 | bracketed + measured: #8806, #8887, #9373, #9628 |
| 09_try_catch_roots | copied_bytes | 0 | 230,352 | bracketed + measured: #8806, #8887, #9373, #9628 |
| 09_try_catch_roots | promoted_objects | 6,240 | 0 | bracketed + measured: #8806, #8887, #9373, #9628 |
| 09_try_catch_roots | promoted_bytes | 421,800 | 0 | bracketed + measured: #8806, #8887, #9373, #9628 |
| 10_store_receiver_across_alloc | heap_total_bytes | 12,582,912 | 16,777,216 | measured: #9833 |
| 10_store_receiver_across_alloc | minor_cycles | 1 | 9 | measured: #9833 |
| 10_store_receiver_across_alloc | step_cycles | 1 | 9 | measured: #9833 |
| 10_store_receiver_across_alloc | copied_objects | 8,160 | 18,660 | measured: #9833 |
| 10_store_receiver_across_alloc | copied_bytes | 506,200 | 825,520 | measured: #9833 |
| 10_store_receiver_across_alloc | promoted_objects | 0 | 4,722 | measured: #9833 |
| 10_store_receiver_across_alloc | promoted_bytes | 0 | 228,440 | measured: #9833 |
| 10_store_receiver_across_alloc | freed_bytes | 8,930,928 | 88,858,544 | measured: #9833 |
| 11_collect_at_depth | copied_objects | 6,245 | 4,975 | bracketed + measured: #8806, #8887, #9373 |
| 11_collect_at_depth | copied_bytes | 422,744 | 230,872 | bracketed + measured: #8806, #8887, #9373 |
| 12_large_live_set | copied_bytes | 2,851,312 | 2,327,008 | measured: #8313 |
| 12_large_live_set | promoted_bytes | 25,395,472 | 21,201,208 | measured: #8313 |
| 12_large_live_set | freed_bytes | 76,315,456 | 63,732,784 | measured: #8313 |
| 13_large_eden_survivors | copied_objects | 0 | 377,569 | measured: #8313, #8900, #9373 |
| 13_large_eden_survivors | copied_bytes | 0 | 21,429,712 | measured: #8313, #8900, #9373 |
| 13_large_eden_survivors | promoted_objects | 541,614 | 386,271 | measured: #8313, #8900, #9373 |
| 13_large_eden_survivors | promoted_bytes | 30,316,904 | 21,777,792 | measured: #8313, #8900, #9373 |
| 13_large_eden_survivors | freed_bytes | 97,086,288 | 108,243,696 | measured: #8313, #8900, #9373 |
| 14_grow_then_churn | copied_objects | 307 | 330 | measured: #8313, #8657, #8900 |

## Reporting and harness integrity

distribution() now rounds samples before deriving summaries, so saving and
reloading timing data cannot manufacture a corrupt-artifact error (#9834).
The regression cases fail before the fix, including a median-rounding case.

Scheduled and main-branch dispatch runs now maintain one open GC ratchet issue,
with the run/SHA, last green SHA, failed steps, regression rows, and added/cleared
cell names. A later successful main run closes it. Missing measurement logs
still produce an incident with the failing job link. PRs, tags, cancelled runs,
and dispatches on other branches cannot write issues. Reporting uses a separate,
serialized job with its own issues:write token; it does not change the gate's
exit status. Extending this to other workflows remains the separate #9830 scope.
