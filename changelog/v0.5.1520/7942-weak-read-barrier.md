Closed a **use-after-sweep** in the budgeted (incremental) collector: a weak-to-strong
transition through a *read* that no barrier observed.

A budgeted cycle performs its one-time `FinalRootRemark` and then keeps opening mutator
windows while `AtomicFinalize` is still sliced — the full path's `RememberedSetRebuild`,
and (since #7892) the weak-holder loop itself. The soundness argument recorded for those
windows is "the incremental mark barrier shades every store, and mid-cycle allocations
are born black". Both mechanisms only observe values the mutator **writes** or
**creates**.

`WeakRef.deref()` and `WeakMap.get()` do neither. They take a **white** object — white
*by construction*, because weak edges are deliberately excluded from the strong trace —
and hand it to compiled code as a strong local. The remark has already run, so no later
root scan can discover the new reference: the next weak slice sees the target unmarked,
tombstones it, and the sweep reclaims memory the mutator is still holding. It surfaces
cycles later as `TypeError: value is not a function`.

The window is reachable on default settings. `gc_incremental_enabled()` has been ON since
#6180, so ordinary allocation pressure runs budgeted cycles, and every `WeakMap`/`WeakSet`
**entry** is its own registered holder — any collection with more entries than the step
budget (2048 on a host safepoint, 256 plus debt scaling on an allocator assist) parks
mid-registry with the rest of the registry pending.

**Fix — a weak-read barrier** (`crates/perry-runtime/src/weakref/read_barrier.rs`). Every
weak read shades the value words it hands out, using the same shade-and-seed the store
barrier uses (`gc::gc_weak_read_shade` → `incremental_mark_barrier_value`): the target in
`WeakRef.deref()`, and the matched key plus returned value in `WeakMap.get()` /
`WeakMap.has()` (`WeakSet.has()` delegates). Three properties make that sufficient rather
than a patch:

* the pending weak decision is a mark-set predicate (`weak_target_should_clear`), so a
  shaded target is kept — which is also what the spec's `AddToKeptObjects` requires of
  `WeakRef.deref`;
* the shade pushes a mark seed, and both pre-sweep drains already exist (the minor arm of
  `RememberedSetRebuild`, the full arm of `DisableBarrier`), so the target's children are
  traced — a marked-but-untraced object would have been the same bug one level down;
* it closes the full path's **pre-existing** window as well, where the sliced
  remembered-set rebuild sits between the remark and the weak decisions. Un-slicing the
  weak loop would not have.

Outside a cycle the barrier is inert, and that is asserted in its own test: a stray mark
laid down with no cycle in flight reads as "already live" to the next cycle's trace.

**Two comments were wrong and are corrected.** The `FinalRootRemark` enum doc still said
"from this subphase to the Sweep transition the minor path runs ATOMICALLY (no mutator
windows)" — untrue since #7892 put `WeakProcessing` in the sliced set; the sibling comment
470 lines away was updated and this one was not. The sliced-set comment argued weak
slicing was safe because "a target the mutator can still name was marked at remark or was
born black" — neither covers a target the mutator names *for the first time* through a
weak read.

**Coverage** (`crates/perry-runtime/src/gc/tests/weak_read_barrier.rs`) is a deterministic
state-machine test, not a "does not throw" test. Four cases: the full budgeted ordering,
the budgeted-minor ordering (no remembered-set rebuild between remark and weak
decisions), the `WeakMap` shape, and the barrier's OFF state. Each run asserts its own
premises:

* **the window opened** — 8 holders against a one-unit budget, `weak_processing` consumed
  exactly one holder and is still the parked subphase;
* **the remark actually ran** — a *remark witness*: a white, unreferenced object installed
  into a shadow slot only at the `AtomicFinalize` boundary, i.e. after `RootScan` and
  `MarkPropagation`. `js_shadow_slot_set` performs no barrier, so nothing but
  `FinalRootRemark` can have marked it by the time weak processing parks;
* **the read shaded something white** — a shade counter, so a run where the target
  happened to be marked already cannot pass as evidence;
* **the target survived** — its `OVERFLOW_FIELDS` side-table entry is intact (a swept
  owner has it cleared by the dead-payload sweep arm, the same shape the production bug
  presented as) *and* `deref()` still returns the same bits.

Sabotage-verified in two arms with the fix committed first. With the shade removed
entirely the subject-live assertions fire, proving the counter is wired to the mark and
not to the call; with the barrier counting but not marking, both survival assertions fire
with the use-after-sweep message — which is the arm that proves the tests catch the bug
rather than the instrument.
