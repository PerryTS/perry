### `gc-ratchet`: `05_closure_capture`'s +16.44% was false-root residue, and there is now a command that says so (#7559)

`05_closure_capture` reported `heap_used_bytes` **+16.44% with every collector
counter at +0.00%** the moment #7557 made the ratchet measure again. Nothing was
kept alive that had not been kept alive before: measured with the conservative
native-stack scan disabled, that probe retains **5,329,880 bytes at both
endpoints, to the byte**. What moved was one falsely-retained 1 MiB nursery
block.

**Why the measurement produces that.** Two properties of the measurement point,
neither of which has anything to do with the collector's retention:

1. Every probe reads `process.memoryUsage()` immediately after an explicit
   `gc()`, and an explicit `gc()` is the **one** site in Perry that *forces* the
   conservative native-stack scan (`ManualGcScanGuard`, #4977; production
   resolves to `SkipDisabled`). `PERRY_GC_DIAG` confirms the suite's entire
   conservative-scan census is the probes' own `gc()` — `site=manual_collect
   automatic=false` on all twelve — plus one automatic `old_reclaim_alloc_point`
   on `12_large_live_set`. The gated number is therefore taken under a root set
   nothing else in the language uses.
2. `js_arena_stats` sums each arena block's bump-pointer **offset**, and a block
   holding one marked object cannot be reset. One stale stack word costs a whole
   1 MiB block — roughly 26,000x amplification. This is the nursery's version of
   the old-generation accounting problem #7437/#7443 fixed for `OLD_ARENA` by
   subtracting swept holes.

`PERRY_GC_DIAG` counts it at the measurement collection: general blocks marked
live on `05_closure_capture` are **5 with the scan off at both endpoints**, 6
with it on at the pinned commit, 7 at v0.5.1321. Both minors are bit-identical
across the arms (`copied_objects` 1422 + 1614 = 3036, exactly the pinned value),
and with the scan off both arms free exactly 8,124,192 bytes at the final
mark-sweep and retain exactly 425 forwarded stubs.

**The whole window.** Across the 74 commits from the 2026-08-05 pin
(`5e236e6e2`) to v0.5.1321 (`b5a2954ec`) — both arms built
`--release -p perry -p perry-runtime-static -p perry-stdlib-static` with a cold
object cache, and the baseline arm reproducing the pinned artifact byte-for-byte
on all twelve probes — `heap_used_bytes` moved on **five** probes, always by
whole blocks, three down and two up; the same probes' scan-off retention was
byte-identical on **ten of twelve** and **fell** on the other two.
`02_survivor_promotion`'s +2.77% is the same shape one survivor block down
(262,160 B), and its precise retention fell by 1,600 bytes.

Nor is the metric reporting the probe's live set, which is ~zero at the
measurement point for every workload size: sweeping `05_closure_capture`'s
`BATCHES` from 690 to 710 with one compiler walks it between 6,501,264 and
7,426,960 in a 1 MiB sawtooth, because what is left over is the un-reset tail
blocks' bump pointers (`arena_reset_empty_blocks` never resets the current block
or the four before it).

**What landed.** `gc_ratchet.py classify` runs every probe under both scan modes
and prints `conservative` / `precise` / `excess` plus the census of which
conservative-scan sites fired. A row whose `excess` moved and whose `precise`
did not is a false-root artifact; a row whose `precise` moved is a real
retention change. It refuses to tabulate a probe whose *stdout* changes when the
scan is disabled (the scan was load-bearing for that probe's correctness, so its
precise number is not evidence about the collector), and refuses to report a
precise reading that is not bit-identical across repeats — while *allowing* the
conservative reading to vary and reporting its spread instead, because that
spread on `12_large_live_set` is exactly why #7554 had to stop gating the cell.
All twelve probes are verified to produce byte-identical stdout under both
modes.

The finding is recorded in `gc_ratchet.py`'s module docstring, in
`benchmarks/gc_ratchet/README.md` (a new section on what `heap_used_bytes`
actually contains, and a mandatory classify step in "When the gate goes red"),
and in `tolerances.json`'s `_readme`. Six new tests cover it, including an
end-to-end `classify` run against a stub compiler that plays the #7559 shape and
one that fails if the runtime renames `PERRY_CONSERVATIVE_STACK_SCAN` — a rename
would otherwise make every `precise` column silently equal its `conservative`
one, i.e. a classifier that classifies nothing.

`run_once` now hands `Popen` the exit status `os.wait4` collected behind its
back, so a harness run stops emitting a `ResourceWarning` per probe.

**Deliberately unchanged:** no band is widened and nothing is re-pinned.
`05_closure_capture` and `02_survivor_promotion` stay red until someone decides,
which is what the ratchet is for. Widening the band is not an available answer:
`heap_used_bytes`'s churn quantum is one whole block, which is also the unit
`test_one_falsely_retained_nursery_block_fails` asserts must go red — a quantity
whose noise quantum equals its signal quantum cannot be gated, so the fix is to
measure a different quantity, not to loosen the band on this one. The artifact's
embedded tolerances copy is synced to the file (prose only; every
`pct`/`abs`/`direction`/`gating` tuple asserted identical before the rewrite)
because `evaluate` reads the artifact's copy, not the file.
