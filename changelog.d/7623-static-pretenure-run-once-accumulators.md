**perf(gc): static pretenuring for run-once accumulator loops (#7598)**

The dominant cost on promote-heavy loops after #7594/#7596 was structural:
every long-lived object was copied twice, Eden→survivor by the first copying
minor and survivor→old by the next. Objects pushed into an accumulator that
codegen can prove long-lived — the all-pointer admission terms, plus the
`let` at loop depth 0, every push at depth ≥ 1, and the region running
exactly once (module main/init; `region_runs_once` is an explicit parameter
at every fact-graph call site) — are now born in old-gen with
`GC_FLAG_TENURED`, via a `mem::take`n per-site flag consumed by the root
allocation only.

Two allocator fixes make it a win instead of the measured loss: the new
`arena_alloc_gc_old_born_tenured_bump` defers `register_old_object_pages`
(per-object it paid a linear dedup scan of the page's object list — quadratic
as a page fills) into a buffer flushed at `old_pages_begin_gc_cycle`, and
skips the hole-reuse probe; and function/method/closure regions refuse
admission outright, since a per-call accumulator's cohort dies at return
(measured 6.6× slower when pretenured).

json_pipeline: 200k 2.40 → 1.54 s (peak RSS 543 → 431 MB), 500k ~8 → 5.4 s
(RSS −173 MB), output hash identical; bytes copied by minors 108 → 0.0 MB.
Mechanism is IR-verified: exactly one pretenured call site in the workload,
zero in the numeric-push and function-region benches.
