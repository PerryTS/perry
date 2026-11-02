### Iterators: spread / `Array.from` silently truncated at 100,000 elements

`[...m.values()]` on a 250,000-entry Map returned **100,000 elements** — no
error, no warning, a plausible-looking wrong answer that a caller would ship.
Node returns all 250,000.

Three drain loops in `array/iterator.rs` carried a hardcoded
`for _ in 0..100_000` "safety limit" and simply fell out of the loop when it
was hit, returning whatever had accumulated. Every spread, `Array.from`, and
iterator-protocol drain was affected — Maps, Sets, generators, and user
iterables alike.

Two changes, and the second matters as much as the first:

- The bound is now `MAX_ITERATOR_DRAIN` = JavaScript's own maximum array
  length (`u32::MAX - 1`), so every realistic workload is unaffected.
- **Exhausting it throws a `RangeError` instead of truncating.** Node applies
  no limit at all — `[...it]` runs until the iterator finishes or memory runs
  out — so matching it exactly would trade silent truncation for an unbounded
  loop. A visible, recoverable error is the right third option; returning
  short data is strictly worse than either.

`test_gap_7562_iterator_no_truncation.ts` covers all three drain paths (Map
values/keys/entries, Set, and a generator) past the old bound, byte-identical
to node. Sabotage-verified: restoring the 100,000 bound makes it exit 1.

Found by the `map_1m` investigation (#7561) and filed as #7562; pre-existing,
confirmed at `969b447cc`.
