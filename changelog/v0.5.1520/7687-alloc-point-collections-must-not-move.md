### Fixed — an allocation-point collection can no longer MOVE anything (#7682)

A 189-statement tree-walking interpreter — ordinary TypeScript, no exotic
construct, `scriptc coverage` reports it fully static — returned a **silently
wrong number** on default settings, every run: `1708662` where Node and a fully
static build both give `1708840`. No crash, no `TypeError`, no diagnostic.

**Root cause.** `gc_check_trigger`'s nursery-churn arm collects from inside
`arena_cell_alloc`, i.e. at whatever half-finished expression happened to need a
fresh arena block. That program point is described by neither root lowering: the
shadow stack names only values codegen has already stored to a slot, and RS4GC
relocates only what it can type as `ptr addrspace(1)`, which a NaN-boxed
`double` operand in an SSA register is not. The arm therefore took
`ManualGcScanGuard::force_full_scan()`, whose job there is not retention but
*immobility* — a conservative native-stack scan makes the copying minor
ineligible (`CopiedMinorFallbackReason::ConservativeStack`), so the non-moving
in-place minor runs and nothing relocates.

`PERRY_GC_SCAVENGE` gated that guard off. The guard is now unconditional.

**Why the gate was ever conditional, and why both halves of the reason were
false.** The flag's doc comment said "Phase-1 de-risking flag (OFF by default) …
NOT sound as a production default yet — the alloc point can be
register-imprecise — so it stays behind this flag for measurement only". Eight
lines below it, the body said `ON BY DEFAULT (#7056)`. That is the #6987 shape
CLAUDE.md warns about, and this time the stale half was the one carrying the
soundness argument. The body's own claim — "enabling this also defers
alloc-point collections to a precise safepoint" — was false too: that deferral
is gated on `gc_moving_loop_polls_enabled()`, OFF by default since #7161. In the
shipped configuration the two flags disagree, the deferral is dead code, and the
alloc-point minor ran right there with neither a scan nor a safepoint.

**The failure, end to end.** `evalNode` lowers `{ names: [n.name], … }` by
reading `n.name` into a register, then inline-bump-allocating the one-element
array. The bump overflows its block, `js_inline_arena_slow_alloc` →
`arena_cell_alloc` → `gc_check_trigger` runs an *evacuating* minor, and the
string moves. Control returns to the shared merge block, which stores the
pre-move address into the new array. `lookup` then compares `names[i] === name`
— a live string against a moved one — falls through to its default, and naive
`fib`, which is just a count of leaves returning `1`, comes back short by
exactly the number of missed lookups.

**Why every existing gate was green.** `PERRY_GC_VERIFY_MARK` reports OK
(marking is correct; it is the post-move holder that is wrong, and it is a
register, so there is nothing in the heap to find). The heap-wide
`PERRY_GC_FROMSPACE_SCAN` finds no live offender for the same reason — every
owner it reports is `marked=false`, i.e. already dead. `scripts/gc_root_dominance_check.py`
reports 0 violations over 380 root stores: the value is never bound to a slot at
all, so there is no store whose dominance it could question. And the GC probe
corpus holds its subjects across an *explicit* churn call; none holds one across
the allocation of the literal being built.

**Cost.** Alloc-point nursery collections are non-moving again, which is what
they were before #7056. Copying minors continue to run at the precise safepoints
(`gc_safepoint_moving_minor`), where the root set is real. `PERRY_GC_SCAVENGE`
keeps its other job — routing nursery-churn triggers to the direct minor instead
of the budgeted non-moving stepper — and is documented as the pacing knob it is.
The `gc-ratchet` artifact was measured under the shipped default and pins the
old evacuating behaviour; it needs regenerating on the pinned host.

**Tests.**

- `gc::tests::scan_fallback::the_alloc_point_nursery_minor_retains_native_stack_values_under_shipped_pacing`
  drives the arm with a value reachable only from a live native-stack word and
  asserts it survives, that a collection actually ran, and that the census
  counted the forced scan. Its sabotage control
  (`the_alloc_point_plant_dies_when_the_scan_is_pinned_off`) runs the identical
  plant with the scan pinned off and asserts the plant DIES — without it, "the
  malloc sweep never ran" and "the guard held" are the same green.
- `policy::force_shipped_default_gc_pacing` pins polls OFF + scavenge ON. That
  combination had no test guard: `force_legacy_gc_pacing` pins both off and
  `force_moving_gc_pacing` pins both on, so every test in the crate declared a
  pacing mode in which the two flags agreed — and the interaction that broke is
  exactly the one where they disagree.
- `test-files/test_gap_gc_alloc_point_no_move.ts` is the interpreter itself,
  compared byte-for-byte against Node. Verified non-vacuous: it prints `1708662`
  against the pre-fix runtime under the gap harness's own
  `PERRY_NO_AUTO_OPTIMIZE=1` configuration, and `1708840` after.
