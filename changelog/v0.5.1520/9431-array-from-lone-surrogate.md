**`Array.from(str)` no longer returns `[]` for a string containing a lone
surrogate.**

```js
Array.from("a\ud83db")   // was []   now 3 elements, node-identical
```

`js_array_from_string_codepoints` validated the payload with
`std::str::from_utf8` and returned an EMPTY array on `Err`. Perry string
payloads are WTF-8, not UTF-8 — a lone surrogate is a legal payload, produced
by slicing a pair, by `charAt`, or by a chunked decoder — so any string
holding one made the whole conversion silently yield nothing. Whole-array
data loss with no error: the result was empty, not wrong-length.

The spread, `for…of` and `[Symbol.iterator]` forms over the same string were
already correct, which is what made this a wrong answer rather than a
consistent limitation. The walk now steps the raw bytes with the bounded
`wtf8_step` decoder the other iterators use, which yields one code point per
step and reports a lone surrogate as its own single-unit step. A part carved
out of a WTF-8 source is built through `js_string_from_wtf8_bytes` so it
keeps `STRING_FLAG_HAS_LONE_SURROGATES` — `isWellFormed()` on the element
still reports `false`, and `JSON.stringify` still escapes it as a broken
half.

The mapped form `Array.from(str, fn)` took the same walk and was empty too;
it is fixed by the same change and asserted alongside.

The rewrite also closes a pre-existing GC hazard the old loop carried: it
held a raw `elements` pointer and a borrow of the source payload across every
per-element allocation, so an evacuating collection could move both out from
under it. The walk now uses the `RuntimeHandleScope` discipline
`string/split.rs` established — root the source and the result, re-read the
source after every allocation, publish each element only after its write and
barrier — which is why this was left out of the earlier surrogate batch
rather than done as a one-line swap.

`test-files/test_gap_9431_array_from_lone_surrogate.ts` is byte-compared
against node and asserts `.length` plus every element's char codes across all
five iteration forms. Built from unfixed `origin/main` the same fixture
diverges on 18 lines.
