### Fixed

- **Every built-in iterator step allocated a `"next"` key string to learn that
  nothing was patched.** `call_overridden_iterator_next` — the per-step probe
  that lets a user replacement of `%ArrayIteratorPrototype%.next` (and the Map
  / Set / String family prototypes) drive `for…of`, spread, `Array.from` and
  manual `.next()` — ended in a by-name prototype lookup that minted a fresh
  4-byte `"next"` string on every call. One 32-byte allocation per iteration
  step of every array, Map, Set and string iterator in the program.

  The existing early-out could not prevent it. `ITERATOR_PROTOTYPE_PTR == 0`
  ("the tower was never materialized, so no override can exist") is **dead on
  any program that has allocated one iterator**: every iterator allocator calls
  `attach_iterator_prototype`, which calls `ensure_iterator_prototypes`, which
  builds the tower. The guard is true exactly once and false forever after.

  Replaced by an allocation-free proof that runs on the path every real program
  takes: the prototype's OWN `next` slot still holds a closure whose native
  entry is the canonical thunk (the certified non-allocating own-field read,
  #9480), AND no accessor descriptor is recorded for `"next"` on it (the
  per-key Bloom bit `set_accessor_descriptor` sets before inserting, #6759 C2 —
  needed because `defineProperty(proto, "next", {get})` leaves the old closure
  in the data slot and puts the accessor in the side table). Anything else —
  replaced, deleted, an accessor, a bound copy — takes the by-name path
  unchanged.

  Affected files:

  - `crates/perry-runtime/src/object/iterator_prototypes.rs` — the
    `prototype_next_is_canonical` probe, ahead of the by-name lookup.

  Measured: the 2026-09-06 claude-code allocation census ranked this site third
  by count — ~122,880 allocations of 32 bytes per 400-character reply, 17.1 %
  of the top-30 allocation count — and misattributed it to `Intl.Segmenter`
  substring copying. Resolved by an explicit caller walk in the shipped binary:
  `js_for_of_next+0xd0` → `dispatch_array_iterator_method_inner+0x218` (a `bl`
  to `call_overridden_iterator_next`) → `+0x67c` (a `bl` to
  `js_string_from_bytes_with_capacity`) → `string_storage_alloc`.

  Validation: `test-files/test_gap_iterator_prototype_next_patch.ts` drives a
  replaced `next` through `for…of`, spread, `Array.from` and manual `.next()`
  on all four families, and covers restore-by-identity, a second replace after
  a restore, a bound copy of the original (which must NOT be mistaken for the
  builtin), an accessor `next`, and a deleted `next`. The unit counter asserts
  that 1,000 probes on an unpatched iterator with the tower materialized move
  the arena by ZERO bytes, with the minor-cycle count pinned so a collection
  inside the window cannot manufacture a zero delta.

  Counter on a relinked claude-code binary (this fix plus a measurement-only
  hit/miss counter; before the fix every probe allocated, so `hits + byname` is
  the pre-fix count and `byname` is what survives): a 400-character reply runs
  **144,189 / 144,303** probes and a 3300-character reply **887,076**, with
  **`byname = 0` on every one of the 173 per-minor reports across three runs**
  — the proof answers 100 % of probes on a real program. At 32 B a string that
  is 4.6 MB and 28.4 MB of allocation removed per process respectively.
