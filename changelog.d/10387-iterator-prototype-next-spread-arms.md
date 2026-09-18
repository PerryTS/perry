### Fixed

- A replaced `%ArrayIteratorPrototype%.next` (and its Map / Set / String
  siblings) now drives spread, `Array.from` and call-spread, not just `for…of`
  and a manual `.next()`. `[...[4, 5]]` under a patch that doubles each value
  printed `4,5`; `[...new Set([1, 2])]` printed `1,2`; `[..."ab"]` printed
  `a,b`; `f(...[13, 14])` passed the raw elements.

  Each of these ends in a runtime element-COPY arm — `dense_spread_source`'s
  memcpy (#7533), `js_set_to_array` / `js_map_entries`,
  `js_string_to_char_array`, `js_array_like_to_array`'s array fast path — that
  materializes the result without ever calling `.next()`, so the per-call
  "is `next` still the builtin?" proof in `object/iterator_prototypes.rs` cannot
  be reached from inside them. This is the same hole #10086 closed for the
  `for…of` index loop and the array-destructuring fast arm, and it is closed the
  same way: the family prototype object can only be patched after it has escaped
  to user code through `Object.getPrototypeOf` / `Reflect.getPrototypeOf`, so
  that escape is the choke point. `note_iterator_prototype_exposed` (was
  `note_array_iterator_prototype_exposed`) now recognises the Map, Set and String
  family prototypes and `%IteratorPrototype%` itself as well as the array one,
  and the copy arms decline on their family's signal and run the real protocol.

  The array arms move from the narrow `array_proto_iterator_modified` (the
  `Symbol.iterator` slot was written) to the broader
  `array_iteration_not_pristine`. The narrow fact implies the broad one, so
  every receiver that declined before still declines. Nothing changes for a
  program that never introspects a built-in iterator: all four signals stay
  false until `Object.getPrototypeOf` hands the prototype out.

  `test_gap_iterator_prototype_next_patch` has been red on `main` since it
  landed on 2026-09-06 (gap-suite shard log for `87dc334920` — the first run that
  contained it — already reported `pass -> parity_fail`), so this is a
  first-time fix of a fixture that over-specified the implementation, not a
  regression repair. The fixture and its
  `crates/perry/tests/issue_9846_iterator_prototype_next_patch.rs` companion
  gained `Array.from(array)`, call-spread, multi-operand spread,
  `Array.from(set)`, `Array.from(map)`, `[...map]` and `Array.from(string)`
  cases under the same patches.

The fixture also could not pass for a reason that was not Perry's: it called
`console.log` while a built-in iterator prototype was patched. Node builds
`SafeMap` out of `internal/per_context/primordials` lazily, and the parity
harness runs the oracle under `FORCE_COLOR=0`, which is the path that defers
that construction into the patched window — so the ORACLE died with

    node:internal/per_context/primordials:449
      class SafeMap extends Map {},

leaving `Node exit: 1, Perry exit: 0` however the runtime behaved. Output is now
buffered inside each patched window and flushed after the prototype is restored;
every value is still computed inside the window, which is the subject, and the
emitted text is byte-identical to the unbuffered run.
