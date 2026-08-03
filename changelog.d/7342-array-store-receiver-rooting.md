### Fixed

- **`a[i] = v` could store through a pre-collection array address.** The
  receiver is evaluated first and the value last (spec order), so it sits in an
  SSA register while the RHS runs. When the RHS allocates, an evacuating minor
  relocates the array — the slot the register was loaded from is a registered
  root and evacuation rewrites it, but the register is not, so the store landed
  in retired from-space.

  Codegen already had the machinery for exactly this (#7154's
  `guard_store_operand_across` / `reread_store_operand`) and the generic object
  paths used it; three array paths did not. All three now do.

  Reproducing it needs a **module-level** array written **inside a function**
  with an **allocating RHS** — a local array, a top-level loop, or an inert RHS
  are each clean on their own. It is invisible from program output because
  evacuation copies rather than zeroes, so the stale address still reads the
  correct old bytes; `PERRY_GC_PROTECT_FROMSPACE=1` is what turns it into a
  fault. Both root backends failed identically, because the value was never
  given a root slot at all.

  Nothing changes on the hot path: the guard is gated on the RHS being able to
  collect, so `a[i] = i * 2` emits the same IR as before.

- **The from-space quarantine now runs over real programs.**
  `gc_instrument_smoke.sh` drove `PERRY_GC_PROTECT_FROMSPACE` with
  `PERRY_GC_MOVING_LOOP_POLLS` over one synthetic fixture, and back-edge polls
  fire only while user JS runs — so it structurally could not expose staleness
  in runtime-internal code. A new arm runs every `gc_ratchet` probe by the
  allocation-point route instead, with a non-vacuity check so an empty probe set
  fails rather than reporting a clean sweep of nothing. This is what surfaced the
  bug above (#7341).
