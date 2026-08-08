**perf(gc): defer the JSON materialiser's per-slot layout notes to one finalize (#7630)**

Step-zero profiling on the pinned bench host put the per-slot layout
machinery at the top of `json_pipeline`'s cost families: parse-built records
are born `POINTER_FREE`, the first string field builds a per-object
side-table pointer mask, every subsequent field store pays a hashmap
round-trip, `layout_transfer` moves the mask on promotion, and
`layout_forget_object` drops it at death.

The materialiser owns each object's whole construction, so its store loops
now use `runtime_store_jsvalue_slot_layout_deferred` — the shared helper
minus the layout note, with the typed-slot canonicalization, string addref
demote, and write barrier (including its SATB shade) kept bit-for-bit — and
settle the layout state once per object: `POINTER_FREE` stays for
number-only records (keeping their whole-payload trace skip), anything with
a pointer becomes `GC_LAYOUT_UNKNOWN`, the tag-checked scan-all state — a
pointer mask can never skip anything for a cohort whose every slot is a
NaN-boxed `JSValue`. Routed through `layout_mark_unknown` so a mask created
by the shaped path's by-name fallback mid-construction is removed rather
than stranded.

Pinned-host, interleaved, hash-identical: `json_pipeline` 200k `build_out`
934 → 730 ms (−22 %), total −17 %, GC pause 816 → 689 ms, peak RSS
451 → 422 MB; `layout_note_slot` / `layout_forget_object` vanish from the
profile. Also splits `barrier.rs`'s slot-store helpers into
`barrier_store.rs` for the 2000-line cap.

Qualification added by #7643: this landed on the strength of its argument
(the materialiser owns each object end to end, finalize is reached on every
path, the pointer case is conservative) plus a clean gap suite — the GC
half of it had no regression test, and #7635 showed why that mattered by
forcing every parsed record to `POINTER_FREE` and getting byte-identical
correct output from every runtime instrument. The argument was right, but
the probe could not have shown it either way: `js_json_parse` is lazy for
1 KB–16 MB top-level arrays, so the records were materialised after the
last collection. #7643 adds the workload-free regression tests
(`gc/tests/copying/deferred_finalize_7635.rs`) and the verification note on
`GC_LAYOUT_POINTER_FREE`.
