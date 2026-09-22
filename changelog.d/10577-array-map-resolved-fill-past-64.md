`Array.prototype.map` filling a plain result array only took the once-resolved
header fast path (`fill_resolved_array_slot`, from the earlier map fill
change) for a source of at most 64 elements; a longer source fell back to
`note_array_slot`, which re-classifies the result's ownership/forwarding
through `clean_arr_ptr` (`array_numeric_layout`) and unconditionally pays
`layout_note_slot`, on every element. `result` is re-derived from the result's
own GC root immediately after the callback returns and before either helper
runs, so the "no intervening allocation or safepoint" contract
`fill_resolved_array_slot` needs holds regardless of length — the 64-element
split was scope, not a correctness boundary. It now applies unconditionally.

Also dropped a redundant raw `ptr::write` of the mapped value that ran
immediately before both branches — both `fill_resolved_array_slot` and
`note_array_slot` perform their own (possibly canonicalized) store of the same
slot, so the first write was always immediately overwritten.

`a.map(x => x + v)` over a 16-vs-80-element `number[]` (measured as the
marginal per-call cost difference of two probes differing only in element
count, per element, N=20000, median of 7): 450.8 -> 206.1 instructions per
element (-54.3%), control `(loop80-loop16)/64` reads -0.39 and +0.08 in the
before/after arms respectively.

New fixture `test-files/test_gap_array_map_resolved_fill_scale.ts` exercises
sources both under and over the old 64-element boundary and the ~2048-element
born-old allocation threshold: a callback returning non-numeric values
(retiring the raw-f64 numeric claim mid-fill), one that allocates heavily to
force collections between the header resolve and the store, one that pushes
to the source mid-fill, one that truncates it mid-fill, a sparse/holey source,
and a plain numeric control. Matches node 26.5.1 and passes under seeded
moving-GC stress (`PERRY_GC_SCHEDULE_SEED`/`PERRY_GC_PROTECT_FROMSPACE`).
