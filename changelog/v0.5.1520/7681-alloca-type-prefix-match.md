### Fixed

**The root-slot readers matched an alloca's whole def text, so one `align`
suffix would have made every negative rooting gate vacuous again (#7675
follow-up).**

`testing::temp_slots::temp_root_slots` selected slots with
`matches!(defs.get(slot), Some(&"alloca i64") | Some(&"alloca ptr addrspace(1)"))`.
If codegen ever printed `alloca i64, align 8`, that filter would match nothing,
`temp_root_slots` would return an empty vector, and **every
`assert_no_temp_rooting` in the tree would pass for a program that roots** —
which is the exact vacuity #7503 was opened to remove. The positive assertions
would have gone red, so the risk was bounded; the negatives would have lost
their meaning silently.

All three places that compared whole def text now go through one shared
`root_slots::alloca_type`, which reads the type and ignores anything after it:
`temp_slots::temp_root_slots` (the silent case), `root_slots::classify` (loud —
an unmatched type hits its panic arm — but it would have panicked on a perfectly
valid spelling) and `root_slots::value_slot_barriers`. Two regression tests
(`an_aligned_alloca_is_still_recognised_as_a_temp_slot`,
`an_aligned_alloca_is_still_classified`) fail against the old compare and pass
against the new one; each asserts its own substitution applied, so neither can
pass by testing an unmodified fixture.

Two smaller repairs from the same review:

* `temp_root_coverage`'s accumulator write-back clause compared registers
  exactly while every neighbouring clause went through `slot_holding`, which
  tolerates the one NaN-boxing step a raw allocation result takes before it
  reaches a slot. A lowering that boxed the push result would have failed a test
  whose contract still held. It now uses `slot_holding` too.
* `zero_seeded_slots` had a load-specific second pass that re-inserted slots the
  generic `, ptr %s` scan above it had already caught. Removed, with a comment
  saying why there is deliberately no load-specific pass — an extra branch that
  reads as coverage it does not add is the same category of problem as the rest
  of this area.
