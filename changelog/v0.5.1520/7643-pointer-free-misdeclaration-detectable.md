**test(gc): make the `POINTER_FREE` misdeclaration hazard detectable (#7635)**

#7635 sabotaged #7633's `layout_finish_deferred_boxed_object(ptr, saw_pointer)`
to `(ptr, false)` — every JSON-parsed record claiming `POINTER_FREE` while
holding heap strings — and got byte-identical correct output under
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` and under
`PERRY_GC_FORCE_EVACUATE=1`, with copying minors and retired quarantine sets
observed live. Every future change to layout-state bookkeeping was about to
inherit that sentence as evidence.

**The instruments were never at fault; the probe's subject never existed.**
`js_json_parse` routes a 1 KB–16 MB top-level array through the lazy tape
(`json_tape`), so `parse_object` — the function carrying the sabotage — runs
when an element is first *read*, not at `JSON.parse` time. The probe read its
records only after the churn, so every misdeclared record was materialised after
the last collection. A temporary audit in `heap_payload_slot_selection` on the
sabotaged build counted zero `POINTER_FREE` objects with pointer-bearing payload
words ever handed to the collector. CLAUDE.md hazard 4, applied to a probe.

Re-run so the records live across a collection and everything fires: under
`PERRY_JSON_TAPE=0` the same sabotage SIGSEGVs, `PERRY_GC_FROMSPACE_SCAN=1`
reports `dangling=8000 owners=4000` (exactly 4,000 records × 2 fields, against
`dangling=0` clean) and `PERRY_GC_PROTECT_FROMSPACE=1` names the faulting
address; with the records merely touched before the churn, the default lazy path
reads back 7,872 of 8,000 values wrong.

Ships four workload-free regression tests
(`gc/tests/copying/deferred_finalize_7635.rs`) that cannot be defeated by a lazy
path or a GC that did not happen to run: the finalize's two exact outcomes, the
child-slot enumerator, relocation across a copying minor gated on
`copied_objects`, the same invariant through the real `js_json_parse` entry
point so the `json/parser.rs` call site is covered, and a permanent sabotage arm
asserting a misdeclared record enumerates ZERO slots. Sabotage-verified in both
directions: #7635's exact parser mutation reddens the parse test, neutering
`layout_finish_deferred_boxed_object` reddens all four.

The doc comment on `GC_LAYOUT_POINTER_FREE` now records what can and cannot
verify a claim about this state — including that `PERRY_GC_VERIFY_EVACUATION` is
blind to a misdeclaration by construction (it walks the same enumeration the
rewrite pass walks) and that `PERRY_GC_FROMSPACE_SCAN` is the
layout-independent instrument. No GC behaviour change, no new knobs, no codegen
change.
