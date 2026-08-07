### Fixed

- **The entire TC39 iterator-helpers surface was dead (#7576).** `Iterator.from(x)`
  returned an iterator that was exhausted before its first step, and `.map` /
  `.filter` / `.take` / `.drop` / `.flatMap` all returned `undefined`, so any
  chain threw `TypeError: Cannot read properties of undefined`.

  **Root cause: a class-id collision.** `ITERATOR_HELPER_CLASS_ID`
  (`crates/perry-runtime/src/iterator_helpers.rs`) and `STRING_ITERATOR_CLASS_ID`
  (`crates/perry-runtime/src/string/iter_object.rs`) were both `0xFFFF_0009`.
  The two constants were introduced independently and each carries the comment
  "sits just past the Set iterator id (0xFFFF0008)"; neither author checked
  whether the slot was taken. Every dispatch tower matches these ids in a fixed
  order with the String arm first, so **every** helper object was dispatched as
  a String iterator: `.next()` read the helper's op-kind field as a cursor index
  against a null backing array and answered `{ done: true }`, and every other
  helper method fell into that dispatcher's `_ => undefined` arm. One cause,
  both symptoms. The helper now takes `0xFFFF_000B` (`0xFFFF_000A` is the
  RegExp-string iterator).

  **Second, independent defect on the same path.** `iterator_step` resolved the
  source iterator's `next` with the *inheriting* getter. Since #321 every
  built-in iterator inherits `.next` from its shared `%…IteratorPrototype%`
  singleton, and that inherited `next` is a thunk that resolves its receiver
  from `js_implicit_this_get()`. The lookup therefore found a callable closure
  for an array / Map / Set / String iterator source, took the raw-closure-call
  branch (which binds no `this`), and ran the thunk against a stale receiver —
  `done` on the first step, or `Method %IteratorPrototype%.next called on
  incompatible receiver`. `array/iterator.rs::js_iterator_to_array` already
  carried the own-field version of this fix; `iterator_step` now matches it
  (`js_object_get_own_field_or_undef`, which is also allocation-free) and binds
  `this` to the iterator per `IteratorNext`'s `Call(next, iterator)`. Both
  halves are load-bearing.

  **Why it survived.** `test-files/test_gap_iterator_helpers_2874.ts` caught this
  and had been listed in `test-parity/known_failures.json` since 2026-07-04. It
  passes byte-for-byte now and the skip is removed.

  New coverage, `cargo-test`-visible per #5960:
  `crates/perry-runtime/src/iterator_helpers/tests.rs` (16 tests, all driving
  `js_native_call_method` — the tower a compiled program reaches, since a test
  calling the helper dispatcher directly would have been green throughout the
  outage), including `iterator_class_ids_are_pairwise_distinct`, which fails on
  any duplicate in the iterator family. Plus
  `test-files/test_gap_iterator_helpers_7576.ts`, 28 lines byte-identical to
  `node --experimental-strip-types`, covering the issue reproducer, hand-stepped
  stored helpers, all six source kinds, every combinator and terminal, laziness
  over an unbounded generator, and spread.
