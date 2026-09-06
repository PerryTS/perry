**A `RegExp` literal's construction no longer pays the write barrier's parent
classification.** Since #9845 the `RegExpHeader` is a nursery allocation, so
its two string field stores — `pattern_ptr` and `flags_ptr` — cannot owe the
remembered set anything; they were still taking the full barrier and
discovering that fact, twice, at a cost of four page-map classifications, two
dirty-page-cache probes and two child classifications per construction, all
ending at `ParentNotOldSkips`.

The fix is the runtime twin of a gate the compiler already emits in front of
every one of its own stores (`emit_parent_may_need_remembering_check`, #7511):
`GC_FLAG_TENURED` clear on the parent's live header **and** a globally idle
incremental mark barrier ⇒ neither the remembered set nor the SATB shading has
anything to record. Both clauses are read live, so a header a collection
promoted between `arena_alloc_gc` and the store, or a
`RegExp.prototype.compile` reassigning a tenured receiver, still takes the
full path.

Why the two clauses and not one: the tenured bit answers the generational
question, and the incremental count is what makes it legal to skip the
insertion shading as well — dropping either is a live child swept, which is
what `gc::tests::inline_generation_gate_contract` already pins for the emitted
gate and now pins for the runtime twin, clause by clause, against the same
codegen predicate. A third test asserts on the header `js_regexp_new` actually
returns, so the skip arm is proven reached rather than merely available.

Measured motivation (segment-loop probe, region B, 60,000 reps, `sample`, main
thread, leaf sum = thread header exactly): the probe constructs one `RegExp`
per grapheme from a literal inside a function body, and the barrier subtree
under `js_regexp_new` is 739 of 14,628 main-thread samples — 32 % of that
function's own subtree.

`PERRY_REGEX_NEWBORN_BARRIER_GATE=0` restores the unconditional pair; with the
gate off nothing else changes, so the OFF arm is the pre-change code path
exactly rather than a handicapped control.

`PERRY_REGEX_DIAG` gains the counters that make the claim checkable rather
than argued: `barrier_taken` / `barrier_gated` (whose sum must equal `new`),
`header_bytes`, `site_verify_bytes` (the site cache's byte-compare volume,
which `pattern_bytes` does not isolate) and `side_table_inserts`. Two
reliability fixes ride along: a diag file the process cannot write now says so
on stderr and falls back there instead of vanishing silently, and the first
snapshot is written at the first tick rather than after a full second, so a
short run can no longer look like a dead instrument.
