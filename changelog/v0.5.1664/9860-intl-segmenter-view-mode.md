### Added

- **`Intl.Segmenter` view mode: five runtime entry points that answer a
  grapheme loop's questions without materialising a record or a substring.**
  The compiler half (PR #9859) proves that a
  `for (let {segment: O} of X.segment(q))` loop never lets the record or `O`
  escape, and then drives a cursor instead of building either.

  ```
  js_segments_view_open(segmenter, input)      -> cursor | 0.0
  js_segments_view_next(cursor)                -> 1.0 | 0.0      (allocation-free)
  js_segments_view_code_point_at(cursor, k)    -> number | undefined (allocation-free)
  js_segments_view_segment(cursor)             -> string          (materialise-on-miss)
  js_segments_view_regexp_test(cursor, regex)  -> true | false | undefined
  ```

  The cursor is an **ordinary GC object** whose slot 0 holds the input as a
  traced value, so the collector rewrites it like any other field — no
  registered root, no side table, no new scanner. Every entry point re-derives
  its `&str` on entry and drops it before returning.

  `open` **declines with no observable effect**, in a fixed order: a
  non-pristine `Intl.Segmenter`, a replaced `segment`, a non-grapheme
  granularity, an input that is not ALREADY a string primitive (checked before
  any coercion, because `build_segments` runs user `toString` and throws on a
  Symbol), a non-UTF-8 (WTF-8 lone surrogate) input, or an empty one. It never
  throws and never allocates before the final step; the compiler then evaluates
  `X.segment(q)` exactly once in its original position.

  `_code_point_at`'s `k` is **segment-relative and segment-bounded** — `k` past
  the segment's end is `undefined` even though the input continues — and decodes
  from the cursor's byte offset, so `k = 0` is O(1) rather than a walk from
  index 0.

  `_regexp_test` matches against a **bounded haystack whose bounds are the
  string's ends**, so `^`, `$` and lookbehind are segment-local; it is
  three-valued and returns `undefined` ("I decline, materialise and call the
  normal path") for a global or sticky regex, whose `test` is stateful in
  `lastIndex`, and for a patched `RegExp.prototype.test`.

  Affected files:

  - `crates/perry-runtime/src/intl/segments_view.rs` — the entry points.
  - `crates/perry-runtime/src/regex.rs` — `regexp_test_str_bounded`, the
    bounded-haystack primitive.
  - `crates/perry-runtime/src/object/regex_proto_thunks.rs` —
    `regexp_prototype_test_is_canonical`, the allocation-free proof that
    `RegExp.prototype.test` is still the builtin.

  Measured: the loop this exists for is 60-85 % of claude-code's active
  main-thread CPU and allocates ~420,000 times per 400-character reply. The
  falsifier is a unit counter — 200 `next` + `code_point_at` steps move
  `arena_in_use_bytes` by **zero**, with the minor-cycle count pinned so a
  collection cannot manufacture the zero.
